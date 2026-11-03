//! Coordenação da entrevista guiada sobre a captura e o auxiliar existentes.
//!
//! O serviço liga a base de dados SQLite, os artefactos de áudio, a captura e o
//! auxiliar Python numa única máquina de estados observável pela interface:
//!
//! - a base de dados abre no arranque em [`tokio::task::spawn_blocking`], com a
//!   fotografia em `opening`; uma falha fica visível e recuperável por
//!   [`AppService::retry_database`], sem fechar a janela nem apagar dados;
//! - [`AppService::start_recording`] reserva a gravação na base e a pasta nos
//!   artefactos e só depois abre o microfone; [`AppService::stop_recording`]
//!   publica o original, regista o derivado e pede a transcrição de uma
//!   pergunta ainda não confirmada;
//! - `play_audio` resolve apenas ficheiros da própria aplicação: uma pergunta
//!   de sessão (com a cache de voz), uma gravação guardada ou um treino antigo
//!   da fase 1, sem aceitar caminhos da interface;
//! - os eventos de áudio e do auxiliar entram por [`AppService::on_audio_event`]
//!   e [`AppService::on_worker_event`] e só se aplicam à operação, à geração e
//!   ao alvo que os originaram — um resultado tardio nunca muda seleções,
//!   textos confirmados nem o progresso;
//! - `prepare_close` finaliza uma captura válida e nunca indica sucesso quando
//!   a publicação falhou.
//!
//! # Concorrência
//!
//! Cada transição adquire o coordenador serial (`transitions`). O estado
//! observável é um `Mutex<InterviewState>` de curta duração: nunca é mantido
//! enquanto se espera por um dispositivo, um processo, a base de dados ou um
//! ficheiro. Os métodos de sessão vivem em [`interview`] e partilham o mesmo
//! coordenador; nenhum deles readquire o mutex do estado dentro de um auxiliar
//! que já o detém.
//!
//! O arranque do [`WorkerClient`] precisa do contexto do runtime; a tarefa
//! bloqueada entra explicitamente no runtime com `Handle::enter` antes de o
//! chamar, porque `WorkerClient::start` compõe o handshake a partir do runtime
//! atual.
//!
//! # Cache da pergunta
//!
//! A chave da cache reproduz exatamente `synthesis_cache_key` de
//! `audio-worker/src/audio_worker/engines.py`:
//! `sha256("kokoro-v1|<voz>|<velocidade com 3 casas>|<texto sem pontas>")`,
//! guardada em `phase1/question-cache/<sha256>.wav`. A síntese usa o texto da
//! cópia atual da pergunta da sessão, pelo que mudar a voz, a velocidade ou o
//! texto (incluindo duas perguntas diferentes da mesma sessão) produz um
//! ficheiro diferente.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc::UnboundedReceiver;

use crate::artifacts::ArtifactStore;
use crate::audio::{AudioEvent, AudioPort, CapturedAudio, ORIGINAL_FILE};
use crate::db::Database;
use crate::domain::{
    error_code, AppError, Availability, CaptureMeta, ComponentState, ComponentStatus, JobStage,
    OperationKind, PlaybackTarget, RestoreResult, Transcript, TranscriptStatus,
};
use crate::interview::{
    AnswerTarget, DatabaseState, Id, InterviewSnapshot, ModelJobView, OperationTarget,
    QuestionDetail, ReviewOrigin, SaveReview, SessionDetail, SessionSummary,
};
use crate::legacy::LegacyStore;
use crate::practice::{CaptureOwner, PracticeDetail};
use crate::worker::protocol::SynthesizeParams;
use crate::worker::{
    HealthResult, SynthesisResult, TranscriptionResult, WorkerClient, WorkerEvent, WorkerMessage,
    WorkerPort, WorkerRequest, WorkerResult,
};

mod ai;
mod interview;
mod practice;

/// Falha de SQLite simulada nos testes, injetada em pontos concretos do arranque
/// da captura de preparação. Nunca é usada pelo caminho real.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreparationFault {
    /// A reserva do candidato falha antes de abrir o microfone.
    ReserveCandidate,
    /// A marcação `started`/lock falha depois de o microfone abrir.
    MarkStarted,
}

/// Pasta da cache de voz da pergunta, dentro de `phase1`.
pub const QUESTION_CACHE_DIR: &str = "question-cache";

/// Voz fixa do Kokoro nesta fase.
pub const SYNTHESIS_VOICE: &str = "af_heart";

/// Velocidade fixa da síntese nesta fase.
pub const SYNTHESIS_SPEED: f32 = 1.0;

/// Taxa do WAV escrito pelo Kokoro nesta fase.
pub const SYNTHESIS_SAMPLE_RATE: u32 = 24_000;

/// Prefixo da chave da cache, igual ao do auxiliar Python.
const CACHE_KEY_PREFIX: &str = "kokoro-v1";

/// Operações bloqueantes do auxiliar usadas pelo serviço.
///
/// `WorkerPort` cobre `submit` e `restart` para substituição nos testes; o
/// encerramento e a geração pertencem ao [`WorkerClient`] concreto e ficam
/// aqui para o serviço poder fechar e validar resultados sem submeter
/// `WorkerOp::Shutdown` pelo caminho público.
pub trait WorkerControl: Send + Sync {
    fn submit(&self, request: WorkerRequest) -> Result<(), AppError>;
    fn restart(&self) -> Result<(), AppError>;
    fn shutdown(&self) -> Result<(), AppError>;
    fn generation(&self) -> u64;
    fn is_running(&self) -> bool;
}

impl WorkerControl for WorkerClient {
    fn submit(&self, request: WorkerRequest) -> Result<(), AppError> {
        WorkerPort::submit(self, request)
    }

    fn restart(&self) -> Result<(), AppError> {
        WorkerPort::restart(self)
    }

    fn shutdown(&self) -> Result<(), AppError> {
        WorkerClient::shutdown(self)
    }

    fn generation(&self) -> u64 {
        WorkerClient::generation(self)
    }

    fn is_running(&self) -> bool {
        WorkerClient::is_running(self)
    }
}

/// Auxiliar recém-arrancado, com o canal de eventos quando existe.
pub struct LaunchedWorker {
    pub control: Arc<dyn WorkerControl>,
    pub events: Option<UnboundedReceiver<WorkerEvent>>,
}

/// Arranque do auxiliar, substituível nos testes.
///
/// O arranque real lê a configuração e cria o processo; a falha é devolvida
/// como erro para o serviço a mostrar como componente em falta.
pub trait WorkerLauncher: Send + Sync {
    fn launch(&self) -> Result<LaunchedWorker, AppError>;
}

/// Destinatário das fotografias publicadas em `interview://snapshot`.
pub type SnapshotListener = Arc<dyn Fn(&InterviewSnapshot) + Send + Sync>;

/// Caminhos de dados do serviço.
///
/// `database` é aberta no arranque (e novamente por `retry_database`); `legacy`
/// conserva a leitura dos treinos da fase 1 e a cache de voz partilhada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServicePaths {
    pub database: PathBuf,
    pub legacy: PathBuf,
}

/// Estado do auxiliar detido pelo serviço.
#[derive(Default)]
struct WorkerSlot {
    control: Option<Arc<dyn WorkerControl>>,
    error: Option<AppError>,
}

/// Identidade da captura em curso, para reconhecer eventos atrasados.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CaptureIdentity {
    recording_id: Id,
    session_id: Id,
    session_question_id: Id,
    /// Dono da captura: a resposta de entrevista ou de prática a que pertence.
    owner: CaptureOwner,
}

/// Captura finalizada cuja publicação falhou, à espera de nova tentativa.
struct PendingPublication {
    capture: CapturedAudio,
    identity: CaptureIdentity,
}

/// Estado observável da entrevista.
///
/// A revisão é monotónica em memória e não substitui as versões persistidas: um
/// commit da base só aparece na fotografia depois de [`AppService::refresh_session`]
/// reler a sessão guardada. A geração do auxiliar e o pedido pendente são
/// internos e não serializam.
struct InterviewState {
    ai_job: Option<crate::ai::AiJobView>,
    pending_ai_publication: Option<Id>,
    revision: u64,
    database: DatabaseState,
    database_error: Option<AppError>,
    active_session: Option<SessionSummary>,
    current: Option<QuestionDetail>,
    availability: Availability,
    recording_id: Option<Id>,
    /// Reserva cuja publicação falhou, espelhada de `pending_publish`.
    pending_recording_id: Option<Id>,
    finalizing: bool,
    model_job: Option<ModelJobView>,
    playback: Option<PlaybackTarget>,
    warnings: Vec<AppError>,
    last_error: Option<AppError>,
    worker_generation: u64,
    active_request: Option<String>,
    /// Prática pendente aberta, independente do trabalho IA e da entrevista.
    active_practice: Option<PracticeDetail>,
    /// Dono da prática pendente, para rotear captura/transcrição.
    active_owner: Option<CaptureOwner>,
    /// Preparação de captura pendente (histórias, tarefa 3; neutra nesta tarefa).
    pending_preparation: Option<Id>,
}

impl InterviewState {
    /// Estado inicial: base a abrir, nada ocupado e nenhum componente verificado.
    fn initial() -> Self {
        Self {
            revision: 0,
            ai_job: None,
            pending_ai_publication: None,
            database: DatabaseState::Opening,
            database_error: None,
            active_session: None,
            current: None,
            availability: Availability::loading(),
            recording_id: None,
            pending_recording_id: None,
            finalizing: false,
            model_job: None,
            playback: None,
            warnings: Vec::new(),
            last_error: None,
            worker_generation: 0,
            active_request: None,
            active_practice: None,
            active_owner: None,
            pending_preparation: None,
        }
    }

    /// Um resultado só se aplica ao pedido pendente e à geração atual do auxiliar.
    fn accepts_result(&self, request_id: &str, generation: u64) -> bool {
        generation > 0
            && generation == self.worker_generation
            && self.active_request.as_deref() == Some(request_id)
    }

    /// Ocupa o estado com um trabalho de modelo, se estiver livre.
    fn begin_model_job(&mut self, job: ModelJobView) -> Result<(), AppError> {
        if self.recording_id.is_some() || self.finalizing {
            return Err(AppError::busy(
                "Termine a gravação antes de pedir uma transcrição ou síntese.",
            ));
        }
        if self.model_job.is_some() {
            return Err(AppError::busy(
                "Há uma transcrição ou síntese em curso. Aguarde que termine.",
            ));
        }
        self.active_request = Some(job.id.clone());
        self.model_job = Some(job);
        Ok(())
    }

    /// Liberta o trabalho de modelo e o pedido pendente.
    fn finish_model_job(&mut self) {
        self.model_job = None;
        self.active_request = None;
    }

    /// Nova geração do auxiliar: resultados anteriores deixam de ser aceites.
    fn set_worker_generation(&mut self, generation: u64) {
        self.worker_generation = generation;
    }

    /// Aplica o detalhe da sessão ativa, se existir.
    ///
    /// A pergunta corrente é a que a posição persistida indica; não se inventa
    /// progresso nem se mantém uma sessão concluída como ativa.
    fn apply_detail(&mut self, detail: Option<SessionDetail>) {
        match detail {
            Some(detail) => {
                let position = detail.summary.position;
                let current = detail
                    .questions
                    .iter()
                    .find(|question| question.question.position == position)
                    .cloned();
                self.active_session = Some(detail.summary);
                self.current = current;
            }
            None => {
                self.active_session = None;
                self.current = None;
            }
        }
    }

    /// Fotografia do estado observável pela interface.
    fn snapshot(&self) -> InterviewSnapshot {
        InterviewSnapshot {
            current_answer: self.current.as_ref().map(|q| {
                let a = q.active();
                crate::interview::AnswerSlot {
                    id: a.question.id.clone(),
                    session_question_id: q.question.id.clone(),
                    role: if a.question.id == q.question.id {
                        "initial"
                    } else {
                        "follow_up"
                    }
                    .into(),
                    text: a.question.text.clone(),
                    selected_recording_id: a.question.selected_recording_id.clone(),
                    selection_version: a.question.selection_version,
                    practice_id: None,
                }
            }),
            ai_job: self.ai_job.clone(),
            pending_ai_publication: self.pending_ai_publication.clone(),
            revision: self.revision,
            database: self.database,
            active_session: self.active_session.clone(),
            current: self.current.clone(),
            availability: self.availability.clone(),
            recording_id: self.recording_id.clone(),
            pending_recording_id: self.pending_recording_id.clone(),
            finalizing: self.finalizing,
            model_job: self.model_job.clone().map(|mut j| {
                j.target.answer_id = Some(j.target.session_question_id.clone());
                if let Some(q) = &self.current {
                    if q.active().question.id == j.target.session_question_id {
                        j.target.session_question_id = q.question.id.clone();
                    }
                }
                j
            }),
            playback: self.playback,
            warnings: self.warnings.clone(),
            last_error: self.last_error.clone(),
            practice: crate::practice::ExerciseSnapshot {
                revision: self.revision,
                active_practice: self.active_practice.clone(),
                active_owner: self.active_owner.clone(),
                pending_preparation: self.pending_preparation.clone(),
            },
        }
    }
}

/// Serviço da fase 2: coordena sessões, captura, SQLite, auxiliar e reprodução.
pub struct AppService {
    ai_transport: Arc<dyn crate::ai::AiTransport>,
    credentials: Arc<dyn crate::ai::CredentialStore>,
    ai_runtime: Mutex<ai::AiRuntime>,
    ai_closed: std::sync::atomic::AtomicBool,
    database_path: PathBuf,
    legacy_root: PathBuf,
    artifacts: ArtifactStore,
    database: Mutex<Option<Database>>,
    audio: Arc<dyn AudioPort>,
    launcher: Arc<dyn WorkerLauncher>,
    models_manifest: Option<PathBuf>,
    state: Mutex<InterviewState>,
    worker: Mutex<WorkerSlot>,
    /// Geração válida do auxiliar; `0` significa que nenhum resultado é aceite.
    generation: AtomicU64,
    /// Captura em curso, para reconhecer interrupções atrasadas.
    capture: Mutex<Option<CaptureIdentity>>,
    /// Captura finalizada cuja publicação falhou, à espera de nova tentativa.
    pending_publish: Mutex<Option<PendingPublication>>,
    /// Pedido `health` em curso, para reportar uma falha de verificação.
    health_request: Mutex<Option<String>>,
    /// Caminho de cache esperado do pedido de síntese em curso.
    synthesis: Mutex<Option<(String, PathBuf)>>,
    /// Coordenador serial: uma transição de cada vez.
    transitions: tokio::sync::Mutex<()>,
    listener: Mutex<Option<SnapshotListener>>,
    /// Falha de SQLite simulada nos testes; ausente no caminho real.
    fault: Mutex<Option<PreparationFault>>,
}

impl AppService {
    /// Cria o serviço. A base de dados abre depois, em [`AppService::start`].
    ///
    /// A abertura e a reconciliação correm fora da thread de arranque para a
    /// janela aparecer com a fotografia em `opening`; uma falha é recuperável
    /// por [`AppService::retry_database`]. Nenhum componente de áudio ou do
    /// auxiliar é aberto aqui.
    pub fn new(
        paths: ServicePaths,
        artifacts: ArtifactStore,
        audio: Arc<dyn AudioPort>,
        launcher: Arc<dyn WorkerLauncher>,
        models_manifest: Option<PathBuf>,
    ) -> Self {
        Self {
            ai_transport: Arc::new(crate::ai::HttpTransport),
            credentials: Arc::new(crate::ai::WindowsCredentialStore),
            ai_runtime: Mutex::new(ai::AiRuntime::default()),
            ai_closed: std::sync::atomic::AtomicBool::new(false),
            database_path: paths.database,
            legacy_root: paths.legacy,
            artifacts,
            database: Mutex::new(None),
            audio,
            launcher,
            models_manifest,
            state: Mutex::new(InterviewState::initial()),
            worker: Mutex::new(WorkerSlot::default()),
            generation: AtomicU64::new(0),
            capture: Mutex::new(None),
            pending_publish: Mutex::new(None),
            health_request: Mutex::new(None),
            synthesis: Mutex::new(None),
            transitions: tokio::sync::Mutex::new(()),
            listener: Mutex::new(None),
            fault: Mutex::new(None),
        }
    }

    /// Regista o destinatário das fotografias (o emissor de `interview://snapshot`).
    pub fn subscribe(&self, listener: SnapshotListener) {
        *self.lock_listener() = Some(listener);
    }

    /// Injeta (ou remove) uma falha de SQLite simulada, apenas para os testes.
    #[doc(hidden)]
    pub fn inject_preparation_fault(&self, fault: Option<PreparationFault>) {
        *self.lock_fault() = fault;
    }

    /// Fotografia atual, sem alterar nem publicar.
    pub fn snapshot(&self) -> InterviewSnapshot {
        self.lock_state().snapshot()
    }

    /// Arranca o serviço: disponibilidade do áudio, auxiliar e base de dados.
    ///
    /// A base abre e reconcilia em `spawn_blocking`. Uma falha de configuração
    /// do auxiliar ou da base não impede a janela de aparecer: fica indicada na
    /// fotografia e pode ser repetida.
    pub async fn start(self: &Arc<Self>) -> InterviewSnapshot {
        let _transition = self.transitions.lock().await;
        self.refresh_audio_availability().await;
        if let Err(error) = self.launch_worker().await {
            self.show_worker_missing(&error.message);
        }
        let _ = self.open_database_locked().await;
        self.snapshot()
    }

    /// Repete a abertura e a reconciliação da base, sem reiniciar a janela.
    pub async fn retry_database(&self) -> Result<InterviewSnapshot, AppError> {
        let _transition = self.transitions.lock().await;
        self.open_database_locked().await
    }

    /// Abre a base e publica as gravações interrompidas por um encerramento abrupto.
    ///
    /// O chamador tem de deter o coordenador serial. A fotografia fica em
    /// `opening` durante a abertura e passa a `ready` só depois de a sessão
    /// ativa ter sido relida da base; uma falha passa a `error`, sem apagar nem
    /// recriar a base.
    async fn open_database_locked(&self) -> Result<InterviewSnapshot, AppError> {
        self.commit(|state| {
            state.database = DatabaseState::Opening;
            state.database_error = None;
            state.last_error = None;
        });

        let path = self.database_path.clone();
        let artifacts = self.artifacts.clone();
        let opened = database_task(move || {
            let database = Database::open(path)?;
            database.recover_ai()?;
            let report = crate::artifacts::reconcile(&database, &artifacts)?;
            let uncertain = crate::artifacts::reconcile_preparations(&database, &artifacts)?;
            let mut active = database.active_session()?;
            if let Some(detail) = active.as_mut() {
                artifacts.annotate(detail);
            }
            let pending_practice = database.pending_practice()?;
            Ok((database, report, active, pending_practice, uncertain))
        })
        .await;

        match opened {
            Ok((database, report, active, pending_practice, uncertain)) => {
                *self.lock_database() = Some(database);
                Ok(self.commit(|state| {
                    state.database = DatabaseState::Ready;
                    state.database_error = None;
                    state.warnings = report.warnings;
                    state.last_error = None;
                    state.apply_detail(active);
                    state.active_practice = pending_practice.clone();
                    state.active_owner = pending_practice.map(|practice| CaptureOwner::Practice {
                        practice_id: practice.id,
                        answer_id: practice.answer.id,
                    });
                    state.pending_preparation = uncertain.into_iter().next();
                }))
            }
            Err(error) => {
                *self.lock_database() = None;
                let shown = error.clone();
                self.commit(|state| {
                    state.database = DatabaseState::Error;
                    state.database_error = Some(shown.clone());
                    state.active_session = None;
                    state.current = None;
                    state.last_error = Some(shown);
                });
                Err(error)
            }
        }
    }

    /// Base de dados aberta, ou o erro que explica a indisponibilidade.
    fn database(&self) -> Result<Database, AppError> {
        match self.lock_database().clone() {
            Some(database) => Ok(database),
            None => {
                let detail = self.lock_state().database_error.clone();
                Err(match detail {
                    Some(error) => AppError::new(
                        error_code::DATABASE_UNAVAILABLE,
                        format!(
                            "A base de dados não está disponível: {}. Repita a abertura.",
                            error.message
                        ),
                        true,
                    ),
                    None => database_unavailable(
                        "A base de dados ainda está a abrir. Aguarde um instante e repita.",
                    ),
                })
            }
        }
    }

    /// Relê a sessão ativa e publica a fotografia.
    ///
    /// Só chamado depois de um commit bem-sucedido: a fotografia reflete apenas
    /// estado guardado. Uma leitura de histórico nunca passa por aqui, pelo que
    /// não muda a pergunta corrente nem retargeta o trabalho de modelo.
    pub(super) async fn refresh_session(
        &self,
        database: &Database,
    ) -> Result<InterviewSnapshot, AppError> {
        let detail = self.load_active_detail(database).await?;
        Ok(self.commit(|state| {
            state.apply_detail(detail);
            state.last_error = None;
        }))
    }

    /// Lê a sessão ativa da base, com os avisos de artefactos do disco.
    async fn load_active_detail(
        &self,
        database: &Database,
    ) -> Result<Option<SessionDetail>, AppError> {
        let database = database.clone();
        let artifacts = self.artifacts.clone();
        database_task(move || {
            let mut detail = database.active_session()?;
            if let Some(detail) = detail.as_mut() {
                artifacts.annotate(detail);
            }
            Ok(detail)
        })
        .await
    }

    // -- arranque e disponibilidade ----------------------------------------

    /// Verifica o microfone, a saída e o auxiliar, sem carregar modelos.
    pub async fn refresh_availability(&self) -> Result<InterviewSnapshot, AppError> {
        let _transition = self.transitions.lock().await;
        self.refresh_audio_availability().await;

        let Some(control) = self.worker_control() else {
            let message = self
                .worker_error()
                .map(|error| error.message)
                .unwrap_or_else(|| "O auxiliar Python não está configurado.".to_owned());
            self.show_worker_missing(&message);
            return Ok(self.snapshot());
        };

        if !control.is_running() {
            self.show_worker_missing(
                "O auxiliar Python não está em execução; reinicie-o para transcrever ou sintetizar.",
            );
            return Ok(self.snapshot());
        }

        let request_id = format!("health-{}", uuid::Uuid::new_v4());
        let request = WorkerRequest::health(request_id.clone());
        match worker_task(move || control.submit(request)).await {
            Ok(()) => {
                // O identificador só é guardado depois de o auxiliar aceitar o
                // pedido: `BUSY` significa que a verificação anterior continua
                // pendente e é essa que tem de ser reconhecida no timeout.
                *self.lock_health() = Some(request_id);
                Ok(self.commit(|state| {
                    state.availability.transcription = ComponentState::loading();
                    state.availability.synthesis = ComponentState::loading();
                }))
            }
            // Um pedido em curso não permite verificar agora; conserva o que se
            // sabe e, sobretudo, o identificador da verificação pendente.
            Err(error) if error.code == error_code::BUSY => Ok(self.snapshot()),
            Err(error) => Ok(self.commit(|state| {
                state.availability.transcription = error_state(&error);
                state.availability.synthesis = error_state(&error);
                state.last_error = Some(error);
            })),
        }
    }

    /// Reinicia o auxiliar e reabre o áudio quando está livre.
    ///
    /// Reiniciar volta a amostrar a disponibilidade do microfone e da saída e
    /// reabre uma porta de áudio que tenha ficado presa num comando expirado.
    /// Uma falha fica visível sem perder a entrevista ativa.
    pub async fn restart_worker(self: &Arc<Self>) -> Result<InterviewSnapshot, AppError> {
        let _transition = self.transitions.lock().await;
        self.reopen_idle_audio().await;
        let working = self.lock_state().model_job.is_some();
        if working {
            let error = AppError::busy(
                "Há uma transcrição ou síntese em curso. Aguarde que termine antes de reiniciar.",
            );
            self.record_error(&error);
            return Err(error);
        }

        let Some(control) = self.worker_control() else {
            return match self.launch_worker().await {
                Ok(()) => Ok(self.snapshot()),
                Err(error) => {
                    self.show_worker_missing(&error.message);
                    self.record_error(&error);
                    Err(error)
                }
            };
        };

        let restart = Arc::clone(&control);
        match worker_task(move || restart.restart()).await {
            Ok(()) => {
                let generation = control.generation();
                *self.lock_health() = None;
                self.generation.store(generation, Ordering::SeqCst);
                Ok(self.commit(|state| {
                    state.set_worker_generation(generation);
                    state.availability.transcription = ComponentState::loading();
                    state.availability.synthesis = ComponentState::loading();
                    state.last_error = None;
                }))
            }
            Err(error) => {
                // O reinício recusado substitui a verificação pendente: uma
                // falha tardia dela não pode reescrever este erro.
                *self.lock_health() = None;
                let shown = error.clone();
                self.commit(|state| {
                    state.availability.transcription = error_state(&shown);
                    state.availability.synthesis = error_state(&shown);
                    state.last_error = Some(shown);
                });
                Err(error)
            }
        }
    }

    // -- reprodução ---------------------------------------------------------

    /// Pára a reprodução em curso, se existir.
    pub async fn stop_audio(&self) -> Result<InterviewSnapshot, AppError> {
        let _transition = self.transitions.lock().await;
        if self.lock_state().playback.is_none() {
            return Ok(self.snapshot());
        }
        let audio = Arc::clone(&self.audio);
        match audio_task(move || audio.stop_playback()).await {
            Ok(()) => Ok(self.commit(|state| state.playback = None)),
            Err(error) => {
                self.record_error(&error);
                Err(error)
            }
        }
    }

    /// Troca a reprodução para `path` e só então a marca na fotografia.
    async fn play_path(
        &self,
        target: PlaybackTarget,
        path: PathBuf,
    ) -> Result<InterviewSnapshot, AppError> {
        if self.lock_state().playback.is_some() {
            let audio = Arc::clone(&self.audio);
            if let Err(error) = audio_task(move || audio.stop_playback()).await {
                self.record_error(&error);
                return Err(error);
            }
        }
        let audio = Arc::clone(&self.audio);
        match audio_task(move || audio.play(path)).await {
            Ok(()) => Ok(self.commit(|state| {
                state.playback = Some(target);
                state.last_error = None;
            })),
            Err(error) => {
                let shown = error.clone();
                self.commit(|state| {
                    state.playback = None;
                    state.last_error = Some(shown);
                });
                Err(error)
            }
        }
    }

    // -- operações ----------------------------------------------------------

    /// Fecha a operação indicada: falhada com o erro, ou interrompida sem ele.
    ///
    /// Uma falha ao fechar a linha não substitui o erro original; fica visível
    /// na fotografia e a reconciliação do próximo arranque recupera a linha.
    async fn close_operation(
        &self,
        database: &Database,
        operation_id: &str,
        error: Option<&AppError>,
    ) {
        let database = database.clone();
        let operation_id = operation_id.to_owned();
        let closed = match error {
            Some(error) => {
                let error = error.clone();
                database_task(move || database.fail_operation(&operation_id, &error)).await
            }
            None => database_task(move || database.interrupt_operation(&operation_id)).await,
        };
        if let Err(close) = closed {
            self.record_error(&close);
        }
    }

    /// Fecha a operação do trabalho de modelo em curso sem resultado aplicado.
    ///
    /// Usada quando o resultado é descartado ou o pedido não chega a ser aceite:
    /// a operação nunca fica `running` para sempre enquanto a aplicação corre.
    async fn abandon_model_operation(&self, error: Option<&AppError>) {
        let operation_id = {
            let state = self.lock_state();
            state
                .model_job
                .as_ref()
                .map(|job| job.target.operation_id.clone())
        };
        let Some(operation_id) = operation_id else {
            return;
        };
        let Some(database) = self.lock_database().clone() else {
            return;
        };
        self.close_operation(&database, &operation_id, error).await;
    }

    /// Encerra como concluída a operação do trabalho de modelo em curso.
    ///
    /// A síntese não grava resultado em tabela própria: o encerramento e os
    /// tempos ficam apenas na linha da operação.
    async fn complete_model_operation(&self) {
        let operation_id = {
            let state = self.lock_state();
            state
                .model_job
                .as_ref()
                .map(|job| job.target.operation_id.clone())
        };
        let Some(operation_id) = operation_id else {
            return;
        };
        let Some(database) = self.lock_database().clone() else {
            return;
        };
        let operation_id = operation_id.clone();
        if let Err(error) = database_task(move || database.complete_operation(&operation_id)).await
        {
            self.record_error(&error);
        }
    }

    // -- encerramento -------------------------------------------------------

    /// Prepara o encerramento da aplicação.
    ///
    /// Finaliza uma captura ativa, encerra o auxiliar e deixa de aceitar
    /// resultados tardios. Uma gravação por publicar ou uma publicação falhada
    /// devolve erro para a janela não fechar em silêncio.
    pub async fn prepare_close(&self) -> Result<InterviewSnapshot, AppError> {
        let _transition = self.transitions.lock().await;

        let unsaved = self.lock_pending().is_some();
        if unsaved {
            let error =
                storage_error("Há uma gravação por guardar. Repita o guardar antes de fechar.");
            self.record_error(&error);
            return Err(error);
        }

        let recovering = self.lock_state().recording_id.is_none() && self.lock_capture().is_some();
        if recovering {
            // O parcial de uma paragem falhada ainda pode chegar como evento.
            let error = AppError::new(
                error_code::CAPTURE_INTERRUPTED,
                "A gravação interrompida ainda está a ser recuperada. Tente fechar novamente.",
                true,
            );
            self.record_error(&error);
            return Err(error);
        }

        // Uma escrita de metadados de preparação pendente não pode ser perdida
        // com o fecho: resolve-se primeiro, sem indicar sucesso em silêncio.
        if self.lock_state().pending_preparation.is_some() {
            let error =
                storage_error("Há uma escrita de preparação pendente. Resolva-a antes de fechar.");
            self.record_error(&error);
            return Err(error);
        }

        let capturing = self.lock_state().recording_id.is_some();
        if capturing {
            let audio = Arc::clone(&self.audio);
            match audio_task(move || audio.stop_capture()).await {
                Ok(capture) => {
                    self.publish_capture(capture, false).await?;
                }
                Err(error) => {
                    let identity = self.lock_capture().clone();
                    let shown = error.clone();
                    self.commit(|state| {
                        state.recording_id = None;
                        state.finalizing = false;
                        state.last_error = Some(shown);
                    });
                    self.resample_after_device_error(&error).await;
                    if error.code == error_code::AUDIO_TIMEOUT {
                        if let Some(identity) = identity {
                            self.recover_timed_out_capture(&identity, error.clone())
                                .await;
                        }
                    }
                    return Err(error);
                }
            }
        }

        if let Ok(db) = self.database() {
            database_task(move || db.recover_ai()).await?;
        }
        self.ai_closed.store(true, Ordering::SeqCst);
        // Nenhum resultado tardio pode voltar a ser aplicado a partir daqui. Um
        // trabalho em curso fica `interrupted` na base, recuperável no arranque.
        self.abandon_model_operation(None).await;
        *self.lock_synthesis() = None;
        self.commit(|state| {
            state.finish_model_job();
            state.finalizing = false;
        });
        self.generation.store(0, Ordering::SeqCst);
        *self.lock_health() = None;
        let control = { self.lock_worker().control.take() };
        if let Some(control) = control {
            // Sem perda de dados: o processo é terminado de qualquer forma; a
            // falha fica visível, mas não impede fechar.
            if let Err(error) = worker_task(move || control.shutdown()).await {
                self.record_error(&error);
            }
        }

        let audio = Arc::clone(&self.audio);
        let _ = audio_task(move || audio.stop_playback()).await;

        Ok(self.commit(|state| {
            state.finish_model_job();
            state.set_worker_generation(0);
            state.playback = None;
            state.finalizing = false;
        }))
    }

    // -- eventos ------------------------------------------------------------

    /// Aplica um evento assíncrono da porta de áudio.
    pub async fn on_audio_event(&self, event: AudioEvent) {
        let _transition = self.transitions.lock().await;
        match event {
            AudioEvent::CaptureInterrupted {
                recording_id,
                capture,
                error,
            } => {
                self.apply_capture_interruption(recording_id, capture, error)
                    .await;
            }
            AudioEvent::PlaybackFinished => {
                let playing = self.lock_state().playback.is_some();
                if !playing {
                    return; // fim de uma reprodução já substituída
                }
                self.commit(|state| state.playback = None);
            }
        }
    }

    /// Aplica um evento do auxiliar, se for do pedido e da geração atuais.
    pub async fn on_worker_event(&self, event: WorkerEvent) {
        let _transition = self.transitions.lock().await;
        let WorkerEvent {
            generation,
            message,
        } = event;
        match message {
            WorkerMessage::Progress { request_id, stage } => {
                self.apply_progress(&request_id, generation, stage);
            }
            WorkerMessage::Completed { request_id, result } => {
                self.apply_completed(&request_id, generation, result).await;
            }
            WorkerMessage::Failed { request_id, error } => {
                self.apply_worker_failure(&request_id, generation, error)
                    .await;
            }
        }
    }

    fn apply_progress(&self, request_id: &str, generation: u64, stage: JobStage) {
        let snapshot = {
            let mut state = self.lock_state();
            if !state.accepts_result(request_id, generation) {
                return;
            }
            let Some(job) = state.model_job.as_mut() else {
                return;
            };
            if job.stage == stage {
                return;
            }
            job.stage = stage;
            state.revision = state.revision.saturating_add(1);
            state.snapshot()
        };
        self.notify(&snapshot);
    }

    async fn apply_completed(&self, request_id: &str, generation: u64, result: WorkerResult) {
        match result {
            WorkerResult::Health(health) => {
                self.clear_health_request(request_id);
                self.apply_health(generation, health);
            }
            WorkerResult::Transcription(transcription) => {
                self.apply_transcription(request_id, generation, transcription)
                    .await;
            }
            WorkerResult::Synthesis(synthesis) => {
                self.apply_synthesis(request_id, generation, synthesis)
                    .await;
            }
            WorkerResult::Shutdown(_) => {}
        }
    }

    /// O handshake só se aplica à geração atual do auxiliar.
    fn apply_health(&self, generation: u64, health: HealthResult) {
        if generation == 0 || generation != self.generation.load(Ordering::SeqCst) {
            return;
        }
        let recovered = health.transcription.status == ComponentStatus::Ready
            && health.synthesis.status == ComponentStatus::Ready;
        self.commit(|state| {
            state.set_worker_generation(generation);
            state.availability.transcription = health.transcription;
            state.availability.synthesis = health.synthesis;
            if recovered {
                // O auxiliar voltou a estar completamente disponível: o erro da
                // verificação anterior já não descreve o estado atual.
                state.last_error = None;
            }
        });
    }

    /// Guarda uma transcrição automática e encerra a operação que a originou.
    ///
    /// O alvo é validado pela base dentro da transação do resultado: uma
    /// pergunta entretanto confirmada, uma seleção substituída ou uma operação
    /// já encerrada terminam a operação como obsoleta, sem alterar textos,
    /// seleções nem progresso, e libertam o trabalho em memória.
    async fn apply_transcription(
        &self,
        request_id: &str,
        generation: u64,
        result: TranscriptionResult,
    ) {
        let target = {
            let state = self.lock_state();
            if !state.accepts_result(request_id, generation) {
                return;
            }
            state.model_job.as_ref().map(|job| job.target.clone())
        };
        let Some(target) = target else {
            self.abandon_model_operation(None).await;
            self.commit(|state| state.finish_model_job());
            return;
        };
        let Some(recording_id) = target.recording_id.clone() else {
            self.abandon_model_operation(None).await;
            self.commit(|state| state.finish_model_job());
            return;
        };
        let Some(database) = self.lock_database().clone() else {
            let error = database_unavailable(
                "A base de dados não está disponível; o resultado não foi guardado.",
            );
            self.commit(|state| {
                state.finish_model_job();
                state.last_error = Some(error);
            });
            return;
        };

        let transcript = Transcript {
            id: request_id.to_owned(),
            status: result.status,
            text: result.text,
            segments: result.segments,
            model: result.model,
            language: result.language,
            created_at: utc_now(),
            load_ms: result.load_ms,
            processing_ms: result.processing_ms,
        };
        let configuration = transcription_configuration();

        let saved_target = target.clone();
        let saved = transcript.clone();
        let db = database.clone();
        let appended =
            database_task(move || db.append_transcript(saved_target, saved, configuration)).await;

        match appended {
            Ok(view) => {
                // Uma revisão inicial só é criada quando ainda não existe; uma
                // nova transcrição nunca substitui uma revisão guardada.
                let needs_review = transcript.status == TranscriptStatus::Completed
                    && !transcript.text.trim().is_empty()
                    && view.review.is_none();
                let mut review_error = None;
                if needs_review {
                    let request = SaveReview {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        target: AnswerTarget {
                            answer_id: None,
                            session_id: target.session_id.clone(),
                            session_question_id: target.session_question_id.clone(),
                            recording_id: recording_id.clone(),
                            selection_version: target.selection_version,
                        },
                        expected_review_version: 0,
                        source_transcript_id: Some(transcript.id.clone()),
                        origin: ReviewOrigin::Automatic,
                        text: transcript.text.clone(),
                    };
                    let db = database.clone();
                    if let Err(error) = database_task(move || db.save_review(request)).await {
                        review_error = Some(error);
                    }
                }
                // A releitura pode falhar sem invalidar o resultado guardado;
                // o erro fica visível e a próxima navegação volta a ler a base.
                if let Err(error) = self.refresh_session(&database).await {
                    self.record_error(&error);
                }
                if let Some(error) = review_error {
                    self.record_error(&error);
                }
                self.commit(|state| state.finish_model_job());
            }
            // Um alvo confirmado ou substituído não é um erro a mostrar: a
            // operação termina como obsoleta e nada é escrito.
            Err(error) if is_obsolete(&error) => {
                self.abandon_model_operation(None).await;
                self.commit(|state| state.finish_model_job());
            }
            Err(error) => {
                self.abandon_model_operation(Some(&error)).await;
                let shown = error.clone();
                self.commit(|state| {
                    state.finish_model_job();
                    state.last_error = Some(shown);
                });
            }
        }
    }

    async fn apply_synthesis(&self, request_id: &str, generation: u64, result: SynthesisResult) {
        let accepted = {
            let state = self.lock_state();
            state.accepts_result(request_id, generation)
                && state
                    .model_job
                    .as_ref()
                    .is_some_and(|job| job.kind == OperationKind::Synthesize)
        };
        if !accepted {
            return;
        }

        let expected = {
            let mut pending = self.lock_synthesis();
            match pending.as_ref() {
                Some((id, _)) if id == request_id => pending.take().map(|(_, path)| path),
                _ => None,
            }
        };
        let Some(expected) = expected else {
            self.abandon_model_operation(None).await;
            self.commit(|state| state.finish_model_job());
            return;
        };

        let candidate = expected.clone();
        let provided = result.audio_path.clone();
        let validation = storage_task(move || {
            let canonical = |path: &Path| {
                fs::canonicalize(path).map_err(|error| {
                    invalid_audio(format!(
                        "O áudio sintetizado em «{}» não existe: {error}.",
                        path.display()
                    ))
                })
            };
            let expected_file = canonical(&candidate)?;
            let provided_file = canonical(&provided)?;
            if expected_file != provided_file {
                return Err(invalid_audio(
                    "O áudio sintetizado não corresponde ao ficheiro de cache da pergunta.",
                ));
            }
            validate_synthesis_wav(&expected_file)
        })
        .await;
        if let Err(error) = validation {
            let _ = fs::remove_file(&expected);
            self.abandon_model_operation(Some(&error)).await;
            let shown = error.clone();
            self.commit(|state| {
                state.finish_model_job();
                state.last_error = Some(shown);
            });
            return;
        }

        // A síntese ficou concluída; a reprodução que se segue pertence ao
        // áudio, não muda o estado da operação.
        self.complete_model_operation().await;

        // A síntese pode terminar enquanto um áudio está a ser ouvido; a
        // reprodução automática da pergunta substitui a reprodução em curso.
        let playing = self.lock_state().playback.is_some();
        if playing {
            let audio = Arc::clone(&self.audio);
            if let Err(error) = audio_task(move || audio.stop_playback()).await {
                let shown = error.clone();
                self.commit(|state| {
                    state.finish_model_job();
                    state.playback = None;
                    state.last_error = Some(shown);
                });
                return;
            }
        }

        let audio = Arc::clone(&self.audio);
        match audio_task(move || audio.play(expected)).await {
            Ok(()) => {
                self.commit(|state| {
                    state.finish_model_job();
                    state.playback = Some(PlaybackTarget::Question);
                });
            }
            Err(error) => {
                self.commit(|state| {
                    state.finish_model_job();
                    state.playback = None;
                    state.last_error = Some(error);
                });
            }
        }
    }

    async fn apply_worker_failure(&self, request_id: &str, generation: u64, error: AppError) {
        // Uma verificação de disponibilidade não é um `ModelJob`: sem este
        // tratamento, a falha deixaria os componentes em `loading` para sempre.
        if self.clear_health_request(request_id) {
            self.commit(|state| {
                state.availability.transcription = error_state(&error);
                state.availability.synthesis = error_state(&error);
                state.last_error = Some(error);
            });
            return;
        }

        let target = {
            let state = self.lock_state();
            if !state.accepts_result(request_id, generation) {
                return;
            }
            state.model_job.as_ref().map(|job| job.kind)
        };
        let Some(kind) = target else {
            return;
        };
        if kind == OperationKind::Synthesize {
            *self.lock_synthesis() = None;
        }
        self.abandon_model_operation(Some(&error)).await;

        let running = self.worker_running();
        let shown = error.clone();
        self.commit(|state| {
            state.finish_model_job();
            state.last_error = Some(shown.clone());
            if !running {
                state.availability.transcription = error_state(&shown);
                state.availability.synthesis = error_state(&shown);
            }
        });
    }

    /// Uma captura interrompida conserva o áudio recuperável e a resposta anterior
    /// enquanto a publicação falhar.
    async fn apply_capture_interruption(
        &self,
        recording_id: Option<String>,
        capture: Option<CapturedAudio>,
        error: AppError,
    ) {
        let current = self.lock_capture().clone();
        // O evento pertence à gravação que o originou. Sem identidade não há
        // como o atribuir, e uma interrupção atrasada nunca pode invalidar a
        // gravação que está a decorrer agora.
        let id = recording_id.or_else(|| capture.as_ref().map(|capture| capture.id.clone()));
        let Some(identity) =
            current.filter(|identity| id.as_deref() == Some(identity.recording_id.as_str()))
        else {
            return;
        };
        *self.lock_capture() = None;

        let Some(capture) = capture else {
            if let Ok(database) = self.database() {
                let recording_id = identity.recording_id.clone();
                let _ = database_task(move || database.mark_interrupted(&recording_id)).await;
            }
            self.commit(|state| {
                state.recording_id = None;
                state.finalizing = false;
                state.last_error = Some(error);
            });
            return;
        };

        // A publicação já registou o seu próprio erro; o timeout ou a falha do
        // dispositivo continua a explicar porque é que a captura terminou.
        if self
            .publish_capture_as(capture, identity, false)
            .await
            .is_ok()
        {
            self.record_error(&error);
        }
    }

    // -- arranque do auxiliar ----------------------------------------------

    async fn launch_worker(self: &Arc<Self>) -> Result<(), AppError> {
        let launcher = Arc::clone(&self.launcher);
        let handle = tokio::runtime::Handle::try_current().ok();
        let launched = match worker_task(move || {
            // `WorkerClient::start` compõe o handshake no runtime atual; a
            // tarefa bloqueada entra explicitamente no runtime antes de o criar.
            let _runtime = handle.as_ref().map(|handle| handle.enter());
            launcher.launch()
        })
        .await
        {
            Ok(launched) => launched,
            Err(error) => {
                self.lock_worker().error = Some(error.clone());
                return Err(error);
            }
        };

        let LaunchedWorker { control, events } = launched;
        let generation = control.generation();
        {
            let mut slot = self.lock_worker();
            slot.control = Some(Arc::clone(&control));
            slot.error = None;
        }
        *self.lock_health() = None;
        self.generation.store(generation, Ordering::SeqCst);
        self.commit(|state| {
            state.set_worker_generation(generation);
            state.availability.transcription = ComponentState::loading();
            state.availability.synthesis = ComponentState::loading();
        });

        if let Some(events) = events {
            self.spawn_pump(events);
        }
        Ok(())
    }

    /// Lê os eventos do auxiliar e entrega-os ao serviço.
    fn spawn_pump(self: &Arc<Self>, mut events: UnboundedReceiver<WorkerEvent>) {
        let service = Arc::clone(self);
        tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                service.on_worker_event(event).await;
            }
        });
    }

    async fn refresh_audio_availability(&self) {
        let audio = Arc::clone(&self.audio);
        match audio_task(move || Ok(audio.availability())).await {
            Ok(available) => {
                self.commit(|state| {
                    state.availability.microphone = available.microphone;
                    state.availability.playback = available.playback;
                });
            }
            Err(error) => {
                self.commit(|state| {
                    state.availability.microphone = error_state(&error);
                    state.availability.playback = error_state(&error);
                });
            }
        }
    }

    /// Reabre a porta de áudio quando nada a está a usar e volta a amostrar a
    /// disponibilidade.
    async fn reopen_idle_audio(&self) {
        let idle = {
            let state = self.lock_state();
            state.recording_id.is_none() && state.playback.is_none() && !state.finalizing
        };
        if idle && self.lock_capture().is_none() {
            let audio = Arc::clone(&self.audio);
            if let Err(error) = audio_task(move || audio.reopen()).await {
                self.record_error(&error);
            }
        }
        self.refresh_audio_availability().await;
    }

    /// Volta a amostrar o áudio depois de uma falha de dispositivo.
    ///
    /// O painel de disponibilidade é a instrução do que preparar: um microfone
    /// removido não pode continuar a aparecer como «Disponível».
    async fn resample_after_device_error(&self, error: &AppError) {
        let device_failure = error.code == error_code::DEVICE_UNAVAILABLE
            || error.code == error_code::DEVICE_FORMAT_UNSUPPORTED
            || error.code == error_code::AUDIO_TIMEOUT;
        if device_failure {
            self.refresh_audio_availability().await;
        }
    }

    fn show_worker_missing(&self, message: &str) {
        let message = message.to_owned();
        self.commit(|state| {
            state.availability.transcription = ComponentState {
                status: ComponentStatus::Missing,
                message: Some(message.clone()),
            };
            state.availability.synthesis = ComponentState {
                status: ComponentStatus::Missing,
                message: Some(message),
            };
        });
    }

    // -- estado interno -----------------------------------------------------

    fn lock_state(&self) -> std::sync::MutexGuard<'_, InterviewState> {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    fn lock_database(&self) -> std::sync::MutexGuard<'_, Option<Database>> {
        self.database
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    fn lock_worker(&self) -> std::sync::MutexGuard<'_, WorkerSlot> {
        self.worker
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    fn lock_capture(&self) -> std::sync::MutexGuard<'_, Option<CaptureIdentity>> {
        self.capture
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    fn lock_pending(&self) -> std::sync::MutexGuard<'_, Option<PendingPublication>> {
        self.pending_publish
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    fn lock_health(&self) -> std::sync::MutexGuard<'_, Option<String>> {
        self.health_request
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    fn lock_synthesis(&self) -> std::sync::MutexGuard<'_, Option<(String, PathBuf)>> {
        self.synthesis
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    fn lock_listener(&self) -> std::sync::MutexGuard<'_, Option<SnapshotListener>> {
        self.listener
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    fn lock_fault(&self) -> std::sync::MutexGuard<'_, Option<PreparationFault>> {
        self.fault
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// Consome o pedido `health` pendente quando o identificador coincide.
    fn clear_health_request(&self, request_id: &str) -> bool {
        let mut pending = self.lock_health();
        if pending.as_deref() == Some(request_id) {
            *pending = None;
            true
        } else {
            false
        }
    }

    /// Aplica uma alteração, aumenta a revisão e publica a fotografia.
    fn commit(&self, apply: impl FnOnce(&mut InterviewState)) -> InterviewSnapshot {
        let snapshot = {
            let mut state = self.lock_state();
            apply(&mut state);
            state.revision = state.revision.saturating_add(1);
            state.snapshot()
        };
        self.notify(&snapshot);
        snapshot
    }

    /// Como [`AppService::commit`], mas uma recusa reverte a alteração.
    fn commit_try(
        &self,
        apply: impl FnOnce(&mut InterviewState) -> Result<(), AppError>,
    ) -> Result<InterviewSnapshot, AppError> {
        let snapshot = {
            let mut state = self.lock_state();
            apply(&mut state)?;
            state.revision = state.revision.saturating_add(1);
            state.snapshot()
        };
        self.notify(&snapshot);
        Ok(snapshot)
    }

    fn record_error(&self, error: &AppError) -> InterviewSnapshot {
        let error = error.clone();
        self.commit(|state| state.last_error = Some(error))
    }

    fn notify(&self, snapshot: &InterviewSnapshot) {
        let listener = self.lock_listener().as_ref().map(Arc::clone);
        if let Some(listener) = listener {
            listener(snapshot);
        }
    }

    fn worker_control(&self) -> Option<Arc<dyn WorkerControl>> {
        self.lock_worker().control.as_ref().map(Arc::clone)
    }

    fn worker_error(&self) -> Option<AppError> {
        self.lock_worker().error.clone()
    }

    fn worker_running(&self) -> bool {
        self.worker_control()
            .map(|control| control.is_running())
            .unwrap_or(false)
    }

    fn transcriber_ready(&self) -> bool {
        self.lock_state().availability.transcription.status == ComponentStatus::Ready
    }

    /// Caminho da cache de voz de um texto de pergunta.
    fn question_cache_path(&self, text: &str) -> PathBuf {
        self.legacy_root
            .join(QUESTION_CACHE_DIR)
            .join(format!("{}.wav", synthesis_cache_key(text)))
    }

    /// Pasta do modelo de transcrição, lida do manifesto preparado offline.
    fn whisper_directory(&self) -> Result<PathBuf, AppError> {
        let Some(manifest) = &self.models_manifest else {
            return Err(model_missing(
                "O manifesto de modelos não está configurado. Execute a preparação offline dos modelos.",
            ));
        };
        let text = fs::read_to_string(manifest).map_err(|error| {
            model_missing(format!(
                "Não foi possível ler o manifesto de modelos em «{}»: {error}.",
                manifest.display()
            ))
        })?;
        let value: Value = serde_json::from_str(&text)
            .map_err(|_| model_missing("O manifesto de modelos não é JSON válido."))?;
        let whisper_dir = value
            .get("whisperDir")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                model_missing(
                    "O manifesto de modelos não indica «whisperDir». Execute a preparação offline.",
                )
            })?;
        let path = PathBuf::from(whisper_dir);
        if !path.is_absolute() || !path.is_dir() {
            return Err(model_missing(format!(
                "A pasta do modelo de transcrição não existe em «{}». Execute a preparação offline.",
                path.display()
            )));
        }
        Ok(path)
    }
}

// ---------------------------------------------------------------------------
// Cache da pergunta
// ---------------------------------------------------------------------------

/// Texto exato da chave da cache, igual ao do auxiliar Python.
pub fn synthesis_payload(text: &str) -> String {
    format!(
        "{CACHE_KEY_PREFIX}|{SYNTHESIS_VOICE}|{SYNTHESIS_SPEED:.3}|{}",
        text.trim()
    )
}

/// Chave SHA-256 da cache `question-cache/<sha256>.wav`.
///
/// Reproduz `synthesis_cache_key` de
/// `audio-worker/src/audio_worker/engines.py` byte a byte: mudar a voz, a
/// velocidade ou o texto produz um ficheiro diferente.
pub fn synthesis_cache_key(text: &str) -> String {
    let digest = Sha256::digest(synthesis_payload(text).as_bytes());
    let mut key = String::with_capacity(digest.len() * 2);
    for byte in digest {
        key.push_str(&format!("{byte:02x}"));
    }
    key
}

// ---------------------------------------------------------------------------
// Auxiliares
// ---------------------------------------------------------------------------

/// Configuração efetiva do reconhecimento enviada ao auxiliar.
///
/// Guardada com o resultado automático sem alterar o protocolo Python: o
/// processo só recebe os parâmetros, o núcleo sabe o que enviou. Os caminhos não
/// entram na configuração guardada.
fn transcription_configuration() -> Value {
    json!({
        "device": "cpu",
        "computeType": "int8",
        "language": "en",
    })
}

/// Trabalho bloqueante num fio próprio, sem ocupar o runtime.
async fn offload<T, F>(code: &'static str, what: &'static str, work: F) -> Result<T, AppError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
{
    match tokio::task::spawn_blocking(work).await {
        Ok(result) => result,
        Err(error) => Err(AppError::new(
            code,
            format!("{what} falhou inesperadamente: {error}."),
            true,
        )),
    }
}

async fn database_task<T>(
    work: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError>
where
    T: Send + 'static,
{
    offload(
        error_code::DATABASE_UNAVAILABLE,
        "A operação na base de dados",
        work,
    )
    .await
}

async fn storage_task<T>(
    work: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError>
where
    T: Send + 'static,
{
    offload(
        error_code::STORAGE_WRITE_FAILED,
        "A operação no disco",
        work,
    )
    .await
}

async fn audio_task<T>(
    work: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError>
where
    T: Send + 'static,
{
    offload(error_code::DEVICE_UNAVAILABLE, "A operação de áudio", work).await
}

async fn worker_task<T>(
    work: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError>
where
    T: Send + 'static,
{
    offload(
        error_code::WORKER_UNAVAILABLE,
        "A operação do auxiliar",
        work,
    )
    .await
}

/// Valida o WAV PCM16 mono a 24 kHz escrito pelo Kokoro.
fn validate_synthesis_wav(path: &Path) -> Result<(), AppError> {
    let reader = hound::WavReader::open(path).map_err(|error| {
        invalid_audio(format!(
            "O áudio sintetizado em «{}» não é um WAV válido: {error}.",
            path.display()
        ))
    })?;
    let spec = reader.spec();
    if spec.sample_rate != SYNTHESIS_SAMPLE_RATE
        || spec.channels != 1
        || spec.bits_per_sample != 16
        || spec.sample_format != hound::SampleFormat::Int
    {
        return Err(invalid_audio(format!(
            "O áudio sintetizado em «{}» não está em PCM16 mono a {SYNTHESIS_SAMPLE_RATE} Hz.",
            path.display()
        )));
    }
    if reader.len() == 0 {
        return Err(invalid_audio("O áudio sintetizado está vazio."));
    }
    Ok(())
}

/// Reconstrói a captura a partir do original que a thread de áudio conseguiu
/// fechar depois de o pedido de paragem ter expirado.
///
/// Só um ficheiro já finalizado e legível é recuperável; o resultado é marcado
/// `incomplete` porque não houve confirmação de que a captura terminou bem.
fn read_recovered_capture(id: &str, original: &Path) -> Result<Option<CapturedAudio>, AppError> {
    if !original.is_file() {
        return Ok(None);
    }
    let reader = match hound::WavReader::open(original) {
        Ok(reader) => reader,
        Err(_) => return Ok(None),
    };
    let spec = reader.spec();
    let bytes = fs::metadata(original)
        .map_err(|error| {
            storage_error(format!(
                "Não foi possível ler o áudio recuperado em «{}»: {error}.",
                original.display()
            ))
        })?
        .len();
    let frames = u64::from(reader.len());
    let duration_ms = if spec.sample_rate > 0 && spec.channels > 0 {
        frames.saturating_mul(1000) / (u64::from(spec.sample_rate) * u64::from(spec.channels))
    } else {
        0
    };
    Ok(Some(CapturedAudio {
        id: id.to_owned(),
        original: original.to_path_buf(),
        meta: CaptureMeta {
            sample_rate: spec.sample_rate,
            channels: spec.channels,
            duration_ms,
            bytes,
            incomplete: true,
        },
    }))
}

/// Resolve uma referência relativa dentro da pasta de uma resposta da fase 1.
///
/// Espelha a regra da persistência antiga: referências absolutas, `..` e
/// prefixos são recusados, e o ficheiro tem de existir. A interface nunca
/// fornece caminhos. A consulta é apenas de leitura e delega na fachada dos
/// treinos antigos.
pub fn resolve_attempt_file(
    root: &Path,
    attempt_id: &str,
    reference: &str,
) -> Result<PathBuf, AppError> {
    LegacyStore::new(root.to_path_buf()).resolve_reference(attempt_id, reference)
}

fn error_state(error: &AppError) -> ComponentState {
    ComponentState {
        status: ComponentStatus::Error,
        message: Some(error.message.clone()),
    }
}

/// Um resultado pertence a um alvo que já foi confirmado ou substituído.
fn is_obsolete(error: &AppError) -> bool {
    error.code == error_code::ANSWER_CONFIRMED || error.code == error_code::STALE_SOURCE
}

fn database_unavailable(message: impl Into<String>) -> AppError {
    AppError::new(error_code::DATABASE_UNAVAILABLE, message, true)
}

fn storage_error(message: impl Into<String>) -> AppError {
    AppError::new(error_code::STORAGE_WRITE_FAILED, message, true)
}

fn artifact_missing(message: impl Into<String>) -> AppError {
    AppError::new(error_code::ARTIFACT_MISSING, message, false)
}

fn invalid_audio(message: impl Into<String>) -> AppError {
    AppError::new(error_code::INVALID_AUDIO, message, false)
}

fn model_missing(message: impl Into<String>) -> AppError {
    AppError::new(error_code::MODEL_MISSING, message, false)
}

fn stale_source(message: impl Into<String>) -> AppError {
    AppError::new(error_code::STALE_SOURCE, message, false)
}

fn answer_confirmed() -> AppError {
    AppError::new(
        error_code::ANSWER_CONFIRMED,
        "A resposta desta pergunta já foi confirmada e é apenas de leitura.",
        false,
    )
}

fn worker_unavailable(message: impl Into<String>) -> AppError {
    AppError::new(error_code::WORKER_UNAVAILABLE, message, true)
}

fn not_capturing() -> AppError {
    AppError::new(
        error_code::CAPTURE_INTERRUPTED,
        "Não há nenhuma gravação em curso para terminar.",
        false,
    )
}

/// Instante atual em UTC e RFC3339, com milissegundos.
///
/// A base de dados tem a mesma conversão civil privada; o `createdAt` de cada
/// transcrição automática é decidido aqui, no momento em que o resultado chega.
fn utc_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let seconds = now.as_secs() as i64;
    let millis = now.subsec_millis();
    let days = seconds.div_euclid(86_400);
    let second_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = second_of_day / 3_600;
    let minute = (second_of_day % 3_600) / 60;
    let second = second_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

/// Data civil a partir do número de dias desde 1970-01-01 (algoritmo de Hinnant).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = (shifted - era * 146_097) as u64;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era as i64 + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_position = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_position + 2) / 5 + 1) as u32;
    let month = if month_position < 10 {
        month_position + 3
    } else {
        month_position - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::QUESTION;

    #[test]
    fn cache_key_matches_the_python_worker() {
        assert_eq!(
            synthesis_payload(QUESTION),
            "kokoro-v1|af_heart|1.000|Tell me about a time you solved a difficult problem at work."
        );
        assert_eq!(
            synthesis_cache_key(QUESTION),
            "218e79307c03893d3b1a6b316d0c06d06e9a91419463f88578ccc47d22d3a45b"
        );
        assert_eq!(
            synthesis_cache_key(&format!("  {QUESTION}\n")),
            synthesis_cache_key(QUESTION),
            "o texto é usado sem espaços nas pontas"
        );
        assert_ne!(
            synthesis_cache_key(&QUESTION.to_uppercase()),
            synthesis_cache_key(QUESTION),
            "o texto não é normalizado além das pontas"
        );
        assert_ne!(
            synthesis_cache_key("Tell me about a time you disagreed with a colleague."),
            synthesis_cache_key(QUESTION),
            "duas perguntas da mesma sessão têm entradas diferentes"
        );
    }

    #[test]
    fn attempt_files_never_leave_the_attempt_directory() {
        let directory = tempfile::tempdir().expect("pasta temporária");
        let root = directory.path();
        let attempt_id = "8a1f4c78-2a4b-4f0e-9b6b-1e6b0f2e4d11";
        let original = root.join("attempts").join(attempt_id).join("original.wav");
        fs::create_dir_all(original.parent().expect("pasta")).expect("criar pasta");
        fs::write(&original, b"RIFF").expect("criar original");

        assert_eq!(
            resolve_attempt_file(root, attempt_id, "original.wav").expect("resolver"),
            original
        );
        for reference in ["../evil.wav", "C:\\Windows\\evil.wav", "/etc/passwd", ""] {
            assert!(
                resolve_attempt_file(root, attempt_id, reference).is_err(),
                "referência aceite: {reference}"
            );
        }
        assert!(resolve_attempt_file(root, "../fora", "original.wav").is_err());
    }
}

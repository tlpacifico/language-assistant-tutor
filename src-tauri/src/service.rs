//! Coordenação do fluxo local da fase 1.
//!
//! O serviço liga a captura, o armazenamento e o auxiliar Python numa única
//! máquina de estados observável pela interface:
//!
//! - [`AppService::start_recording`] reserva pasta e identificador e abre o
//!   microfone; [`AppService::stop_recording`] fecha a captura, publica a nova
//!   resposta e só depois a ativa, normalizando o derivado e pedindo a
//!   transcrição quando o transcritor está pronto;
//! - `play_audio` resolve apenas ficheiros da própria aplicação: o original da
//!   resposta ativa ou o WAV da pergunta em `question-cache`, pedindo a síntese
//!   ao auxiliar quando ainda não existe;
//! - os eventos de áudio e do auxiliar entram por [`AppService::on_audio_event`]
//!   e [`AppService::on_worker_event`] e só se aplicam ao pedido e à geração que
//!   os originaram;
//! - `prepare_close` finaliza a captura, assinala o trabalho pendente como
//!   interrompido e encerra o auxiliar; uma falha que possa perder dados impede
//!   fechar em silêncio.
//!
//! # Concorrência
//!
//! Cada transição adquire o coordenador serial (`transitions`). O estado
//! observável é um `Mutex<AppState>` de curta duração: nunca é mantido enquanto
//! se espera por um dispositivo, um processo ou um ficheiro. As operações que
//! podem bloquear — captura e reprodução, escrita em disco e, sobretudo, o
//! arranque, o reinício e o encerramento do auxiliar (até ~15 s) — correm em
//! `tokio::task::spawn_blocking`, para nunca ocuparem uma thread do runtime.
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
//! guardada em `phase1/question-cache/<sha256>.wav`. Mudar a voz, a velocidade
//! ou o texto produz um ficheiro diferente.

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::mpsc::UnboundedReceiver;

use crate::audio::{normalize_wav, AudioEvent, AudioPort, CapturedAudio};
use crate::domain::{
    error_code, Action, AppError, AppSnapshot, AppState, Attempt, CaptureMeta, ComponentState,
    ComponentStatus, JobStage, ModelJob, OperationKind, OperationRecord, OperationState,
    PlaybackTarget, RestoreResult, Transcript, TranscriptStatus,
};
use crate::storage::{AttemptStore, NORMALIZED_FILE, ORIGINAL_FILE};
use crate::worker::protocol::SynthesizeParams;
use crate::worker::protocol::TranscribeParams;
use crate::worker::{
    HealthResult, SynthesisResult, TranscriptionResult, WorkerClient, WorkerEvent, WorkerMessage,
    WorkerPort, WorkerRequest, WorkerResult,
};

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

/// Pasta das respostas, relativa à raiz `phase1` (igual à da persistência).
const ATTEMPTS_DIR: &str = "attempts";

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

/// Destinatário das fotografias publicadas em `practice://snapshot`.
pub type SnapshotListener = Arc<dyn Fn(&AppSnapshot) + Send + Sync>;

/// Estado do auxiliar detido pelo serviço.
#[derive(Default)]
struct WorkerSlot {
    control: Option<Arc<dyn WorkerControl>>,
    error: Option<AppError>,
}

/// Serviço da fase 1: coordena captura, persistência, auxiliar e reprodução.
pub struct AppService {
    store: AttemptStore,
    audio: Arc<dyn AudioPort>,
    root: PathBuf,
    launcher: Arc<dyn WorkerLauncher>,
    models_manifest: Option<PathBuf>,
    state: Mutex<AppState>,
    worker: Mutex<WorkerSlot>,
    /// Geração válida do auxiliar; `0` significa que nenhum resultado é aceite.
    generation: AtomicU64,
    /// Captura finalizada cuja publicação falhou, à espera de nova tentativa.
    pending_publish: Mutex<Option<CapturedAudio>>,
    /// Identificador da captura cuja interrupção ainda pode chegar.
    current_capture: Mutex<Option<String>>,
    /// Pedido `health` em curso, para reportar uma falha de verificação.
    health_request: Mutex<Option<String>>,
    /// Coordenador serial: uma transição de cada vez.
    transitions: tokio::sync::Mutex<()>,
    listener: Mutex<Option<SnapshotListener>>,
}

impl AppService {
    /// Cria o serviço, recuperando a última resposta guardada.
    ///
    /// A recuperação não escreve nem apaga nada. Um aviso (por exemplo, um
    /// áudio em falta) é apresentado como `lastError`, porque é o único canal
    /// de avisos da fotografia; os restantes dados da resposta são mantidos.
    /// Nenhum componente de áudio ou do auxiliar é aberto aqui: o arranque é
    /// assíncrono em [`AppService::start_worker`], para a interface aparecer
    /// mesmo sem Python ou modelos.
    pub fn new(
        store: AttemptStore,
        audio: Arc<dyn AudioPort>,
        root: PathBuf,
        launcher: Arc<dyn WorkerLauncher>,
        models_manifest: Option<PathBuf>,
    ) -> Self {
        let mut state = AppState::initial();
        match store.restore_last() {
            Ok(RestoreResult { attempt, warnings }) => {
                state.attempt = attempt;
                state.last_error = warnings.first().cloned();
            }
            Err(error) => state.last_error = Some(error),
        }

        Self {
            store,
            audio,
            root,
            launcher,
            models_manifest,
            state: Mutex::new(state),
            worker: Mutex::new(WorkerSlot::default()),
            generation: AtomicU64::new(0),
            pending_publish: Mutex::new(None),
            current_capture: Mutex::new(None),
            health_request: Mutex::new(None),
            transitions: tokio::sync::Mutex::new(()),
            listener: Mutex::new(None),
        }
    }

    /// Regista o destinatário das fotografias (o emissor de `practice://snapshot`).
    pub fn subscribe(&self, listener: SnapshotListener) {
        *self.lock_listener() = Some(listener);
    }

    /// Fotografia atual, sem alterar nem publicar.
    pub fn snapshot(&self) -> AppSnapshot {
        self.lock_state().snapshot()
    }

    /// Aplica a intenção correspondente a um comando da interface.
    pub async fn apply(self: &Arc<Self>, action: Action) -> Result<AppSnapshot, AppError> {
        match action {
            Action::RefreshAvailability => self.refresh_availability().await,
            Action::StartRecording => self.start_recording().await,
            Action::StopRecording => self.stop_recording().await,
            Action::RetrySaveRecording => self.retry_save_recording().await,
            Action::TranscribeResponse { attempt_id } => self.transcribe_response(attempt_id).await,
            Action::PlayAudio { target } => self.play_audio(target).await,
            Action::StopAudio => self.stop_audio().await,
            Action::SaveReview {
                attempt_id,
                source_id,
                text,
            } => self.save_review(attempt_id, source_id, text).await,
            Action::RestartWorker => self.restart_worker().await,
            Action::PrepareClose => self.prepare_close().await,
        }
    }

    // -- arranque e disponibilidade ----------------------------------------

    /// Atualiza a disponibilidade do áudio e arranca o auxiliar.
    ///
    /// Uma falha de configuração ou arranque do auxiliar não impede a captura:
    /// fica indicada na disponibilidade da transcrição e da síntese.
    pub async fn start_worker(self: &Arc<Self>) -> AppSnapshot {
        let _transition = self.transitions.lock().await;
        self.refresh_audio_availability().await;
        if let Err(error) = self.launch_worker().await {
            self.show_worker_missing(&error.message);
        }
        self.snapshot()
    }

    /// Verifica o microfone, a saída e o auxiliar, sem carregar modelos.
    pub async fn refresh_availability(&self) -> Result<AppSnapshot, AppError> {
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
            Err(error) => {
                let snapshot = self.commit(|state| {
                    state.availability.transcription = error_state(&error);
                    state.availability.synthesis = error_state(&error);
                    state.last_error = Some(error.clone());
                });
                Ok(snapshot)
            }
        }
    }

    /// Reinicia o auxiliar e reabre o áudio quando está livre.
    ///
    /// Reiniciar volta a amostrar a disponibilidade do microfone e da saída
    /// (secção 5.1) e reabre uma porta de áudio que tenha ficado presa num
    /// comando expirado. Uma falha fica visível sem perder a resposta ativa.
    pub async fn restart_worker(self: &Arc<Self>) -> Result<AppSnapshot, AppError> {
        let _transition = self.transitions.lock().await;
        self.reopen_idle_audio().await;
        if self.lock_state().model_job.is_some() {
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
                let snapshot = self.commit(|state| {
                    state.availability.transcription = error_state(&error);
                    state.availability.synthesis = error_state(&error);
                    state.last_error = Some(error.clone());
                });
                let _ = snapshot;
                Err(error)
            }
        }
    }

    // -- captura ------------------------------------------------------------

    /// Reserva a pasta da gravação e abre o microfone.
    ///
    /// A captura não depende do auxiliar nem dos modelos; só uma gravação, um
    /// trabalho de modelo ou uma publicação pendente impedem começar. Uma
    /// reprodução em curso não impede gravar: é parada aqui, como a spec exige
    /// (secção 4).
    pub async fn start_recording(&self) -> Result<AppSnapshot, AppError> {
        let _transition = self.transitions.lock().await;

        if self.lock_pending().is_some() {
            let error = storage_error(
                "A gravação anterior ainda não foi guardada. Repita o guardar antes de gravar outra resposta.",
            );
            self.record_error(&error);
            return Err(error);
        }
        // A guarda do estado cai no fim desta instrução. Um `if let` manteria a
        // temporária viva durante todo o bloco e `record_error` voltaria a
        // adquirir o mesmo mutex — o mutex não é reentrante, por isso o pedido
        // ficaria bloqueado para sempre em vez de devolver `BUSY`.
        let gate = self.lock_state().can_start_recording();
        if let Err(error) = gate {
            self.record_error(&error);
            return Err(error);
        }
        // Uma paragem que falhou pode ainda entregar o áudio recuperável como
        // `CaptureInterrupted`; gravar por cima perdia esse parcial sem aviso.
        if self.lock_capture().is_some() {
            let error = AppError::busy(
                "A gravação interrompida ainda está a ser recuperada. Aguarde um instante e tente novamente.",
            );
            self.record_error(&error);
            return Err(error);
        }

        let audio = Arc::clone(&self.audio);
        if let Err(error) = audio_task(move || audio.stop_playback()).await {
            self.record_error(&error);
            return Err(error);
        }
        // A reprodução terminou antes de abrir a entrada; o estado acompanha a
        // paragem mesmo que a reserva da pasta falhe de seguida.
        if self.lock_state().playback.is_some() {
            self.commit(|state| state.playback = None);
        }

        let store = self.store.clone();
        let target = match storage_task(move || store.allocate()).await {
            Ok(target) => target,
            Err(error) => {
                self.record_error(&error);
                return Err(error);
            }
        };
        let recording_id = target.id.clone();
        let directory = target.directory.clone();

        let audio = Arc::clone(&self.audio);
        match audio_task(move || audio.start_capture(target)).await {
            Ok(()) => {
                *self.lock_capture() = Some(recording_id.clone());
                Ok(self.commit(|state| {
                    state.recording_id = Some(recording_id);
                    state.playback = None;
                    state.last_error = None;
                }))
            }
            Err(error) => {
                // A reserva vazia não fica a poluir a pasta de respostas.
                let _ = fs::remove_dir(&directory);
                let _ = self.commit(|state| {
                    state.playback = None;
                    state.last_error = Some(error.clone());
                });
                // Um microfone que desapareceu tem de aparecer no painel de
                // disponibilidade, que é a instrução do que preparar (5.1).
                self.resample_after_device_error(&error).await;
                Err(error)
            }
        }
    }

    /// Termina a captura e publica a nova resposta ativa.
    pub async fn stop_recording(&self) -> Result<AppSnapshot, AppError> {
        let _transition = self.transitions.lock().await;
        if self.lock_state().recording_id.is_none() {
            let error = not_capturing();
            self.record_error(&error);
            return Err(error);
        }

        let audio = Arc::clone(&self.audio);
        match audio_task(move || audio.stop_capture()).await {
            Ok(capture) => self.finish_capture(capture, true).await,
            Err(error) => {
                // A thread de áudio pode recuperar o parcial e entregá-lo como
                // `CaptureInterrupted`; a resposta anterior mantém-se ativa até lá.
                let recording_id = self.lock_state().recording_id.clone();
                let snapshot = self.commit(|state| {
                    state.recording_id = None;
                    state.last_error = Some(error.clone());
                });
                let _ = snapshot;
                self.resample_after_device_error(&error).await;
                if error.code == error_code::AUDIO_TIMEOUT {
                    // Um pedido que expirou não produz evento de interrupção:
                    // a recuperação tem de acontecer aqui, senão `current_capture`
                    // bloqueava gravar e fechar para sempre.
                    if let Some(recording_id) = recording_id {
                        self.recover_timed_out_capture(&recording_id, error.clone())
                            .await;
                    }
                }
                Err(error)
            }
        }
    }

    /// Repete a publicação de uma captura já finalizada, sem voltar a gravar.
    pub async fn retry_save_recording(&self) -> Result<AppSnapshot, AppError> {
        let _transition = self.transitions.lock().await;
        let Some(capture) = self.lock_pending().take() else {
            let error = artifact_missing("Não há nenhuma gravação à espera de publicação.");
            self.record_error(&error);
            return Err(error);
        };
        self.finish_capture(capture, true).await
    }

    /// Publica a captura e, quando pedido, inicia a transcrição automática.
    async fn finish_capture(
        &self,
        capture: CapturedAudio,
        transcribe: bool,
    ) -> Result<AppSnapshot, AppError> {
        let publishable = capture.clone();
        let store = self.store.clone();
        let published = storage_task(move || store.publish(publishable)).await;

        let attempt = match published {
            Ok(attempt) => attempt,
            Err(error) => {
                // O áudio finalizado fica disponível para repetir a publicação.
                *self.lock_pending() = Some(capture);
                *self.lock_capture() = None;
                let snapshot = self.commit(|state| {
                    state.recording_id = None;
                    state.last_error = Some(error.clone());
                });
                let _ = snapshot;
                return Err(error);
            }
        };

        *self.lock_pending() = None;
        *self.lock_capture() = None;
        self.commit(|state| {
            state.recording_id = None;
            state.attempt = Some(attempt.clone());
            state.playback = None;
            state.last_error = None;
        });

        if transcribe && self.transcriber_ready() {
            // A resposta já está publicada: uma falha de normalização ou do
            // auxiliar fica visível e o áudio conserva-se para repetir.
            if let Err(error) = self.submit_transcription(&attempt.id).await {
                let _ = error;
            }
        }

        Ok(self.snapshot())
    }

    // -- transcrição --------------------------------------------------------

    /// Transcreve a resposta ativa, reutilizando o derivado já normalizado.
    pub async fn transcribe_response(&self, attempt_id: String) -> Result<AppSnapshot, AppError> {
        let _transition = self.transitions.lock().await;
        let active = self
            .lock_state()
            .attempt
            .as_ref()
            .map(|attempt| attempt.id.clone());
        if active.as_deref() != Some(attempt_id.as_str()) {
            let error =
                stale_source("A transcrição pedida refere uma resposta que já não está ativa.");
            self.record_error(&error);
            return Err(error);
        }
        self.submit_transcription(&attempt_id).await
    }

    /// Prepara o áudio e submete a transcrição ao auxiliar.
    ///
    /// O chamador tem de deter o coordenador serial. Uma falha registra o
    /// estado da operação e deixa a resposta e o áudio intactos.
    async fn submit_transcription(&self, attempt_id: &str) -> Result<AppSnapshot, AppError> {
        let attempt = match self.lock_state().attempt.clone() {
            Some(attempt) if attempt.id == attempt_id => attempt,
            Some(_) => {
                let error = stale_source("A resposta indicada já não está ativa.");
                self.record_error(&error);
                return Err(error);
            }
            None => {
                let error = artifact_missing("Ainda não há nenhuma resposta para transcrever.");
                self.record_error(&error);
                return Err(error);
            }
        };
        if self.lock_state().model_job.is_some() {
            let error =
                AppError::busy("Já está um trabalho de modelo em curso. Aguarde que termine.");
            self.record_error(&error);
            return Err(error);
        }
        let Some(control) = self.worker_control() else {
            let error = worker_unavailable(
                "O auxiliar Python não está configurado; a resposta continua guardada.",
            );
            self.record_error(&error);
            return Err(error);
        };
        if !control.is_running() {
            let error = worker_unavailable(
                "O auxiliar Python não está em execução; reinicie-o antes de transcrever.",
            );
            self.record_error(&error);
            return Err(error);
        }

        let model_path = match self.whisper_directory() {
            Ok(path) => path,
            Err(error) => {
                self.record_error(&error);
                return Err(error);
            }
        };
        let audio_path = match self.transcription_audio(&attempt).await {
            Ok(path) => path,
            Err(error) => {
                self.record_error(&error);
                return Err(error);
            }
        };

        let request_id = uuid::Uuid::new_v4().to_string();
        let operation = OperationRecord {
            id: request_id.clone(),
            kind: OperationKind::Transcribe,
            state: OperationState::Running,
            error: None,
        };
        {
            let store = self.store.clone();
            let id = attempt.id.clone();
            let record = operation.clone();
            if let Err(error) = storage_task(move || store.set_operation(&id, record)).await {
                self.record_error(&error);
                return Err(error);
            }
        }
        self.commit(|state| {
            if let Some(active) = &mut state.attempt {
                active.last_operation = Some(operation.clone());
            }
            state.last_error = None;
        });

        let params = TranscribeParams::cpu(&audio_path, &model_path);
        let duration = std::time::Duration::from_millis(attempt.capture.duration_ms.max(1));
        let request = match WorkerRequest::transcribe(request_id.clone(), params, duration) {
            Ok(request) => request,
            Err(error) => {
                self.record_error(&error);
                return Err(error);
            }
        };

        let job = ModelJob {
            id: request_id.clone(),
            attempt_id: Some(attempt.id.clone()),
            kind: OperationKind::Transcribe,
            stage: JobStage::Loading,
        };
        if let Err(error) = self.commit_try(|state| state.begin_model_job(job, request_id.clone()))
        {
            self.record_error(&error);
            return Err(error);
        }

        match worker_task(move || control.submit(request)).await {
            Ok(()) => Ok(self.snapshot()),
            Err(error) => {
                let failed = OperationRecord {
                    state: OperationState::Failed,
                    error: Some(error.clone()),
                    ..operation
                };
                {
                    let store = self.store.clone();
                    let id = attempt.id.clone();
                    let record = failed.clone();
                    let _ = storage_task(move || store.set_operation(&id, record)).await;
                }
                let snapshot = self.commit(|state| {
                    state.finish_model_job();
                    if let Some(active) = &mut state.attempt {
                        if active.id == attempt.id {
                            active.last_operation = Some(failed.clone());
                        }
                    }
                    state.last_error = Some(error.clone());
                });
                let _ = snapshot;
                Err(error)
            }
        }
    }

    /// Caminho absoluto do áudio a transcrever, criando o derivado se preciso.
    async fn transcription_audio(&self, attempt: &Attempt) -> Result<PathBuf, AppError> {
        if let Some(reference) = &attempt.normalized_file {
            if let Ok(path) = self.attempt_file(&attempt.id, reference) {
                return Ok(path);
            }
            // O derivado desapareceu: reconstrói-se a partir do original.
        }

        let source = self.attempt_file(&attempt.id, &attempt.original_file)?;
        let destination = self.attempt_directory(&attempt.id)?.join(NORMALIZED_FILE);
        let store = self.store.clone();
        let attempt_id = attempt.id.clone();
        let normalized = storage_task(move || {
            normalize_wav(&source, &destination)?;
            store.set_normalized(&attempt_id, NORMALIZED_FILE)
        })
        .await?;
        self.commit(|state| state.attempt = Some(normalized));
        self.attempt_file(&attempt.id, NORMALIZED_FILE)
    }

    // -- revisão ------------------------------------------------------------

    /// Guarda a revisão de uma transcrição da resposta ativa.
    ///
    /// Guardar uma revisão não confirma a resposta para avaliação. Se a
    /// escrita falhar, a revisão anterior mantém-se intacta na resposta; a
    /// interface conserva a edição para repetir.
    pub async fn save_review(
        &self,
        attempt_id: String,
        source_id: String,
        text: String,
    ) -> Result<AppSnapshot, AppError> {
        let _transition = self.transitions.lock().await;
        let active = self.lock_state().attempt.clone();
        let Some(attempt) = active.filter(|attempt| attempt.id == attempt_id) else {
            let error = stale_source("A revisão refere uma resposta que já não está ativa.");
            self.record_error(&error);
            return Err(error);
        };
        if !attempt
            .transcripts
            .iter()
            .any(|transcript| transcript.id == source_id)
        {
            let error =
                stale_source("A revisão refere uma transcrição que não pertence a esta resposta.");
            self.record_error(&error);
            return Err(error);
        }

        let store = self.store.clone();
        let id = attempt.id.clone();
        let source = source_id.clone();
        let body = text;
        match storage_task(move || store.save_review(&id, &source, &body)).await {
            Ok(review) => Ok(self.commit(|state| {
                if let Some(active) = &mut state.attempt {
                    if active.id == attempt.id {
                        active.review = Some(review.clone());
                    }
                }
                state.last_error = None;
            })),
            Err(error) => {
                self.record_error(&error);
                Err(error)
            }
        }
    }

    // -- reprodução ---------------------------------------------------------

    /// Reproduz a pergunta ou o original da resposta ativa.
    ///
    /// Só a captura e uma síntese em curso impedem reproduzir. Uma transcrição
    /// pode demorar minutos em CPU e ouvir uma resposta já guardada é
    /// exatamente o que a spec permite nesse intervalo (secções 4 e 8).
    pub async fn play_audio(&self, target: PlaybackTarget) -> Result<AppSnapshot, AppError> {
        let _transition = self.transitions.lock().await;
        {
            let state = self.lock_state();
            if state.recording_id.is_some() {
                let error = AppError::busy("Termine a gravação antes de reproduzir áudio.");
                drop(state);
                self.record_error(&error);
                return Err(error);
            }
            if let Some(job) = &state.model_job {
                if job.kind == OperationKind::Synthesize {
                    let error = AppError::busy(
                        "A voz da pergunta está a ser preparada. Aguarde que termine antes de reproduzir áudio.",
                    );
                    drop(state);
                    self.record_error(&error);
                    return Err(error);
                }
            }
        }

        match target {
            PlaybackTarget::Response => {
                let Some(attempt) = self.lock_state().attempt.clone() else {
                    let error = artifact_missing("Ainda não há nenhuma resposta para ouvir.");
                    self.record_error(&error);
                    return Err(error);
                };
                let path = match self.attempt_file(&attempt.id, &attempt.original_file) {
                    Ok(path) => path,
                    Err(error) => {
                        self.record_error(&error);
                        return Err(error);
                    }
                };
                self.play_path(PlaybackTarget::Response, path).await
            }
            PlaybackTarget::Question => self.play_question().await,
        }
    }

    /// Pára a reprodução em curso, se existir.
    pub async fn stop_audio(&self) -> Result<AppSnapshot, AppError> {
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

    /// Reproduz a pergunta, usando a cache ou pedindo a síntese ao auxiliar.
    async fn play_question(&self) -> Result<AppSnapshot, AppError> {
        let question = self.lock_state().question.clone();
        let cache = self.question_cache_path();
        if cache.is_file() {
            let candidate = cache.clone();
            match storage_task(move || validate_synthesis_wav(&candidate)).await {
                Ok(()) => return self.play_path(PlaybackTarget::Question, cache).await,
                // Um derivado inválido nunca é reproduzido: volta a sintetizar-se.
                Err(_) => {
                    let _ = fs::remove_file(&cache);
                }
            }
        }

        let synthesis = self.lock_state().availability.synthesis.clone();
        if synthesis.status != ComponentStatus::Ready {
            let error = model_missing(synthesis.message.unwrap_or_else(|| {
                "A síntese de voz não está disponível nesta instalação.".to_owned()
            }));
            self.record_error(&error);
            return Err(error);
        }
        let Some(control) = self.worker_control() else {
            let error = worker_unavailable(
                "O auxiliar Python não está configurado; a pergunta não pode ser sintetizada.",
            );
            self.record_error(&error);
            return Err(error);
        };
        if !control.is_running() {
            let error = worker_unavailable(
                "O auxiliar Python não está em execução; reinicie-o antes de ouvir a pergunta.",
            );
            self.record_error(&error);
            return Err(error);
        }

        if let Some(parent) = cache.parent() {
            let directory = parent.to_path_buf();
            if let Err(error) = storage_task(move || {
                fs::create_dir_all(&directory).map_err(|error| {
                    storage_error(format!(
                        "Não foi possível preparar a cache da pergunta: {error}."
                    ))
                })
            })
            .await
            {
                self.record_error(&error);
                return Err(error);
            }
        }

        let request_id = uuid::Uuid::new_v4().to_string();
        let params = SynthesizeParams::new(question, cache);
        let request = match WorkerRequest::synthesize(request_id.clone(), params) {
            Ok(request) => request,
            Err(error) => {
                self.record_error(&error);
                return Err(error);
            }
        };
        let job = ModelJob {
            id: request_id.clone(),
            attempt_id: None,
            kind: OperationKind::Synthesize,
            stage: JobStage::Loading,
        };
        if let Err(error) = self.commit_try(|state| state.begin_model_job(job, request_id)) {
            self.record_error(&error);
            return Err(error);
        }

        match worker_task(move || control.submit(request)).await {
            Ok(()) => Ok(self.snapshot()),
            Err(error) => {
                let snapshot = self.commit(|state| {
                    state.finish_model_job();
                    state.last_error = Some(error.clone());
                });
                let _ = snapshot;
                Err(error)
            }
        }
    }

    /// Troca a reprodução para `path` e só então a marca na fotografia.
    async fn play_path(
        &self,
        target: PlaybackTarget,
        path: PathBuf,
    ) -> Result<AppSnapshot, AppError> {
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
                let snapshot = self.commit(|state| {
                    state.playback = None;
                    state.last_error = Some(error.clone());
                });
                let _ = snapshot;
                Err(error)
            }
        }
    }

    // -- encerramento -------------------------------------------------------

    /// Prepara o encerramento da aplicação.
    ///
    /// Finaliza uma captura ativa, marca o trabalho pendente como interrompido
    /// e encerra o auxiliar. Uma falha que possa perder dados — uma captura por
    /// publicar ou um estado de operação que não pôde ser gravado — devolve erro
    /// para a janela não fechar em silêncio.
    pub async fn prepare_close(&self) -> Result<AppSnapshot, AppError> {
        let _transition = self.transitions.lock().await;

        if self.lock_pending().is_some() {
            let error =
                storage_error("Há uma gravação por guardar. Repita o guardar antes de fechar.");
            self.record_error(&error);
            return Err(error);
        }

        if self.lock_state().recording_id.is_none() && self.lock_capture().is_some() {
            // O parcial de uma paragem falhada ainda pode chegar como evento.
            let error = AppError::new(
                error_code::CAPTURE_INTERRUPTED,
                "A gravação interrompida ainda está a ser recuperada. Tente fechar novamente.",
                true,
            );
            self.record_error(&error);
            return Err(error);
        }

        if self.lock_state().recording_id.is_some() {
            let audio = Arc::clone(&self.audio);
            match audio_task(move || audio.stop_capture()).await {
                Ok(capture) => {
                    self.finish_capture(capture, false).await?;
                }
                Err(error) => {
                    let recording_id = self.lock_state().recording_id.clone();
                    let snapshot = self.commit(|state| {
                        state.recording_id = None;
                        state.last_error = Some(error.clone());
                    });
                    let _ = snapshot;
                    self.resample_after_device_error(&error).await;
                    if error.code == error_code::AUDIO_TIMEOUT {
                        if let Some(recording_id) = recording_id {
                            self.recover_timed_out_capture(&recording_id, error.clone())
                                .await;
                        }
                    }
                    return Err(error);
                }
            }
        }

        let job = self.lock_state().model_job.clone();
        if let Some(job) = &job {
            if let Some(attempt_id) = &job.attempt_id {
                let operation = self
                    .lock_state()
                    .attempt
                    .as_ref()
                    .filter(|attempt| &attempt.id == attempt_id)
                    .and_then(|attempt| attempt.last_operation.clone())
                    .filter(|operation| operation.id == job.id);
                if let Some(operation) = operation {
                    let interrupted = OperationRecord {
                        state: OperationState::Interrupted,
                        ..operation
                    };
                    let store = self.store.clone();
                    let id = attempt_id.clone();
                    let record = interrupted.clone();
                    if let Err(error) = storage_task(move || store.set_operation(&id, record)).await
                    {
                        self.record_error(&error);
                        return Err(error);
                    }
                    let attempt_id = attempt_id.clone();
                    self.commit(|state| {
                        if let Some(attempt) = &mut state.attempt {
                            if attempt.id == attempt_id {
                                attempt.last_operation = Some(interrupted.clone());
                            }
                        }
                    });
                }
            }
        }

        // Nenhum resultado tardio pode voltar a ser aplicado a partir daqui.
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
                if self.lock_state().playback.is_none() {
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
            state
                .model_job
                .as_ref()
                .and_then(|job| job.attempt_id.clone())
        };
        let Some(attempt_id) = target else {
            let snapshot = self.commit(|state| state.finish_model_job());
            let _ = snapshot;
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

        let store = self.store.clone();
        let id = attempt_id.clone();
        let saved = transcript.clone();
        let appended = storage_task(move || store.append_transcript(&id, saved)).await;
        let attempt = match appended {
            Ok(attempt) => attempt,
            Err(error) => {
                self.fail_operation(
                    request_id,
                    generation,
                    &attempt_id,
                    OperationKind::Transcribe,
                    error,
                )
                .await;
                return;
            }
        };

        // Uma revisão inicial só é criada quando ainda não existe; versões
        // anteriores nunca são substituídas por uma nova transcrição.
        let mut attempt = attempt;
        let attempt_id = attempt.id.clone();
        let needs_review = attempt.review.is_none()
            && transcript.status == TranscriptStatus::Completed
            && !transcript.text.trim().is_empty();
        if needs_review {
            let store = self.store.clone();
            let source_id = transcript.id.clone();
            let text = transcript.text.clone();
            let id = attempt.id.clone();
            match storage_task(move || store.save_review(&id, &source_id, &text)).await {
                Ok(review) => attempt.review = Some(review),
                Err(error) => {
                    let _ = self.commit(|state| {
                        if state.attempt.as_ref().map(|active| active.id.as_str())
                            == Some(attempt_id.as_str())
                        {
                            state.attempt = Some(attempt.clone());
                        }
                        state.finish_model_job();
                        state.last_error = Some(error);
                    });
                    return;
                }
            }
        }

        let _ = self.commit(|state| {
            // Uma resposta mais recente nunca é substituída por um resultado antigo.
            if state.attempt.as_ref().map(|active| active.id.as_str()) == Some(attempt_id.as_str())
            {
                state.attempt = Some(attempt);
            }
            state.finish_model_job();
        });
    }

    async fn apply_synthesis(&self, request_id: &str, generation: u64, result: SynthesisResult) {
        let accepted = {
            let state = self.lock_state();
            state.accepts_result(request_id, generation)
        };
        if !accepted {
            return;
        }

        let expected = self.question_cache_path();
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
            let snapshot = self.commit(|state| {
                state.finish_model_job();
                state.last_error = Some(error);
            });
            let _ = snapshot;
            return;
        }

        // A síntese pode terminar enquanto uma resposta está a ser ouvida; a
        // reprodução automática da pergunta substitui a reprodução em curso em
        // vez de colidir com ela (a captura já está terminada aqui).
        if self.lock_state().playback.is_some() {
            let audio = Arc::clone(&self.audio);
            if let Err(error) = audio_task(move || audio.stop_playback()).await {
                let snapshot = self.commit(|state| {
                    state.finish_model_job();
                    state.playback = None;
                    state.last_error = Some(error);
                });
                let _ = snapshot;
                return;
            }
        }

        let audio = Arc::clone(&self.audio);
        match audio_task(move || audio.play(expected)).await {
            Ok(()) => {
                let snapshot = self.commit(|state| {
                    state.finish_model_job();
                    state.playback = Some(PlaybackTarget::Question);
                });
                let _ = snapshot;
            }
            Err(error) => {
                let snapshot = self.commit(|state| {
                    state.finish_model_job();
                    state.playback = None;
                    state.last_error = Some(error);
                });
                let _ = snapshot;
            }
        }
    }

    async fn apply_worker_failure(&self, request_id: &str, generation: u64, error: AppError) {
        // Uma verificação de disponibilidade não é um `ModelJob`: sem este
        // tratamento, a falha (por exemplo, um timeout) deixaria os componentes
        // em `loading` para sempre e esconderia o auxiliar bloqueado.
        if self.clear_health_request(request_id) {
            let _ = self.commit(|state| {
                state.availability.transcription = error_state(&error);
                state.availability.synthesis = error_state(&error);
                state.last_error = Some(error.clone());
            });
            return;
        }

        let target = {
            let state = self.lock_state();
            if !state.accepts_result(request_id, generation) {
                return;
            }
            state
                .model_job
                .as_ref()
                .map(|job| (job.kind, job.attempt_id.clone()))
        };
        let Some((kind, attempt_id)) = target else {
            return;
        };

        let running = self.worker_running();
        if let Some(attempt_id) = attempt_id {
            let operation = OperationRecord {
                id: request_id.to_owned(),
                kind,
                state: OperationState::Failed,
                error: Some(error.clone()),
            };
            let store = self.store.clone();
            let id = attempt_id.clone();
            let record = operation.clone();
            let _ = storage_task(move || store.set_operation(&id, record)).await;

            let snapshot = self.commit(|state| {
                if let Some(attempt) = &mut state.attempt {
                    if attempt.id == attempt_id {
                        attempt.last_operation = Some(operation.clone());
                    }
                }
                state.finish_model_job();
                state.last_error = Some(error.clone());
                if !running {
                    state.availability.transcription = error_state(&error);
                    state.availability.synthesis = error_state(&error);
                }
            });
            let _ = snapshot;
            return;
        }

        let snapshot = self.commit(|state| {
            state.finish_model_job();
            state.last_error = Some(error.clone());
            if !running {
                state.availability.transcription = error_state(&error);
                state.availability.synthesis = error_state(&error);
            }
        });
        let _ = snapshot;
    }

    /// Falha de uma operação: registra o estado e liberta o trabalho pendente.
    async fn fail_operation(
        &self,
        request_id: &str,
        generation: u64,
        attempt_id: &str,
        kind: OperationKind,
        error: AppError,
    ) {
        let operation = OperationRecord {
            id: request_id.to_owned(),
            kind,
            state: OperationState::Failed,
            error: Some(error.clone()),
        };
        let store = self.store.clone();
        let id = attempt_id.to_owned();
        let record = operation.clone();
        let _ = storage_task(move || store.set_operation(&id, record)).await;

        let accepted = {
            let state = self.lock_state();
            state.accepts_result(request_id, generation)
        };
        if !accepted {
            return;
        }
        let attempt_id = attempt_id.to_owned();
        let snapshot = self.commit(|state| {
            if let Some(attempt) = &mut state.attempt {
                if attempt.id == attempt_id {
                    attempt.last_operation = Some(operation.clone());
                }
            }
            state.finish_model_job();
            state.last_error = Some(error.clone());
        });
        let _ = snapshot;
    }

    /// Uma captura interrompida conserva o áudio recuperável e a resposta anterior
    /// enquanto a publicação falhar.
    async fn apply_capture_interruption(
        &self,
        recording_id: Option<String>,
        capture: Option<CapturedAudio>,
        error: AppError,
    ) {
        let recording = self.lock_state().recording_id.clone();
        let expected = self.lock_capture().clone();
        // O evento pertence à gravação que o originou. Sem identidade não há
        // como o atribuir, e uma interrupção atrasada nunca pode invalidar a
        // gravação que está a decorrer agora.
        let id = recording_id.or_else(|| capture.as_ref().map(|capture| capture.id.clone()));
        let matches = match id {
            Some(id) => {
                expected.as_deref() == Some(id.as_str())
                    || recording.as_deref() == Some(id.as_str())
            }
            None => false,
        };
        if !matches {
            return; // interrupção de uma captura já tratada ou já substituída
        }
        *self.lock_capture() = None;

        let Some(capture) = capture else {
            let snapshot = self.commit(|state| {
                state.recording_id = None;
                state.last_error = Some(error);
            });
            let _ = snapshot;
            return;
        };

        let store = self.store.clone();
        let publishable = capture.clone();
        match storage_task(move || store.publish(publishable)).await {
            Ok(attempt) => {
                let snapshot = self.commit(|state| {
                    state.recording_id = None;
                    state.attempt = Some(attempt);
                    state.playback = None;
                    state.last_error = Some(error);
                });
                let _ = snapshot;
            }
            Err(publish_error) => {
                *self.lock_pending() = Some(capture);
                let snapshot = self.commit(|state| {
                    state.recording_id = None;
                    state.last_error = Some(publish_error);
                });
                let _ = snapshot;
            }
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
                let snapshot = self.commit(|state| {
                    state.availability.microphone = error_state(&error);
                    state.availability.playback = error_state(&error);
                });
                let _ = snapshot;
            }
        }
    }

    /// Reabre a porta de áudio quando nada a está a usar e volta a amostrar a
    /// disponibilidade.
    ///
    /// É a recuperação explícita de um comando que excedeu o prazo: a porta
    /// antiga fica presa e o serviço não pode voltar a confiar no dispositivo
    /// até ser reaberta.
    async fn reopen_idle_audio(&self) {
        let idle = {
            let state = self.lock_state();
            state.recording_id.is_none() && state.playback.is_none()
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
    /// O painel de disponibilidade é a instrução do que preparar (secção 5.1):
    /// um microfone removido não pode continuar a aparecer como «Disponível»,
    /// nem o contrário depois de restaurado.
    async fn resample_after_device_error(&self, error: &AppError) {
        let device_failure = error.code == error_code::DEVICE_UNAVAILABLE
            || error.code == error_code::DEVICE_FORMAT_UNSUPPORTED
            || error.code == error_code::AUDIO_TIMEOUT;
        if device_failure {
            self.refresh_audio_availability().await;
        }
    }

    /// Recupera uma captura cuja paragem excedeu o prazo.
    ///
    /// A thread de áudio pode ter finalizado `original.wav` depois de o pedido
    /// expirar. Se existir um WAV legível, é publicado como recuperável e
    /// marcado `incomplete`; sem ficheiro, a reserva é libertada para permitir
    /// gravar de novo. Em ambos os casos `current_capture` é limpo, para o
    /// serviço não ficar à espera de um evento que já não chega.
    async fn recover_timed_out_capture(&self, recording_id: &str, error: AppError) {
        *self.lock_capture() = None;
        let directory = match self.attempt_directory(recording_id) {
            Ok(directory) => directory,
            Err(_) => return,
        };
        let id = recording_id.to_owned();
        let original = directory.join(ORIGINAL_FILE);
        match storage_task(move || read_recovered_capture(&id, &original)).await {
            Ok(Some(capture)) => {
                // A publicação limpa o erro; o timeout continua a ser o que
                // explica porque é que a resposta foi recuperada. Se a
                // publicação falhar, `finish_capture` já deixou a captura
                // pendente para repetir.
                if self.finish_capture(capture, false).await.is_ok() {
                    self.record_error(&error);
                }
            }
            Ok(None) => {
                // Nunca foi finalizado áudio: a pasta reservada vazia não fica.
                let _ = fs::remove_dir(&directory);
                self.record_error(&error);
            }
            Err(storage_error) => {
                self.record_error(&storage_error);
            }
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

    fn lock_state(&self) -> std::sync::MutexGuard<'_, AppState> {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    fn lock_worker(&self) -> std::sync::MutexGuard<'_, WorkerSlot> {
        self.worker
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    fn lock_pending(&self) -> std::sync::MutexGuard<'_, Option<CapturedAudio>> {
        self.pending_publish
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    fn lock_capture(&self) -> std::sync::MutexGuard<'_, Option<String>> {
        self.current_capture
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    fn lock_health(&self) -> std::sync::MutexGuard<'_, Option<String>> {
        self.health_request
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// Consome o pedido `health` pendente quando o identificador coincide.
    ///
    /// Devolve `true` apenas para a verificação em curso; uma falha de outro
    /// pedido, ou de uma verificação já substituída, não altera o estado.
    fn clear_health_request(&self, request_id: &str) -> bool {
        let mut pending = self.lock_health();
        if pending.as_deref() == Some(request_id) {
            *pending = None;
            true
        } else {
            false
        }
    }

    fn lock_listener(&self) -> std::sync::MutexGuard<'_, Option<SnapshotListener>> {
        self.listener
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// Aplica uma alteração, aumenta a revisão e publica a fotografia.
    fn commit(&self, apply: impl FnOnce(&mut AppState)) -> AppSnapshot {
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
        apply: impl FnOnce(&mut AppState) -> Result<(), AppError>,
    ) -> Result<AppSnapshot, AppError> {
        let snapshot = {
            let mut state = self.lock_state();
            apply(&mut state)?;
            state.revision = state.revision.saturating_add(1);
            state.snapshot()
        };
        self.notify(&snapshot);
        Ok(snapshot)
    }

    fn record_error(&self, error: &AppError) -> AppSnapshot {
        let error = error.clone();
        self.commit(|state| state.last_error = Some(error))
    }

    fn notify(&self, snapshot: &AppSnapshot) {
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

    fn attempt_directory(&self, attempt_id: &str) -> Result<PathBuf, AppError> {
        if !is_safe_id(attempt_id) {
            return Err(artifact_missing(
                "O identificador indicado não corresponde a uma resposta guardada.",
            ));
        }
        Ok(self.root.join(ATTEMPTS_DIR).join(attempt_id))
    }

    /// Resolve uma referência relativa dentro da pasta da resposta ativa.
    fn attempt_file(&self, attempt_id: &str, reference: &str) -> Result<PathBuf, AppError> {
        resolve_attempt_file(&self.root, attempt_id, reference)
    }

    /// Caminho da cache de voz da pergunta.
    fn question_cache_path(&self) -> PathBuf {
        self.question_cache_path_for(&self.lock_state().question)
    }

    fn question_cache_path_for(&self, question: &str) -> PathBuf {
        self.root
            .join(QUESTION_CACHE_DIR)
            .join(format!("{}.wav", synthesis_cache_key(question)))
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

/// Resolve uma referência relativa dentro da pasta de uma resposta.
///
/// Espelha a regra da persistência: referências absolutas, `..` e prefixos são
/// recusados, e o ficheiro tem de existir. A interface nunca fornece caminhos.
pub fn resolve_attempt_file(
    root: &Path,
    attempt_id: &str,
    reference: &str,
) -> Result<PathBuf, AppError> {
    if !is_safe_id(attempt_id) {
        return Err(artifact_missing(
            "O identificador indicado não corresponde a uma resposta guardada.",
        ));
    }
    let reference = Path::new(reference);
    if reference.is_absolute() {
        return Err(artifact_missing(
            "A referência do ficheiro da resposta não é válida.",
        ));
    }
    let mut resolved = root.join(ATTEMPTS_DIR).join(attempt_id);
    let mut components = 0;
    for component in reference.components() {
        match component {
            Component::Normal(part) => {
                resolved.push(part);
                components += 1;
            }
            _ => {
                return Err(artifact_missing(
                    "A referência do ficheiro da resposta não é válida.",
                ))
            }
        }
    }
    if components == 0 || !resolved.is_file() {
        return Err(artifact_missing(
            "O ficheiro da resposta não foi encontrado; os restantes dados foram conservados.",
        ));
    }
    Ok(resolved)
}

fn is_safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '-' || character == '_'
        })
}

fn error_state(error: &AppError) -> ComponentState {
    ComponentState {
        status: ComponentStatus::Error,
        message: Some(error.message.clone()),
    }
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
/// A persistência tem a mesma conversão civil privada; a fase 1 não traz uma
/// biblioteca de datas e o `createdAt` de cada transcrição é decidido aqui.
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
    }

    #[test]
    fn attempt_files_never_leave_the_attempt_directory() {
        let directory = tempfile::tempdir().expect("pasta temporária");
        let root = directory.path();
        let attempt_id = "8a1f4c78-2a4b-4f0e-9b6b-1e6b0f2e4d11";
        let original = root
            .join(ATTEMPTS_DIR)
            .join(attempt_id)
            .join("original.wav");
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

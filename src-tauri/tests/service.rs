//! Testes de coordenação do serviço da fase 2 (entrevista guiada).
//!
//! Nenhum teste abre um microfone, carrega modelos ou arranca subprocessos: a
//! porta de áudio, o auxiliar e o arranque do auxiliar são substituídos por
//! adaptadores controlados. A base de dados e os artefactos são reais (diretório
//! temporário), para cobrir publicação, derivado normalizado, revisões,
//! confirmações e recuperação.
//!
//! O banco inicial é aplicado ao abrir a base; o arranjo reduz depois o banco
//! às duas perguntas fixas abaixo para o conteúdo da sessão ser determinístico.
//! A pergunta fixa é a mesma da fase 1, para a chave de síntese continuar a ser
//! comparada com o auxiliar Python.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hound::{SampleFormat, WavSpec, WavWriter};
use rusqlite::params;
use serde_json::json;

use language_assistant_tutor::artifacts::{ArtifactStore, NORMALIZED_FILE};
use language_assistant_tutor::audio::{
    AudioEvent, AudioPort, AvailabilityAudio, CapturedAudio, RecordingTarget,
};
use language_assistant_tutor::db::Database;
use language_assistant_tutor::domain::{
    error_code, AppError, CaptureMeta, ComponentState, ComponentStatus, OperationKind,
    PlaybackTarget, Segment, TranscriptStatus, QUESTION,
};
use language_assistant_tutor::interview::{
    AdvanceSession, AnswerTarget, AudioSelection, Category, ConfirmAnswer, DatabaseState,
    InterviewSnapshot, QuestionDetail, ReviewOrigin, SaveReview, SessionDetail, SessionStatus,
    StartSession,
};
use language_assistant_tutor::service::{
    synthesis_cache_key, AppService, LaunchedWorker, ServicePaths, WorkerControl, WorkerLauncher,
};
use language_assistant_tutor::worker::protocol::WorkerOp;
use language_assistant_tutor::worker::{
    HealthResult, SynthesisResult, TranscriptionResult, WorkerEvent, WorkerMessage, WorkerRequest,
    WorkerResult,
};

/// Segunda pergunta do banco inicial, mantida para sessões de duas perguntas.
const SECOND_QUESTION: &str = "Tell me about a time you had to reduce the scope of a task.";

// ---------------------------------------------------------------------------
// Porta de áudio controlada
// ---------------------------------------------------------------------------

/// Escritor WAV mínimo para as capturas e sínteses simuladas.
fn write_rate_wav(path: &Path, sample_rate: u32) {
    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).expect("criar WAV de teste");
    for _ in 0..(sample_rate / 2) {
        writer.write_sample(0i16).expect("escrever amostra");
    }
    writer.finalize().expect("fechar WAV de teste");
}

/// WAV válido sem amostras: a publicação tem de recusá-lo.
fn write_empty_wav(path: &Path) {
    let spec = WavSpec {
        channels: 1,
        sample_rate: 48_000,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let writer = WavWriter::create(path, spec).expect("criar WAV vazio");
    writer.finalize().expect("fechar WAV vazio");
}

#[derive(Debug, Default)]
struct AudioState {
    capture: Option<RecordingTarget>,
    fail_stop: bool,
    /// A paragem seguinte excede o prazo, mas o original fica finalizado.
    stop_timeout: bool,
    /// A paragem seguinte escreve um WAV sem amostras (publicação falha).
    corrupt_stop: bool,
    /// Falha injetada na abertura seguinte do microfone.
    fail_start: Option<AppError>,
    /// Disponibilidade amostrada; `None` significa pronto.
    microphone: Option<ComponentState>,
    played: Vec<PathBuf>,
    playing: Option<PathBuf>,
    stops: usize,
    reopens: usize,
}

/// Adaptador de áudio controlado pelos testes.
#[derive(Debug, Default)]
struct FakeAudio {
    state: Mutex<AudioState>,
}

impl FakeAudio {
    fn fail_next_stop(&self) {
        self.state.lock().expect("estado do áudio").fail_stop = true;
    }

    /// A próxima paragem expira, como uma thread de áudio que responde tarde.
    fn fail_next_stop_timeout(&self) {
        self.state.lock().expect("estado do áudio").stop_timeout = true;
    }

    /// A próxima paragem deixa um original inválido, para a publicação falhar.
    fn corrupt_next_stop(&self) {
        self.state.lock().expect("estado do áudio").corrupt_stop = true;
    }

    /// A próxima abertura do microfone falha com o erro indicado.
    fn fail_next_start(&self, error: AppError) {
        self.state.lock().expect("estado do áudio").fail_start = Some(error);
    }

    /// Muda a disponibilidade devolvida pela porta até nova alteração.
    fn set_microphone(&self, state: ComponentState) {
        self.state.lock().expect("estado do áudio").microphone = Some(state);
    }

    fn played(&self) -> Vec<PathBuf> {
        self.state.lock().expect("estado do áudio").played.clone()
    }

    fn playback_stops(&self) -> usize {
        self.state.lock().expect("estado do áudio").stops
    }

    fn reopens(&self) -> usize {
        self.state.lock().expect("estado do áudio").reopens
    }
}

impl AudioPort for FakeAudio {
    fn availability(&self) -> AvailabilityAudio {
        let state = self.state.lock().expect("estado do áudio");
        AvailabilityAudio {
            microphone: state
                .microphone
                .clone()
                .unwrap_or_else(ComponentState::ready),
            playback: ComponentState::ready(),
        }
    }

    fn start_capture(&self, target: RecordingTarget) -> Result<(), AppError> {
        let mut state = self.state.lock().expect("estado do áudio");
        if let Some(error) = state.fail_start.take() {
            return Err(error);
        }
        write_rate_wav(&target.directory.join("original.wav"), 48_000);
        state.capture = Some(target);
        Ok(())
    }

    fn stop_capture(&self) -> Result<CapturedAudio, AppError> {
        let mut state = self.state.lock().expect("estado do áudio");
        if state.stop_timeout {
            // A thread de áudio pode acabar por fechar o original depois de o
            // pedido expirar: o ficheiro fica escrito e o serviço não recebe
            // resposta nem evento.
            state.stop_timeout = false;
            state.capture = None;
            return Err(AppError::new(
                error_code::AUDIO_TIMEOUT,
                "A thread de áudio não respondeu a tempo.",
                true,
            ));
        }
        let target = state.capture.take().ok_or_else(|| {
            AppError::new(
                error_code::CAPTURE_INTERRUPTED,
                "Não há nenhuma gravação em curso.",
                false,
            )
        })?;
        if state.fail_stop {
            state.fail_stop = false;
            return Err(AppError::new(
                error_code::CAPTURE_INTERRUPTED,
                "A gravação foi interrompida pelo dispositivo.",
                true,
            ));
        }
        let original = target.directory.join("original.wav");
        if state.corrupt_stop {
            state.corrupt_stop = false;
            write_empty_wav(&original);
        }
        let bytes = fs::metadata(&original)
            .expect("a captura escreveu o original")
            .len();
        Ok(CapturedAudio {
            id: target.id,
            original,
            meta: CaptureMeta {
                sample_rate: 48_000,
                channels: 1,
                duration_ms: 500,
                bytes,
                incomplete: false,
            },
        })
    }

    fn play(&self, path: PathBuf) -> Result<(), AppError> {
        let mut state = self.state.lock().expect("estado do áudio");
        state.played.push(path.clone());
        state.playing = Some(path);
        Ok(())
    }

    fn stop_playback(&self) -> Result<(), AppError> {
        let mut state = self.state.lock().expect("estado do áudio");
        state.playing = None;
        state.stops += 1;
        Ok(())
    }

    fn reopen(&self) -> Result<(), AppError> {
        let mut state = self.state.lock().expect("estado do áudio");
        state.reopens += 1;
        state.playing = None;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Auxiliar controlado
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct WorkerState {
    requests: Vec<WorkerRequest>,
    generation: u64,
    running: bool,
    shutdowns: usize,
    busy_submit: bool,
    fail_restart: bool,
}

/// Auxiliar controlado: registra pedidos e permite injetar eventos.
struct FakeWorker {
    state: Mutex<WorkerState>,
}

impl Default for FakeWorker {
    fn default() -> Self {
        Self {
            state: Mutex::new(WorkerState {
                requests: Vec::new(),
                generation: 1,
                running: true,
                shutdowns: 0,
                busy_submit: false,
                fail_restart: false,
            }),
        }
    }
}

impl FakeWorker {
    fn requests(&self) -> Vec<WorkerRequest> {
        self.state
            .lock()
            .expect("estado do auxiliar")
            .requests
            .clone()
    }

    fn shutdowns(&self) -> usize {
        self.state.lock().expect("estado do auxiliar").shutdowns
    }

    /// O próximo `submit` é recusado como se já houvesse um pedido pendente.
    fn fail_next_submit_busy(&self) {
        self.state.lock().expect("estado do auxiliar").busy_submit = true;
    }

    /// O próximo `restart` é recusado como se já houvesse um pedido pendente.
    fn fail_next_restart(&self) {
        self.state.lock().expect("estado do auxiliar").fail_restart = true;
    }
}

impl WorkerControl for FakeWorker {
    fn submit(&self, request: WorkerRequest) -> Result<(), AppError> {
        let mut state = self.state.lock().expect("estado do auxiliar");
        if !state.running {
            return Err(AppError::new(
                error_code::WORKER_UNAVAILABLE,
                "O auxiliar não está em execução.",
                true,
            ));
        }
        if state.busy_submit {
            state.busy_submit = false;
            return Err(AppError::busy("O auxiliar já está a processar um pedido."));
        }
        state.requests.push(request);
        Ok(())
    }

    fn restart(&self) -> Result<(), AppError> {
        let mut state = self.state.lock().expect("estado do auxiliar");
        if state.fail_restart {
            state.fail_restart = false;
            return Err(AppError::busy(
                "Há um pedido em curso; aguarde que termine antes de reiniciar.",
            ));
        }
        state.generation += 1;
        state.running = true;
        Ok(())
    }

    fn shutdown(&self) -> Result<(), AppError> {
        let mut state = self.state.lock().expect("estado do auxiliar");
        state.shutdowns += 1;
        state.running = false;
        Ok(())
    }

    fn generation(&self) -> u64 {
        self.state.lock().expect("estado do auxiliar").generation
    }

    fn is_running(&self) -> bool {
        self.state.lock().expect("estado do auxiliar").running
    }
}

/// Arranque controlado do auxiliar.
struct FakeLauncher {
    worker: Arc<FakeWorker>,
    failure: Option<AppError>,
    launches: AtomicUsize,
}

impl FakeLauncher {
    fn new(worker: Arc<FakeWorker>) -> Self {
        Self {
            worker,
            failure: None,
            launches: AtomicUsize::new(0),
        }
    }

    fn failing(error: AppError) -> Self {
        Self {
            worker: Arc::new(FakeWorker::default()),
            failure: Some(error),
            launches: AtomicUsize::new(0),
        }
    }
}

impl WorkerLauncher for FakeLauncher {
    fn launch(&self) -> Result<LaunchedWorker, AppError> {
        self.launches.fetch_add(1, Ordering::SeqCst);
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        Ok(LaunchedWorker {
            control: self.worker.clone(),
            events: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Utilitários de teste
// ---------------------------------------------------------------------------

/// Pedido de transcrição já submetido, com o áudio indicado.
struct RecordedRequest {
    id: String,
    input_path: PathBuf,
}

fn input_path(request: &WorkerRequest) -> PathBuf {
    PathBuf::from(
        request.params["audioPath"]
            .as_str()
            .expect("o pedido de transcrição indica audioPath"),
    )
}

fn transcription_result(text: &str) -> WorkerResult {
    WorkerResult::Transcription(TranscriptionResult {
        status: if text.is_empty() {
            TranscriptStatus::NoSpeech
        } else {
            TranscriptStatus::Completed
        },
        text: text.to_owned(),
        segments: if text.is_empty() {
            Vec::new()
        } else {
            vec![Segment {
                start_ms: 0,
                end_ms: 1_000,
                text: text.to_owned(),
            }]
        },
        model: "small".to_owned(),
        language: "en".to_owned(),
        load_ms: 10,
        processing_ms: 20,
    })
}

/// Resultado de síntese que aponta para o WAV escrito pelo auxiliar.
fn synthesis_result(path: &Path) -> WorkerResult {
    WorkerResult::Synthesis(SynthesisResult {
        audio_path: path.to_path_buf(),
        sample_rate: 24_000,
        duration_ms: 500,
        load_ms: 1,
        processing_ms: 2,
    })
}

/// Trabalho de modelo pendente no snapshot.
fn pending_jobs(snapshot: &InterviewSnapshot) -> usize {
    usize::from(snapshot.model_job.is_some())
}

fn selected_recording(
    current: &QuestionDetail,
) -> &language_assistant_tutor::interview::RecordingView {
    let selected = current
        .question
        .selected_recording_id
        .as_deref()
        .expect("a pergunta tem uma gravação selecionada");
    current
        .recordings
        .iter()
        .find(|recording| recording.id == selected)
        .expect("a gravação selecionada pertence à pergunta")
}

fn answer_target(current: &QuestionDetail) -> AnswerTarget {
    AnswerTarget {
        session_id: current.question.session_id.clone(),
        session_question_id: current.question.id.clone(),
        recording_id: selected_recording(current).id.clone(),
        selection_version: current.question.selection_version,
    }
}

fn new_request_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Especificação da sessão criada pelo arranjo.
#[derive(Debug, Clone, Copy)]
struct StartSpec {
    category: Option<Category>,
    count: u32,
}

/// Sessão de uma pergunta com o texto fixo da fase 1.
const SINGLE_FIXED: StartSpec = StartSpec {
    category: Some(Category::ProblemSolving),
    count: 1,
};

/// Sessão com as duas perguntas fixas do arranjo, por qualquer ordem.
const PAIR: StartSpec = StartSpec {
    category: None,
    count: 2,
};

// ---------------------------------------------------------------------------
// Arranjo do serviço
// ---------------------------------------------------------------------------

struct Harness {
    _directory: tempfile::TempDir,
    legacy_root: PathBuf,
    artifacts_root: PathBuf,
    database: Database,
    audio: Arc<FakeAudio>,
    worker: Arc<FakeWorker>,
    launcher: Arc<FakeLauncher>,
    service: Arc<AppService>,
    snapshots: Arc<Mutex<Vec<InterviewSnapshot>>>,
}

impl Harness {
    /// Serviço com auxiliar ligado, sem verificação de disponibilidade e com uma
    /// sessão da pergunta fixa.
    async fn new() -> Self {
        Self::build(
            Arc::new(FakeLauncher::new(Arc::new(FakeWorker::default()))),
            None,
            Some(SINGLE_FIXED),
        )
        .await
    }

    /// Serviço com transcrição pronta: cada gravação publicada é transcrita.
    async fn with_transcriber() -> Self {
        Self::build(
            Arc::new(FakeLauncher::new(Arc::new(FakeWorker::default()))),
            Some(ComponentStatus::Ready),
            Some(SINGLE_FIXED),
        )
        .await
    }

    /// Serviço com transcrição pronta e duas perguntas na sessão.
    async fn with_transcriber_and_pair() -> Self {
        Self::build(
            Arc::new(FakeLauncher::new(Arc::new(FakeWorker::default()))),
            Some(ComponentStatus::Ready),
            Some(PAIR),
        )
        .await
    }

    /// Serviço com duas perguntas e sem transcrição automática.
    async fn with_pair() -> Self {
        Self::build(
            Arc::new(FakeLauncher::new(Arc::new(FakeWorker::default()))),
            None,
            Some(PAIR),
        )
        .await
    }

    /// Serviço cujo auxiliar nunca arranca (configuração ausente).
    async fn without_worker() -> Self {
        Self::build(
            Arc::new(FakeLauncher::failing(AppError::new(
                error_code::WORKER_UNAVAILABLE,
                "O auxiliar Python não está configurado.",
                false,
            ))),
            None,
            Some(SINGLE_FIXED),
        )
        .await
    }

    /// Serviço sem sessão iniciada: a interface ainda não pediu nenhuma.
    async fn without_session() -> Self {
        Self::build(
            Arc::new(FakeLauncher::new(Arc::new(FakeWorker::default()))),
            None,
            None,
        )
        .await
    }

    async fn build(
        launcher: Arc<FakeLauncher>,
        transcription: Option<ComponentStatus>,
        session: Option<StartSpec>,
    ) -> Self {
        let directory = tempfile::tempdir().expect("pasta temporária");
        let legacy_root = directory.path().join("phase1");
        let artifacts_root = directory.path().join("sessions");
        let database_path = directory.path().join("tutor.sqlite3");
        let whisper = directory.path().join("whisper");
        fs::create_dir_all(&whisper).expect("pasta dos modelos");
        fs::create_dir_all(&legacy_root).expect("pasta da fase 1");
        let manifest = directory.path().join("models.json");
        fs::write(
            &manifest,
            serde_json::to_vec(&json!({
                "schemaVersion": 1,
                "whisperDir": whisper.display().to_string(),
            }))
            .expect("codificar manifesto"),
        )
        .expect("escrever manifesto de modelos");

        // O arranjo abre a base primeiro (aplica o banco inicial) e reduz o
        // banco às perguntas fixas, para o conteúdo da sessão ser determinístico.
        let database = Database::open(database_path.clone()).expect("abrir a base de teste");
        keep_fixed_questions(&database);

        let worker = launcher.worker.clone();
        let audio = Arc::new(FakeAudio::default());
        let service = Arc::new(AppService::new(
            ServicePaths {
                database: database_path.clone(),
                legacy: legacy_root.clone(),
            },
            ArtifactStore::new(artifacts_root.clone()),
            audio.clone(),
            launcher.clone(),
            Some(manifest),
        ));
        let snapshots = Arc::new(Mutex::new(Vec::new()));
        let collected = snapshots.clone();
        service.subscribe(Arc::new(move |snapshot: &InterviewSnapshot| {
            collected
                .lock()
                .expect("guardar snapshots")
                .push(snapshot.clone());
        }));
        service.start().await;

        let harness = Self {
            _directory: directory,
            legacy_root,
            artifacts_root,
            database,
            audio,
            worker,
            launcher,
            service,
            snapshots,
        };

        if let Some(status) = transcription {
            health(&harness.service, status, ComponentStatus::Missing).await;
        }
        if let Some(spec) = session {
            harness.start_session(spec).await;
        }
        harness
    }

    async fn start_session(&self, spec: StartSpec) -> SessionDetail {
        let snapshot = self
            .service
            .start_session(StartSession {
                request_id: new_request_id(),
                category: spec.category,
                count: spec.count,
            })
            .await
            .expect("iniciar a sessão");
        self.detail_of(&snapshot)
    }

    fn snapshot(&self) -> InterviewSnapshot {
        self.service.snapshot()
    }

    fn session_id(&self) -> String {
        self.snapshot()
            .active_session
            .expect("a sessão está em curso")
            .id
    }

    fn current(&self) -> QuestionDetail {
        self.snapshot()
            .current
            .expect("a sessão tem pergunta atual")
    }

    /// Detalhe da sessão lido da base (a única autoridade).
    fn detail_of(&self, snapshot: &InterviewSnapshot) -> SessionDetail {
        let session_id = snapshot
            .active_session
            .as_ref()
            .expect("a sessão está em curso")
            .id
            .clone();
        self.database
            .load_session(&session_id)
            .expect("ler a sessão guardada")
    }

    async fn load_detail(&self, session_id: &str) -> SessionDetail {
        self.service
            .load_session(session_id.to_owned())
            .await
            .expect("abrir a sessão")
    }

    fn requests(&self) -> Vec<WorkerRequest> {
        self.worker.requests()
    }

    fn transcription_requests(&self) -> Vec<RecordedRequest> {
        self.requests()
            .iter()
            .filter(|request| request.op == WorkerOp::Transcribe)
            .map(|request| RecordedRequest {
                id: request.id.clone(),
                input_path: input_path(request),
            })
            .collect()
    }

    fn synthesis_request(&self) -> PathBuf {
        let request = self
            .requests()
            .into_iter()
            .rfind(|request| request.op == WorkerOp::Synthesize)
            .expect("pedido de síntese");
        PathBuf::from(
            request.params["outputPath"]
                .as_str()
                .expect("outputPath da síntese"),
        )
    }

    fn revisions(&self) -> Vec<u64> {
        self.snapshots
            .lock()
            .expect("ler snapshots")
            .iter()
            .map(|snapshot| snapshot.revision)
            .collect()
    }

    fn recording_dir(&self, current: &QuestionDetail, recording_id: &str) -> PathBuf {
        self.artifacts_root
            .join(&current.question.session_id)
            .join(&current.question.id)
            .join(recording_id)
    }

    fn original_path(&self, current: &QuestionDetail, recording_id: &str) -> PathBuf {
        self.recording_dir(current, recording_id)
            .join("original.wav")
    }

    fn normalized_path(&self, current: &QuestionDetail, recording_id: &str) -> PathBuf {
        self.recording_dir(current, recording_id)
            .join(NORMALIZED_FILE)
    }

    fn cache_path(&self, text: &str) -> PathBuf {
        self.legacy_root
            .join("question-cache")
            .join(format!("{}.wav", synthesis_cache_key(text)))
    }

    fn target_of(&self, current: &QuestionDetail) -> AnswerTarget {
        answer_target(current)
    }

    /// Grava e publica uma resposta na pergunta atual.
    async fn record_once(&self) -> InterviewSnapshot {
        let current = self.current();
        self.service
            .start_recording(
                current.question.id.clone(),
                current.question.selection_version,
            )
            .await
            .expect("começar a gravar");
        self.service
            .stop_recording()
            .await
            .expect("terminar a gravar")
    }

    /// Guarda a revisão manual de uma resposta, sem confirmar.
    async fn save_manual_review(&self, text: &str) -> InterviewSnapshot {
        let current = self.current();
        let target = self.target_of(&current);
        let expected = selected_recording(&current)
            .review
            .as_ref()
            .map_or(0, |review| review.version);
        self.service
            .save_review(SaveReview {
                request_id: new_request_id(),
                target,
                expected_review_version: expected,
                source_transcript_id: None,
                origin: ReviewOrigin::ManualExplicit,
                text: text.to_owned(),
            })
            .await
            .expect("guardar a revisão")
    }

    /// Confirma a resposta atual com o texto guardado na revisão.
    async fn confirm_current(&self, text: &str) -> InterviewSnapshot {
        let snapshot = self.snapshot();
        let current = snapshot
            .current
            .as_ref()
            .expect("a sessão tem pergunta atual");
        let target = self.target_of(current);
        let review_version = selected_recording(current)
            .review
            .as_ref()
            .expect("a revisão foi guardada")
            .version;
        let expected_session_version = snapshot
            .active_session
            .as_ref()
            .expect("sessão em curso")
            .version;
        self.service
            .confirm_answer(ConfirmAnswer {
                request_id: new_request_id(),
                target,
                expected_review_version: review_version,
                source_transcript_id: None,
                origin: ReviewOrigin::ManualExplicit,
                text: text.to_owned(),
                expected_session_version,
            })
            .await
            .expect("confirmar a resposta")
    }

    /// Confirma a pergunta atual e avança para a seguinte.
    async fn confirm_and_advance(&self, text: &str) -> InterviewSnapshot {
        self.save_manual_review(text).await;
        let confirmed = self.confirm_current(text).await;
        let session = confirmed
            .active_session
            .as_ref()
            .expect("sessão em curso depois de confirmar");
        let current = confirmed.current.as_ref().expect("pergunta confirmada");
        self.service
            .advance_session(AdvanceSession {
                request_id: new_request_id(),
                session_id: session.id.clone(),
                session_question_id: current.question.id.clone(),
                expected_session_version: session.version,
            })
            .await
            .expect("avançar a sessão")
    }
}

/// Reduz o banco inicial às duas perguntas fixas do arranjo.
///
/// A base é temporária e nenhuma sessão existe ainda; a pergunta fixa da fase 1
/// fica com o texto exato usado na chave de síntese.
fn keep_fixed_questions(database: &Database) {
    let kept = database
        .with_connection(|connection| {
            connection
                .execute(
                    "DELETE FROM questions WHERE text NOT IN (?1, ?2)",
                    params![QUESTION, SECOND_QUESTION],
                )
                .map_err(|error| AppError::new("TEST_SQLITE", error.to_string(), false))?;
            connection
                .query_row("SELECT count(*) FROM questions", [], |row| {
                    row.get::<_, i64>(0)
                })
                .map_err(|error| AppError::new("TEST_SQLITE", error.to_string(), false))
        })
        .expect("reduzir o banco às perguntas fixas");
    assert_eq!(kept, 2, "o banco de teste tem de ficar com duas perguntas");
}

fn last_health_request(harness: &Harness) -> String {
    harness
        .requests()
        .into_iter()
        .rfind(|request| request.op == WorkerOp::Health)
        .expect("pedido de disponibilidade")
        .id
}

/// Injeta o resultado `health` do handshake do auxiliar.
async fn health(service: &AppService, transcription: ComponentStatus, synthesis: ComponentStatus) {
    service
        .on_worker_event(WorkerEvent {
            generation: 1,
            message: WorkerMessage::Completed {
                request_id: "worker-health-1".to_owned(),
                result: WorkerResult::Health(HealthResult {
                    transcription: ComponentState {
                        status: transcription,
                        message: None,
                    },
                    synthesis: ComponentState {
                        status: synthesis,
                        message: None,
                    },
                }),
            },
        })
        .await;
}

fn codes(snapshot: &InterviewSnapshot) -> Vec<String> {
    snapshot
        .warnings
        .iter()
        .chain(snapshot.last_error.iter())
        .map(|error| error.code.clone())
        .collect()
}

/// Identificadores das transcrições de uma pergunta da sessão.
fn transcript_ids(current: &QuestionDetail) -> Vec<String> {
    current
        .recordings
        .iter()
        .flat_map(|recording| recording.transcripts.iter())
        .map(|transcript| transcript.id.clone())
        .collect()
}

fn unique_ids(values: impl IntoIterator<Item = String>) -> HashSet<String> {
    values.into_iter().collect()
}

// ---------------------------------------------------------------------------
// Abertura da base e arranque
// ---------------------------------------------------------------------------

#[tokio::test]
async fn database_error_keeps_window_recoverable() {
    // A pasta da base não existe: a abertura falha, mas a janela continua
    // utilizável e a fotografia diz exatamente isso.
    let directory = tempfile::tempdir().expect("pasta temporária");
    let legacy_root = directory.path().join("phase1");
    fs::create_dir_all(&legacy_root).expect("pasta da fase 1");
    let database_path = directory.path().join("missing").join("tutor.sqlite3");

    let worker = Arc::new(FakeWorker::default());
    let launcher = Arc::new(FakeLauncher::new(worker.clone()));
    let audio = Arc::new(FakeAudio::default());
    let service = Arc::new(AppService::new(
        ServicePaths {
            database: database_path.clone(),
            legacy: legacy_root,
        },
        ArtifactStore::new(directory.path().join("sessions")),
        audio.clone(),
        launcher,
        None,
    ));

    let started = service.start().await;
    assert_eq!(started.database, DatabaseState::Error);
    assert_eq!(
        started.last_error.expect("erro visível").code,
        error_code::DATABASE_UNAVAILABLE
    );
    assert!(started.active_session.is_none());
    assert_eq!(
        started.availability.microphone.status,
        ComponentStatus::Ready,
        "a disponibilidade do áudio continua a ser amostrada"
    );

    // Nenhuma sessão pode ser iniciada como se estivesse guardada.
    let error = service
        .start_session(StartSession {
            request_id: new_request_id(),
            category: None,
            count: 1,
        })
        .await
        .expect_err("sem base não se inicia uma sessão");
    assert_eq!(error.code, error_code::DATABASE_UNAVAILABLE);
    assert_eq!(service.snapshot().database, DatabaseState::Error);

    // Criada a pasta, repetir a abertura recupera a mesma janela.
    let retried = tokio::time::timeout(Duration::from_secs(5), async {
        fs::create_dir_all(database_path.parent().expect("pasta da base"))
            .expect("criar a pasta da base");
        service.retry_database().await
    })
    .await
    .expect("a abertura recuperável não fica bloqueada")
    .expect("a base abre à segunda tentativa");

    assert_eq!(retried.database, DatabaseState::Ready);
    assert!(retried.last_error.is_none());
    let questions = service
        .list_questions(None)
        .await
        .expect("listar o banco inicial");
    assert_eq!(questions.len(), 20, "o banco inicial é aplicado ao abrir");
}

// ---------------------------------------------------------------------------
// Captura
// ---------------------------------------------------------------------------

#[tokio::test]
async fn can_record_and_play_with_worker_missing() {
    let harness = Harness::without_worker().await;

    // O componente em falta é indicado, sem impedir a captura.
    let snapshot = harness.snapshot();
    assert_eq!(
        snapshot.availability.transcription.status,
        ComponentStatus::Missing
    );
    assert!(snapshot.availability.transcription.message.is_some());

    let current = harness.current();
    let snapshot = harness.record_once().await;
    let recording = selected_recording(snapshot.current.as_ref().expect("pergunta atual"));
    assert!(snapshot.recording_id.is_none());
    assert!(harness.requests().is_empty(), "nada é enviado ao auxiliar");
    assert!(
        recording.normalized_file.is_some(),
        "o derivado é produzido na publicação, sem depender do auxiliar"
    );
    assert!(
        harness.normalized_path(&current, &recording.id).is_file(),
        "o derivado de transcrição fica na pasta gerida"
    );
    assert!(
        harness.original_path(&current, &recording.id).is_file(),
        "o original é guardado na pasta gerida"
    );
    assert!(recording.capture.as_ref().expect("metadados").duration_ms > 0);

    // O áudio guardado continua a poder ser ouvido.
    let snapshot = harness
        .service
        .play_audio(AudioSelection::Recording {
            recording_id: recording.id.clone(),
        })
        .await
        .expect("ouvir sem auxiliar");
    assert_eq!(snapshot.playback, Some(PlaybackTarget::Response));
    assert_eq!(
        harness.audio.played(),
        vec![harness.original_path(&current, &recording.id)]
    );
}

#[tokio::test]
async fn legacy_attempt_audio_is_read_only() {
    let harness = Harness::new().await;
    let attempt_id = "8a1f4c78-2a4b-4f0e-9b6b-1e6b0f2e4d11";
    let directory = harness.legacy_root.join("attempts").join(attempt_id);
    fs::create_dir_all(&directory).expect("pasta do treino antigo");
    let original = directory.join("original.wav");
    write_rate_wav(&original, 48_000);

    let snapshot = harness
        .service
        .play_audio(AudioSelection::Legacy {
            attempt_id: attempt_id.to_owned(),
        })
        .await
        .expect("ouvir um treino da fase 1");
    assert_eq!(snapshot.playback, Some(PlaybackTarget::Response));
    assert_eq!(harness.audio.played(), vec![original.clone()]);
    assert!(
        original.is_file(),
        "a consulta não altera o ficheiro antigo"
    );

    // Uma referência desconhecida não é inventada nem sai da pasta da fase 1.
    let error = harness
        .service
        .play_audio(AudioSelection::Legacy {
            attempt_id: "../fora".to_owned(),
        })
        .await
        .expect_err("um identificador inválido é recusado");
    assert_eq!(error.code, error_code::ARTIFACT_MISSING);
}

#[tokio::test]
async fn failed_stop_keeps_previous_recording() {
    let harness = Harness::new().await;
    let first = harness.record_once().await;
    let first_current = first.current.clone().expect("pergunta atual");
    let previous_id = selected_recording(&first_current).id.clone();

    harness.audio.fail_next_stop();
    let current = harness.current();
    harness
        .service
        .start_recording(
            current.question.id.clone(),
            current.question.selection_version,
        )
        .await
        .expect("segunda gravação");
    let interrupted_id = harness
        .snapshot()
        .recording_id
        .clone()
        .expect("gravação em curso");
    let error = harness
        .service
        .stop_recording()
        .await
        .expect_err("a paragem falha");

    assert_eq!(error.code, error_code::CAPTURE_INTERRUPTED);
    let snapshot = harness.snapshot();
    assert_eq!(
        snapshot
            .current
            .as_ref()
            .and_then(|current| current.question.selected_recording_id.clone()),
        Some(previous_id),
        "a gravação anterior mantém-se selecionada"
    );
    assert!(snapshot.recording_id.is_none());
    assert_eq!(
        snapshot.last_error.expect("erro visível").code,
        error_code::CAPTURE_INTERRUPTED
    );

    // Enquanto o parcial recuperável não chega, gravar por cima perdia áudio.
    let current = harness.current();
    let busy = harness
        .service
        .start_recording(
            current.question.id.clone(),
            current.question.selection_version,
        )
        .await
        .expect_err("a recuperação está pendente");
    assert_eq!(busy.code, error_code::BUSY);

    harness
        .service
        .on_audio_event(AudioEvent::CaptureInterrupted {
            recording_id: Some(interrupted_id),
            capture: None,
            error: AppError::new(
                error_code::CAPTURE_INTERRUPTED,
                "Não há áudio recuperável desta gravação.",
                true,
            ),
        })
        .await;
    let current = harness.current();
    harness
        .service
        .start_recording(
            current.question.id.clone(),
            current.question.selection_version,
        )
        .await
        .expect("a captura seguinte não fica bloqueada");
    assert!(harness.snapshot().recording_id.is_some());
}

#[tokio::test]
async fn repeated_start_returns_busy_instead_of_blocking() {
    let harness = Harness::new().await;
    let current = harness.current();
    harness
        .service
        .start_recording(
            current.question.id.clone(),
            current.question.selection_version,
        )
        .await
        .expect("primeira gravação");
    let recording_id = harness
        .snapshot()
        .recording_id
        .clone()
        .expect("gravação em curso");

    // O mutex do estado não é reentrante: se a guarda não cair antes de
    // `record_error`, o pedido repetido fica bloqueado para sempre.
    let second = tokio::time::timeout(
        Duration::from_secs(2),
        harness.service.start_recording(
            current.question.id.clone(),
            current.question.selection_version,
        ),
    )
    .await
    .expect("o pedido repetido devolve em vez de bloquear")
    .expect_err("a gravação em curso impede outra");
    assert_eq!(second.code, error_code::BUSY);
    assert_eq!(
        harness.snapshot().recording_id.as_deref(),
        Some(recording_id.as_str()),
        "a gravação em curso mantém-se"
    );

    // E o serviço continua a responder aos restantes comandos.
    let snapshot = tokio::time::timeout(Duration::from_secs(2), harness.service.stop_recording())
        .await
        .expect("o serviço não ficou preso")
        .expect("terminar a gravação");
    assert!(snapshot.recording_id.is_none());
}

#[tokio::test]
async fn late_interruption_from_an_older_capture_is_ignored() {
    let harness = Harness::new().await;
    let current = harness.current();
    harness
        .service
        .start_recording(
            current.question.id.clone(),
            current.question.selection_version,
        )
        .await
        .expect("primeira gravação");
    let first_id = harness
        .snapshot()
        .recording_id
        .clone()
        .expect("gravação em curso");
    harness
        .service
        .stop_recording()
        .await
        .expect("paragem normal");

    let current = harness.current();
    harness
        .service
        .start_recording(
            current.question.id.clone(),
            current.question.selection_version,
        )
        .await
        .expect("nova gravação");
    let second_id = harness
        .snapshot()
        .recording_id
        .clone()
        .expect("nova gravação em curso");

    // Interrupção atrasada da gravação anterior: não pode retirar o botão de
    // terminar à gravação que está a decorrer agora.
    harness
        .service
        .on_audio_event(AudioEvent::CaptureInterrupted {
            recording_id: Some(first_id),
            capture: None,
            error: AppError::new(
                error_code::CAPTURE_INTERRUPTED,
                "Interrupção atrasada da gravação anterior.",
                true,
            ),
        })
        .await;

    let snapshot = harness.snapshot();
    assert_eq!(
        snapshot.recording_id.as_deref(),
        Some(second_id.as_str()),
        "a gravação atual continua a poder ser terminada"
    );
    assert!(
        snapshot.last_error.is_none(),
        "um evento de outra gravação não altera o erro visível"
    );
}

#[tokio::test]
async fn timed_out_stop_recovers_capture() {
    let harness = Harness::new().await;
    let current = harness.current();
    harness
        .service
        .start_recording(
            current.question.id.clone(),
            current.question.selection_version,
        )
        .await
        .expect("gravar");
    let recording_id = harness
        .snapshot()
        .recording_id
        .expect("identificador da gravação");

    harness.audio.fail_next_stop_timeout();
    let error = harness
        .service
        .stop_recording()
        .await
        .expect_err("a paragem expira");
    assert_eq!(error.code, error_code::AUDIO_TIMEOUT);

    // A thread pode ter fechado o original depois do prazo: é publicado como
    // recuperável e o estado deixa de esperar um evento que não chega.
    let snapshot = harness.snapshot();
    assert!(snapshot.recording_id.is_none());
    let current = snapshot.current.clone().expect("pergunta atual");
    let recording = selected_recording(&current);
    assert_eq!(recording.id, recording_id);
    assert!(
        recording.capture.as_ref().expect("metadados").incomplete,
        "a captura recuperada é marcada incompleta"
    );
    assert!(harness.original_path(&current, &recording_id).is_file());
    assert_eq!(
        snapshot.last_error.expect("erro visível").code,
        error_code::AUDIO_TIMEOUT
    );

    // Sem a limpeza da captura em curso, gravar ficava bloqueado.
    harness
        .service
        .start_recording(
            current.question.id.clone(),
            current.question.selection_version,
        )
        .await
        .expect("gravar outra resposta");
    harness
        .service
        .stop_recording()
        .await
        .expect("terminar a segunda gravação");

    // O mesmo timeout ao fechar não pode deixar o encerramento bloqueado.
    let current = harness.current();
    harness
        .service
        .start_recording(
            current.question.id.clone(),
            current.question.selection_version,
        )
        .await
        .expect("terceira gravação");
    harness.audio.fail_next_stop_timeout();
    let close_error = harness
        .service
        .prepare_close()
        .await
        .expect_err("o fecho falha no timeout");
    assert_eq!(close_error.code, error_code::AUDIO_TIMEOUT);
    assert!(harness.snapshot().current.is_some());
    harness
        .service
        .prepare_close()
        .await
        .expect("o fecho seguinte conclui");
}

#[tokio::test]
async fn save_publication_can_retry() {
    let harness = Harness::new().await;
    let first = harness.record_once().await;
    let first_current = first.current.clone().expect("pergunta atual");
    let previous_id = selected_recording(&first_current).id.clone();

    let current = harness.current();
    harness
        .service
        .start_recording(
            current.question.id.clone(),
            current.question.selection_version,
        )
        .await
        .expect("segunda gravação");
    let pending_id = harness
        .snapshot()
        .recording_id
        .expect("identificador da segunda gravação");

    // O original finalizado fica sem amostras: a publicação falha e nada é
    // perdido, mas também não se anuncia uma resposta guardada.
    harness.audio.corrupt_next_stop();
    let error = harness
        .service
        .stop_recording()
        .await
        .expect_err("a publicação falha");
    assert_eq!(error.code, error_code::INVALID_AUDIO);
    let snapshot = harness.snapshot();
    assert_eq!(
        snapshot
            .current
            .as_ref()
            .unwrap()
            .question
            .selected_recording_id,
        Some(previous_id)
    );
    assert!(snapshot.recording_id.is_none());
    assert_eq!(
        snapshot.last_error.expect("erro visível").code,
        error_code::INVALID_AUDIO
    );

    // Uma gravação por publicar não pode ser esquecida nem fechar em silêncio.
    let current = harness.current();
    assert!(
        harness
            .service
            .start_recording(
                current.question.id.clone(),
                current.question.selection_version
            )
            .await
            .is_err(),
        "uma gravação por publicar não pode ser esquecida"
    );
    assert_eq!(
        harness
            .service
            .prepare_close()
            .await
            .expect_err("o fecho não promete sucesso")
            .code,
        error_code::STORAGE_WRITE_FAILED
    );

    // Reparado o ficheiro, a mesma gravação é publicada sem nova captura.
    let current = harness.current();
    write_rate_wav(&harness.original_path(&current, &pending_id), 48_000);
    let retried = harness
        .service
        .retry_save_recording(pending_id.clone())
        .await
        .expect("repetir a publicação");
    assert_eq!(
        retried
            .current
            .as_ref()
            .and_then(|current| current.question.selected_recording_id.clone()),
        Some(pending_id.clone())
    );
    let current = retried.current.expect("pergunta atual");
    assert_eq!(
        current
            .recordings
            .iter()
            .find(|recording| recording.id == pending_id)
            .expect("gravação publicada")
            .state
            .as_str(),
        "ready"
    );
    assert!(harness.original_path(&current, &pending_id).is_file());
}

#[tokio::test]
async fn recording_requires_active_unconfirmed_question() {
    // Sem sessão em curso não há pergunta para gravar.
    let harness = Harness::without_session().await;
    let error = harness
        .service
        .start_recording("unknown-question".to_owned(), 0)
        .await
        .expect_err("sem sessão não se grava");
    assert_eq!(error.code, error_code::STALE_SOURCE);
    assert!(harness.snapshot().recording_id.is_none());
    assert!(harness.audio.played().is_empty());

    // Com sessão, a versão de seleção esperada tem de ser a guardada.
    let harness = Harness::new().await;
    let current = harness.current();
    let error = harness
        .service
        .start_recording(current.question.id.clone(), 99)
        .await
        .expect_err("uma seleção desatualizada é recusada");
    assert_eq!(error.code, error_code::STALE_SOURCE);
    assert!(harness.snapshot().recording_id.is_none());

    // E uma pergunta que não pertence à sessão também não é um alvo.
    let error = harness
        .service
        .start_recording(
            "not-in-session".to_owned(),
            current.question.selection_version,
        )
        .await
        .expect_err("uma pergunta alheia é recusada");
    assert_eq!(error.code, error_code::STALE_SOURCE);
    assert!(harness.snapshot().recording_id.is_none());
}

#[tokio::test]
async fn confirmed_question_rejects_capture_and_transcription() {
    let harness = Harness::new().await;
    harness.record_once().await;
    let text = "I prioritised the release and told the team early.";
    harness.save_manual_review(text).await;
    let confirmed = harness.confirm_current(text).await;
    let current = confirmed.current.clone().expect("pergunta confirmada");
    assert!(current.confirmation.is_some());
    let target = harness.target_of(&current);
    let session_question_id = current.question.id.clone();
    let selection_version = current.question.selection_version;

    let capture = harness
        .service
        .start_recording(session_question_id, selection_version)
        .await
        .expect_err("uma resposta confirmada não volta a ser gravada");
    assert_eq!(capture.code, error_code::ANSWER_CONFIRMED);
    assert!(harness.snapshot().recording_id.is_none());

    let transcription = harness
        .service
        .transcribe_response(target)
        .await
        .expect_err("uma resposta confirmada não volta a ser transcrita");
    assert_eq!(transcription.code, error_code::ANSWER_CONFIRMED);
    assert_eq!(
        harness.snapshot().current.unwrap().recordings.len(),
        1,
        "nada é acrescentado à pergunta confirmada"
    );
}

// ---------------------------------------------------------------------------
// Transcrição
// ---------------------------------------------------------------------------

#[tokio::test]
async fn retry_transcription_reuses_audio() {
    let harness = Harness::with_transcriber().await;
    let snapshot = harness.record_once().await;
    let current = snapshot.current.clone().expect("pergunta atual");
    let recording = selected_recording(&current).clone();

    let first = harness
        .transcription_requests()
        .into_iter()
        .next()
        .expect("transcrição automática");
    let normalized = harness.normalized_path(&current, &recording.id);
    assert_eq!(first.input_path, normalized);
    let expected_reference = format!(
        "{}/{}/{}/{}",
        current.question.session_id, current.question.id, recording.id, NORMALIZED_FILE
    );
    assert_eq!(
        recording.normalized_file.as_deref(),
        Some(expected_reference.as_str()),
        "a referência do derivado fica guardada"
    );

    // O auxiliar perde o pedido: a resposta e o derivado têm de continuar.
    harness
        .service
        .on_worker_event(WorkerEvent {
            generation: 1,
            message: WorkerMessage::Failed {
                request_id: first.id.clone(),
                error: AppError::new(
                    error_code::WORKER_TIMEOUT,
                    "O auxiliar não devolveu o resultado dentro do prazo.",
                    true,
                ),
            },
        })
        .await;
    let failed = harness.snapshot();
    assert!(failed.model_job.is_none());
    assert!(normalized.is_file());
    assert!(transcript_ids(&harness.current()).is_empty());

    let after = harness
        .service
        .transcribe_response(harness.target_of(&harness.current()))
        .await
        .expect("repetir transcrição");

    let requests = harness.transcription_requests();
    assert_eq!(requests.len(), 2);
    let retry = &requests[1];
    assert_eq!(retry.input_path, normalized);
    assert_ne!(retry.id, first.id, "o identificador tem de ser novo");
    assert!(after.model_job.is_some());

    let revisions = harness.revisions();
    assert!(
        revisions.windows(2).all(|pair| pair[0] < pair[1]),
        "revisões não crescem: {revisions:?}"
    );
}

#[tokio::test]
async fn late_result_does_not_change_confirmed_text() {
    let harness = Harness::with_transcriber().await;
    harness.record_once().await;
    let request = harness
        .transcription_requests()
        .into_iter()
        .next()
        .expect("transcrição automática");

    harness
        .service
        .on_worker_event(WorkerEvent {
            generation: 1,
            message: WorkerMessage::Completed {
                request_id: request.id.clone(),
                result: transcription_result("machine words"),
            },
        })
        .await;

    let transcribed = harness.current();
    let recording = selected_recording(&transcribed).clone();
    assert_eq!(recording.transcripts.len(), 1);
    assert_eq!(
        recording.review.as_ref().expect("revisão inicial").text,
        "machine words"
    );
    let source_id = recording.transcripts[0].id.clone();

    let target = harness.target_of(&transcribed);
    harness
        .service
        .save_review(SaveReview {
            request_id: new_request_id(),
            target,
            expected_review_version: recording.review.as_ref().unwrap().version,
            source_transcript_id: Some(source_id.clone()),
            origin: ReviewOrigin::ManualExplicit,
            text: "my words".to_owned(),
        })
        .await
        .expect("guardar revisão");
    assert_eq!(
        selected_recording(&harness.current())
            .review
            .as_ref()
            .unwrap()
            .text,
        "my words"
    );

    // Resultado atrasado de outra geração do auxiliar.
    harness
        .service
        .on_worker_event(WorkerEvent {
            generation: 2,
            message: WorkerMessage::Completed {
                request_id: request.id.clone(),
                result: transcription_result("late words"),
            },
        })
        .await;
    let after = harness.current();
    assert_eq!(
        selected_recording(&after).review.as_ref().unwrap().text,
        "my words"
    );
    assert_eq!(transcript_ids(&after).len(), 1);

    // Resultado atrasado de um pedido que já não está pendente.
    harness
        .service
        .on_worker_event(WorkerEvent {
            generation: 1,
            message: WorkerMessage::Completed {
                request_id: "req-late".to_owned(),
                result: transcription_result("late words"),
            },
        })
        .await;
    assert_eq!(
        selected_recording(&harness.current())
            .review
            .as_ref()
            .unwrap()
            .text,
        "my words"
    );
}

#[tokio::test]
async fn retranscribe_preserves_existing_review() {
    let harness = Harness::with_transcriber().await;
    harness.record_once().await;
    let first_request = harness
        .transcription_requests()
        .into_iter()
        .next()
        .expect("transcrição automática");

    harness
        .service
        .on_worker_event(WorkerEvent {
            generation: 1,
            message: WorkerMessage::Completed {
                request_id: first_request.id.clone(),
                result: transcription_result("first transcript"),
            },
        })
        .await;

    let current = harness.current();
    let source_id = selected_recording(&current).transcripts[0].id.clone();
    let target = harness.target_of(&current);
    let review_version = selected_recording(&current)
        .review
        .as_ref()
        .unwrap()
        .version;
    harness
        .service
        .save_review(SaveReview {
            request_id: new_request_id(),
            target: target.clone(),
            expected_review_version: review_version,
            source_transcript_id: Some(source_id.clone()),
            origin: ReviewOrigin::ManualExplicit,
            text: "edited by me".to_owned(),
        })
        .await
        .expect("guardar revisão");

    harness
        .service
        .transcribe_response(harness.target_of(&harness.current()))
        .await
        .expect("transcrever novamente");
    let requests = harness.transcription_requests();
    assert_eq!(requests.len(), 2);
    let second_request = requests.last().expect("segundo pedido");
    assert_ne!(second_request.id, first_request.id);

    harness
        .service
        .on_worker_event(WorkerEvent {
            generation: 1,
            message: WorkerMessage::Completed {
                request_id: second_request.id.clone(),
                result: transcription_result("second transcript"),
            },
        })
        .await;

    let after = harness.current();
    let recording = selected_recording(&after);
    assert_eq!(recording.transcripts.len(), 2);
    assert_eq!(recording.transcripts[0].text, "first transcript");
    assert_eq!(recording.transcripts[1].text, "second transcript");
    let review = recording.review.as_ref().expect("revisão preservada");
    assert_eq!(review.text, "edited by me");
    assert_eq!(review.source_transcript_id, Some(source_id));
    assert_eq!(pending_jobs(&harness.snapshot()), 0);
}

#[tokio::test]
async fn no_speech_result_is_explicit_and_creates_no_review() {
    let harness = Harness::with_transcriber().await;
    harness.record_once().await;
    let request = harness
        .transcription_requests()
        .into_iter()
        .next()
        .expect("transcrição automática");

    harness
        .service
        .on_worker_event(WorkerEvent {
            generation: 1,
            message: WorkerMessage::Completed {
                request_id: request.id,
                result: transcription_result(""),
            },
        })
        .await;

    let after = harness.snapshot();
    let current = after.current.clone().expect("pergunta atual");
    let recording = selected_recording(&current).clone();
    assert_eq!(recording.transcripts.len(), 1);
    assert_eq!(recording.transcripts[0].status, TranscriptStatus::NoSpeech);
    assert!(recording.transcripts[0].text.is_empty());
    assert!(recording.review.is_none(), "ausência de fala não cria nota");
    assert!(after.model_job.is_none());

    // A resposta continua a poder ser ouvida e substituída por outra gravação.
    let playing = harness
        .service
        .play_audio(AudioSelection::Recording {
            recording_id: recording.id.clone(),
        })
        .await
        .expect("ouvir a resposta sem fala");
    assert_eq!(playing.playback, Some(PlaybackTarget::Response));
    harness.service.stop_audio().await.expect("parar");
    harness.record_once().await;
}

#[tokio::test]
async fn history_read_does_not_retarget_model_job() {
    let harness = Harness::with_transcriber().await;
    harness.record_once().await;
    let running = harness.snapshot();
    let job = running.model_job.clone().expect("transcrição em curso");
    assert_eq!(job.kind, OperationKind::Transcribe);
    let session_id = harness.session_id();

    // Consultar o histórico durante a inferência não muda a identidade do
    // trabalho nem inicia outro.
    let detail = harness.load_detail(&session_id).await;
    assert_eq!(detail.questions.len(), 1);
    harness
        .service
        .list_sessions()
        .await
        .expect("listar sessões");
    harness
        .service
        .list_questions(None)
        .await
        .expect("listar perguntas");

    let after_reads = harness.snapshot();
    assert_eq!(
        after_reads.model_job.as_ref().map(|job| job.target.clone()),
        Some(job.target.clone()),
        "o alvo do trabalho não é retargetado por uma leitura"
    );

    // E o resultado do pedido original continua a ser aplicado ao alvo certo.
    harness
        .service
        .on_worker_event(WorkerEvent {
            generation: 1,
            message: WorkerMessage::Completed {
                request_id: job.id,
                result: transcription_result("a stored answer"),
            },
        })
        .await;
    let after = harness.current();
    let recording = selected_recording(&after);
    assert_eq!(recording.transcripts.len(), 1);
    assert_eq!(recording.transcripts[0].text, "a stored answer");
    assert!(harness.snapshot().model_job.is_none());
}

#[tokio::test]
async fn late_worker_result_cannot_target_next_question() {
    let harness = Harness::with_transcriber_and_pair().await;
    harness.record_once().await;
    let first = harness.current();
    let first_id = first.question.id.clone();
    let request = harness
        .transcription_requests()
        .into_iter()
        .next()
        .expect("transcrição automática");

    // Confirmar e avançar enquanto a transcrição ainda está em curso.
    let text = "We reduced the scope and delivered the important part.";
    let advanced = harness.confirm_and_advance(text).await;
    let next = advanced.current.clone().expect("pergunta seguinte");
    assert_eq!(advanced.active_session.as_ref().unwrap().position, 2);
    assert_ne!(next.question.id, first_id);
    assert!(transcript_ids(&next).is_empty());

    // Um resultado tardio da pergunta anterior não pode escrever na nova.
    harness
        .service
        .on_worker_event(WorkerEvent {
            generation: 1,
            message: WorkerMessage::Completed {
                request_id: request.id,
                result: transcription_result("late words for the old question"),
            },
        })
        .await;

    let after = harness.snapshot();
    let current = after.current.clone().expect("pergunta seguinte");
    assert_eq!(current.question.id, next.question.id);
    assert!(
        transcript_ids(&current).is_empty(),
        "o resultado tardio não é escrito na pergunta seguinte"
    );
    assert!(after.model_job.is_none(), "o trabalho é libertado");
    assert_eq!(
        after
            .active_session
            .as_ref()
            .expect("sessão em curso")
            .position,
        2
    );

    // A pergunta anterior também não recebe o resultado: já está confirmada.
    let detail = harness.load_detail(&harness.session_id()).await;
    let confirmed_question = detail
        .questions
        .iter()
        .find(|question| question.question.id == first_id)
        .expect("pergunta confirmada");
    assert!(confirmed_question.confirmation.is_some());
    assert_eq!(
        confirmed_question.confirmation.as_ref().unwrap().text,
        text,
        "o texto confirmado não é alterado"
    );
    assert!(transcript_ids(confirmed_question).is_empty());
}

// ---------------------------------------------------------------------------
// Reprodução, disponibilidade e recuperação de áudio
// ---------------------------------------------------------------------------

#[tokio::test]
async fn playback_is_allowed_while_transcribing() {
    let harness = Harness::with_transcriber().await;
    let snapshot = harness.record_once().await;
    let current = snapshot.current.clone().expect("pergunta atual");
    let recording = selected_recording(&current).clone();
    let job = harness.snapshot().model_job.expect("transcrição em curso");
    assert_eq!(job.kind, OperationKind::Transcribe);

    // Transcrever em CPU é a espera mais longa: ouvir a resposta é permitido.
    let playing = harness
        .service
        .play_audio(AudioSelection::Recording {
            recording_id: recording.id.clone(),
        })
        .await
        .expect("ouvir a resposta durante a transcrição");
    assert_eq!(playing.playback, Some(PlaybackTarget::Response));
    assert!(playing.model_job.is_some());

    // A pergunta já em cache também substitui a resposta a meio da transcrição.
    let cache = harness.cache_path(&current.question.text);
    fs::create_dir_all(cache.parent().expect("pasta da cache")).expect("criar a cache");
    write_rate_wav(&cache, 24_000);

    let question = harness
        .service
        .play_audio(AudioSelection::Question {
            session_id: current.question.session_id.clone(),
            session_question_id: current.question.id.clone(),
        })
        .await
        .expect("ouvir a pergunta em cache");
    assert_eq!(question.playback, Some(PlaybackTarget::Question));
    assert_eq!(
        harness.audio.played(),
        vec![harness.original_path(&current, &recording.id), cache],
        "a reprodução da pergunta substitui a da resposta"
    );
}

#[tokio::test]
async fn playback_is_refused_while_recording() {
    let harness = Harness::new().await;
    let current = harness.current();
    harness
        .service
        .start_recording(
            current.question.id.clone(),
            current.question.selection_version,
        )
        .await
        .expect("começar a gravar");

    let error = harness
        .service
        .play_audio(AudioSelection::Question {
            session_id: current.question.session_id.clone(),
            session_question_id: current.question.id.clone(),
        })
        .await
        .expect_err("gravar impede reproduzir");
    assert_eq!(error.code, error_code::BUSY);
    assert!(harness.audio.played().is_empty());
}

#[tokio::test]
async fn starting_recording_stops_playback() {
    let harness = Harness::new().await;
    let snapshot = harness.record_once().await;
    let current = snapshot.current.clone().expect("pergunta atual");
    let recording = selected_recording(&current).clone();
    harness
        .service
        .play_audio(AudioSelection::Recording {
            recording_id: recording.id.clone(),
        })
        .await
        .expect("ouvir a resposta");
    let stops = harness.audio.playback_stops();

    // A spec manda terminar a reprodução ao começar a gravar, não exigir que o
    // utilizador pare o áudio primeiro.
    let recording_now = harness
        .service
        .start_recording(
            current.question.id.clone(),
            current.question.selection_version,
        )
        .await
        .expect("gravar com uma reprodução ativa");
    assert!(recording_now.recording_id.is_some());
    assert_eq!(recording_now.playback, None);
    assert!(
        harness.audio.playback_stops() > stops,
        "a reprodução em curso tem de ser parada"
    );
    assert_eq!(
        harness.audio.played(),
        vec![harness.original_path(&current, &recording.id)]
    );
}

#[tokio::test]
async fn question_audio_is_synthesized_and_validated() {
    let harness = Harness::with_transcriber().await;
    health(
        &harness.service,
        ComponentStatus::Ready,
        ComponentStatus::Ready,
    )
    .await;
    let current = harness.current();

    let requested = harness
        .service
        .play_audio(AudioSelection::Question {
            session_id: current.question.session_id.clone(),
            session_question_id: current.question.id.clone(),
        })
        .await
        .expect("pedir a síntese da pergunta");
    assert_eq!(requested.playback, None);
    let job = requested.model_job.expect("síntese em curso");
    assert_eq!(job.kind, OperationKind::Synthesize);

    let expected = harness.cache_path(QUESTION);
    assert_eq!(harness.synthesis_request(), expected);
    write_rate_wav(&expected, 24_000);
    harness
        .service
        .on_worker_event(WorkerEvent {
            generation: 1,
            message: WorkerMessage::Completed {
                request_id: job.id,
                result: synthesis_result(&expected),
            },
        })
        .await;
    let after = harness.snapshot();
    assert_eq!(after.playback, Some(PlaybackTarget::Question));
    assert!(after.model_job.is_none());
    assert_eq!(harness.audio.played(), vec![expected.clone()]);

    // Um WAV fora do formato esperado nunca é reproduzido.
    let harness = Harness::with_transcriber().await;
    health(
        &harness.service,
        ComponentStatus::Ready,
        ComponentStatus::Ready,
    )
    .await;
    let current = harness.current();
    harness
        .service
        .play_audio(AudioSelection::Question {
            session_id: current.question.session_id.clone(),
            session_question_id: current.question.id.clone(),
        })
        .await
        .expect("pedir a síntese da pergunta");
    let expected = harness.cache_path(QUESTION);
    let job = harness.snapshot().model_job.expect("síntese em curso");
    write_rate_wav(&expected, 48_000);
    harness
        .service
        .on_worker_event(WorkerEvent {
            generation: 1,
            message: WorkerMessage::Completed {
                request_id: job.id,
                result: synthesis_result(&expected),
            },
        })
        .await;

    let rejected = harness.snapshot();
    assert_eq!(rejected.playback, None);
    assert_eq!(
        rejected.last_error.expect("erro visível").code,
        error_code::INVALID_AUDIO
    );
    assert!(harness.audio.played().is_empty());
    assert!(
        !expected.exists(),
        "o derivado inválido é descartado antes de uma nova síntese"
    );
}

#[tokio::test]
async fn question_cache_distinguishes_session_texts() {
    let harness = Harness::with_pair().await;
    health(
        &harness.service,
        ComponentStatus::Missing,
        ComponentStatus::Ready,
    )
    .await;
    let detail = harness.load_detail(&harness.session_id()).await;
    assert_eq!(detail.questions.len(), 2);
    let first = detail.questions[0].question.clone();
    let second = detail.questions[1].question.clone();
    assert_ne!(
        first.text, second.text,
        "as duas perguntas da sessão têm textos distintos"
    );

    let requested = harness
        .service
        .play_audio(AudioSelection::Question {
            session_id: first.session_id.clone(),
            session_question_id: first.id.clone(),
        })
        .await
        .expect("sintetizar a primeira pergunta");
    let first_job = requested.model_job.expect("síntese em curso");
    let first_path = harness.synthesis_request();
    assert_eq!(first_path, harness.cache_path(&first.text));

    // O auxiliar perde o primeiro pedido: a cache da segunda pergunta é
    // independente do texto da primeira.
    harness
        .service
        .on_worker_event(WorkerEvent {
            generation: 1,
            message: WorkerMessage::Failed {
                request_id: first_job.id,
                error: AppError::new(
                    error_code::WORKER_TIMEOUT,
                    "O auxiliar não devolveu o resultado dentro do prazo.",
                    true,
                ),
            },
        })
        .await;
    harness
        .service
        .play_audio(AudioSelection::Question {
            session_id: second.session_id.clone(),
            session_question_id: second.id.clone(),
        })
        .await
        .expect("sintetizar a segunda pergunta");
    let second_path = harness.synthesis_request();
    assert_eq!(second_path, harness.cache_path(&second.text));
    assert_ne!(
        first_path, second_path,
        "cada texto da sessão tem a sua própria entrada de cache"
    );
    assert!(
        first_path.starts_with(harness.legacy_root.join("question-cache").as_path()),
        "a cache partilhada continua em phase1/question-cache"
    );
}

#[tokio::test]
async fn failed_health_check_reports_component_error_and_recovers() {
    let harness = Harness::new().await;

    let checking = harness
        .service
        .refresh_availability()
        .await
        .expect("verificar disponibilidade");
    assert_eq!(
        checking.availability.transcription.status,
        ComponentStatus::Loading
    );
    let first = last_health_request(&harness);

    // O auxiliar bloqueia e o supervisor falha o `health` no prazo: os
    // componentes não podem ficar em `loading` para sempre.
    harness
        .service
        .on_worker_event(WorkerEvent {
            generation: 1,
            message: WorkerMessage::Failed {
                request_id: first.clone(),
                error: AppError::new(
                    error_code::WORKER_TIMEOUT,
                    "O auxiliar não devolveu o resultado dentro do prazo.",
                    true,
                ),
            },
        })
        .await;

    let failed = harness.snapshot();
    assert_eq!(
        failed.availability.transcription.status,
        ComponentStatus::Error
    );
    assert_eq!(failed.availability.synthesis.status, ComponentStatus::Error);
    assert_eq!(
        failed.last_error.expect("erro visível").code,
        error_code::WORKER_TIMEOUT
    );
    assert!(failed.model_job.is_none());

    // Uma verificação posterior com sucesso volta a `ready`.
    harness
        .service
        .refresh_availability()
        .await
        .expect("repetir disponibilidade");
    let second = last_health_request(&harness);
    assert_ne!(second, first);
    harness
        .service
        .on_worker_event(WorkerEvent {
            generation: 1,
            message: WorkerMessage::Completed {
                request_id: second,
                result: WorkerResult::Health(HealthResult {
                    transcription: ComponentState::ready(),
                    synthesis: ComponentState::ready(),
                }),
            },
        })
        .await;

    let recovered = harness.snapshot();
    assert_eq!(
        recovered.availability.transcription.status,
        ComponentStatus::Ready
    );
    assert_eq!(
        recovered.availability.synthesis.status,
        ComponentStatus::Ready
    );
    assert!(
        recovered.last_error.is_none(),
        "a recuperação completa limpa o erro da verificação"
    );
}

#[tokio::test]
async fn busy_refresh_keeps_pending_health_tracking() {
    let harness = Harness::new().await;

    let first = harness
        .service
        .refresh_availability()
        .await
        .expect("primeira verificação");
    assert_eq!(
        first.availability.transcription.status,
        ComponentStatus::Loading
    );
    let pending = last_health_request(&harness);

    // Uma segunda verificação dentro da janela do pedido pendente recebe BUSY
    // e não pode substituir o identificador da verificação em curso.
    harness.worker.fail_next_submit_busy();
    let busy = harness
        .service
        .refresh_availability()
        .await
        .expect("verificação ocupada");
    assert_eq!(
        busy.availability.transcription.status,
        ComponentStatus::Loading
    );
    assert_eq!(
        harness
            .requests()
            .iter()
            .filter(|request| request.op == WorkerOp::Health)
            .count(),
        1,
        "o pedido recusado com BUSY não chega ao auxiliar"
    );

    // O timeout continua a corresponder ao pedido que ficou pendente.
    harness
        .service
        .on_worker_event(WorkerEvent {
            generation: 1,
            message: WorkerMessage::Failed {
                request_id: pending.clone(),
                error: AppError::new(
                    error_code::WORKER_TIMEOUT,
                    "O auxiliar não devolveu o resultado dentro do prazo.",
                    true,
                ),
            },
        })
        .await;

    let failed = harness.snapshot();
    assert_eq!(
        failed.availability.transcription.status,
        ComponentStatus::Error
    );
    assert_eq!(failed.availability.synthesis.status, ComponentStatus::Error);
    assert_eq!(
        failed.last_error.expect("erro visível").code,
        error_code::WORKER_TIMEOUT
    );

    // Uma verificação posterior recupera e limpa o erro.
    harness
        .service
        .refresh_availability()
        .await
        .expect("verificação de recuperação");
    let recovered_request = last_health_request(&harness);
    assert_ne!(recovered_request, pending);
    harness
        .service
        .on_worker_event(WorkerEvent {
            generation: 1,
            message: WorkerMessage::Completed {
                request_id: recovered_request,
                result: WorkerResult::Health(HealthResult {
                    transcription: ComponentState::ready(),
                    synthesis: ComponentState::ready(),
                }),
            },
        })
        .await;
    let recovered = harness.snapshot();
    assert_eq!(
        recovered.availability.transcription.status,
        ComponentStatus::Ready
    );
    assert_eq!(
        recovered.availability.synthesis.status,
        ComponentStatus::Ready
    );
    assert!(recovered.last_error.is_none());
}

#[tokio::test]
async fn failed_restart_discards_the_pending_health_handle() {
    let harness = Harness::new().await;
    harness
        .service
        .refresh_availability()
        .await
        .expect("verificação pendente");
    let pending = last_health_request(&harness);

    harness.worker.fail_next_restart();
    let error = harness
        .service
        .restart_worker()
        .await
        .expect_err("o reinício é recusado");
    assert_eq!(error.code, error_code::BUSY);
    let refused = harness.snapshot();
    assert_eq!(
        refused.availability.transcription.status,
        ComponentStatus::Error
    );
    assert_eq!(
        refused.last_error.as_ref().expect("erro visível").code,
        error_code::BUSY
    );

    // A falha tardia da verificação antiga não substitui o erro do reinício.
    harness
        .service
        .on_worker_event(WorkerEvent {
            generation: 1,
            message: WorkerMessage::Failed {
                request_id: pending,
                error: AppError::new(
                    error_code::WORKER_TIMEOUT,
                    "O auxiliar não devolveu o resultado dentro do prazo.",
                    true,
                ),
            },
        })
        .await;
    let after = harness.snapshot();
    assert_eq!(
        after.last_error.expect("erro do reinício").code,
        error_code::BUSY
    );
}

#[tokio::test]
async fn synthesis_completion_stops_current_playback() {
    let harness = Harness::new().await;
    health(
        &harness.service,
        ComponentStatus::Missing,
        ComponentStatus::Ready,
    )
    .await;
    let snapshot = harness.record_once().await;
    let current = snapshot.current.clone().expect("pergunta atual");
    let recording = selected_recording(&current).clone();
    harness
        .service
        .play_audio(AudioSelection::Recording {
            recording_id: recording.id.clone(),
        })
        .await
        .expect("ouvir a resposta");
    let stops = harness.audio.playback_stops();

    let requested = harness
        .service
        .play_audio(AudioSelection::Question {
            session_id: current.question.session_id.clone(),
            session_question_id: current.question.id.clone(),
        })
        .await
        .expect("pedir a síntese da pergunta");
    let job = requested.model_job.expect("síntese em curso");
    let expected = harness.cache_path(QUESTION);
    write_rate_wav(&expected, 24_000);

    harness
        .service
        .on_worker_event(WorkerEvent {
            generation: 1,
            message: WorkerMessage::Completed {
                request_id: job.id,
                result: synthesis_result(&expected),
            },
        })
        .await;

    let after = harness.snapshot();
    assert_eq!(after.playback, Some(PlaybackTarget::Question));
    assert!(after.model_job.is_none());
    assert!(
        harness.audio.playback_stops() > stops,
        "a reprodução automática da pergunta pára a resposta em curso"
    );
    assert_eq!(harness.audio.played().last(), Some(&expected));
}

#[tokio::test]
async fn restart_worker_resamples_audio_availability() {
    let harness = Harness::new().await;
    assert_eq!(
        harness.snapshot().availability.microphone.status,
        ComponentStatus::Ready
    );

    // O microfone desaparece e o painel não pode continuar a dizer «Disponível».
    harness.audio.set_microphone(ComponentState {
        status: ComponentStatus::Missing,
        message: Some("O microfone não está ligado.".to_owned()),
    });
    harness
        .service
        .restart_worker()
        .await
        .expect("reiniciar com o microfone em falta");
    let missing = harness.snapshot();
    assert_eq!(
        missing.availability.microphone.status,
        ComponentStatus::Missing
    );
    assert_eq!(harness.audio.reopens(), 1, "a porta de áudio é reaberta");

    // Restaurado, o painel volta a ficar correto sem reiniciar a aplicação.
    harness.audio.set_microphone(ComponentState::ready());
    harness.service.restart_worker().await.expect("reiniciar");
    assert_eq!(
        harness.snapshot().availability.microphone.status,
        ComponentStatus::Ready
    );
}

#[tokio::test]
async fn failed_start_resamples_microphone() {
    let harness = Harness::new().await;
    harness.audio.set_microphone(ComponentState {
        status: ComponentStatus::Missing,
        message: Some("O microfone não está ligado.".to_owned()),
    });
    harness.audio.fail_next_start(AppError::new(
        error_code::DEVICE_UNAVAILABLE,
        "O microfone foi desligado.",
        true,
    ));
    let current = harness.current();

    let error = harness
        .service
        .start_recording(
            current.question.id.clone(),
            current.question.selection_version,
        )
        .await
        .expect_err("o microfone não abre");
    assert_eq!(error.code, error_code::DEVICE_UNAVAILABLE);
    let after = harness.snapshot();
    assert_eq!(
        after.availability.microphone.status,
        ComponentStatus::Missing,
        "uma falha de dispositivo volta a amostrar a disponibilidade"
    );
    assert!(after.recording_id.is_none());
}

#[tokio::test]
async fn close_finalizes_capture_and_ignores_late_results() {
    // Uma captura ativa é finalizada e publicada antes de fechar.
    let harness = Harness::without_worker().await;
    let current = harness.current();
    harness
        .service
        .start_recording(
            current.question.id.clone(),
            current.question.selection_version,
        )
        .await
        .expect("começar a gravar");
    let closed = harness
        .service
        .prepare_close()
        .await
        .expect("fechar com captura ativa");
    assert_eq!(pending_jobs(&closed), 0);
    let current = closed.current.clone().expect("pergunta atual");
    let recording = selected_recording(&current).clone();
    assert!(closed.recording_id.is_none());
    assert!(closed.model_job.is_none());
    assert!(harness.original_path(&current, &recording.id).is_file());

    // Um trabalho em curso fica interrompido e o auxiliar é encerrado.
    let harness = Harness::with_transcriber().await;
    harness.record_once().await;
    let running = harness.snapshot();
    assert!(running.model_job.is_some(), "transcrição em curso");
    let request = harness
        .transcription_requests()
        .into_iter()
        .next()
        .expect("transcrição automática");
    let closed = harness
        .service
        .prepare_close()
        .await
        .expect("fechar com trabalho em curso");
    assert_eq!(pending_jobs(&closed), 0);
    assert!(closed.recording_id.is_none());
    assert!(closed.model_job.is_none());
    assert_eq!(harness.worker.shutdowns(), 1);
    assert_eq!(harness.launcher.launches.load(Ordering::SeqCst), 1);
    assert!(harness.audio.playback_stops() > 0);

    // Nenhum resultado tardio volta a ser aplicado depois do fecho.
    harness
        .service
        .on_worker_event(WorkerEvent {
            generation: 1,
            message: WorkerMessage::Completed {
                request_id: request.id,
                result: transcription_result("late after close"),
            },
        })
        .await;
    assert!(transcript_ids(&harness.current()).is_empty());
}

// ---------------------------------------------------------------------------
// Sessões e revisões
// ---------------------------------------------------------------------------

#[tokio::test]
async fn start_confirm_and_advance_persist_the_session() {
    let harness = Harness::with_pair().await;
    harness.record_once().await;
    let first = harness.current();
    assert_eq!(first.question.position, 1);
    assert_eq!(harness.snapshot().active_session.as_ref().unwrap().count, 2);

    let text = "I disagreed with the checklist and we agreed on a fix.";
    let advanced = harness.confirm_and_advance(text).await;
    let session = advanced.active_session.as_ref().expect("sessão");
    assert_eq!(session.position, 2);
    assert_eq!(session.confirmed_count, 1);
    assert_eq!(session.status, SessionStatus::InProgress);

    // A cópia confirmada fica fixa e o texto exato foi o que o utilizador viu.
    let detail = harness.load_detail(&session.id).await;
    let confirmation = detail.questions[0]
        .confirmation
        .as_ref()
        .expect("primeira pergunta confirmada");
    assert_eq!(confirmation.text, text);

    // A pergunta seguinte está livre para gravar e o histórico não a altera.
    let next = advanced.current.expect("pergunta seguinte");
    assert_eq!(next.question.position, 2);
    assert!(next.confirmation.is_none());
}

#[tokio::test]
async fn finishing_the_last_question_completes_the_session() {
    let harness = Harness::new().await;
    harness.record_once().await;
    let session_id = harness.session_id();
    let text = "I fixed the release process and told the team early.";

    // Na última pergunta, «Terminar sessão» conclui e a sessão deixa de ser a
    // ativa; o resumo continua consultável no histórico.
    let finished = harness.confirm_and_advance(text).await;
    assert!(finished.active_session.is_none());
    assert!(finished.current.is_none());

    let detail = harness.load_detail(&session_id).await;
    assert_eq!(detail.summary.status, SessionStatus::Completed);
    assert_eq!(detail.summary.confirmed_count, 1);
    assert!(detail.summary.completed_at.is_some());
    assert_eq!(
        detail.questions[0]
            .confirmation
            .as_ref()
            .expect("resposta confirmada")
            .text,
        text
    );

    // Uma sessão concluída permite iniciar outra.
    let next = harness
        .service
        .start_session(StartSession {
            request_id: new_request_id(),
            category: Some(Category::ProblemSolving),
            count: 1,
        })
        .await
        .expect("iniciar outra entrevista");
    assert_ne!(next.active_session.expect("nova sessão").id, session_id);
}

#[tokio::test]
async fn only_one_active_session_is_presented() {
    let harness = Harness::new().await;
    let error = harness
        .service
        .start_session(StartSession {
            request_id: new_request_id(),
            category: Some(Category::ProblemSolving),
            count: 1,
        })
        .await
        .expect_err("já existe uma entrevista em curso");
    assert_eq!(error.code, error_code::ACTIVE_SESSION_EXISTS);
    assert_eq!(harness.snapshot().active_session.expect("sessão").count, 1);
}

#[tokio::test]
async fn database_snapshot_is_ready_with_the_seeded_bank() {
    let harness = Harness::new().await;
    let snapshot = harness.snapshot();
    assert_eq!(snapshot.database, DatabaseState::Ready);
    assert!(snapshot.last_error.is_none());
    assert!(codes(&snapshot).is_empty());

    let questions = harness
        .service
        .list_questions(None)
        .await
        .expect("listar perguntas");
    assert_eq!(questions.len(), 2);

    let ids: HashSet<String> = unique_ids(questions.iter().map(|question| question.id.clone()));
    assert_eq!(ids.len(), 2);
}

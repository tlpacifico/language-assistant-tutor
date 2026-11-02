//! Testes de coordenação do serviço da fase 1.
//!
//! Nenhum teste abre um microfone, carrega modelos ou arranca subprocessos: a
//! porta de áudio, o auxiliar e o arranque do auxiliar são substituídos por
//! adaptadores controlados. O armazenamento é real (pasta temporária), para
//! cobrir publicação, derivado normalizado e revisões.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hound::{SampleFormat, WavSpec, WavWriter};
use serde_json::json;

use language_assistant_tutor::audio::{
    AudioEvent, AudioPort, AvailabilityAudio, CapturedAudio, RecordingTarget,
};
use language_assistant_tutor::domain::{
    error_code, AppError, AppSnapshot, Attempt, CaptureMeta, ComponentState, ComponentStatus,
    OperationKind, OperationState, PlaybackTarget, Segment, TranscriptStatus, QUESTION,
};
use language_assistant_tutor::service::{
    synthesis_cache_key, AppService, LaunchedWorker, WorkerControl, WorkerLauncher,
};
use language_assistant_tutor::storage::AttemptStore;
use language_assistant_tutor::worker::protocol::WorkerOp;
use language_assistant_tutor::worker::{
    HealthResult, SynthesisResult, TranscriptionResult, WorkerEvent, WorkerMessage, WorkerRequest,
    WorkerResult,
};

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

#[derive(Debug, Default)]
struct AudioState {
    capture: Option<RecordingTarget>,
    fail_stop: bool,
    /// A paragem seguinte excede o prazo, mas o original fica finalizado.
    stop_timeout: bool,
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

/// Operações ainda pendentes no snapshot.
fn pending_jobs(snapshot: &AppSnapshot) -> usize {
    let job = usize::from(snapshot.model_job.is_some());
    let running = snapshot
        .attempt
        .as_ref()
        .and_then(|attempt| attempt.last_operation.as_ref())
        .map(|operation| usize::from(operation.state == OperationState::Running))
        .unwrap_or(0);
    job + running
}

fn attempt_transcript_ids(attempt: &Attempt) -> Vec<String> {
    attempt
        .transcripts
        .iter()
        .map(|transcript| transcript.id.clone())
        .collect()
}

// ---------------------------------------------------------------------------
// Arranjo do serviço
// ---------------------------------------------------------------------------

struct Harness {
    _directory: tempfile::TempDir,
    root: PathBuf,
    audio: Arc<FakeAudio>,
    worker: Arc<FakeWorker>,
    launcher: Arc<FakeLauncher>,
    service: Arc<AppService>,
    snapshots: Arc<Mutex<Vec<AppSnapshot>>>,
}

impl Harness {
    /// Serviço com auxiliar ligado mas sem verificação de disponibilidade feita.
    async fn new() -> Self {
        Self::build(
            Arc::new(FakeLauncher::new(Arc::new(FakeWorker::default()))),
            None,
        )
        .await
    }

    /// Serviço com transcrição pronta: cada captura publicada é transcrita.
    async fn with_transcriber() -> Self {
        Self::build(
            Arc::new(FakeLauncher::new(Arc::new(FakeWorker::default()))),
            Some(ComponentStatus::Ready),
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
        )
        .await
    }

    async fn build(launcher: Arc<FakeLauncher>, transcription: Option<ComponentStatus>) -> Self {
        let directory = tempfile::tempdir().expect("pasta temporária");
        let root = directory.path().join("phase1");
        let whisper = directory.path().join("whisper");
        fs::create_dir_all(&whisper).expect("pasta dos modelos");
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

        let worker = launcher.worker.clone();
        let audio = Arc::new(FakeAudio::default());
        let service = Arc::new(AppService::new(
            AttemptStore::new(root.clone()),
            audio.clone(),
            root.clone(),
            launcher.clone(),
            Some(manifest),
        ));
        let snapshots = Arc::new(Mutex::new(Vec::new()));
        let collected = snapshots.clone();
        service.subscribe(Arc::new(move |snapshot: &AppSnapshot| {
            collected
                .lock()
                .expect("guardar snapshots")
                .push(snapshot.clone());
        }));
        service.start_worker().await;

        if let Some(status) = transcription {
            health(&service, status, ComponentStatus::Missing).await;
        }

        Self {
            _directory: directory,
            root,
            audio,
            worker,
            launcher,
            service,
            snapshots,
        }
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

    fn revisions(&self) -> Vec<u64> {
        self.snapshots
            .lock()
            .expect("ler snapshots")
            .iter()
            .map(|snapshot| snapshot.revision)
            .collect()
    }

    fn original_path(&self, attempt_id: &str) -> PathBuf {
        self.root
            .join("attempts")
            .join(attempt_id)
            .join("original.wav")
    }

    fn normalized_path(&self, attempt_id: &str) -> PathBuf {
        self.root
            .join("attempts")
            .join(attempt_id)
            .join("normalized.wav")
    }

    /// Grava uma resposta e devolve-a já ativa.
    async fn record_once(&self) -> AppSnapshot {
        self.service
            .start_recording()
            .await
            .expect("começar a gravar");
        self.service
            .stop_recording()
            .await
            .expect("terminar a gravar")
    }
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

// ---------------------------------------------------------------------------
// Testes exigidos
// ---------------------------------------------------------------------------

#[tokio::test]
async fn can_record_with_worker_missing() {
    let harness = Harness::without_worker().await;

    // O componente em falta é indicado, sem impedir a captura.
    let snapshot = harness.service.snapshot();
    assert_eq!(
        snapshot.availability.transcription.status,
        ComponentStatus::Missing
    );
    assert!(snapshot.availability.transcription.message.is_some());

    let snapshot = harness.record_once().await;
    let attempt = snapshot.attempt.expect("a resposta foi guardada");
    assert!(snapshot.recording_id.is_none());
    assert!(harness.requests().is_empty(), "nada é enviado ao auxiliar");
    assert!(
        attempt.normalized_file.is_none(),
        "sem transcritor não se cria o derivado"
    );
    assert!(harness.original_path(&attempt.id).is_file());

    // O áudio guardado continua a poder ser ouvido.
    let snapshot = harness
        .service
        .play_audio(PlaybackTarget::Response)
        .await
        .expect("ouvir sem auxiliar");
    assert_eq!(snapshot.playback, Some(PlaybackTarget::Response));
    assert_eq!(
        harness.audio.played(),
        vec![harness.original_path(&attempt.id)]
    );
}

#[tokio::test]
async fn failed_stop_keeps_previous_attempt() {
    let harness = Harness::new().await;
    let first = harness.record_once().await;
    let previous_id = first.attempt.expect("primeira resposta").id;

    harness.audio.fail_next_stop();
    harness
        .service
        .start_recording()
        .await
        .expect("segunda gravação");
    let interrupted_id = harness
        .service
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
    let snapshot = harness.service.snapshot();
    assert_eq!(snapshot.attempt.unwrap().id, previous_id);
    assert!(snapshot.recording_id.is_none());
    assert_eq!(
        snapshot.last_error.expect("erro visível").code,
        error_code::CAPTURE_INTERRUPTED
    );

    // Enquanto o parcial recuperável não chega, gravar por cima perdia áudio.
    let busy = harness
        .service
        .start_recording()
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
    harness
        .service
        .start_recording()
        .await
        .expect("a captura seguinte não fica bloqueada");
    assert!(harness.service.snapshot().recording_id.is_some());
}

#[tokio::test]
async fn repeated_start_returns_busy_instead_of_blocking() {
    let harness = Harness::new().await;
    harness
        .service
        .start_recording()
        .await
        .expect("primeira gravação");
    let recording_id = harness
        .service
        .snapshot()
        .recording_id
        .clone()
        .expect("gravação em curso");

    // O mutex do estado não é reentrante: se a guarda não cair antes de
    // `record_error`, o pedido repetido fica bloqueado para sempre.
    let second = tokio::time::timeout(Duration::from_secs(2), harness.service.start_recording())
        .await
        .expect("o pedido repetido devolve em vez de bloquear")
        .expect_err("a gravação em curso impede outra");
    assert_eq!(second.code, error_code::BUSY);
    assert_eq!(
        harness.service.snapshot().recording_id.as_deref(),
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
    harness
        .service
        .start_recording()
        .await
        .expect("primeira gravação");
    let first_id = harness
        .service
        .snapshot()
        .recording_id
        .clone()
        .expect("gravação em curso");
    harness
        .service
        .stop_recording()
        .await
        .expect("paragem normal");

    harness
        .service
        .start_recording()
        .await
        .expect("nova gravação");
    let second_id = harness
        .service
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

    let snapshot = harness.service.snapshot();
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
async fn retry_transcription_reuses_audio() {
    let harness = Harness::with_transcriber().await;
    let snapshot = harness.record_once().await;
    let attempt = snapshot.attempt.expect("resposta publicada");

    let first = harness
        .transcription_requests()
        .into_iter()
        .next()
        .expect("transcrição automática");
    let original_normalized_path = harness.normalized_path(&attempt.id);
    assert_eq!(first.input_path, original_normalized_path);

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
    let failed = harness.service.snapshot();
    assert!(failed.model_job.is_none());
    assert_eq!(
        failed
            .attempt
            .as_ref()
            .and_then(|attempt| attempt.last_operation.as_ref())
            .map(|operation| operation.state),
        Some(OperationState::Failed)
    );
    assert!(original_normalized_path.is_file());

    let after = harness
        .service
        .transcribe_response(attempt.id.clone())
        .await
        .expect("repetir transcrição");

    let requests = harness.transcription_requests();
    assert_eq!(requests.len(), 2);
    let retry = &requests[1];
    assert_eq!(retry.input_path, original_normalized_path);
    assert_ne!(retry.id, first.id, "o identificador tem de ser novo");
    assert_eq!(
        after
            .attempt
            .as_ref()
            .and_then(|attempt| attempt.normalized_file.as_deref()),
        Some("normalized.wav")
    );

    let revisions = harness.revisions();
    assert!(
        revisions.windows(2).all(|pair| pair[0] < pair[1]),
        "revisões não crescem: {revisions:?}"
    );
}

#[tokio::test]
async fn late_result_does_not_change_active_review() {
    let harness = Harness::with_transcriber().await;
    let snapshot = harness.record_once().await;
    assert!(snapshot.attempt.is_some(), "resposta publicada");
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

    let transcribed = harness.service.snapshot();
    let attempt = transcribed.attempt.expect("resposta transcrita");
    assert_eq!(attempt.transcripts.len(), 1);
    assert_eq!(
        attempt.review.as_ref().expect("revisão inicial").text,
        "machine words"
    );
    let source_id = attempt_transcript_ids(&attempt)[0].clone();

    let saved = harness
        .service
        .save_review(attempt.id.clone(), source_id, "my words".to_owned())
        .await
        .expect("guardar revisão");
    assert_eq!(saved.attempt.unwrap().review.unwrap().text, "my words");

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
    let after_late_result = harness.service.snapshot();
    assert_eq!(
        after_late_result
            .attempt
            .as_ref()
            .unwrap()
            .review
            .as_ref()
            .unwrap()
            .text,
        "my words"
    );
    assert_eq!(
        after_late_result
            .attempt
            .as_ref()
            .unwrap()
            .transcripts
            .len(),
        1
    );

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
    let after_late_result = harness.service.snapshot();
    assert_eq!(
        after_late_result.attempt.unwrap().review.unwrap().text,
        "my words"
    );
}

#[tokio::test]
async fn save_publication_can_retry() {
    let harness = Harness::new().await;
    let first = harness.record_once().await;
    let previous_id = first.attempt.expect("primeira resposta").id;

    harness
        .service
        .start_recording()
        .await
        .expect("segunda gravação");
    let pending_id = harness
        .service
        .snapshot()
        .recording_id
        .expect("identificador da segunda gravação");

    // Bloqueia a escrita do manifesto da resposta nova.
    let blocker = harness
        .root
        .join("attempts")
        .join(&pending_id)
        .join("attempt.json");
    fs::create_dir_all(&blocker).expect("bloquear o manifesto");

    let error = harness
        .service
        .stop_recording()
        .await
        .expect_err("a publicação falha");
    assert_eq!(error.code, error_code::STORAGE_WRITE_FAILED);
    let snapshot = harness.service.snapshot();
    assert_eq!(snapshot.attempt.unwrap().id, previous_id);
    assert!(snapshot.recording_id.is_none());
    assert_eq!(
        snapshot.last_error.expect("erro visível").code,
        error_code::STORAGE_WRITE_FAILED
    );
    assert!(
        harness.service.start_recording().await.is_err(),
        "uma gravação por publicar não pode ser esquecida"
    );

    fs::remove_dir(&blocker).expect("desbloquear o manifesto");
    let retried = harness
        .service
        .retry_save_recording()
        .await
        .expect("repetir a publicação");
    assert_eq!(retried.attempt.unwrap().id, pending_id);
    assert!(harness.original_path(&pending_id).is_file());
}

#[tokio::test]
async fn retranscribe_preserves_existing_review() {
    let harness = Harness::with_transcriber().await;
    let snapshot = harness.record_once().await;
    let attempt = snapshot.attempt.expect("resposta publicada");
    let first_id = attempt.id.clone();
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

    let attempt = harness.service.snapshot().attempt.expect("transcrita");
    let source_id = attempt_transcript_ids(&attempt)[0].clone();
    harness
        .service
        .save_review(
            first_id.clone(),
            source_id.clone(),
            "edited by me".to_owned(),
        )
        .await
        .expect("guardar revisão");

    harness
        .service
        .transcribe_response(first_id.clone())
        .await
        .expect("transcrever novamente");
    let requests = harness.transcription_requests();
    let second_request = requests.last().expect("segundo pedido");
    assert_eq!(requests.len(), 2);
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

    let after = harness.service.snapshot().attempt.expect("resposta");
    assert_eq!(after.transcripts.len(), 2);
    assert_eq!(after.transcripts[0].text, "first transcript");
    assert_eq!(after.transcripts[1].text, "second transcript");
    let review = after.review.expect("revisão preservada");
    assert_eq!(review.text, "edited by me");
    assert_eq!(review.source_transcript_id, source_id);
    assert_eq!(
        after.last_operation.expect("operação aplicada").id,
        second_request.id
    );
    assert_eq!(pending_jobs(&harness.service.snapshot()), 0);
}

#[tokio::test]
async fn close_finalizes_capture() {
    // Uma captura ativa é finalizada e publicada antes de fechar.
    let harness = Harness::without_worker().await;
    harness
        .service
        .start_recording()
        .await
        .expect("começar a gravar");
    let closed = harness
        .service
        .prepare_close()
        .await
        .expect("fechar com captura ativa");
    assert_eq!(pending_jobs(&closed), 0);
    let attempt = closed.attempt.clone().expect("captura finalizada");
    assert!(closed.recording_id.is_none());
    assert!(closed.model_job.is_none());
    assert!(harness.original_path(&attempt.id).is_file());

    // Um trabalho em curso fica interrompido e o auxiliar é encerrado.
    let harness = Harness::with_transcriber().await;
    harness.record_once().await;
    let running = harness.service.snapshot();
    assert!(running.model_job.is_some(), "transcrição em curso");
    let closed = harness
        .service
        .prepare_close()
        .await
        .expect("fechar com trabalho em curso");
    assert_eq!(pending_jobs(&closed), 0);
    let attempt = closed.attempt.clone().expect("resposta guardada");
    assert!(closed.recording_id.is_none());
    assert!(closed.model_job.is_none());
    assert_eq!(
        attempt.last_operation.expect("operação").state,
        OperationState::Interrupted
    );
    assert_eq!(harness.worker.shutdowns(), 1);
    assert_eq!(harness.launcher.launches.load(Ordering::SeqCst), 1);
    assert!(harness.audio.playback_stops() > 0);
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

    let after = harness.service.snapshot();
    let attempt = after.attempt.clone().expect("resposta guardada");
    assert_eq!(attempt.transcripts.len(), 1);
    assert_eq!(attempt.transcripts[0].status, TranscriptStatus::NoSpeech);
    assert!(attempt.transcripts[0].text.is_empty());
    assert!(attempt.review.is_none(), "ausência de fala não cria nota");
    assert_eq!(
        attempt.last_operation.expect("operação").state,
        OperationState::NoSpeech
    );
    assert!(after.model_job.is_none());

    // A resposta continua a poder ser ouvida e substituída por outra gravação.
    let playing = harness
        .service
        .play_audio(PlaybackTarget::Response)
        .await
        .expect("ouvir a resposta sem fala");
    assert_eq!(playing.playback, Some(PlaybackTarget::Response));
    harness
        .service
        .stop_audio()
        .await
        .expect("parar a reprodução");
    harness
        .service
        .start_recording()
        .await
        .expect("gravar outra resposta");
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

    let requested = harness
        .service
        .play_audio(PlaybackTarget::Question)
        .await
        .expect("pedir a síntese da pergunta");
    assert_eq!(requested.playback, None);
    let job = requested.model_job.expect("síntese em curso");
    assert_eq!(job.kind, OperationKind::Synthesize);

    let request = harness
        .requests()
        .into_iter()
        .find(|request| request.op == WorkerOp::Synthesize)
        .expect("pedido de síntese");
    let expected = harness
        .root
        .join("question-cache")
        .join(format!("{}.wav", synthesis_cache_key(QUESTION)));
    assert_eq!(
        PathBuf::from(
            request.params["outputPath"]
                .as_str()
                .expect("outputPath da síntese")
        ),
        expected
    );

    write_rate_wav(&expected, 24_000);
    harness
        .service
        .on_worker_event(WorkerEvent {
            generation: 1,
            message: WorkerMessage::Completed {
                request_id: request.id,
                result: synthesis_result(&expected),
            },
        })
        .await;
    let after = harness.service.snapshot();
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
    harness
        .service
        .play_audio(PlaybackTarget::Question)
        .await
        .expect("pedir a síntese da pergunta");
    let request = harness
        .requests()
        .into_iter()
        .find(|request| request.op == WorkerOp::Synthesize)
        .expect("pedido de síntese");
    let expected = harness
        .root
        .join("question-cache")
        .join(format!("{}.wav", synthesis_cache_key(QUESTION)));
    write_rate_wav(&expected, 48_000);
    harness
        .service
        .on_worker_event(WorkerEvent {
            generation: 1,
            message: WorkerMessage::Completed {
                request_id: request.id,
                result: synthesis_result(&expected),
            },
        })
        .await;

    let rejected = harness.service.snapshot();
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

    let failed = harness.service.snapshot();
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

    let recovered = harness.service.snapshot();
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

    let failed = harness.service.snapshot();
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
    let recovered = harness.service.snapshot();
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
    let refused = harness.service.snapshot();
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
    let after = harness.service.snapshot();
    assert_eq!(
        after.last_error.expect("erro do reinício").code,
        error_code::BUSY
    );
}

// ---------------------------------------------------------------------------
// Reprodução, disponibilidade e recuperação de áudio
// ---------------------------------------------------------------------------

#[tokio::test]
async fn playback_is_allowed_while_transcribing() {
    let harness = Harness::with_transcriber().await;
    let snapshot = harness.record_once().await;
    let attempt = snapshot.attempt.expect("resposta publicada");
    let job = harness
        .service
        .snapshot()
        .model_job
        .expect("transcrição em curso");
    assert_eq!(job.kind, OperationKind::Transcribe);

    // Transcrever em CPU é a espera mais longa: ouvir a resposta é permitido.
    let playing = harness
        .service
        .play_audio(PlaybackTarget::Response)
        .await
        .expect("ouvir a resposta durante a transcrição");
    assert_eq!(playing.playback, Some(PlaybackTarget::Response));
    assert!(playing.model_job.is_some());

    // A pergunta já em cache também substitui a resposta a meio da transcrição.
    let cache = harness
        .root
        .join("question-cache")
        .join(format!("{}.wav", synthesis_cache_key(QUESTION)));
    fs::create_dir_all(cache.parent().expect("pasta da cache")).expect("criar a cache");
    write_rate_wav(&cache, 24_000);

    let question = harness
        .service
        .play_audio(PlaybackTarget::Question)
        .await
        .expect("ouvir a pergunta em cache");
    assert_eq!(question.playback, Some(PlaybackTarget::Question));
    assert_eq!(
        harness.audio.played(),
        vec![harness.original_path(&attempt.id), cache],
        "a reprodução da pergunta substitui a da resposta"
    );
}

#[tokio::test]
async fn playback_is_refused_while_recording() {
    let harness = Harness::new().await;
    harness
        .service
        .start_recording()
        .await
        .expect("começar a gravar");

    let error = harness
        .service
        .play_audio(PlaybackTarget::Question)
        .await
        .expect_err("gravar impede reproduzir");
    assert_eq!(error.code, error_code::BUSY);
    assert!(harness.audio.played().is_empty());
}

#[tokio::test]
async fn starting_recording_stops_playback() {
    let harness = Harness::new().await;
    let snapshot = harness.record_once().await;
    let attempt = snapshot.attempt.expect("resposta publicada");
    harness
        .service
        .play_audio(PlaybackTarget::Response)
        .await
        .expect("ouvir a resposta");
    let stops = harness.audio.playback_stops();

    // A spec manda terminar a reprodução ao começar a gravar, não exigir que o
    // utilizador pare o áudio primeiro.
    let recording = harness
        .service
        .start_recording()
        .await
        .expect("gravar com uma reprodução ativa");
    assert!(recording.recording_id.is_some());
    assert_eq!(recording.playback, None);
    assert!(
        harness.audio.playback_stops() > stops,
        "a reprodução em curso tem de ser parada"
    );
    assert_eq!(
        harness.audio.played(),
        vec![harness.original_path(&attempt.id)]
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
    harness.record_once().await;
    harness
        .service
        .play_audio(PlaybackTarget::Response)
        .await
        .expect("ouvir a resposta");
    let stops = harness.audio.playback_stops();

    harness
        .service
        .play_audio(PlaybackTarget::Question)
        .await
        .expect("pedir a síntese da pergunta");
    let request = harness
        .requests()
        .into_iter()
        .find(|request| request.op == WorkerOp::Synthesize)
        .expect("pedido de síntese");
    let expected = harness
        .root
        .join("question-cache")
        .join(format!("{}.wav", synthesis_cache_key(QUESTION)));
    write_rate_wav(&expected, 24_000);

    harness
        .service
        .on_worker_event(WorkerEvent {
            generation: 1,
            message: WorkerMessage::Completed {
                request_id: request.id,
                result: synthesis_result(&expected),
            },
        })
        .await;

    let after = harness.service.snapshot();
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
        harness.service.snapshot().availability.microphone.status,
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
    let missing = harness.service.snapshot();
    assert_eq!(
        missing.availability.microphone.status,
        ComponentStatus::Missing
    );
    assert_eq!(harness.audio.reopens(), 1, "a porta de áudio é reaberta");

    // Restaurado, o painel volta a ficar correto sem reiniciar a aplicação.
    harness.audio.set_microphone(ComponentState::ready());
    harness.service.restart_worker().await.expect("reiniciar");
    assert_eq!(
        harness.service.snapshot().availability.microphone.status,
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

    let error = harness
        .service
        .start_recording()
        .await
        .expect_err("o microfone não abre");
    assert_eq!(error.code, error_code::DEVICE_UNAVAILABLE);
    let after = harness.service.snapshot();
    assert_eq!(
        after.availability.microphone.status,
        ComponentStatus::Missing,
        "uma falha de dispositivo volta a amostrar a disponibilidade"
    );
    assert!(after.recording_id.is_none());
}

#[tokio::test]
async fn timed_out_stop_recovers_capture() {
    let harness = Harness::new().await;
    harness.service.start_recording().await.expect("gravar");
    let recording_id = harness
        .service
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
    let snapshot = harness.service.snapshot();
    assert!(snapshot.recording_id.is_none());
    let attempt = snapshot.attempt.expect("o original é publicado");
    assert_eq!(attempt.id, recording_id);
    assert!(attempt.capture.incomplete);
    assert!(harness.original_path(&recording_id).is_file());
    assert_eq!(
        snapshot.last_error.expect("erro visível").code,
        error_code::AUDIO_TIMEOUT
    );

    // Sem a limpeza de `current_capture`, gravar ficava bloqueado.
    harness
        .service
        .start_recording()
        .await
        .expect("gravar outra resposta");
    harness
        .service
        .stop_recording()
        .await
        .expect("terminar a segunda gravação");

    // O mesmo timeout ao fechar não pode deixar o encerramento bloqueado.
    harness
        .service
        .start_recording()
        .await
        .expect("terceira gravação");
    harness.audio.fail_next_stop_timeout();
    let close_error = harness
        .service
        .prepare_close()
        .await
        .expect_err("o fecho falha no timeout");
    assert_eq!(close_error.code, error_code::AUDIO_TIMEOUT);
    assert!(harness.service.snapshot().attempt.is_some());
    harness
        .service
        .prepare_close()
        .await
        .expect("o fecho seguinte conclui");
}

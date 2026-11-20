//! Orquestração da conversa sobre artigos: gravação sem interrupções, envio
//! automático autorizado, invalidação por revogação/término e exclusão com o STAR.
//!
//! Sem microfone, subprocessos nem rede: áudio, auxiliar e fornecedor são
//! adaptadores controlados; a base e os ficheiros são reais (pasta temporária).

use std::collections::VecDeque;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use hound::{SampleFormat, WavSpec, WavWriter};
use tokio::sync::Notify;

use language_assistant_tutor::ai::{
    AiSettingsView, ArticleAiTask, ArticleTransport, ArticleTransportRequest, CredentialStore,
    NormalizedUsage, RemoteCompletion, OPENCODE_GO_BASE_URL,
};
use language_assistant_tutor::article::{
    begin_entry, finish_entry, read_cache, voice_key, ArticleCommand, ArticleConsent,
    ArticleCreate, ArticlePreferences, ArticleSnapshot, ArticleSpeaker, ArticleState,
    ARTICLE_NOTICE_VERSION, ENGINE_ID,
};
use language_assistant_tutor::artifacts::ArtifactStore;
use language_assistant_tutor::audio::{
    AudioEvent, AudioPort, AvailabilityAudio, CapturedAudio, PlaybackId, PlaybackProgress,
    RecordingTarget,
};
use language_assistant_tutor::db::Database;
use language_assistant_tutor::domain::{
    error_code, AppError, CaptureMeta, ComponentState, PlaybackTarget, TranscriptStatus,
};
use language_assistant_tutor::interview::{AudioSelection, StartSession};
use language_assistant_tutor::service::{
    AppService, LaunchedWorker, ServicePaths, WorkerControl, WorkerLauncher,
};
use language_assistant_tutor::worker::protocol::WorkerOp;
use language_assistant_tutor::worker::{
    TranscriptionResult, WorkerEvent, WorkerMessage, WorkerRequest, WorkerResult,
};

// ---------------------------------------------------------------------------
// Adaptadores controlados
// ---------------------------------------------------------------------------

fn write_wav(path: &Path) {
    let spec = WavSpec {
        channels: 1,
        sample_rate: 48_000,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).expect("criar WAV de teste");
    for _ in 0..24_000 {
        writer.write_sample(0i16).expect("escrever amostra");
    }
    writer.finalize().expect("fechar WAV de teste");
}

#[derive(Clone, Default)]
struct OrderLog(Arc<Mutex<Vec<&'static str>>>);

impl OrderLog {
    fn push(&self, step: &'static str) {
        self.0.lock().unwrap().push(step);
    }
    fn steps(&self) -> Vec<&'static str> {
        self.0.lock().unwrap().clone()
    }
}

#[derive(Default)]
struct FakeAudio {
    capture: Mutex<Option<RecordingTarget>>,
    starts: Mutex<usize>,
    plays: Mutex<Vec<PathBuf>>,
    appends: Mutex<Vec<PathBuf>>,
    stops: Mutex<usize>,
    last_id: Mutex<Option<PlaybackId>>,
    /// Quando presente, `pause_playback` bloqueia até receber um sinal, para
    /// simular um dispositivo lento que detém o coordenador serial.
    pause_gate: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
    /// Quantas vezes `pause_playback` foi invocado (antes do bloqueio).
    pause_attempts: Mutex<usize>,
    /// Quando ligado, `start_playback` falha com `DEVICE_UNAVAILABLE`.
    start_fail: Mutex<bool>,
    /// Quando ligado, `start_capture` falha com `DEVICE_UNAVAILABLE`.
    capture_fail: Mutex<bool>,
    /// Quando ligado, `stop_playback`/`stop_all_playback` falham.
    stop_fail: Mutex<bool>,
    /// Registo ordenado dos passos de áudio e do auxiliar.
    order: OrderLog,
}

impl FakeAudio {
    fn with_order(order: OrderLog) -> Self {
        Self {
            order,
            ..Self::default()
        }
    }
    fn starts(&self) -> usize {
        *self.starts.lock().unwrap()
    }
    fn plays(&self) -> Vec<PathBuf> {
        self.plays.lock().unwrap().clone()
    }
    fn appends(&self) -> Vec<PathBuf> {
        self.appends.lock().unwrap().clone()
    }
    fn stops(&self) -> usize {
        *self.stops.lock().unwrap()
    }
    /// A identidade da última reprodução iniciada, para simular o fim.
    fn last_id(&self) -> PlaybackId {
        self.last_id
            .lock()
            .unwrap()
            .clone()
            .expect("reprodução iniciada")
    }
    fn pause_attempts(&self) -> usize {
        *self.pause_attempts.lock().unwrap()
    }
    /// Retém a próxima pausa, devolvendo o remetente que a liberta.
    fn hold_pause(&self) -> std::sync::mpsc::Sender<()> {
        let (tx, rx) = std::sync::mpsc::channel();
        *self.pause_gate.lock().unwrap() = Some(rx);
        tx
    }
    /// Faz a próxima reprodução falhar como se o dispositivo estivesse ausente.
    fn fail_start(&self) {
        *self.start_fail.lock().unwrap() = true;
    }
    /// Faz a próxima captura falhar como se o microfone estivesse ausente.
    fn fail_capture(&self) {
        *self.capture_fail.lock().unwrap() = true;
    }
    /// Faz a próxima paragem da saída falhar.
    fn fail_stop(&self) {
        *self.stop_fail.lock().unwrap() = true;
    }
}

impl AudioPort for FakeAudio {
    fn availability(&self) -> AvailabilityAudio {
        AvailabilityAudio {
            microphone: ComponentState::ready(),
            playback: ComponentState::ready(),
        }
    }
    fn start_capture(&self, target: RecordingTarget) -> Result<(), AppError> {
        if *self.capture_fail.lock().unwrap() {
            return Err(AppError::new(
                error_code::DEVICE_UNAVAILABLE,
                "micrófone indisponível.",
                true,
            ));
        }
        self.order.push("start_capture");
        write_wav(&target.directory.join("original.wav"));
        *self.starts.lock().unwrap() += 1;
        *self.capture.lock().unwrap() = Some(target);
        Ok(())
    }
    fn stop_capture(&self) -> Result<CapturedAudio, AppError> {
        let target = self.capture.lock().unwrap().take().ok_or_else(|| {
            AppError::new(error_code::CAPTURE_INTERRUPTED, "Sem gravação.", false)
        })?;
        let original = target.directory.join("original.wav");
        let bytes = std::fs::metadata(&original).unwrap().len();
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
    fn start_playback(&self, path: PathBuf) -> Result<PlaybackId, AppError> {
        if *self.start_fail.lock().unwrap() {
            return Err(AppError::new(
                error_code::DEVICE_UNAVAILABLE,
                "dispositivo de saída indisponível.",
                true,
            ));
        }
        let id = uuid::Uuid::new_v4().to_string();
        self.plays.lock().unwrap().push(path);
        *self.last_id.lock().unwrap() = Some(id.clone());
        Ok(id)
    }
    fn append_chunk(&self, path: PathBuf, id: &PlaybackId) -> Result<PlaybackId, AppError> {
        self.appends.lock().unwrap().push(path);
        Ok(id.clone())
    }
    fn pause_playback(&self, _id: &PlaybackId) -> Result<(), AppError> {
        *self.pause_attempts.lock().unwrap() += 1;
        if let Some(gate) = self.pause_gate.lock().unwrap().take() {
            let _ = gate.recv();
        }
        Ok(())
    }
    fn resume_playback(&self, _id: &PlaybackId) -> Result<(), AppError> {
        Ok(())
    }
    fn stop_playback(&self, _id: &PlaybackId) -> Result<(), AppError> {
        if *self.stop_fail.lock().unwrap() {
            return Err(AppError::new(
                error_code::DEVICE_UNAVAILABLE,
                "saída indisponível.",
                true,
            ));
        }
        self.order.push("stop");
        *self.stops.lock().unwrap() += 1;
        Ok(())
    }
    fn stop_all_playback(&self) -> Result<(), AppError> {
        if *self.stop_fail.lock().unwrap() {
            return Err(AppError::new(
                error_code::DEVICE_UNAVAILABLE,
                "saída indisponível.",
                true,
            ));
        }
        self.order.push("stop");
        *self.stops.lock().unwrap() += 1;
        Ok(())
    }
    fn reopen(&self) -> Result<(), AppError> {
        Ok(())
    }
}

#[derive(Default)]
struct FakeWorker {
    requests: Mutex<Vec<WorkerRequest>>,
    busy: Mutex<bool>,
    cancels: Mutex<usize>,
    order: OrderLog,
}

impl FakeWorker {
    fn with_order(order: OrderLog) -> Self {
        Self {
            order,
            ..Self::default()
        }
    }
    fn transcriptions(&self) -> Vec<WorkerRequest> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.op == WorkerOp::Transcribe)
            .cloned()
            .collect()
    }
    fn total(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
    fn streams(&self) -> Vec<WorkerRequest> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.op == WorkerOp::SynthesizeStream)
            .cloned()
            .collect()
    }
    fn cancels(&self) -> usize {
        *self.cancels.lock().unwrap()
    }
}

impl WorkerControl for FakeWorker {
    fn is_busy(&self) -> bool {
        *self.busy.lock().unwrap()
    }
    fn submit(&self, request: WorkerRequest) -> Result<(), AppError> {
        if self.is_busy() {
            return Err(AppError::busy("O auxiliar está ocupado."));
        }
        self.requests.lock().unwrap().push(request);
        Ok(())
    }
    fn restart(&self) -> Result<(), AppError> {
        Ok(())
    }
    fn shutdown(&self) -> Result<(), AppError> {
        Ok(())
    }
    fn generation(&self) -> u64 {
        1
    }
    fn is_running(&self) -> bool {
        true
    }
    fn cancel(&self) -> Result<(), AppError> {
        self.order.push("cancel");
        *self.cancels.lock().unwrap() += 1;
        Ok(())
    }
}

struct FakeLauncher(Arc<FakeWorker>);

impl WorkerLauncher for FakeLauncher {
    fn launch(&self) -> Result<LaunchedWorker, AppError> {
        Ok(LaunchedWorker {
            control: self.0.clone(),
            events: None,
        })
    }
}

struct Store;

impl CredentialStore for Store {
    fn get(&self) -> language_assistant_tutor::ai::Result<Option<String>> {
        Ok(Some("oc_sk_test".into()))
    }
    fn set(&self, _key: &str) -> language_assistant_tutor::ai::Result<()> {
        Ok(())
    }
    fn delete(&self) -> language_assistant_tutor::ai::Result<()> {
        Ok(())
    }
}

/// Fornecedor controlado: regista cada pedido e pode reter o próximo.
#[derive(Default)]
struct FakeTransport {
    calls: Mutex<Vec<(ArticleAiTask, usize)>>,
    results: Mutex<VecDeque<Result<String, AppError>>>,
    gate: Mutex<Option<Arc<Notify>>>,
}

impl FakeTransport {
    fn calls(&self) -> Vec<ArticleAiTask> {
        self.calls.lock().unwrap().iter().map(|c| c.0).collect()
    }
    fn count(&self, task: ArticleAiTask) -> usize {
        self.calls().into_iter().filter(|t| *t == task).count()
    }
    fn hold_next(&self) -> Arc<Notify> {
        let gate = Arc::new(Notify::new());
        *self.gate.lock().unwrap() = Some(gate.clone());
        gate
    }
    fn push(&self, result: Result<String, AppError>) {
        self.results.lock().unwrap().push_back(result);
    }
}

fn default_content(task: ArticleAiTask) -> String {
    let dimension = r#"{"status":"insufficient_evidence","summary":"Pouco texto.","evidence":[]}"#;
    match task {
        ArticleAiTask::Opening => {
            r#"{"schemaVersion":"article-v1","question":"What is the main argument?"}"#.into()
        }
        ArticleAiTask::Reply => {
            r#"{"schemaVersion":"article-v1","comment":"Good point.","question":"Why does that matter?"}"#
                .into()
        }
        ArticleAiTask::Feedback => format!(
            r#"{{"schemaVersion":"article-v1","coherence":{dimension},"cohesion":{dimension},"development":{dimension},"interaction":{dimension},"improvements":[]}}"#
        ),
    }
}

impl ArticleTransport for FakeTransport {
    fn complete(
        &self,
        request: ArticleTransportRequest,
    ) -> Pin<Box<dyn Future<Output = language_assistant_tutor::ai::Result<RemoteCompletion>> + Send>>
    {
        self.calls
            .lock()
            .unwrap()
            .push((request.task, request.context.turns.len()));
        let gate = self.gate.lock().unwrap().take();
        let scripted = self.results.lock().unwrap().pop_front();
        let content = scripted.unwrap_or_else(|| Ok(default_content(request.task)));
        Box::pin(async move {
            if let Some(gate) = gate {
                gate.notified().await;
            }
            content.map(|content| RemoteCompletion {
                model: "test-model".into(),
                content,
                usage: NormalizedUsage {
                    input: Some(10),
                    output: Some(5),
                    ..Default::default()
                },
                invalid: false,
            })
        })
    }
}

// ---------------------------------------------------------------------------
// Arranjo
// ---------------------------------------------------------------------------

struct Harness {
    directory: tempfile::TempDir,
    database_path: PathBuf,
    manifest: PathBuf,
    database: Database,
    audio: Arc<FakeAudio>,
    worker: Arc<FakeWorker>,
    transport: Arc<FakeTransport>,
    service: Arc<AppService>,
    order: OrderLog,
}

fn build_service(
    directory: &Path,
    database_path: &Path,
    manifest: &Path,
    audio: Arc<FakeAudio>,
    worker: Arc<FakeWorker>,
    transport: Arc<FakeTransport>,
) -> Arc<AppService> {
    Arc::new(
        AppService::new(
            ServicePaths {
                database: database_path.to_path_buf(),
                legacy: directory.join("phase1"),
            },
            ArtifactStore::new(directory.join("sessions")),
            audio,
            Arc::new(FakeLauncher(worker)),
            Some(manifest.to_path_buf()),
        )
        .with_ai(Arc::new(NoStarTransport), Arc::new(Store))
        .with_article_transport(transport),
    )
}

/// O modo STAR nunca é exercido pelo fornecedor nestes testes.
struct NoStarTransport;

impl language_assistant_tutor::ai::AiTransport for NoStarTransport {
    fn complete(
        &self,
        _request: language_assistant_tutor::ai::TransportRequest,
    ) -> Pin<Box<dyn Future<Output = language_assistant_tutor::ai::Result<RemoteCompletion>> + Send>>
    {
        Box::pin(async { panic!("o modo STAR não é usado nestes testes") })
    }
}

impl Harness {
    async fn new() -> Self {
        let directory = tempfile::tempdir().expect("pasta temporária");
        let database_path = directory.path().join("tutor.sqlite3");
        std::fs::create_dir_all(directory.path().join("phase1")).unwrap();
        let whisper = directory.path().join("whisper");
        std::fs::create_dir_all(&whisper).unwrap();
        let manifest = directory.path().join("models.json");
        std::fs::write(
            &manifest,
            serde_json::to_vec(&serde_json::json!({
                "schemaVersion": 1,
                "whisperDir": whisper.display().to_string(),
            }))
            .unwrap(),
        )
        .unwrap();
        let database = Database::open(database_path.clone()).expect("abrir a base");
        let order = OrderLog::default();
        let audio = Arc::new(FakeAudio::with_order(order.clone()));
        let worker = Arc::new(FakeWorker::with_order(order.clone()));
        let transport = Arc::new(FakeTransport::default());
        let service = build_service(
            directory.path(),
            &database_path,
            &manifest,
            audio.clone(),
            worker.clone(),
            transport.clone(),
        );
        service.start().await;
        Self {
            directory,
            database_path,
            manifest,
            database,
            audio,
            worker,
            transport,
            service,
            order,
        }
    }

    fn cmd(&self, snapshot: &ArticleSnapshot) -> ArticleCommand {
        ArticleCommand {
            request_id: uuid::Uuid::new_v4().to_string(),
            session_id: snapshot.session.id.clone(),
            expected_version: snapshot.session.version,
        }
    }

    fn consent(&self, snapshot: &ArticleSnapshot) -> ArticleConsent {
        ArticleConsent {
            command: self.cmd(snapshot),
            destination: OPENCODE_GO_BASE_URL.into(),
            notice_version: ARTICLE_NOTICE_VERSION.into(),
        }
    }

    async fn create(&self) -> ArticleSnapshot {
        self.service
            .article_create(ArticleCreate {
                request_id: uuid::Uuid::new_v4().to_string(),
                title: Some("Remote work".into()),
                text: "Remote work changed how teams collaborate.".into(),
            })
            .await
            .expect("criar a conversa")
    }

    async fn snapshot(&self, id: &str) -> ArticleSnapshot {
        self.service.article_snapshot(id).await.expect("snapshot")
    }

    /// Conversa autorizada, com a pergunta de abertura do tutor, pronta a gravar.
    async fn ready(&self) -> ArticleSnapshot {
        let created = self.create().await;
        self.service
            .article_view_focus(&created.session.id, "harness")
            .await
            .unwrap();
        let consent = self.consent(&created);
        self.service.article_authorize(consent).await.unwrap();
        self.service.article_settle().await;
        let snapshot = self.snapshot(&created.session.id).await;
        assert_eq!(snapshot.session.state, ArticleState::Ready);
        snapshot
    }

    async fn record(&self, snapshot: &ArticleSnapshot) -> ArticleSnapshot {
        let started = self
            .service
            .article_start_recording(self.cmd(snapshot))
            .await
            .expect("começar a gravar");
        self.service
            .article_stop_recording(self.cmd(&started))
            .await
            .expect("parar a gravação")
    }

    async fn transcript(&self, text: &str) {
        self.transcript_event(text).await;
        self.service.article_settle().await;
    }

    async fn transcript_event(&self, text: &str) {
        let request = self
            .worker
            .transcriptions()
            .pop()
            .expect("pedido ao auxiliar");
        self.service
            .on_worker_event(WorkerEvent {
                generation: 1,
                message: WorkerMessage::Completed {
                    request_id: request.id,
                    result: WorkerResult::Transcription(TranscriptionResult {
                        status: TranscriptStatus::Completed,
                        text: text.into(),
                        segments: vec![],
                        model: "small".into(),
                        language: "en".into(),
                        load_ms: 1,
                        processing_ms: 1,
                    }),
                },
            })
            .await;
    }

    async fn wait_for_calls(&self, n: usize) {
        for _ in 0..200 {
            if self.transport.calls().len() >= n {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("o fornecedor não recebeu {n} pedidos");
    }
}

// ---------------------------------------------------------------------------
// Testes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn audio_is_saved_first_and_the_send_follows_only_the_transcript() {
    let h = Harness::new().await;
    let ready = h.ready().await;
    assert_eq!(h.transport.calls(), vec![ArticleAiTask::Opening]);

    let started = h
        .service
        .article_start_recording(h.cmd(&ready))
        .await
        .unwrap();
    assert_eq!(started.session.state, ArticleState::Recording);
    assert_eq!(h.audio.starts(), 1);
    // A gravação não termina sozinha: nenhuma transcrição nem novo envio.
    assert_eq!(h.worker.transcriptions().len(), 0);
    assert_eq!(h.transport.calls().len(), 1);

    let stopped = h
        .service
        .article_stop_recording(h.cmd(&started))
        .await
        .unwrap();
    assert_eq!(stopped.session.state, ArticleState::Transcribing);
    // O áudio já está guardado no disco antes de qualquer envio.
    assert_eq!(stopped.turns.len(), 2, "a gravação aguarda transcrição");
    assert!(stopped.turns[1].version.text.is_empty());
    let id = ready.session.id.clone();
    let original = h
        .directory
        .path()
        .join("articles")
        .join(&id)
        .read_dir()
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path()
        .join("original.wav");
    assert!(original.is_file());
    assert_eq!(h.worker.transcriptions().len(), 1);
    assert_eq!(h.transport.calls().len(), 1, "ainda nada enviado");

    h.transcript("Remote work helps people focus.").await;
    let after = h.snapshot(&id).await;
    assert_eq!(after.session.state, ArticleState::Ready);
    assert_eq!(h.transport.count(ArticleAiTask::Reply), 1);
    let speakers: Vec<_> = after.turns.iter().map(|t| t.speaker).collect();
    assert_eq!(
        speakers,
        vec![
            ArticleSpeaker::Tutor,
            ArticleSpeaker::User,
            ArticleSpeaker::Tutor
        ]
    );
    assert!(after.turns[2]
        .version
        .text
        .contains("Why does that matter?"));
}

#[tokio::test]
async fn empty_or_failed_transcripts_are_never_sent() {
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    h.record(&ready).await;
    h.transcript("   ").await;
    let after = h.snapshot(&id).await;
    assert_eq!(after.session.state, ArticleState::NeedsReview);
    assert_eq!(h.transport.count(ArticleAiTask::Reply), 0);

    // Nova gravação a partir de NeedsReview; desta vez o auxiliar falha.
    h.record(&after).await;
    let request = h.worker.transcriptions().pop().unwrap();
    h.service
        .on_worker_event(WorkerEvent {
            generation: 1,
            message: WorkerMessage::Failed {
                request_id: request.id,
                error: AppError::new(error_code::WORKER_UNAVAILABLE, "falhou", true),
            },
        })
        .await;
    h.service.article_settle().await;
    assert_eq!(
        h.snapshot(&id).await.session.state,
        ArticleState::NeedsReview
    );
    assert_eq!(h.transport.count(ArticleAiTask::Reply), 0);
}

#[tokio::test]
async fn a_double_click_has_a_single_effect() {
    let h = Harness::new().await;
    let created = h.create().await;
    let consent = h.consent(&created);
    h.service.article_authorize(consent.clone()).await.unwrap();
    h.service.article_authorize(consent).await.unwrap();
    h.service.article_settle().await;
    assert_eq!(h.transport.count(ArticleAiTask::Opening), 1);
    let id = created.session.id.clone();
    let ready = h.snapshot(&id).await;
    assert_eq!(ready.turns.len(), 1, "uma só pergunta de abertura");

    let start = h.cmd(&ready);
    let started = h
        .service
        .article_start_recording(start.clone())
        .await
        .unwrap();
    h.service.article_start_recording(start).await.unwrap();
    assert_eq!(h.audio.starts(), 1);

    let stop = h.cmd(&started);
    h.service
        .article_stop_recording(stop.clone())
        .await
        .unwrap();
    h.service.article_stop_recording(stop).await.unwrap();
    assert_eq!(h.worker.transcriptions().len(), 1);
}

#[tokio::test]
async fn star_and_article_never_use_the_microphone_together() {
    let h = Harness::new().await;
    let ready = h.ready().await;
    h.service
        .start_session(StartSession {
            mode: Default::default(),
            request_id: uuid::Uuid::new_v4().to_string(),
            category: None,
            count: 1,
        })
        .await
        .unwrap();
    let star = h.service.snapshot();
    let question = star.current.as_ref().unwrap().question.id.clone();
    let selection = star.current_answer.as_ref().unwrap().selection_version;
    h.service
        .start_recording(question.clone(), selection)
        .await
        .unwrap();

    // Com o STAR a gravar, o artigo recusa e nada muda.
    let refused = h
        .service
        .article_start_recording(h.cmd(&ready))
        .await
        .unwrap_err();
    assert_eq!(refused.code, error_code::BUSY);
    assert_eq!(h.audio.starts(), 1);
    assert_eq!(
        h.snapshot(&ready.session.id).await.session.state,
        ArticleState::Ready
    );
    h.service.stop_recording().await.unwrap();

    // Com o artigo a gravar, o STAR recusa.
    let started = h
        .service
        .article_start_recording(h.cmd(&ready))
        .await
        .unwrap();
    let refused = h
        .service
        .start_recording(question, selection)
        .await
        .unwrap_err();
    assert_eq!(refused.code, error_code::BUSY);
    assert_eq!(
        h.snapshot(&started.session.id).await.session.state,
        ArticleState::Recording
    );
}

#[tokio::test]
async fn a_storage_failure_before_recording_changes_nothing_and_never_calls_the_network() {
    let h = Harness::new().await;
    let ready = h.ready().await;
    // A pasta da sessão é um ficheiro: a pasta da gravação não pode ser criada.
    let session_dir = h.directory.path().join("articles").join(&ready.session.id);
    let _ = std::fs::remove_dir_all(&session_dir);
    std::fs::write(&session_dir, b"x").unwrap();
    let error = h
        .service
        .article_start_recording(h.cmd(&ready))
        .await
        .unwrap_err();
    assert_eq!(error.code, error_code::STORAGE_WRITE_FAILED);
    assert_eq!(h.audio.starts(), 0);
    let after = h.snapshot(&ready.session.id).await;
    assert_eq!(after.session.state, ArticleState::Ready);
    assert_eq!(after.session.version, ready.session.version);
    assert_eq!(h.transport.count(ArticleAiTask::Reply), 0);
}

#[tokio::test]
async fn reading_a_snapshot_has_no_external_effects() {
    let h = Harness::new().await;
    let ready = h.ready().await;
    let calls = h.transport.calls().len();
    let worker = h.worker.total();
    for _ in 0..3 {
        h.service.article_snapshot(&ready.session.id).await.unwrap();
        h.service.article_list().await.unwrap();
    }
    assert_eq!(h.transport.calls().len(), calls);
    assert_eq!(h.worker.total(), worker);
    assert_eq!(h.audio.starts(), 0);
}

#[tokio::test]
async fn revoking_during_the_request_discards_the_result_and_keeps_known_usage() {
    let h = Harness::new().await;
    let created = h.create().await;
    let id = created.session.id.clone();
    let gate = h.transport.hold_next();
    h.service
        .article_authorize(h.consent(&created))
        .await
        .unwrap();
    h.wait_for_calls(1).await;
    let pending = h.snapshot(&id).await;
    assert_eq!(pending.session.state, ArticleState::AwaitingTutor);
    h.service.article_revoke(h.cmd(&pending)).await.unwrap();
    gate.notify_one();
    h.service.article_settle().await;

    let after = h.snapshot(&id).await;
    assert_eq!(after.session.state, ArticleState::Draft);
    assert!(after.turns.is_empty(), "a pergunta obsoleta não é aplicada");
    // O consumo conhecido não é escondido.
    assert_eq!(after.session.usage.as_ref().unwrap().input_tokens, Some(10));
    let stale: i64 = h
        .database
        .with_connection(|c| {
            Ok(c.query_row(
                "SELECT count(*) FROM article_requests WHERE state = 'stale'",
                [],
                |r| r.get(0),
            )
            .unwrap())
        })
        .unwrap();
    assert_eq!(stale, 1);
}

#[tokio::test]
async fn a_changed_destination_blocks_the_automatic_send() {
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    h.record(&ready).await;
    // O destino configurado muda antes de a transcrição chegar.
    let settings = AiSettingsView {
        base_url: "https://other.example/v1".into(),
        version: 2,
        ..Default::default()
    };
    let raw = serde_json::to_string(&settings).unwrap();
    h.database
        .with_connection(|c| {
            c.execute(
                "INSERT OR REPLACE INTO ai_settings (id, version, json) VALUES (1, 2, ?1)",
                [&raw],
            )
            .unwrap();
            Ok(())
        })
        .unwrap();
    h.transcript("Remote work helps people focus.").await;
    let after = h.snapshot(&id).await;
    assert_eq!(h.transport.count(ArticleAiTask::Reply), 0);
    assert_eq!(after.session.state, ArticleState::RecoverableError);
    assert_eq!(after.session.error.as_ref().unwrap().code, "AI_CONSENT");
}

#[tokio::test]
async fn finishing_during_the_request_invalidates_it_and_reports_once() {
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    h.record(&ready).await;
    let gate = h.transport.hold_next();
    h.transcript_event("Remote work helps people focus.").await;
    h.wait_for_calls(2).await;
    let waiting = h.snapshot(&id).await;
    assert_eq!(waiting.session.state, ArticleState::AwaitingTutor);

    h.service.article_finish(h.cmd(&waiting)).await.unwrap();
    h.wait_for_calls(3).await; // o relatório não está retido
    gate.notify_one();
    h.service.article_settle().await;

    let after = h.snapshot(&id).await;
    assert_eq!(after.session.state, ArticleState::Completed);
    assert!(after.report.is_some());
    assert_eq!(h.transport.count(ArticleAiTask::Feedback), 1);
    assert!(
        after
            .turns
            .iter()
            .all(|t| !t.version.text.contains("Why does that matter?")),
        "a pergunta em voo ficou obsoleta"
    );
}

#[tokio::test]
async fn finishing_without_answers_completes_without_inventing_an_evaluation() {
    let h = Harness::new().await;
    let ready = h.ready().await;
    h.service.article_finish(h.cmd(&ready)).await.unwrap();
    h.service.article_settle().await;
    let after = h.snapshot(&ready.session.id).await;
    assert_eq!(after.session.state, ArticleState::Completed);
    assert!(after.report.is_none());
    assert_eq!(h.transport.count(ArticleAiTask::Feedback), 0);
}

#[tokio::test]
async fn nothing_is_sent_after_a_restart() {
    let h = Harness::new().await;
    let created = h.create().await;
    let gate = h.transport.hold_next();
    h.service
        .article_authorize(h.consent(&created))
        .await
        .unwrap();
    h.wait_for_calls(1).await;
    drop(gate); // o pedido nunca termina: simula o fecho abrupto

    let transport = Arc::new(FakeTransport::default());
    let restarted = build_service(
        h.directory.path(),
        &h.database_path,
        &h.manifest,
        Arc::new(FakeAudio::default()),
        Arc::new(FakeWorker::default()),
        transport.clone(),
    );
    restarted.start().await;
    restarted.article_settle().await;
    assert!(transport.calls().is_empty(), "sem retry no arranque");
    let state = restarted
        .article_snapshot(&created.session.id)
        .await
        .unwrap()
        .session
        .state;
    assert_eq!(state, ArticleState::RecoverableError);
}

#[tokio::test]
async fn a_late_result_after_deleting_is_discarded() {
    let h = Harness::new().await;
    let created = h.create().await;
    let id = created.session.id.clone();
    let gate = h.transport.hold_next();
    h.service
        .article_authorize(h.consent(&created))
        .await
        .unwrap();
    h.wait_for_calls(1).await;
    let pending = h.snapshot(&id).await;

    // Uma entrada de cache de voz vive na pasta da sessão e desaparece com ela.
    let session_dir = h.directory.path().join("articles").join(&id);
    let key = voice_key("Why does that matter?", "af_heart", 10, ENGINE_ID);
    let cache_dir = begin_entry(&session_dir, &key).unwrap();
    write_wav(&cache_dir.join("chunk-0000.wav"));
    finish_entry(
        &cache_dir,
        "Why does that matter?",
        "af_heart",
        10,
        ENGINE_ID,
        &[(0, 500)],
    )
    .unwrap();
    assert!(read_cache(&session_dir, &key).is_some());

    h.service.article_delete(h.cmd(&pending)).await.unwrap();
    gate.notify_one();
    h.service.article_settle().await;
    assert!(h.service.article_snapshot(&id).await.is_err());
    assert!(!session_dir.exists(), "a cache de voz sai com a sessão");
    let turns: i64 = h
        .database
        .with_connection(|c| {
            Ok(
                c.query_row("SELECT count(*) FROM article_turns", [], |r| r.get(0))
                    .unwrap(),
            )
        })
        .unwrap();
    assert_eq!(turns, 0);
}

#[tokio::test]
async fn a_failed_request_can_be_retried_without_hidden_retries() {
    let h = Harness::new().await;
    let created = h.create().await;
    h.transport.push(Err(AppError::new(
        "AI_UNAVAILABLE",
        "Serviço indisponível.",
        true,
    )));
    h.service
        .article_authorize(h.consent(&created))
        .await
        .unwrap();
    h.service.article_settle().await;
    let failed = h.snapshot(&created.session.id).await;
    assert_eq!(failed.session.state, ArticleState::RecoverableError);
    assert_eq!(
        h.transport.count(ArticleAiTask::Opening),
        1,
        "sem retry oculto"
    );
    let retried = h.service.article_retry(h.cmd(&failed)).await.unwrap();
    assert_eq!(retried.session.state, ArticleState::AwaitingTutor);
    h.service.article_settle().await;
    assert_eq!(h.transport.count(ArticleAiTask::Opening), 2);
    assert_eq!(
        h.snapshot(&created.session.id).await.session.state,
        ArticleState::Ready
    );
}

#[tokio::test]
async fn a_correction_forks_a_branch_and_the_retry_regenerates_the_report_there() {
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    h.record(&ready).await;
    h.transcript("Remote work helps people focus.").await;
    let replied = h.snapshot(&id).await;
    h.service.article_finish(h.cmd(&replied)).await.unwrap();
    h.service.article_settle().await;
    let done = h.snapshot(&id).await;
    assert_eq!(done.session.state, ArticleState::Completed);
    let old_report = done.report.clone().unwrap();
    let user_turn = done.turns[1].id.clone();

    let calls = h.transport.calls().len();
    let corrected = h
        .service
        .article_correct(language_assistant_tutor::article::ArticleCorrection {
            command: h.cmd(&done),
            turn_id: user_turn,
            text: "Remote work helps people concentrate.".into(),
        })
        .await
        .unwrap();
    // A correção não envia nada; a regeneração espera pela ação do utilizador.
    assert_eq!(h.transport.calls().len(), calls);
    assert_eq!(corrected.session.state, ArticleState::RecoverableError);
    assert_ne!(
        corrected.session.active_branch_id,
        done.session.active_branch_id
    );

    h.service.article_retry(h.cmd(&corrected)).await.unwrap();
    h.service.article_settle().await;
    let regenerated = h.snapshot(&id).await;
    assert_eq!(regenerated.session.state, ArticleState::Completed);
    let new_report = regenerated.report.unwrap();
    assert_ne!(new_report.id, old_report.id);
    assert_eq!(new_report.branch_id, regenerated.session.active_branch_id);
    assert_eq!(h.transport.count(ArticleAiTask::Feedback), 2);
}

#[tokio::test]
async fn resuming_recovers_local_state_without_the_network() {
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    // Fecho abrupto a meio de uma gravação: estado persistido sem captura viva.
    h.database
        .apply_article(
            &h.cmd(&ready),
            language_assistant_tutor::article::ArticleMutation::StartRecording {
                turn_id: "orphan".into(),
                audio_path: format!("articles/{id}/orphan/original.wav"),
            },
        )
        .unwrap();
    let calls = h.transport.calls().len();
    let snapshot = h.snapshot(&id).await;
    let resumed = h.service.article_resume(h.cmd(&snapshot)).await.unwrap();
    assert_eq!(resumed.session.state, ArticleState::RecoverableError);
    assert_eq!(
        resumed.session.error.as_ref().unwrap().code,
        "CAPTURE_INTERRUPTED"
    );
    assert_eq!(h.transport.calls().len(), calls, "sem HTTP");
    assert_eq!(h.worker.transcriptions().len(), 0);
}

#[tokio::test]
async fn manual_text_after_an_empty_transcript_is_sent_as_a_distinct_version() {
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    h.record(&ready).await;
    h.transcript("   ").await;
    let review = h.snapshot(&id).await;
    assert_eq!(review.session.state, ArticleState::NeedsReview);
    let turn = review.turns.last().unwrap().id.clone();
    assert_eq!(h.transport.count(ArticleAiTask::Reply), 0);

    let sent = h
        .service
        .article_submit_text(language_assistant_tutor::article::ArticleCorrection {
            command: h.cmd(&review),
            turn_id: turn,
            text: "I typed this instead.".into(),
        })
        .await
        .unwrap();
    assert_eq!(sent.session.state, ArticleState::AwaitingTutor);
    h.service.article_settle().await;
    let after = h.snapshot(&id).await;
    assert_eq!(h.transport.count(ArticleAiTask::Reply), 1);
    assert_eq!(after.session.state, ArticleState::Ready);
    assert_eq!(
        after.turns[1].version.source,
        Some(language_assistant_tutor::article::ArticleVersionSource::ManualReview)
    );
}

#[tokio::test]
async fn a_saved_answer_can_be_played_but_not_while_recording() {
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    h.record(&ready).await;
    h.transcript("Remote work helps people focus.").await;
    let replied = h.snapshot(&id).await;
    let user_turn = replied.turns[1].id.clone();
    assert!(replied.turns[1].audio_available);

    h.service.article_play_turn(&id, &user_turn).await.unwrap();
    let plays = h.audio.plays();
    assert_eq!(plays.len(), 1);
    assert!(plays[0].ends_with(format!("articles/{id}/{user_turn}/original.wav")));

    // Um turno do tutor, ou inexistente, não tem gravação para ouvir.
    let tutor_turn = replied.turns[0].id.clone();
    for turn in [tutor_turn.as_str(), "missing"] {
        let error = h.service.article_play_turn(&id, turn).await.unwrap_err();
        assert_eq!(error.code, error_code::STALE_SOURCE);
    }
    assert_eq!(h.audio.plays().len(), 1);

    // Durante uma gravação não se reproduz nada.
    h.service
        .article_start_recording(h.cmd(&replied))
        .await
        .unwrap();
    let busy = h
        .service
        .article_play_turn(&id, &user_turn)
        .await
        .unwrap_err();
    assert_eq!(busy.code, error_code::BUSY);
    assert_eq!(h.audio.plays().len(), 1);
}

// -- voz incremental do tutor -------------------------------------------------

fn chunk_path(request: &WorkerRequest, index: u32) -> PathBuf {
    let dir = request.params["outputDir"].as_str().unwrap();
    PathBuf::from(dir).join(format!("chunk-{index:04}.wav"))
}

async fn send_chunk(h: &Harness, request: &WorkerRequest, index: u32, path: PathBuf) {
    h.service
        .on_worker_event(WorkerEvent {
            generation: 1,
            message: WorkerMessage::AudioChunk {
                request_id: request.id.clone(),
                index,
                audio_path: path,
                sample_rate: 24_000,
                duration_ms: 800,
            },
        })
        .await;
}

/// Envia um bloco e escreve o WAV na pasta do pedido, para a cache fechar.
async fn send_chunk_file(h: &Harness, request: &WorkerRequest, index: u32) {
    let path = chunk_path(request, index);
    write_wav(&path);
    send_chunk(h, request, index, path).await;
}

/// Progresso audível da reprodução atual, para a máquina passar a `playing`.
async fn progress(h: &Harness, position_ms: u64, buffered_ms: u64) {
    let id = h.audio.last_id();
    h.service
        .on_audio_event(AudioEvent::PlaybackProgress {
            id,
            progress: PlaybackProgress {
                position_ms,
                buffered_ms,
                paused: false,
            },
        })
        .await;
}

async fn finish_stream(h: &Harness, request: &WorkerRequest, chunks: u32) {
    h.service
        .on_worker_event(WorkerEvent {
            generation: 1,
            message: WorkerMessage::Completed {
                request_id: request.id.clone(),
                result: WorkerResult::SynthesisStream(
                    language_assistant_tutor::worker::SynthesisStreamResult {
                        chunks,
                        sample_rate: 24_000,
                        cancelled: false,
                        load_ms: 1,
                        processing_ms: 1,
                    },
                ),
            },
        })
        .await;
}

async fn playback(h: &Harness, id: &str) -> language_assistant_tutor::article::PlayerState {
    h.snapshot(id).await.audio.state
}

#[tokio::test]
async fn a_new_tutor_turn_is_spoken_in_ordered_chunks_and_bad_chunks_stop_the_voice() {
    use language_assistant_tutor::article::PlayerState;
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let streams = h.worker.streams();
    assert_eq!(streams.len(), 1, "a abertura nova toca sozinha");
    let request = streams[0].clone();
    assert!(request.params["text"]
        .as_str()
        .unwrap()
        .contains("main argument"));
    // Antes de qualquer progresso audível, a reprodução está `preparing`.
    assert_eq!(playback(&h, &id).await, PlayerState::Preparing);

    send_chunk(&h, &request, 0, chunk_path(&request, 0)).await;
    send_chunk(&h, &request, 1, chunk_path(&request, 1)).await;
    // O primeiro bloco inicia a reprodução; o segundo acrescenta à fila.
    assert_eq!(h.audio.plays(), vec![chunk_path(&request, 0)]);
    assert_eq!(h.audio.appends(), vec![chunk_path(&request, 1)]);
    // Progresso audível: passa a `playing`.
    progress(&h, 200, 800).await;
    assert_eq!(playback(&h, &id).await, PlayerState::Playing);
    // Duplicado: inofensivo, não toca duas vezes nem pára a voz.
    send_chunk(&h, &request, 0, chunk_path(&request, 0)).await;
    assert_eq!(h.audio.appends().len(), 1);
    assert_eq!(playback(&h, &id).await, PlayerState::Playing);

    let stops = h.audio.stops();
    // Fora de ordem ou fora da pasta do pedido: a voz falha e o auxiliar é parado.
    send_chunk(&h, &request, 5, chunk_path(&request, 5)).await;
    assert_eq!(playback(&h, &id).await, PlayerState::Failed);
    assert_eq!(h.audio.appends().len(), 1);
    assert_eq!(h.audio.plays().len(), 1);
    assert!(h.worker.cancels() >= 1);
    assert_eq!(h.audio.stops(), stops + 1);
    // O texto continua visível.
    assert!(h.snapshot(&id).await.turns[0]
        .version
        .text
        .contains("main argument"));
}

#[tokio::test]
async fn an_outside_path_is_refused_without_playing_it() {
    use language_assistant_tutor::article::PlayerState;
    let h = Harness::new().await;
    let ready = h.ready().await;
    let request = h.worker.streams()[0].clone();
    send_chunk(&h, &request, 0, PathBuf::from(r"C:\Windows\chunk-0000.wav")).await;
    assert!(h.audio.appends().is_empty());
    assert_eq!(playback(&h, &ready.session.id).await, PlayerState::Failed);
}

#[tokio::test]
async fn opening_listing_or_resuming_never_starts_the_voice() {
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    assert_eq!(h.worker.streams().len(), 1);
    for _ in 0..3 {
        h.service.article_snapshot(&id).await.unwrap();
        h.service.article_list().await.unwrap();
        h.service.article_resume(h.cmd(&ready)).await.unwrap();
    }
    assert_eq!(h.worker.streams().len(), 1);

    // Um reinício e uma nova montagem também não tocam nada.
    let worker = Arc::new(FakeWorker::default());
    let restarted = build_service(
        h.directory.path(),
        &h.database_path,
        &h.manifest,
        Arc::new(FakeAudio::default()),
        worker.clone(),
        Arc::new(FakeTransport::default()),
    );
    restarted.start().await;
    restarted.article_snapshot(&id).await.unwrap();
    restarted.article_settle().await;
    assert!(worker.streams().is_empty());
}

#[tokio::test]
async fn stopping_the_voice_clears_the_queue_and_late_chunks_are_dropped() {
    use language_assistant_tutor::article::PlayerState;
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let request = h.worker.streams()[0].clone();
    send_chunk(&h, &request, 0, chunk_path(&request, 0)).await;
    let stops = h.audio.stops();

    h.service.article_stop_voice().await.unwrap();
    assert_eq!(h.worker.cancels(), 1);
    assert!(h.audio.stops() > stops);
    assert_eq!(playback(&h, &id).await, PlayerState::Idle);

    // Um bloco tardio do pedido cancelado não toca.
    send_chunk(&h, &request, 1, chunk_path(&request, 1)).await;
    assert!(h.audio.appends().is_empty());
    assert_eq!(h.audio.plays().len(), 1);
    // Parar sem voz ativa é inofensivo.
    h.service.article_stop_voice().await.unwrap();
    assert_eq!(h.worker.cancels(), 1);
}

#[tokio::test]
async fn starting_to_record_stops_the_voice_first() {
    let h = Harness::new().await;
    let ready = h.ready().await;
    let request = h.worker.streams()[0].clone();
    send_chunk(&h, &request, 0, chunk_path(&request, 0)).await;
    h.service
        .article_start_recording(h.cmd(&ready))
        .await
        .unwrap();
    assert_eq!(h.worker.cancels(), 1);
    send_chunk(&h, &request, 1, chunk_path(&request, 1)).await;
    assert!(h.audio.appends().is_empty(), "o bloco tardio não toca");
}

#[tokio::test]
async fn a_failed_voice_keeps_the_text_and_repeats_without_the_text_provider() {
    use language_assistant_tutor::article::PlayerState;
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let request = h.worker.streams()[0].clone();
    send_chunk(&h, &request, 0, chunk_path(&request, 0)).await;
    h.service
        .on_worker_event(WorkerEvent {
            generation: 1,
            message: WorkerMessage::Failed {
                request_id: request.id.clone(),
                error: AppError::new(error_code::WORKER_UNAVAILABLE, "falhou", true),
            },
        })
        .await;
    assert_eq!(playback(&h, &id).await, PlayerState::Failed);
    let calls = h.transport.calls().len();
    let tutor = h.snapshot(&id).await.turns[0].id.clone();
    assert!(h.snapshot(&id).await.turns[0]
        .version
        .text
        .contains("main argument"));

    h.service.article_speak_turn(&id, &tutor).await.unwrap();
    assert_eq!(
        h.worker.streams().len(),
        2,
        "nova síntese, a mesma entrada da cache regenerada"
    );
    assert_eq!(h.transport.calls().len(), calls, "sem nova chamada IA");
    assert_eq!(playback(&h, &id).await, PlayerState::Preparing);
    // Só falas do tutor da conversa ativa se repetem.
    assert!(h.service.article_speak_turn(&id, "missing").await.is_err());
}

#[tokio::test]
async fn a_momentarily_empty_queue_is_not_the_end_of_the_request() {
    use language_assistant_tutor::article::PlayerState;
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let request = h.worker.streams()[0].clone();
    send_chunk_file(&h, &request, 0).await;
    progress(&h, 200, 400).await;
    assert_eq!(playback(&h, &id).await, PlayerState::Playing);
    // A fila esvazia antes do 2.º bloco: `buffering`, não o fim do pedido.
    let first_id = h.audio.last_id();
    h.service
        .on_audio_event(AudioEvent::PlaybackFinished { id: first_id })
        .await;
    assert_eq!(playback(&h, &id).await, PlayerState::Buffering);
    // O bloco seguinte retoma a fila.
    send_chunk_file(&h, &request, 1).await;
    progress(&h, 300, 800).await;
    assert_eq!(playback(&h, &id).await, PlayerState::Playing);
    finish_stream(&h, &request, 2).await;
    // Só termina quando a síntese acabou e a fila voltou a esvaziar.
    let last_id = h.audio.last_id();
    h.service
        .on_audio_event(AudioEvent::PlaybackFinished { id: last_id })
        .await;
    assert_eq!(playback(&h, &id).await, PlayerState::Ended);
}

#[tokio::test]
async fn starting_article_playback_clears_star_playback_state() {
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    h.record(&ready).await;
    h.transcript("Remote work helps people focus.").await;
    let replied = h.snapshot(&id).await;
    let user_turn = replied
        .turns
        .iter()
        .find(|t| t.speaker == ArticleSpeaker::User)
        .expect("resposta do utilizador")
        .id
        .clone();

    // Reprodução STAR ativa (um treino legado ouvido pela interface).
    let attempt = "8a1f4c78-2a4b-4f0e-9b6b-1e6b0f2e4d11";
    let dir = h
        .directory
        .path()
        .join("phase1")
        .join("attempts")
        .join(attempt);
    std::fs::create_dir_all(&dir).expect("pasta do treino");
    write_wav(&dir.join("original.wav"));
    let playing = h
        .service
        .play_audio(AudioSelection::Legacy {
            attempt_id: attempt.to_owned(),
        })
        .await
        .expect("ouvir o treino legado");
    assert_eq!(playing.playback, Some(PlaybackTarget::Response));

    // Ouvir a resposta do artigo substitui a reprodução do STAR: o estado do
    // STAR deixa de apontar para a reprodução que perdeu o dispositivo.
    h.service
        .article_play_turn(&id, &user_turn)
        .await
        .expect("ouvir a resposta do artigo");
    assert_eq!(
        h.service.snapshot().playback,
        None,
        "a reprodução do STAR é limpa quando o artigo toma o dispositivo"
    );
}

#[tokio::test]
async fn a_star_selection_replaces_an_article_playback() {
    use language_assistant_tutor::article::PlayerState;
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let request = h.worker.streams()[0].clone();
    // Primeiro bloco: a voz do artigo fica em reprodução.
    send_chunk(&h, &request, 0, chunk_path(&request, 0)).await;
    progress(&h, 200, 400).await;
    assert_eq!(playback(&h, &id).await, PlayerState::Playing);
    let old_playback = h.audio.last_id();
    let old_generation = h.snapshot(&id).await.audio.generation;

    // Reprodução STAR (um treino legado) pára a voz do artigo.
    let attempt = "9b2e5d89-3b5c-4f1f-aa7c-2f7c1e3f5e22";
    let dir = h
        .directory
        .path()
        .join("phase1")
        .join("attempts")
        .join(attempt);
    std::fs::create_dir_all(&dir).expect("pasta do treino");
    write_wav(&dir.join("original.wav"));
    let playing = h
        .service
        .play_audio(AudioSelection::Legacy {
            attempt_id: attempt.to_owned(),
        })
        .await
        .expect("ouvir o treino legado");

    assert!(
        h.worker.cancels() >= 1,
        "a síntese da voz do artigo é cancelada"
    );
    assert_eq!(
        playback(&h, &id).await,
        PlayerState::Idle,
        "a voz do artigo pára quando o STAR reproduz"
    );
    assert_eq!(playing.playback, Some(PlaybackTarget::Response));
    h.service
        .article_player_stop(&id, old_generation)
        .await
        .unwrap();
    h.service
        .on_audio_event(AudioEvent::PlaybackFinished { id: old_playback })
        .await;
    assert_eq!(
        h.service.snapshot().playback,
        Some(PlaybackTarget::Response),
        "o STAR fica com a reprodução pedida"
    );
}

#[tokio::test]
async fn turn_metrics_use_closed_operations_with_durations_and_no_content() {
    use language_assistant_tutor::diagnostics::{self, BuildInfo, Diagnostics};
    let dir = tempfile::tempdir().unwrap();
    let diag = Diagnostics::open(
        dir.path().join("diag"),
        BuildInfo {
            version: "0.0.0".into(),
            revision: "abc1234".into(),
            dirty: false,
            built_at: "123".into(),
        },
    );
    diagnostics::install(diag.clone());

    let h = Harness::new().await;
    let ready = h.ready().await; // abertura: article_reply
    let id = ready.session.id.clone();
    let request = h.worker.streams()[0].clone();
    send_chunk(&h, &request, 0, chunk_path(&request, 0)).await; // article_first_audio
    h.record(&ready).await;
    h.transcript("Remote work helps people focus.").await; // article_transcribe
    let replied = h.snapshot(&id).await;
    h.service.article_finish(h.cmd(&replied)).await.unwrap();
    h.service.article_settle().await; // article_feedback
                                      // Controlo explícito do leitor: article_play_start / article_play_stop.
    let tutor = h.snapshot(&id).await.turns[0].id.clone();
    let view = h.service.article_player_play(&id, &tutor).await.unwrap();
    h.service
        .article_player_stop(&id, view.generation)
        .await
        .unwrap();

    diag.flush().unwrap();
    let export = diag.export(dir.path()).unwrap();
    let text = std::fs::read_to_string(export).unwrap();
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    let events = value["events"].as_array().unwrap();
    let mine: Vec<_> = events
        .iter()
        .filter(|e| e["operation"].as_str().unwrap().starts_with("article_"))
        .collect();
    let mut operations: Vec<&str> = mine
        .iter()
        .map(|e| e["operation"].as_str().unwrap())
        .collect();
    operations.sort_unstable();
    operations.dedup();
    assert_eq!(
        operations,
        vec![
            "article_feedback",
            "article_first_audio",
            "article_play_start",
            "article_play_stop",
            "article_record_handoff",
            "article_reply",
            "article_transcribe"
        ]
    );
    // O registo é global ao processo: outros testes também escrevem nele, por
    // isso cada operação exige pelo menos um evento bem-sucedido e todos têm duração.
    for event in &mine {
        assert!(event["durationMs"].is_u64(), "{event}");
        for field in [
            "text",
            "transcript",
            "audioPath",
            "level",
            "elapsedMs",
            "credential",
        ] {
            assert!(event.get(field).is_none(), "{event}");
        }
    }
    for operation in &operations {
        assert!(
            mine.iter()
                .any(|e| e["operation"] == *operation && e["code"] == "OK"),
            "{operation}"
        );
    }
    // Nenhum conteúdo, título nem caminho nos diagnósticos.
    for secret in [
        "Remote work helps",
        "Remote work changed",
        "main argument",
        "original.wav",
        "articles/",
        "chunk-0000",
    ] {
        assert!(!text.contains(secret), "{secret}");
    }
}

#[tokio::test]
async fn full_conversation_authorize_five_turns_correct_regenerate_complete_and_reopen() {
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();

    // Cinco turnos: gravar, parar, transcrever, resposta do tutor.
    let mut snapshot = ready;
    for n in 1..=5 {
        h.record(&snapshot).await;
        h.transcript(&format!("Answer number {n} about remote work."))
            .await;
        snapshot = h.snapshot(&id).await;
        assert_eq!(snapshot.session.state, ArticleState::Ready, "turno {n}");
        assert_eq!(snapshot.session.answers, n);
    }
    assert!(snapshot.session.suggest_finish, "sugere terminar à quinta");
    assert_eq!(h.transport.count(ArticleAiTask::Reply), 5);

    // Terminar e gerar o relatório.
    h.service.article_finish(h.cmd(&snapshot)).await.unwrap();
    h.service.article_settle().await;
    let done = h.snapshot(&id).await;
    assert_eq!(done.session.state, ArticleState::Completed);
    let first_report = done.report.clone().expect("relatório");

    // Corrigir uma resposta antiga: ramo novo, sem chamadas até ao pedido explícito.
    let calls = h.transport.calls().len();
    let second_user = done
        .turns
        .iter()
        .filter(|t| t.speaker == ArticleSpeaker::User)
        .nth(1)
        .unwrap()
        .clone();
    let corrected = h
        .service
        .article_correct(language_assistant_tutor::article::ArticleCorrection {
            command: h.cmd(&done),
            turn_id: second_user.id.clone(),
            text: "A corrected second answer about remote work.".into(),
        })
        .await
        .unwrap();
    assert_eq!(h.transport.calls().len(), calls);
    assert_eq!(corrected.turns.len(), 4, "prefixo até à resposta corrigida");

    // Regenerar no ramo novo e concluir outra vez.
    h.service.article_retry(h.cmd(&corrected)).await.unwrap();
    h.service.article_settle().await;
    let regenerated = h.snapshot(&id).await;
    assert_eq!(regenerated.session.state, ArticleState::Completed);
    let second_report = regenerated.report.clone().unwrap();
    assert_ne!(second_report.id, first_report.id);
    assert_ne!(second_report.branch_id, first_report.branch_id);

    // Fontes e artefactos: nada foi apagado ou reescrito.
    let (turns, versions, reports, audio_rows): (i64, i64, i64, i64) = h
        .database
        .with_connection(|c| {
            let count = |sql: &str| c.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap();
            Ok((
                count("SELECT count(*) FROM article_turns"),
                count("SELECT count(*) FROM article_versions"),
                count("SELECT count(*) FROM article_reports"),
                count("SELECT count(*) FROM article_artifacts WHERE state = 'present'"),
            ))
        })
        .unwrap();
    assert_eq!(turns, 11, "1 abertura + 5 respostas + 5 réplicas");
    assert_eq!(versions, 12, "uma por turno e a correção manual");
    assert_eq!(reports, 2);
    assert_eq!(audio_rows, 5);
    let article_dir = h.directory.path().join("articles").join(&id);
    let originals = std::fs::read_dir(&article_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("original.wav").is_file())
        .count();
    assert_eq!(originals, 5, "um áudio original por resposta, intacto");

    // Consumo conhecido: abertura + 5 réplicas + 2 relatórios, 10 tokens de entrada cada.
    let usage = regenerated.session.usage.clone().unwrap();
    assert_eq!(usage.input_tokens, Some(80));
    assert_eq!(usage.output_tokens, Some(40));
    assert_eq!(h.transport.count(ArticleAiTask::Feedback), 2);

    // Reabrir (novo serviço sobre a mesma base) não faz nenhuma chamada.
    let transport = Arc::new(FakeTransport::default());
    let worker = Arc::new(FakeWorker::default());
    let reopened = build_service(
        h.directory.path(),
        &h.database_path,
        &h.manifest,
        Arc::new(FakeAudio::default()),
        worker.clone(),
        transport.clone(),
    );
    reopened.start().await;
    let again = reopened.article_snapshot(&id).await.unwrap();
    reopened.article_list().await.unwrap();
    reopened.article_resume(h.cmd(&again)).await.unwrap();
    reopened.article_settle().await;
    assert_eq!(again.session.state, ArticleState::Completed);
    assert_eq!(again.report.unwrap().id, second_report.id);
    assert!(transport.calls().is_empty());
    assert_eq!(worker.total(), 0);
}

#[tokio::test]
async fn a_stale_stop_after_the_audio_closed_does_not_leave_the_microphone_stuck() {
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let started = h
        .service
        .article_start_recording(h.cmd(&ready))
        .await
        .unwrap();
    // Comando com versão obsoleta: o áudio fecha mas o repositório recusa o fim.
    let mut stale = h.cmd(&started);
    stale.expected_version -= 1;
    let error = h.service.article_stop_recording(stale).await.unwrap_err();
    assert_eq!(error.code, "STALE_SOURCE");
    // A gravação deixou de estar viva: recuperável, sem bloquear o microfone.
    let after = h.snapshot(&id).await;
    assert_eq!(after.session.state, ArticleState::RecoverableError);
    assert_eq!(h.worker.transcriptions().len(), 0);
    let retried = h.service.article_retry(h.cmd(&after)).await.unwrap();
    assert_eq!(retried.session.state, ArticleState::Ready);
    h.service
        .article_start_recording(h.cmd(&retried))
        .await
        .expect("o microfone voltou a estar livre");
}

// -- leitor de artigos: cache, pausa, autoplay, foco e geração ------------------

#[tokio::test]
async fn replacing_a_busy_synthesis_waits_for_worker_release_and_honours_stop() {
    for stop in [false, true] {
        let h = Harness::new().await;
        let ready = h.ready().await;
        let id = &ready.session.id;
        let first = h.worker.streams()[0].clone();
        *h.worker.busy.lock().unwrap() = true;
        let mut prefs = h.service.article_preferences_load().await.unwrap();
        prefs.speed_x10 = 12;
        h.service.article_preferences_save(prefs).await.unwrap();
        let view = h
            .service
            .article_player_play(id, &ready.turns[0].id)
            .await
            .unwrap();
        assert_eq!(
            view.state,
            language_assistant_tutor::article::PlayerState::Preparing
        );
        assert_eq!(h.worker.streams().len(), 1);
        if stop {
            h.service
                .article_player_stop(id, view.generation)
                .await
                .unwrap();
        }
        *h.worker.busy.lock().unwrap() = false;
        finish_stream(&h, &first, 0).await;
        assert_eq!(h.worker.streams().len(), if stop { 1 } else { 2 });
    }
}

#[tokio::test]
async fn play_reuses_a_complete_cache_without_a_worker_request() {
    use language_assistant_tutor::article::{PlayerSource, PlayerState};
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let tutor = ready.turns[0].id.clone();
    let request = h.worker.streams()[0].clone();
    // Conclui a síntese automática para escrever a entrada da cache.
    send_chunk_file(&h, &request, 0).await;
    finish_stream(&h, &request, 1).await;
    assert_eq!(h.worker.streams().len(), 1);

    // Reproduzir de novo reutiliza a cache completa, sem nova síntese.
    let view = h.service.article_player_play(&id, &tutor).await.unwrap();
    assert_eq!(h.worker.streams().len(), 1, "sem novo pedido ao auxiliar");
    assert_eq!(view.source, Some(PlayerSource::TutorVoice));
    assert_eq!(view.state, PlayerState::Preparing);
    assert_eq!(view.duration_ms, Some(500), "duração conhecida da cache");
}

#[tokio::test]
async fn a_cache_miss_synthesizes_without_a_text_request() {
    use language_assistant_tutor::article::PlayerState;
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let tutor = ready.turns[0].id.clone();
    // Para a voz antes de concluir: a entrada fica incompleta.
    h.service.article_stop_voice().await.unwrap();
    let calls = h.transport.calls().len();
    let streams = h.worker.streams().len();

    let view = h.service.article_player_play(&id, &tutor).await.unwrap();
    assert_eq!(h.worker.streams().len(), streams + 1, "síntese local nova");
    assert_eq!(
        h.transport.calls().len(),
        calls,
        "sem chamada ao fornecedor de texto"
    );
    assert_eq!(view.state, PlayerState::Preparing);
}

#[tokio::test]
async fn a_corrupt_cache_entry_falls_back_to_a_local_synthesis() {
    use language_assistant_tutor::article::PlayerState;
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let tutor = ready.turns[0].id.clone();
    let text = ready.turns[0].version.text.clone();
    h.service.article_stop_voice().await.unwrap();

    let session_dir = h.directory.path().join("articles").join(&id);
    let key = voice_key(&text, "af_heart", 10, ENGINE_ID);
    let cache_dir = begin_entry(&session_dir, &key).unwrap();
    write_wav(&cache_dir.join("chunk-0000.wav"));
    finish_entry(&cache_dir, &text, "af_heart", 10, ENGINE_ID, &[(0, 500)]).unwrap();
    assert!(read_cache(&session_dir, &key).is_some());
    std::fs::remove_file(cache_dir.join("chunk-0000.wav")).unwrap();
    assert!(read_cache(&session_dir, &key).is_none());

    let calls = h.transport.calls().len();
    let streams = h.worker.streams().len();
    let view = h.service.article_player_play(&id, &tutor).await.unwrap();
    assert_eq!(
        h.worker.streams().len(),
        streams + 1,
        "cache corrompida cai na síntese"
    );
    assert_eq!(h.transport.calls().len(), calls, "sem texto");
    assert_eq!(view.state, PlayerState::Preparing);
}

#[tokio::test]
async fn pause_then_resume_keep_generation_and_position() {
    use language_assistant_tutor::article::PlayerState;
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let request = h.worker.streams()[0].clone();
    send_chunk(&h, &request, 0, chunk_path(&request, 0)).await;
    progress(&h, 400, 800).await;
    let view = h.snapshot(&id).await.audio;
    assert_eq!(view.state, PlayerState::Playing);
    let generation = view.generation;
    let position = view.position_ms;

    let paused = h
        .service
        .article_player_pause(&id, generation)
        .await
        .unwrap();
    assert_eq!(paused.state, PlayerState::Paused);
    assert_eq!(paused.generation, generation);
    assert_eq!(paused.position_ms, position);

    let resumed = h
        .service
        .article_player_resume(&id, generation)
        .await
        .unwrap();
    assert_eq!(resumed.state, PlayerState::Playing);
    assert_eq!(resumed.generation, generation);
    assert_eq!(resumed.position_ms, position);
}

#[tokio::test]
async fn pause_while_synthesizing_is_not_ended() {
    use language_assistant_tutor::article::PlayerState;
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let request = h.worker.streams()[0].clone();

    let generation = h.snapshot(&id).await.audio.generation;
    let paused = h
        .service
        .article_player_pause(&id, generation)
        .await
        .unwrap();
    assert_eq!(paused.state, PlayerState::Paused);

    // A síntese termina enquanto a reprodução está em pausa.
    send_chunk_file(&h, &request, 0).await;
    send_chunk_file(&h, &request, 1).await;
    finish_stream(&h, &request, 2).await;
    assert_eq!(playback(&h, &id).await, PlayerState::Paused);
    assert!(h.audio.plays().is_empty());
    let resumed = h
        .service
        .article_player_resume(&id, generation)
        .await
        .unwrap();
    assert_eq!(h.audio.plays(), vec![chunk_path(&request, 0)]);
    assert_eq!(h.audio.appends(), vec![chunk_path(&request, 1)]);
    assert_eq!(resumed.position_ms, paused.position_ms);
    let playback_id = h.audio.last_id();
    progress(&h, 50, 100).await;
    h.service
        .on_audio_event(AudioEvent::PlaybackFinished { id: playback_id })
        .await;
    assert_eq!(playback(&h, &id).await, PlayerState::Ended);
}

#[tokio::test]
async fn a_stale_finished_event_does_not_stop_a_new_item() {
    use language_assistant_tutor::article::{PlayerSource, PlayerState};
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    h.record(&ready).await;
    h.transcript("First answer.").await;
    let first = h.snapshot(&id).await;
    h.record(&first).await;
    h.transcript("Second answer.").await;
    let second = h.snapshot(&id).await;
    let users: Vec<_> = second
        .turns
        .iter()
        .filter(|t| t.speaker == ArticleSpeaker::User)
        .map(|t| t.id.clone())
        .collect();
    assert_eq!(users.len(), 2);

    h.service.article_player_play(&id, &users[0]).await.unwrap();
    let first_id = h.audio.last_id();
    h.service.article_player_play(&id, &users[1]).await.unwrap();
    let second_id = h.audio.last_id();
    assert_ne!(first_id, second_id);

    // A substituição fica audivelmente a tocar.
    progress(&h, 300, 500).await;
    let playing = h.snapshot(&id).await.audio;
    assert_eq!(playing.state, PlayerState::Playing);
    assert_eq!(playing.position_ms, 300);

    // O fim tardio do item antigo não pára nem regride a substituição.
    h.service
        .on_audio_event(AudioEvent::PlaybackFinished { id: first_id })
        .await;
    let view = h.snapshot(&id).await.audio;
    assert_eq!(view.turn_id.as_deref(), Some(users[1].as_str()));
    assert_eq!(view.source, Some(PlayerSource::UserRecording));
    assert_eq!(view.state, PlayerState::Playing);
    assert_eq!(view.position_ms, 300);
}

#[tokio::test]
async fn a_late_chunk_after_stop_is_not_played() {
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let request = h.worker.streams()[0].clone();
    send_chunk(&h, &request, 0, chunk_path(&request, 0)).await;
    assert_eq!(h.audio.plays().len(), 1);

    h.service.article_stop_voice().await.unwrap();
    let plays = h.audio.plays().len();
    let appends = h.audio.appends().len();
    send_chunk(&h, &request, 1, chunk_path(&request, 1)).await;
    assert_eq!(h.audio.plays().len(), plays);
    assert_eq!(h.audio.appends().len(), appends);
    assert_eq!(
        h.snapshot(&id).await.audio.state,
        language_assistant_tutor::article::PlayerState::Idle
    );
}

#[tokio::test]
async fn restart_during_generation_reuses_its_ordered_prefix() {
    use language_assistant_tutor::article::PlayerState;
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let tutor = ready.turns[0].id.clone();
    let request = h.worker.streams()[0].clone();
    send_chunk_file(&h, &request, 0).await;
    send_chunk_file(&h, &request, 1).await;
    assert_eq!(h.audio.plays(), vec![chunk_path(&request, 0)]);
    assert_eq!(h.audio.appends(), vec![chunk_path(&request, 1)]);

    // Reinicia durante a geração: reutiliza o prefixo ordenado, sem nova síntese.
    let view = h.service.article_player_play(&id, &tutor).await.unwrap();
    assert_eq!(h.worker.streams().len(), 1, "sem nova síntese");
    assert_eq!(view.state, PlayerState::Preparing);
    assert_eq!(
        h.audio.plays(),
        vec![chunk_path(&request, 0), chunk_path(&request, 0)]
    );
    assert_eq!(
        h.audio.appends(),
        vec![chunk_path(&request, 1), chunk_path(&request, 1)]
    );

    // O bloco restante é aceite uma vez.
    send_chunk_file(&h, &request, 2).await;
    assert_eq!(h.audio.appends().len(), 3);
    finish_stream(&h, &request, 3).await;
}

#[tokio::test]
async fn position_does_not_regress_when_a_drain_restarts_playback() {
    use language_assistant_tutor::article::PlayerState;
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let request = h.worker.streams()[0].clone();

    send_chunk_file(&h, &request, 0).await;
    progress(&h, 500, 500).await;
    assert_eq!(h.snapshot(&id).await.audio.position_ms, 500);

    // A fila esvazia antes do 2.º bloco.
    let first_id = h.audio.last_id();
    h.service
        .on_audio_event(AudioEvent::PlaybackFinished { id: first_id })
        .await;
    assert_eq!(playback(&h, &id).await, PlayerState::Buffering);

    // O bloco seguinte continua a rendição: a posição não regride ao zero do
    // dispositivo novo.
    send_chunk_file(&h, &request, 1).await;
    progress(&h, 300, 500).await;
    let view = h.snapshot(&id).await.audio;
    assert_eq!(view.position_ms, 800, "posição continua na rendição");
    assert_eq!(view.buffered_ms, Some(1000));

    // O fim da rendição fecha com a soma total, não com a posição local.
    finish_stream(&h, &request, 2).await;
    let view = h.snapshot(&id).await.audio;
    assert_eq!(view.duration_ms, Some(1000));
    assert_eq!(view.buffered_ms, Some(1000));
}

#[tokio::test]
async fn an_explicit_restart_starts_the_position_at_zero() {
    use language_assistant_tutor::article::PlayerState;
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let tutor = ready.turns[0].id.clone();
    let request = h.worker.streams()[0].clone();

    send_chunk_file(&h, &request, 0).await;
    progress(&h, 500, 500).await;
    assert_eq!(h.snapshot(&id).await.audio.position_ms, 500);

    // Reinicia a mesma fala do início: a posição, a preparação e a duração
    // voltam ao estado inicial.
    let view = h.service.article_player_play(&id, &tutor).await.unwrap();
    assert_eq!(view.state, PlayerState::Preparing);
    assert_eq!(view.position_ms, 0, "o reinício começa do início");
    assert_eq!(view.buffered_ms, None, "a preparação volta ao início");
    assert_eq!(view.duration_ms, None, "a duração volta a desconhecida");

    // O progresso seguinte é a posição local do dispositivo, sem desvio.
    progress(&h, 100, 500).await;
    assert_eq!(h.snapshot(&id).await.audio.position_ms, 100);
}

#[tokio::test]
async fn a_new_selection_replaces_the_previous_article_playback() {
    use language_assistant_tutor::article::PlayerSource;
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    h.record(&ready).await;
    h.transcript("First answer.").await;
    let first = h.snapshot(&id).await;
    h.record(&first).await;
    h.transcript("Second answer.").await;
    let second = h.snapshot(&id).await;
    let users: Vec<_> = second
        .turns
        .iter()
        .filter(|t| t.speaker == ArticleSpeaker::User)
        .map(|t| t.id.clone())
        .collect();

    let view1 = h.service.article_player_play(&id, &users[0]).await.unwrap();
    let id1 = h.audio.last_id();
    assert_eq!(view1.turn_id.as_deref(), Some(users[0].as_str()));
    assert_eq!(view1.source, Some(PlayerSource::UserRecording));

    let view2 = h.service.article_player_play(&id, &users[1]).await.unwrap();
    let id2 = h.audio.last_id();
    assert_eq!(view2.turn_id.as_deref(), Some(users[1].as_str()));
    assert_ne!(id1, id2);
    assert!(view2.generation > view1.generation);
}

#[tokio::test]
async fn autoplay_runs_once_only_for_a_fresh_turn_in_the_active_view() {
    let h = Harness::new().await;
    let created = h.create().await;
    let id = created.session.id.clone();
    h.service.article_view_focus(&id, "token-1").await.unwrap();
    h.service
        .article_authorize(h.consent(&created))
        .await
        .unwrap();
    h.service.article_settle().await;
    assert_eq!(h.worker.streams().len(), 1, "a abertura toca uma vez");
    let ready = h.snapshot(&id).await;
    assert_eq!(ready.session.state, ArticleState::Ready);

    // Reabrir, retomar e reler nunca voltam a tocar.
    h.service.article_resume(h.cmd(&ready)).await.unwrap();
    h.service.article_snapshot(&id).await.unwrap();
    h.service.article_list().await.unwrap();
    h.service.article_settle().await;
    assert_eq!(h.worker.streams().len(), 1, "autoplay apenas uma vez");
}

#[tokio::test]
async fn autoplay_never_plays_a_turn_that_arrived_while_the_view_was_away() {
    let h = Harness::new().await;
    let created = h.create().await;
    let id = created.session.id.clone();
    h.service.article_view_focus(&id, "token-1").await.unwrap();
    h.service.article_view_blur("token-1").await.unwrap();
    // A vista está ausente: a abertura que chega não toca.
    h.service
        .article_authorize(h.consent(&created))
        .await
        .unwrap();
    h.service.article_settle().await;
    assert_eq!(
        h.worker.streams().len(),
        0,
        "sem voz enquanto a vista está ausente"
    );
    assert_eq!(h.snapshot(&id).await.session.state, ArticleState::Ready);
}

#[tokio::test]
async fn disabling_autoplay_stops_automatic_voice_but_not_an_explicit_replay() {
    use language_assistant_tutor::article::{PlaybackTrigger, PlayerState};
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let tutor = ready.turns[0].id.clone();
    let request = h.worker.streams()[0].clone();
    send_chunk(&h, &request, 0, chunk_path(&request, 0)).await;
    progress(&h, 200, 400).await;
    assert_eq!(playback(&h, &id).await, PlayerState::Playing);

    // Desligar a reprodução automática pára apenas a voz automática.
    h.service
        .article_preferences_save(ArticlePreferences {
            autoplay: false,
            speed_x10: 10,
        })
        .await
        .unwrap();
    assert_eq!(playback(&h, &id).await, PlayerState::Idle);

    // Volta a ligar e inicia uma repetição explícita.
    h.service
        .article_preferences_save(ArticlePreferences {
            autoplay: true,
            speed_x10: 10,
        })
        .await
        .unwrap();
    let view = h.service.article_player_play(&id, &tutor).await.unwrap();
    assert_eq!(view.state, PlayerState::Preparing);
    assert_eq!(view.trigger, Some(PlaybackTrigger::Explicit));

    // Desligar (true -> false) não pára a repetição explícita em curso.
    h.service
        .article_preferences_save(ArticlePreferences {
            autoplay: false,
            speed_x10: 10,
        })
        .await
        .unwrap();
    assert_eq!(
        playback(&h, &id).await,
        PlayerState::Preparing,
        "a repetição explícita continua ativa"
    );
}

#[tokio::test]
async fn a_speed_change_uses_the_matching_cache_entry() {
    use language_assistant_tutor::article::PlayerState;
    let h = Harness::new().await;
    h.service
        .article_preferences_save(ArticlePreferences {
            autoplay: true,
            speed_x10: 12,
        })
        .await
        .unwrap();
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let tutor = ready.turns[0].id.clone();
    let request = h.worker.streams()[0].clone();
    let speed = request.params["speed"].as_f64().unwrap();
    assert!(
        (speed - 1.2).abs() < 0.0001,
        "velocidade 1.2× no pedido: {speed}"
    );

    send_chunk_file(&h, &request, 0).await;
    finish_stream(&h, &request, 1).await;
    h.service.article_stop_voice().await.unwrap();
    let streams = h.worker.streams().len();

    // Mesma velocidade: a cache a 1.2× é reutilizada.
    let view = h.service.article_player_play(&id, &tutor).await.unwrap();
    assert_eq!(
        h.worker.streams().len(),
        streams,
        "cache a 1.2× reutilizada"
    );
    assert_eq!(view.duration_ms, Some(500));

    // Velocidade 1.0×: outra chave, cache falha e sintetiza de novo.
    h.service
        .article_preferences_save(ArticlePreferences {
            autoplay: true,
            speed_x10: 10,
        })
        .await
        .unwrap();
    let view = h.service.article_player_play(&id, &tutor).await.unwrap();
    assert_eq!(h.worker.streams().len(), streams + 1, "síntese nova a 1.0×");
    assert_eq!(view.state, PlayerState::Preparing);
}

#[tokio::test]
async fn a_user_recording_ignores_the_tutor_speed() {
    use language_assistant_tutor::article::PlayerSource;
    let h = Harness::new().await;
    h.service
        .article_preferences_save(ArticlePreferences {
            autoplay: true,
            speed_x10: 12,
        })
        .await
        .unwrap();
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    h.record(&ready).await;
    h.transcript("My answer.").await;
    let snapshot = h.snapshot(&id).await;
    let user = snapshot
        .turns
        .iter()
        .find(|t| t.speaker == ArticleSpeaker::User)
        .unwrap()
        .id
        .clone();
    let streams = h.worker.streams().len();

    let view = h.service.article_player_play(&id, &user).await.unwrap();
    assert_eq!(view.source, Some(PlayerSource::UserRecording));
    assert_eq!(
        h.worker.streams().len(),
        streams,
        "sem síntese para a gravação"
    );
    let plays = h.audio.plays();
    assert!(plays
        .iter()
        .any(|p| p.ends_with(format!("articles/{id}/{user}/original.wav"))));
}

#[tokio::test]
async fn an_old_focus_token_cannot_clear_a_newer_view() {
    let h = Harness::new().await;
    let created = h.create().await;
    let id = created.session.id.clone();
    h.service.article_view_focus(&id, "token-1").await.unwrap();
    h.service.article_view_focus(&id, "token-2").await.unwrap();
    h.service.article_view_blur("token-1").await.unwrap();

    // A vista mais recente (token-2) continua focada: a autoplay ainda acontece.
    h.service
        .article_authorize(h.consent(&created))
        .await
        .unwrap();
    h.service.article_settle().await;
    assert_eq!(
        h.worker.streams().len(),
        1,
        "a vista mais recente continua focada"
    );
}

#[tokio::test]
async fn deleting_finishing_or_revoking_cancels_pending_player_work() {
    use language_assistant_tutor::article::PlayerState;
    let h = Harness::new().await;

    // Terminar.
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let request = h.worker.streams().last().cloned().unwrap();
    send_chunk(&h, &request, 0, chunk_path(&request, 0)).await;
    progress(&h, 200, 400).await;
    assert_eq!(playback(&h, &id).await, PlayerState::Playing);
    let generation = h.snapshot(&id).await.audio.generation;
    let cancels = h.worker.cancels();
    h.service.article_finish(h.cmd(&ready)).await.unwrap();
    assert!(h.worker.cancels() > cancels, "terminar cancela a síntese");
    let after = h.snapshot(&id).await.audio;
    assert_eq!(after.state, PlayerState::Idle);
    assert!(after.generation > generation, "terminar invalida a geração");
    // Um bloco tardio não toca.
    let appends = h.audio.appends().len();
    send_chunk(&h, &request, 1, chunk_path(&request, 1)).await;
    assert_eq!(h.audio.appends().len(), appends, "o bloco tardio não toca");

    // Eliminar.
    let ready = h.ready().await;
    let request = h.worker.streams().last().cloned().unwrap();
    send_chunk(&h, &request, 0, chunk_path(&request, 0)).await;
    let cancels = h.worker.cancels();
    h.service.article_delete(h.cmd(&ready)).await.unwrap();
    assert!(h.worker.cancels() > cancels, "eliminar cancela a síntese");
    // Um bloco tardio não toca depois de eliminar.
    let appends = h.audio.appends().len();
    send_chunk(&h, &request, 1, chunk_path(&request, 1)).await;
    assert_eq!(h.audio.appends().len(), appends, "o bloco tardio não toca");

    // Revogar.
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let request = h.worker.streams().last().cloned().unwrap();
    send_chunk(&h, &request, 0, chunk_path(&request, 0)).await;
    let generation = h.snapshot(&id).await.audio.generation;
    let cancels = h.worker.cancels();
    h.service.article_revoke(h.cmd(&ready)).await.unwrap();
    assert!(h.worker.cancels() > cancels, "revogar cancela a síntese");
    let after = h.snapshot(&id).await.audio;
    assert!(after.generation > generation, "revogar invalida a geração");
}

#[tokio::test]
async fn reopening_an_older_session_uses_defaults_and_never_plays() {
    use language_assistant_tutor::article::PlayerState;
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let request = h.worker.streams()[0].clone();
    send_chunk(&h, &request, 0, chunk_path(&request, 0)).await;
    progress(&h, 200, 400).await;
    assert_eq!(playback(&h, &id).await, PlayerState::Playing);

    // Recria o esquema anterior à tabela de preferências, mantendo a conversa.
    rusqlite::Connection::open(&h.database_path)
        .unwrap()
        .execute_batch("DROP TABLE article_preferences; PRAGMA user_version=5;")
        .unwrap();
    let audio = Arc::new(FakeAudio::default());
    let worker = Arc::new(FakeWorker::default());
    let transport = Arc::new(FakeTransport::default());
    // Reabrir: o leitor começa `idle`.
    let restarted = build_service(
        h.directory.path(),
        &h.database_path,
        &h.manifest,
        audio.clone(),
        worker.clone(),
        transport.clone(),
    );
    restarted.start().await;
    let snapshot = restarted.article_snapshot(&id).await.unwrap();
    assert_eq!(snapshot.audio.state, PlayerState::Idle);
    assert_eq!(snapshot.audio.generation, 0);
    assert_eq!(
        restarted.article_preferences_load().await.unwrap(),
        ArticlePreferences {
            autoplay: true,
            speed_x10: 10,
        }
    );
    assert!(audio.plays.lock().unwrap().is_empty());
    assert!(worker.streams().is_empty());
    assert!(transport.calls().is_empty());
}

// -- revisão: autoplay obsoleta, foco, saída e geração --------------------------

#[tokio::test]
async fn stale_autoplay_does_not_start_after_autoplay_is_disabled() {
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let tutor = ready.turns[0].id.clone();
    let gate = h.transport.hold_next();
    h.record(&ready).await;
    h.transcript_event("My answer.").await;
    h.wait_for_calls(2).await;

    let waiting = h.snapshot(&id).await;
    assert_eq!(waiting.session.state, ArticleState::AwaitingTutor);
    assert!(h.service.article_player_play(&id, &tutor).await.is_err());
    h.service
        .article_preferences_save(ArticlePreferences {
            autoplay: false,
            speed_x10: 10,
        })
        .await
        .unwrap();
    gate.notify_one();
    h.service.article_settle().await;

    assert_eq!(h.snapshot(&id).await.session.state, ArticleState::Ready);
    assert_eq!(
        h.worker.streams().len(),
        1,
        "a resposta chegou sem nova síntese"
    );
}

#[tokio::test]
async fn stale_autoplay_does_not_start_after_the_view_blurs() {
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let gate = h.transport.hold_next();
    h.record(&ready).await;
    h.transcript_event("My answer.").await;
    h.wait_for_calls(2).await;

    h.service.article_view_blur("harness").await.unwrap();
    gate.notify_one();
    h.service.article_settle().await;

    assert_eq!(h.snapshot(&id).await.session.state, ArticleState::Ready);
    assert_eq!(h.worker.streams().len(), 1, "sem autoplay depois do blur");
}

#[tokio::test]
async fn blur_stops_the_focused_session_playback() {
    use language_assistant_tutor::article::PlayerState;
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let request = h.worker.streams()[0].clone();
    send_chunk(&h, &request, 0, chunk_path(&request, 0)).await;
    progress(&h, 200, 400).await;
    assert_eq!(playback(&h, &id).await, PlayerState::Playing);

    h.service.article_view_blur("harness").await.unwrap();
    assert_eq!(playback(&h, &id).await, PlayerState::Idle);
    assert!(h.worker.cancels() >= 1, "o blur cancela a síntese");
}

#[tokio::test]
async fn focusing_another_session_stops_the_previous_player_even_without_a_prior_focus() {
    use language_assistant_tutor::article::PlayerState;
    let h = Harness::new().await;
    let created = h.create().await;
    let id = created.session.id.clone();
    // Autoriza sem focar: a abertura não toca sozinha.
    h.service
        .article_authorize(h.consent(&created))
        .await
        .unwrap();
    h.service.article_settle().await;
    let ready = h.snapshot(&id).await;
    assert_eq!(ready.session.state, ArticleState::Ready);
    let tutor = ready.turns[0].id.clone();

    // Reproduz explicitamente sem focar: o leitor pertence a esta sessão.
    h.service.article_player_play(&id, &tutor).await.unwrap();
    let request = h.worker.streams().last().cloned().unwrap();
    send_chunk(&h, &request, 0, chunk_path(&request, 0)).await;
    progress(&h, 200, 400).await;
    assert_eq!(playback(&h, &id).await, PlayerState::Playing);

    // Outra sessão toma o foco: a reprodução anterior pára, mesmo sem um foco
    // anterior registado.
    h.service
        .article_view_focus("other-session", "token")
        .await
        .unwrap();
    assert_eq!(playback(&h, &id).await, PlayerState::Idle);
}

#[tokio::test]
async fn completion_after_a_drain_restart_stays_buffering_until_audio_ends() {
    use language_assistant_tutor::article::PlayerState;
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let request = h.worker.streams()[0].clone();
    send_chunk_file(&h, &request, 0).await;
    progress(&h, 500, 500).await;
    // A fila esvazia.
    let first_id = h.audio.last_id();
    h.service
        .on_audio_event(AudioEvent::PlaybackFinished { id: first_id })
        .await;
    assert_eq!(playback(&h, &id).await, PlayerState::Buffering);
    // O bloco seguinte retoma a fila.
    send_chunk_file(&h, &request, 1).await;
    // A síntese termina antes de qualquer tick: continua `buffering`, não `ended`.
    finish_stream(&h, &request, 2).await;
    assert_eq!(playback(&h, &id).await, PlayerState::Buffering);
    assert_eq!(h.snapshot(&id).await.audio.duration_ms, Some(1000));
    // Um tick audível passa a `playing`.
    progress(&h, 300, 500).await;
    assert_eq!(playback(&h, &id).await, PlayerState::Playing);
    assert_eq!(h.snapshot(&id).await.audio.position_ms, 800);
    // O fim da fila conclui a rendição.
    let last_id = h.audio.last_id();
    h.service
        .on_audio_event(AudioEvent::PlaybackFinished { id: last_id })
        .await;
    assert_eq!(playback(&h, &id).await, PlayerState::Ended);
}

#[tokio::test]
async fn active_prefix_reuse_is_scoped_to_the_same_session() {
    use language_assistant_tutor::article::{ArticleMutation, PlayerState};
    let h = Harness::new().await;
    // Sessão A com uma síntese em curso e um prefixo já entregue.
    let a = h.ready().await;
    let request_a = h.worker.streams()[0].clone();
    send_chunk_file(&h, &request_a, 0).await;
    // Elimina A diretamente na base, sem passar pela limpeza do serviço: a
    // síntese de A continua viva em memória.
    h.database
        .apply_article(&h.cmd(&a), ArticleMutation::Delete)
        .unwrap();

    // Sessão B com o mesmo artigo (a abertura tem o mesmo texto).
    let b = h.create().await;
    let b_id = b.session.id.clone();
    h.service.article_authorize(h.consent(&b)).await.unwrap();
    h.service.article_settle().await;
    let b_snapshot = h.snapshot(&b_id).await;
    let b_turn = b_snapshot.turns[0].id.clone();
    let streams = h.worker.streams().len();

    // Reproduzir a abertura de B não reutiliza o prefixo da sessão A.
    let view = h.service.article_player_play(&b_id, &b_turn).await.unwrap();
    assert_eq!(
        h.worker.streams().len(),
        streams + 1,
        "síntese nova para a sessão B"
    );
    assert_eq!(view.state, PlayerState::Preparing);
}

#[tokio::test]
async fn finish_forces_a_stop_before_waiting_for_a_pending_play() {
    use language_assistant_tutor::article::PlayerState;
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let request = h.worker.streams()[0].clone();
    send_chunk(&h, &request, 0, chunk_path(&request, 0)).await;
    progress(&h, 200, 400).await;
    assert_eq!(playback(&h, &id).await, PlayerState::Playing);
    let generation = h.snapshot(&id).await.audio.generation;

    // Uma pausa pendente detém o coordenador serial.
    let release = h.audio.hold_pause();
    let service = Arc::clone(&h.service);
    let pause = tokio::spawn({
        let id = id.clone();
        async move {
            let _ = service.article_player_pause(&id, generation).await;
        }
    });
    for _ in 0..200 {
        if h.audio.pause_attempts() >= 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert_eq!(h.audio.pause_attempts(), 1, "a pausa ficou pendente");

    // Terminar pára o dispositivo antes de esperar pelo coordenador.
    let finish = {
        let service = Arc::clone(&h.service);
        let cmd = h.cmd(&ready);
        tokio::spawn(async move {
            let _ = service.article_finish(cmd).await;
        })
    };
    for _ in 0..200 {
        if h.audio.stops() > 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert!(h.audio.stops() > 0, "o dispositivo parou antes de esperar");

    // Liberta a pausa e o término; o resultado tardio fica invalidado.
    drop(release);
    pause.await.unwrap();
    finish.await.unwrap();
    let after = h.snapshot(&id).await.audio;
    assert_eq!(after.state, PlayerState::Idle);
    assert!(after.generation > generation, "a geração foi invalidada");
}

#[tokio::test]
async fn a_completion_with_more_chunks_than_accepted_is_not_cached_as_complete() {
    use language_assistant_tutor::article::PlayerState;
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let text = ready.turns[0].version.text.clone();
    let request = h.worker.streams()[0].clone();
    send_chunk_file(&h, &request, 0).await;
    // O auxiliar reporta mais blocos do que os aceites: rendição incompleta.
    finish_stream(&h, &request, 2).await;
    assert_eq!(playback(&h, &id).await, PlayerState::Failed);
    let session_dir = h.directory.path().join("articles").join(&id);
    let key = voice_key(&text, "af_heart", 10, ENGINE_ID);
    assert!(
        read_cache(&session_dir, &key).is_none(),
        "sem cache completa"
    );
}

#[tokio::test]
async fn autoplay_respects_the_shared_audio_owner() {
    let h = Harness::new().await;
    let created = h.create().await;
    let id = created.session.id.clone();
    h.service.article_view_focus(&id, "token").await.unwrap();
    // O STAR detém o áudio (gravação em curso).
    h.service
        .start_session(StartSession {
            mode: Default::default(),
            request_id: uuid::Uuid::new_v4().to_string(),
            category: None,
            count: 1,
        })
        .await
        .unwrap();
    let star = h.service.snapshot();
    let question = star.current.as_ref().unwrap().question.id.clone();
    let selection = star.current_answer.as_ref().unwrap().selection_version;
    h.service
        .start_recording(question, selection)
        .await
        .unwrap();

    // A abertura do artigo não toca enquanto o STAR grava.
    h.service
        .article_authorize(h.consent(&created))
        .await
        .unwrap();
    h.service.article_settle().await;
    assert_eq!(
        h.worker.streams().len(),
        0,
        "a autoplay respeita o dono do áudio"
    );
    assert_eq!(h.snapshot(&id).await.session.state, ArticleState::Ready);
}

#[tokio::test]
async fn play_errors_do_not_expose_paths() {
    use language_assistant_tutor::article::ArticleSpeaker;
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    h.record(&ready).await;
    h.transcript("My answer.").await;
    let snapshot = h.snapshot(&id).await;
    let user = snapshot
        .turns
        .iter()
        .find(|t| t.speaker == ArticleSpeaker::User)
        .unwrap()
        .id
        .clone();
    // Corrompe o original para provocar INVALID_AUDIO.
    let original = h
        .directory
        .path()
        .join("articles")
        .join(&id)
        .join(&user)
        .join("original.wav");
    std::fs::write(&original, b"not a wav").unwrap();

    let error = h.service.article_player_play(&id, &user).await.unwrap_err();
    assert_eq!(error.code, error_code::INVALID_AUDIO);
    assert!(
        !error.message.contains('\\'),
        "sem caminho: {}",
        error.message
    );
    assert!(
        !error.message.contains(':'),
        "sem unidade: {}",
        error.message
    );
    assert!(
        !error.message.contains("original.wav"),
        "sem ficheiro: {}",
        error.message
    );
}

#[tokio::test]
async fn an_output_failure_cancels_the_pending_synthesis() {
    use language_assistant_tutor::article::{PlayerOperation, PlayerState};
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let request = h.worker.streams()[0].clone();
    // O dispositivo falha ao iniciar a reprodução do primeiro bloco.
    h.audio.fail_start();
    send_chunk(&h, &request, 0, chunk_path(&request, 0)).await;

    // A falha de saída cancela a síntese pendente e é rotulada `Output`.
    assert!(h.worker.cancels() >= 1, "a síntese pendente é cancelada");
    let view = h.snapshot(&id).await.audio;
    assert_eq!(view.state, PlayerState::Failed);
    assert_eq!(
        view.failure.as_ref().map(|f| f.operation),
        Some(PlayerOperation::Output)
    );

    // Um bloco tardio não toca.
    let appends = h.audio.appends().len();
    send_chunk(&h, &request, 1, chunk_path(&request, 1)).await;
    assert_eq!(h.audio.appends().len(), appends, "o bloco tardio não toca");
}

#[tokio::test]
async fn a_cached_play_device_failure_is_labelled_output() {
    use language_assistant_tutor::article::{PlayerOperation, PlayerState};
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let tutor = ready.turns[0].id.clone();
    // Conclui a síntese automática para a cache ficar completa.
    let request = h.worker.streams()[0].clone();
    send_chunk_file(&h, &request, 0).await;
    finish_stream(&h, &request, 1).await;
    h.service.article_stop_voice().await.unwrap();

    // O dispositivo falha ao reproduzir a entrada da cache.
    h.audio.fail_start();
    let error = h
        .service
        .article_player_play(&id, &tutor)
        .await
        .unwrap_err();
    assert_eq!(error.code, error_code::DEVICE_UNAVAILABLE);
    let view = h.snapshot(&id).await.audio;
    assert_eq!(view.state, PlayerState::Failed);
    assert_eq!(
        view.failure.as_ref().map(|f| f.operation),
        Some(PlayerOperation::Output)
    );
}

// -- «Responder agora» e o ciclo de vida da gravação --------------------------

#[tokio::test]
async fn respond_now_stops_output_before_capture() {
    let h = Harness::new().await;
    let ready = h.ready().await;
    h.service
        .article_start_recording(h.cmd(&ready))
        .await
        .unwrap();
    assert_eq!(h.order.steps(), vec!["stop", "cancel", "start_capture"]);
}

#[tokio::test]
async fn a_capture_failure_does_not_restore_voice() {
    use language_assistant_tutor::article::{ArticleRecovery, PlayerState};
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let streams = h.worker.streams().len();
    let plays = h.audio.plays().len();

    h.audio.fail_capture();
    let error = h
        .service
        .article_start_recording(h.cmd(&ready))
        .await
        .unwrap_err();
    assert_eq!(error.code, error_code::DEVICE_UNAVAILABLE);

    // A voz não volta: sem nova síntese nem reprodução, e o leitor fica inativo.
    assert_eq!(h.worker.streams().len(), streams, "sem nova síntese");
    assert_eq!(h.audio.plays().len(), plays, "sem nova reprodução");
    assert_eq!(h.snapshot(&id).await.audio.state, PlayerState::Idle);
    // A falha fica recuperável: gravar de novo.
    let after = h.snapshot(&id).await;
    assert_eq!(after.session.state, ArticleState::RecoverableError);
    assert_eq!(after.session.recovery, Some(ArticleRecovery::ReRecord));
}

#[tokio::test]
async fn a_duplicate_respond_now_starts_one_capture_and_one_turn() {
    let h = Harness::new().await;
    let ready = h.ready().await;
    let start = h.cmd(&ready);
    let first = h
        .service
        .article_start_recording(start.clone())
        .await
        .unwrap();
    let second = h.service.article_start_recording(start).await.unwrap();
    assert_eq!(h.audio.starts(), 1, "uma só captura");
    let turns: i64 = h
        .database
        .with_connection(|c| {
            Ok(c.query_row(
                "SELECT count(*) FROM article_turns WHERE speaker = 'user'",
                [],
                |r| r.get(0),
            )
            .unwrap())
        })
        .unwrap();
    assert_eq!(turns, 1, "um só turno do utilizador");
    assert_eq!(first.session.version, second.session.version);
}

#[tokio::test]
async fn recording_events_carry_capture_identity_and_sequence() {
    use language_assistant_tutor::article::{ArticleRecordingView, RecordingState};
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let events: Arc<Mutex<Vec<ArticleRecordingView>>> = Default::default();
    let sink = events.clone();
    h.service
        .subscribe_article_recording(Arc::new(move |view: &ArticleRecordingView| {
            sink.lock().unwrap().push(view.clone());
        }));

    h.service
        .article_start_recording(h.cmd(&ready))
        .await
        .unwrap();
    let first = events.lock().unwrap().clone();
    assert_eq!(first.len(), 1, "o reconhecimento publica um evento");
    let view = &first[0];
    assert_eq!(view.state, RecordingState::Active);
    assert_eq!(view.session_id, id);
    assert!(!view.capture_id.is_empty(), "capture_id novo");
    assert!(!view.turn_id.is_empty());
    assert_eq!(view.sequence, 1);
    assert_eq!(view.elapsed_ms, 0);
    assert_eq!(view.level, None);

    // O progresso da captura atual avança a sequência, mantendo a identidade.
    h.service
        .on_audio_event(AudioEvent::CaptureProgress {
            recording_id: Some(view.turn_id.clone()),
            elapsed_ms: 120,
            level: Some(0.4),
        })
        .await;
    let second = events.lock().unwrap().clone();
    assert_eq!(second.len(), 2);
    assert_eq!(second[1].capture_id, view.capture_id);
    assert_eq!(second[1].sequence, 2);
    assert_eq!(second[1].state, RecordingState::Active);
}

#[tokio::test]
async fn a_resubscription_gets_the_current_recording_or_inactive() {
    use language_assistant_tutor::article::RecordingState;
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();

    assert_eq!(
        h.snapshot(&id).await.recording.state,
        RecordingState::Inactive
    );

    let started = h
        .service
        .article_start_recording(h.cmd(&ready))
        .await
        .unwrap();
    let active = h.snapshot(&id).await.recording;
    assert_eq!(active.state, RecordingState::Active);
    assert!(!active.capture_id.is_empty());
    assert!(!active.turn_id.is_empty());

    h.service
        .article_stop_recording(h.cmd(&started))
        .await
        .unwrap();
    let ended = h.snapshot(&id).await.recording;
    assert_eq!(ended.state, RecordingState::Ended);
    assert_eq!(ended.elapsed_ms, 500, "o tempo final vem da captura");
    assert_eq!(ended.level, None);

    h.transcript("Remote work helps people focus.").await;
    assert_eq!(
        h.snapshot(&id).await.recording.state,
        RecordingState::Inactive
    );
}

#[tokio::test]
async fn a_late_active_event_cannot_revive_a_finished_capture() {
    use language_assistant_tutor::article::{ArticleRecordingView, RecordingState};
    let h = Harness::new().await;
    let ready = h.ready().await;
    let events: Arc<Mutex<Vec<ArticleRecordingView>>> = Default::default();
    let sink = events.clone();
    h.service
        .subscribe_article_recording(Arc::new(move |view: &ArticleRecordingView| {
            sink.lock().unwrap().push(view.clone());
        }));
    let started = h
        .service
        .article_start_recording(h.cmd(&ready))
        .await
        .unwrap();
    let old_turn_id = events.lock().unwrap()[0].turn_id.clone();

    h.service
        .article_stop_recording(h.cmd(&started))
        .await
        .unwrap();
    assert_eq!(
        events.lock().unwrap().last().unwrap().state,
        RecordingState::Ended
    );
    let len = events.lock().unwrap().len();
    let sequence = events.lock().unwrap().last().unwrap().sequence;

    // Um progresso tardio da captura já terminada não revive a gravação.
    h.service
        .on_audio_event(AudioEvent::CaptureProgress {
            recording_id: Some(old_turn_id),
            elapsed_ms: 999,
            level: Some(0.9),
        })
        .await;
    let now = events.lock().unwrap().clone();
    assert_eq!(now.len(), len, "sem novo evento");
    assert_eq!(now.last().unwrap().sequence, sequence);
    assert_eq!(now.last().unwrap().state, RecordingState::Ended);
}

#[tokio::test]
async fn recording_progress_uses_native_elapsed_and_real_level() {
    use language_assistant_tutor::article::{ArticleRecordingView, RecordingState};
    let h = Harness::new().await;
    let ready = h.ready().await;
    let events: Arc<Mutex<Vec<ArticleRecordingView>>> = Default::default();
    let sink = events.clone();
    h.service
        .subscribe_article_recording(Arc::new(move |view: &ArticleRecordingView| {
            sink.lock().unwrap().push(view.clone());
        }));
    h.service
        .article_start_recording(h.cmd(&ready))
        .await
        .unwrap();
    let turn_id = events.lock().unwrap()[0].turn_id.clone();

    h.service
        .on_audio_event(AudioEvent::CaptureProgress {
            recording_id: Some(turn_id),
            elapsed_ms: 2500,
            level: Some(0.75),
        })
        .await;
    let view = events.lock().unwrap().last().unwrap().clone();
    assert_eq!(view.elapsed_ms, 2500);
    assert_eq!(view.level, Some(0.75));
    assert_eq!(view.state, RecordingState::Active);
}

#[tokio::test]
async fn microphone_loss_keeps_the_recoverable_recording_without_a_new_take() {
    use language_assistant_tutor::article::{ArticleRecordingView, RecordingState};
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let events: Arc<Mutex<Vec<ArticleRecordingView>>> = Default::default();
    let sink = events.clone();
    h.service
        .subscribe_article_recording(Arc::new(move |view: &ArticleRecordingView| {
            sink.lock().unwrap().push(view.clone());
        }));
    h.service
        .article_start_recording(h.cmd(&ready))
        .await
        .unwrap();
    let turn_id = events.lock().unwrap()[0].turn_id.clone();

    // O microfone cai a meio: áudio parcial recuperável.
    let partial = CapturedAudio {
        id: turn_id.clone(),
        original: h
            .directory
            .path()
            .join("articles")
            .join(&id)
            .join(&turn_id)
            .join("original.wav"),
        meta: CaptureMeta {
            sample_rate: 48_000,
            channels: 1,
            duration_ms: 300,
            bytes: 1440,
            incomplete: true,
        },
    };
    h.service
        .on_audio_event(AudioEvent::CaptureInterrupted {
            recording_id: Some(turn_id),
            capture: Some(partial),
            error: AppError::new(error_code::CAPTURE_INTERRUPTED, "microfone perdido.", true),
        })
        .await;

    // A gravação recuperável fica guardada e não obriga a uma nova gravação:
    // segue para revisão, com o áudio disponível e a vista em Inactive.
    let after = h.snapshot(&id).await;
    assert_eq!(after.session.state, ArticleState::NeedsReview);
    assert_eq!(after.session.recovery, None);
    assert!(after
        .turns
        .iter()
        .any(|t| t.speaker == ArticleSpeaker::User && t.audio_available));
    assert_eq!(after.recording.state, RecordingState::Inactive);
}

#[tokio::test]
async fn meter_values_are_not_persisted_or_logged() {
    use language_assistant_tutor::article::{ArticleRecordingView, RecordingState};
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let events: Arc<Mutex<Vec<ArticleRecordingView>>> = Default::default();
    let sink = events.clone();
    h.service
        .subscribe_article_recording(Arc::new(move |view: &ArticleRecordingView| {
            sink.lock().unwrap().push(view.clone());
        }));
    let started = h
        .service
        .article_start_recording(h.cmd(&ready))
        .await
        .unwrap();
    let turn_id = events.lock().unwrap()[0].turn_id.clone();

    // Progresso com nível e tempo reais: chegam à vista em memória…
    h.service
        .on_audio_event(AudioEvent::CaptureProgress {
            recording_id: Some(turn_id),
            elapsed_ms: 987_654,
            level: Some(0.625),
        })
        .await;
    assert_eq!(
        events.lock().unwrap().last().unwrap().level,
        Some(0.625),
        "o nível real chega à vista"
    );

    // …mas nunca são persistidos: a fotografia da base reconstrói sempre a
    // gravação como `Inactive`, sem nível nem tempo. O registo estruturado de
    // diagnóstico só guarda operação/código/duração (`record_metric`), nunca o
    // nível nem o tempo da gravação — coberto também pelo teste
    // `turn_metrics_use_closed_operations_with_durations_and_no_content`.
    h.service
        .article_stop_recording(h.cmd(&started))
        .await
        .unwrap();
    h.transcript("Remote work helps people focus.").await;
    let persisted = h.database.article_snapshot(&id).unwrap();
    assert_eq!(persisted.recording.state, RecordingState::Inactive);
    assert_eq!(persisted.recording.level, None);
    assert_eq!(persisted.recording.elapsed_ms, 0);
}

#[tokio::test]
async fn a_failed_output_stop_does_not_start_capture() {
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();

    h.audio.fail_stop();
    let error = h
        .service
        .article_start_recording(h.cmd(&ready))
        .await
        .unwrap_err();
    assert_eq!(error.code, error_code::DEVICE_UNAVAILABLE);
    assert_eq!(h.audio.starts(), 0, "a captura nunca começa");
    assert_eq!(
        h.snapshot(&id).await.session.state,
        ArticleState::Ready,
        "a conversa continua por gravar"
    );
}

#[tokio::test]
async fn a_replay_queued_before_revocation_cannot_start_voice_after_it() {
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let turn = ready.turns[0].id.clone();
    h.service.article_revoke(h.cmd(&ready)).await.unwrap();
    let streams = h.worker.streams().len();
    let plays = h.audio.plays().len();
    let error = h.service.article_player_play(&id, &turn).await.unwrap_err();
    assert_eq!(error.code, error_code::INVALID_INPUT);
    assert_eq!(h.worker.streams().len(), streams);
    assert_eq!(h.audio.plays().len(), plays);
}

#[tokio::test]
async fn a_missing_original_is_unavailable_without_selecting_the_player() {
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    h.record(&ready).await;
    h.transcript("A saved answer.").await;
    let reply = h.snapshot(&id).await;
    let turn = &reply.turns[1];
    assert!(turn.audio_available);
    std::fs::remove_file(
        h.directory
            .path()
            .join("articles")
            .join(&id)
            .join(&turn.id)
            .join("original.wav"),
    )
    .unwrap();
    let before = h.snapshot(&id).await;
    assert!(!before.turns[1].audio_available);
    assert!(h.service.article_player_play(&id, &turn.id).await.is_err());
    assert_eq!(h.snapshot(&id).await.audio, before.audio);
    assert!(h.audio.plays().is_empty());
}

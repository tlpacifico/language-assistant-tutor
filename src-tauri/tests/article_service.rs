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
    ArticleCommand, ArticleConsent, ArticleCreate, ArticleSnapshot, ArticleSpeaker, ArticleState,
    ARTICLE_NOTICE_VERSION,
};
use language_assistant_tutor::artifacts::ArtifactStore;
use language_assistant_tutor::audio::{
    AudioPort, AvailabilityAudio, CapturedAudio, RecordingTarget,
};
use language_assistant_tutor::db::Database;
use language_assistant_tutor::domain::{
    error_code, AppError, CaptureMeta, ComponentState, TranscriptStatus,
};
use language_assistant_tutor::interview::StartSession;
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

#[derive(Default)]
struct FakeAudio {
    capture: Mutex<Option<RecordingTarget>>,
    starts: Mutex<usize>,
    plays: Mutex<Vec<PathBuf>>,
    appends: Mutex<Vec<PathBuf>>,
    stops: Mutex<usize>,
}

impl FakeAudio {
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
}

impl AudioPort for FakeAudio {
    fn availability(&self) -> AvailabilityAudio {
        AvailabilityAudio {
            microphone: ComponentState::ready(),
            playback: ComponentState::ready(),
        }
    }
    fn start_capture(&self, target: RecordingTarget) -> Result<(), AppError> {
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
    fn play(&self, path: PathBuf) -> Result<(), AppError> {
        self.plays.lock().unwrap().push(path);
        Ok(())
    }
    fn append(&self, path: PathBuf) -> Result<(), AppError> {
        self.appends.lock().unwrap().push(path);
        Ok(())
    }
    fn stop_playback(&self) -> Result<(), AppError> {
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
    cancels: Mutex<usize>,
}

impl FakeWorker {
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
    fn submit(&self, request: WorkerRequest) -> Result<(), AppError> {
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
        let audio = Arc::new(FakeAudio::default());
        let worker = Arc::new(FakeWorker::default());
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
    assert_eq!(stopped.turns.len(), 1, "o turno sem texto não aparece");
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
    h.service.article_delete(h.cmd(&pending)).await.unwrap();
    gate.notify_one();
    h.service.article_settle().await;
    assert!(h.service.article_snapshot(&id).await.is_err());
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

async fn playback(h: &Harness, id: &str) -> language_assistant_tutor::article::ArticlePlayback {
    h.snapshot(id).await.audio.playback
}

#[tokio::test]
async fn a_new_tutor_turn_is_spoken_in_ordered_chunks_and_bad_chunks_stop_the_voice() {
    use language_assistant_tutor::article::ArticlePlayback;
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
    assert_eq!(playback(&h, &id).await, ArticlePlayback::Playing);

    send_chunk(&h, &request, 0, chunk_path(&request, 0)).await;
    send_chunk(&h, &request, 1, chunk_path(&request, 1)).await;
    assert_eq!(
        h.audio.appends(),
        vec![chunk_path(&request, 0), chunk_path(&request, 1)]
    );
    // Duplicado: inofensivo, não toca duas vezes nem pára a voz.
    send_chunk(&h, &request, 0, chunk_path(&request, 0)).await;
    assert_eq!(h.audio.appends().len(), 2);
    assert_eq!(playback(&h, &id).await, ArticlePlayback::Playing);

    // Fora de ordem ou fora da pasta do pedido: a voz falha e o auxiliar é parado.
    send_chunk(&h, &request, 5, chunk_path(&request, 5)).await;
    assert_eq!(playback(&h, &id).await, ArticlePlayback::Failed);
    assert_eq!(h.audio.appends().len(), 2);
    assert!(h.worker.cancels() >= 1);
    // O texto continua visível.
    assert!(h.snapshot(&id).await.turns[0]
        .version
        .text
        .contains("main argument"));
}

#[tokio::test]
async fn an_outside_path_is_refused_without_playing_it() {
    use language_assistant_tutor::article::ArticlePlayback;
    let h = Harness::new().await;
    let ready = h.ready().await;
    let request = h.worker.streams()[0].clone();
    send_chunk(&h, &request, 0, PathBuf::from(r"C:\Windows\chunk-0000.wav")).await;
    assert!(h.audio.appends().is_empty());
    assert_eq!(
        playback(&h, &ready.session.id).await,
        ArticlePlayback::Failed
    );
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
    use language_assistant_tutor::article::ArticlePlayback;
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let request = h.worker.streams()[0].clone();
    send_chunk(&h, &request, 0, chunk_path(&request, 0)).await;
    let stops = h.audio.stops();

    h.service.article_stop_voice().await.unwrap();
    assert_eq!(h.worker.cancels(), 1);
    assert!(h.audio.stops() > stops);
    assert_eq!(playback(&h, &id).await, ArticlePlayback::Idle);

    // Um bloco tardio do pedido cancelado não toca.
    send_chunk(&h, &request, 1, chunk_path(&request, 1)).await;
    assert_eq!(h.audio.appends().len(), 1);
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
    assert_eq!(h.audio.appends().len(), 1, "o bloco tardio não toca");
}

#[tokio::test]
async fn a_failed_voice_keeps_the_text_and_repeats_without_the_text_provider() {
    use language_assistant_tutor::article::ArticlePlayback;
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
    assert_eq!(playback(&h, &id).await, ArticlePlayback::Failed);
    let calls = h.transport.calls().len();
    let tutor = h.snapshot(&id).await.turns[0].id.clone();
    assert!(h.snapshot(&id).await.turns[0]
        .version
        .text
        .contains("main argument"));

    h.service.article_speak_turn(&id, &tutor).await.unwrap();
    assert_eq!(h.worker.streams().len(), 2, "nova síntese, nova pasta");
    assert_ne!(
        h.worker.streams()[0].params["outputDir"],
        h.worker.streams()[1].params["outputDir"]
    );
    assert_eq!(h.transport.calls().len(), calls, "sem nova chamada IA");
    assert_eq!(playback(&h, &id).await, ArticlePlayback::Playing);
    // Só falas do tutor da conversa ativa se repetem.
    assert!(h.service.article_speak_turn(&id, "missing").await.is_err());
}

#[tokio::test]
async fn a_momentarily_empty_queue_is_not_the_end_of_the_request() {
    use language_assistant_tutor::article::ArticlePlayback;
    use language_assistant_tutor::audio::AudioEvent;
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let request = h.worker.streams()[0].clone();
    send_chunk(&h, &request, 0, chunk_path(&request, 0)).await;
    // A fila esvazia antes do 2.º bloco: continua a tocar.
    h.service.on_audio_event(AudioEvent::PlaybackFinished).await;
    assert_eq!(playback(&h, &id).await, ArticlePlayback::Playing);
    send_chunk(&h, &request, 1, chunk_path(&request, 1)).await;
    finish_stream(&h, &request, 2).await;
    // Só termina quando a síntese acabou e a fila voltou a esvaziar.
    assert_eq!(playback(&h, &id).await, ArticlePlayback::Playing);
    h.service.on_audio_event(AudioEvent::PlaybackFinished).await;
    assert_eq!(playback(&h, &id).await, ArticlePlayback::Idle);
}

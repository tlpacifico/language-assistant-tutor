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

#[derive(Default)]
struct FakeAudio {
    capture: Mutex<Option<RecordingTarget>>,
    starts: Mutex<usize>,
    plays: Mutex<Vec<PathBuf>>,
    appends: Mutex<Vec<PathBuf>>,
    stops: Mutex<usize>,
    last_id: Mutex<Option<PlaybackId>>,
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
    /// A identidade da última reprodução iniciada, para simular o fim.
    fn last_id(&self) -> PlaybackId {
        self.last_id
            .lock()
            .unwrap()
            .clone()
            .expect("reprodução iniciada")
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
    fn start_playback(&self, path: PathBuf) -> Result<PlaybackId, AppError> {
        let id = uuid::Uuid::new_v4().to_string();
        self.plays.lock().unwrap().push(path);
        *self.last_id.lock().unwrap() = Some(id.clone());
        Ok(id)
    }
    fn append_chunk(&self, path: PathBuf, _id: &PlaybackId) -> Result<(), AppError> {
        self.appends.lock().unwrap().push(path);
        Ok(())
    }
    fn pause_playback(&self, _id: &PlaybackId) -> Result<(), AppError> {
        Ok(())
    }
    fn resume_playback(&self, _id: &PlaybackId) -> Result<(), AppError> {
        Ok(())
    }
    fn stop_playback(&self, _id: &PlaybackId) -> Result<(), AppError> {
        *self.stops.lock().unwrap() += 1;
        Ok(())
    }
    fn stop_all_playback(&self) -> Result<(), AppError> {
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

    // Fora de ordem ou fora da pasta do pedido: a voz falha e o auxiliar é parado.
    send_chunk(&h, &request, 5, chunk_path(&request, 5)).await;
    assert_eq!(playback(&h, &id).await, PlayerState::Failed);
    assert_eq!(h.audio.appends().len(), 1);
    assert_eq!(h.audio.plays().len(), 1);
    assert!(h.worker.cancels() >= 1);
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
async fn starting_star_playback_stops_article_voice() {
    use language_assistant_tutor::article::PlayerState;
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let request = h.worker.streams()[0].clone();
    // Primeiro bloco: a voz do artigo fica em reprodução.
    send_chunk(&h, &request, 0, chunk_path(&request, 0)).await;
    progress(&h, 200, 400).await;
    assert_eq!(playback(&h, &id).await, PlayerState::Playing);

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
            "article_reply",
            "article_transcribe"
        ]
    );
    // O registo é global ao processo: outros testes também escrevem nele, por
    // isso cada operação exige pelo menos um evento bem-sucedido e todos têm duração.
    for event in &mine {
        assert!(event["durationMs"].is_u64(), "{event}");
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
    finish_stream(&h, &request, 1).await;
    assert_eq!(playback(&h, &id).await, PlayerState::Paused);
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

    h.service
        .on_audio_event(AudioEvent::PlaybackFinished { id: first_id })
        .await;
    let view = h.snapshot(&id).await.audio;
    assert_eq!(view.turn_id.as_deref(), Some(users[1].as_str()));
    assert_eq!(view.source, Some(PlayerSource::UserRecording));
    assert_ne!(view.state, PlayerState::Idle);
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

    // Uma repetição explícita continua a funcionar.
    let view = h.service.article_player_play(&id, &tutor).await.unwrap();
    assert_eq!(view.state, PlayerState::Preparing);
    assert_eq!(view.trigger, Some(PlaybackTrigger::Explicit));
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
    let cancels = h.worker.cancels();
    h.service.article_finish(h.cmd(&ready)).await.unwrap();
    assert!(h.worker.cancels() > cancels, "terminar cancela a síntese");
    assert_eq!(playback(&h, &id).await, PlayerState::Idle);

    // Eliminar.
    let ready = h.ready().await;
    let request = h.worker.streams().last().cloned().unwrap();
    send_chunk(&h, &request, 0, chunk_path(&request, 0)).await;
    let cancels = h.worker.cancels();
    h.service.article_delete(h.cmd(&ready)).await.unwrap();
    assert!(h.worker.cancels() > cancels, "eliminar cancela a síntese");

    // Revogar.
    let ready = h.ready().await;
    let request = h.worker.streams().last().cloned().unwrap();
    send_chunk(&h, &request, 0, chunk_path(&request, 0)).await;
    let cancels = h.worker.cancels();
    h.service.article_revoke(h.cmd(&ready)).await.unwrap();
    assert!(h.worker.cancels() > cancels, "revogar cancela a síntese");
}

#[tokio::test]
async fn reopening_starts_the_player_idle() {
    use language_assistant_tutor::article::PlayerState;
    let h = Harness::new().await;
    let ready = h.ready().await;
    let id = ready.session.id.clone();
    let request = h.worker.streams()[0].clone();
    send_chunk(&h, &request, 0, chunk_path(&request, 0)).await;
    progress(&h, 200, 400).await;
    assert_eq!(playback(&h, &id).await, PlayerState::Playing);

    // Reabrir: o leitor começa `idle`.
    let restarted = build_service(
        h.directory.path(),
        &h.database_path,
        &h.manifest,
        Arc::new(FakeAudio::default()),
        Arc::new(FakeWorker::default()),
        Arc::new(FakeTransport::default()),
    );
    restarted.start().await;
    let snapshot = restarted.article_snapshot(&id).await.unwrap();
    assert_eq!(snapshot.audio.state, PlayerState::Idle);
    assert_eq!(snapshot.audio.generation, 0);
}

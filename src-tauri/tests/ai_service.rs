use language_assistant_tutor::{
    ai::*,
    artifacts::ArtifactStore,
    audio::*,
    db::Database,
    domain::{AppError, ComponentState},
    interview::*,
    service::*,
};
use serde_json::json;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

#[derive(Default)]
struct Store(Mutex<Option<String>>);
impl CredentialStore for Store {
    fn get(&self) -> language_assistant_tutor::ai::Result<Option<String>> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn set(&self, key: &str) -> language_assistant_tutor::ai::Result<()> {
        *self.0.lock().unwrap() = Some(key.into());
        Ok(())
    }
    fn delete(&self) -> language_assistant_tutor::ai::Result<()> {
        *self.0.lock().unwrap() = None;
        Ok(())
    }
}
struct Launcher;
impl WorkerLauncher for Launcher {
    fn launch(&self) -> std::result::Result<LaunchedWorker, AppError> {
        Err(failure("Sem modelos no teste."))
    }
}
#[derive(Default)]
struct Audio(Mutex<Option<RecordingTarget>>);
impl AudioPort for Audio {
    fn availability(&self) -> AvailabilityAudio {
        AvailabilityAudio {
            microphone: ComponentState::ready(),
            playback: ComponentState::ready(),
        }
    }
    fn start_capture(&self, t: RecordingTarget) -> std::result::Result<(), AppError> {
        *self.0.lock().unwrap() = Some(t);
        Ok(())
    }
    fn stop_capture(&self) -> std::result::Result<CapturedAudio, AppError> {
        let t = self.0.lock().unwrap().take().unwrap();
        let path = t.directory.join("original.wav");
        let mut w = hound::WavWriter::create(
            &path,
            hound::WavSpec {
                channels: 1,
                sample_rate: 16000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for _ in 0..8000 {
            w.write_sample(0i16).unwrap();
        }
        w.finalize().unwrap();
        Ok(CapturedAudio {
            id: t.id,
            original: path,
            meta: language_assistant_tutor::artifacts::inspect_file(
                &t.directory.join("original.wav"),
            )?,
        })
    }
    fn play(&self, _: PathBuf) -> std::result::Result<(), AppError> {
        Ok(())
    }
    fn stop_playback(&self) -> std::result::Result<(), AppError> {
        Ok(())
    }
    fn reopen(&self) -> std::result::Result<(), AppError> {
        Ok(())
    }
}
struct Transport {
    calls: AtomicUsize,
    gate: Arc<tokio::sync::Notify>,
    script: Mutex<Vec<String>>,
}
impl Transport {
    fn new(script: Vec<String>) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            gate: Arc::new(tokio::sync::Notify::new()),
            script: Mutex::new(script),
        }
    }
}
impl AiTransport for Transport {
    fn complete(
        &self,
        r: TransportRequest,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = language_assistant_tutor::ai::Result<RemoteCompletion>>
                + Send,
        >,
    > {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        let value = self
            .script
            .lock()
            .unwrap()
            .get(n)
            .cloned()
            .unwrap_or("gate".into());
        let future = self.gate.clone().notified_owned();
        let s = r.settings.clone();
        Box::pin(async move {
            if value == "gate" {
                future.await;
            }
            if value == "error" {
                return Err(failure("Rede falhou."));
            }
            let content = if value == "gate" || value == "ask" {
                json!({"schemaVersion":SCHEMA,"decision":"ask","reason":"Falta resultado","missingDimension":"result","question":"What happened as a result?"}).to_string()
            } else if value == "report" {
                valid_report(&r.sources)
            } else {
                value
            };
            Ok(RemoteCompletion {
                model: s.model,
                content,
                usage: NormalizedUsage {
                    input: Some(100),
                    output: Some(20),
                    reported: Some(0.01),
                    ..Default::default()
                },
                invalid: false,
            })
        })
    }
}
fn valid_report(s: &AiSources) -> String {
    json!({"schemaVersion":SCHEMA,"content":(["relevance","situation","individual_responsibility","actions","result","learning"].iter().map(|d|json!({"id":d,"dimension":d,"rating":"partial","explanation":"Conteúdo parcial no texto.","evidence":[{"type":"quote","confirmationId":s.answers[0].confirmation_id,"role":"initial","quote":s.answers[0].text}]})).collect::<Vec<_>>()),"strengths":[],"gaps":[],"english":{"necessaryCorrections":[],"optionalStyle":[]},"priorities":[],"fewerPrioritiesReason":"Não se identificaram outras prioridades sustentadas."}).to_string()
}
struct Harness {
    _dir: tempfile::TempDir,
    db: Database,
    service: Arc<AppService>,
    transport: Arc<Transport>,
    store: Arc<Store>,
}
impl Harness {
    async fn new(count: u32, script: Vec<String>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.sqlite");
        let db = Database::open(path.clone()).unwrap();
        let transport = Arc::new(Transport::new(script));
        let store = Arc::new(Store::default());
        let service = Arc::new(
            AppService::new(
                ServicePaths {
                    database: path,
                    legacy: dir.path().join("legacy"),
                },
                ArtifactStore::new(dir.path().join("sessions")),
                Arc::new(Audio::default()),
                Arc::new(Launcher),
                None,
            )
            .with_ai(transport.clone(), store.clone()),
        );
        service.start().await;
        service
            .start_session(StartSession {
                request_id: uuid::Uuid::new_v4().to_string(),
                category: None,
                count,
                mode: SessionMode::AiV3,
            })
            .await
            .unwrap();
        Self {
            _dir: dir,
            db,
            service,
            transport,
            store,
        }
    }
    async fn consent(&self) {
        self.service
            .save_ai_key("TEST-SECRET".into())
            .await
            .unwrap();
        let s = self.service.load_ai_settings().await.unwrap();
        self.service
            .accept_ai_consent(AcceptAiConsent {
                request_id: uuid::Uuid::new_v4().to_string(),
                expected_settings_version: s.version,
                destination: s.base_url,
                notice_version: "text-external-v1".into(),
            })
            .await
            .unwrap();
    }
    fn command(&self) -> AiQuestionCommand {
        let s = self.service.snapshot();
        AiQuestionCommand {
            request_id: uuid::Uuid::new_v4().to_string(),
            session_id: s.active_session.unwrap().id,
            session_question_id: s.current.unwrap().question.id,
            expected_session_version: self.service.snapshot().active_session.unwrap().version,
        }
    }
    async fn answer(&self) {
        let s = self.service.snapshot();
        let q = s.current.unwrap();
        let a = q.active();
        self.service
            .start_recording(a.question.id.clone(), a.question.selection_version)
            .await
            .unwrap();
        self.service.stop_recording().await.unwrap();
        let s = self.service.snapshot();
        let q = s.current.unwrap();
        let a = q.active();
        let id = a.question.selected_recording_id.clone().unwrap();
        self.service
            .confirm_answer(ConfirmAnswer {
                request_id: uuid::Uuid::new_v4().to_string(),
                target: AnswerTarget {
                    answer_id: Some(a.question.id.clone()),
                    session_id: q.question.session_id.clone(),
                    session_question_id: q.question.id.clone(),
                    recording_id: id,
                    selection_version: a.question.selection_version,
                },
                expected_review_version: 0,
                source_transcript_id: None,
                origin: ReviewOrigin::ManualExplicit,
                text: "I learned from a failed deployment.".into(),
                expected_session_version: s.active_session.unwrap().version,
            })
            .await
            .unwrap();
    }
    async fn waive_advance(&self) {
        self.service
            .waive_follow_up(WaiveFollowUp {
                command: self.command(),
                reason: "unavailable".into(),
            })
            .await
            .unwrap();
        self.advance().await;
    }
    async fn advance(&self) {
        let c = self.command();
        self.service
            .advance_session(AdvanceSession {
                request_id: uuid::Uuid::new_v4().to_string(),
                session_id: c.session_id,
                session_question_id: c.session_question_id,
                expected_session_version: c.expected_session_version,
            })
            .await
            .unwrap();
    }
    async fn done(&self) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if self.service.snapshot().ai_job.is_none() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
    }
}
#[tokio::test]
async fn no_network_before_confirmation_and_destination_consent() {
    let h = Harness::new(1, vec!["ask".into()]).await;
    assert!(h.service.request_follow_up(h.command()).await.is_err());
    h.answer().await;
    assert!(h.service.request_follow_up(h.command()).await.is_err());
    assert_eq!(h.transport.calls.load(Ordering::SeqCst), 0);
    h.consent().await;
    h.service.request_follow_up(h.command()).await.unwrap();
    h.done().await;
    assert!(h.service.snapshot().current.unwrap().follow_up.is_some());
}
#[tokio::test]
async fn waiver_during_network_allows_next_capture_and_discards_late_content() {
    let h = Harness::new(2, vec!["gate".into()]).await;
    h.consent().await;
    h.answer().await;
    let c = h.command();
    let first = c.session_question_id.clone();
    h.service.request_follow_up(c.clone()).await.unwrap();
    while h.transport.calls.load(Ordering::SeqCst) == 0 {
        tokio::task::yield_now().await;
    }
    h.service.request_follow_up(c).await.unwrap();
    assert_eq!(h.transport.calls.load(Ordering::SeqCst), 1);
    h.waive_advance().await;
    let next = h.service.snapshot().current.unwrap().question.id;
    assert_ne!(first, next);
    h.answer().await;
    h.transport.gate.notify_one();
    h.done().await;
    let d = h.db.load_session(&h.command().session_id).unwrap();
    let old = d.questions.iter().find(|q| q.question.id == first).unwrap();
    assert!(old.follow_up.is_none());
    assert_eq!(old.follow_up_stage.as_deref(), Some("waived"));
    let feedback = h.db.load_feedback(&d.summary.id).unwrap();
    assert_eq!(feedback.attempts[0]["state"], "discarded");
    assert_eq!(feedback.usage.known_reported_subtotal, 0.01);
}
#[tokio::test]
async fn follow_up_has_separate_audio_confirmation_and_counter() {
    let h = Harness::new(1, vec!["ask".into()]).await;
    h.consent().await;
    h.answer().await;
    let original = h.service.snapshot().current.unwrap().confirmation.unwrap();
    h.service.request_follow_up(h.command()).await.unwrap();
    h.done().await;
    h.answer().await;
    let s = h.service.snapshot();
    assert_eq!(s.active_session.unwrap().confirmed_count, 1);
    let q = s.current.unwrap();
    assert_eq!(q.confirmation.unwrap().id, original.id);
    assert_ne!(q.follow_up.unwrap().confirmation.unwrap().id, original.id);
    assert!(h.service.request_follow_up(h.command()).await.is_err());
    h.advance().await;
}
#[tokio::test]
async fn failed_publication_reuses_received_result_without_network() {
    let h = Harness::new(1, vec!["gate".into()]).await;
    h.consent().await;
    h.answer().await;
    h.service.request_follow_up(h.command()).await.unwrap();
    while h.transport.calls.load(Ordering::SeqCst) == 0 {
        tokio::task::yield_now().await;
    }
    h.db.with_connection(|c|{c.execute_batch("CREATE TRIGGER fail_ai BEFORE UPDATE ON ai_attempts BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();Ok(())}).unwrap();
    h.transport.gate.notify_one();
    h.done().await;
    let id = h.service.snapshot().pending_ai_publication.unwrap();
    assert!(h.service.request_follow_up(h.command()).await.is_err());
    h.db.with_connection(|c| {
        c.execute_batch("DROP TRIGGER fail_ai;").unwrap();
        Ok(())
    })
    .unwrap();
    h.service.retry_ai_publication(id).await.unwrap();
    assert_eq!(h.transport.calls.load(Ordering::SeqCst), 1);
    assert!(h.service.snapshot().current.unwrap().follow_up.is_some());
}
#[tokio::test]
async fn partial_feedback_retries_only_missing_reports() {
    let h = Harness::new(
        3,
        vec![
            "report".into(),
            "error".into(),
            "report".into(),
            "report".into(),
        ],
    )
    .await;
    h.consent().await;
    for _ in 0..3 {
        h.answer().await;
        h.waive_advance().await;
    }
    let id = h.db.list_sessions().unwrap()[0].id.clone();
    h.service
        .generate_feedback(AiSessionCommand {
            request_id: "batch1".into(),
            session_id: id.clone(),
        })
        .await
        .unwrap();
    h.done().await;
    let partial = h.service.load_feedback(id.clone()).await.unwrap();
    assert_eq!(partial.state, "partial");
    assert_eq!(partial.reports.len(), 1);
    assert_eq!(h.transport.calls.load(Ordering::SeqCst), 2);
    h.service
        .generate_feedback(AiSessionCommand {
            request_id: "batch2".into(),
            session_id: id.clone(),
        })
        .await
        .unwrap();
    h.done().await;
    let ready = h.service.load_feedback(id).await.unwrap();
    assert_eq!(ready.state, "ready");
    assert_eq!(ready.reports.len(), 3);
    assert_eq!(h.transport.calls.load(Ordering::SeqCst), 4);
}
#[tokio::test]
async fn settings_remove_secret_and_consent_on_destination_change() {
    let h = Harness::new(1, vec![]).await;
    h.consent().await;
    let old = h.service.load_ai_settings().await.unwrap();
    let v = h
        .service
        .save_ai_settings(SaveAiSettings {
            request_id: "config".into(),
            expected_version: old.version,
            input: AiSettingsInput {
                base_url: "https://example.com/api".into(),
                model: old.model,
                tariff: None,
            },
        })
        .await
        .unwrap();
    assert!(v.consent.is_none());
    assert!(!v.credential_configured);
    assert!(h.store.get().unwrap().is_none());
    assert!(!serde_json::to_string(&h.db.ai_settings().unwrap())
        .unwrap()
        .contains("TEST-SECRET"));
}

#[tokio::test]
async fn configuration_and_consent_replays_survive_new_versions() {
    let h = Harness::new(1, vec![]).await;
    let old = h.service.load_ai_settings().await.unwrap();
    let request = SaveAiSettings {
        request_id: "same-settings".into(),
        expected_version: old.version,
        input: AiSettingsInput {
            base_url: old.base_url,
            model: old.model,
            tariff: old.tariff,
        },
    };
    let saved = h.service.save_ai_settings(request.clone()).await.unwrap();
    assert_eq!(
        h.service.save_ai_settings(request).await.unwrap().version,
        saved.version
    );
    let consent = AcceptAiConsent {
        request_id: "same-consent".into(),
        expected_settings_version: saved.version,
        destination: saved.base_url,
        notice_version: "text-external-v1".into(),
    };
    let agreed = h.service.accept_ai_consent(consent.clone()).await.unwrap();
    assert_eq!(
        h.service.accept_ai_consent(consent).await.unwrap().version,
        agreed.version
    );
}

#[tokio::test]
async fn restart_marks_request_uncertain_without_automatic_resend() {
    let h = Harness::new(1, vec!["gate".into()]).await;
    h.consent().await;
    h.answer().await;
    h.service.request_follow_up(h.command()).await.unwrap();
    while h.transport.calls.load(Ordering::SeqCst) == 0 {
        tokio::task::yield_now().await;
    }
    let reopened = Arc::new(
        AppService::new(
            ServicePaths {
                database: h._dir.path().join("test.sqlite"),
                legacy: h._dir.path().join("legacy"),
            },
            ArtifactStore::new(h._dir.path().join("sessions")),
            Arc::new(Audio::default()),
            Arc::new(Launcher),
            None,
        )
        .with_ai(h.transport.clone(), h.store.clone()),
    );
    reopened.start().await;
    let snapshot = reopened.snapshot();
    assert_eq!(
        snapshot.current.unwrap().follow_up_stage.as_deref(),
        Some("failed")
    );
    assert_eq!(h.transport.calls.load(Ordering::SeqCst), 1);
    let id = snapshot.active_session.unwrap().id;
    let f = reopened.load_feedback(id).await.unwrap();
    assert_eq!(f.attempts[0]["state"], "interrupted");
    assert_eq!(f.usage.unknown_attempt_count, 1);
    h.transport.gate.notify_one();
    h.done().await;
    assert!(h.service.snapshot().current.unwrap().follow_up.is_none());
}

#[tokio::test]
async fn closing_does_not_wait_for_http_or_start_more_batch_requests() {
    let h = Harness::new(2, vec!["gate".into(), "report".into()]).await;
    h.consent().await;
    for _ in 0..2 {
        h.answer().await;
        h.waive_advance().await;
    }
    let id = h.db.list_sessions().unwrap()[0].id.clone();
    h.service
        .generate_feedback(AiSessionCommand {
            request_id: "close-batch".into(),
            session_id: id,
        })
        .await
        .unwrap();
    while h.transport.calls.load(Ordering::SeqCst) == 0 {
        tokio::task::yield_now().await;
    }
    tokio::time::timeout(Duration::from_secs(1), h.service.prepare_close())
        .await
        .unwrap()
        .unwrap();
    h.transport.gate.notify_one();
    h.done().await;
    assert_eq!(h.transport.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn closing_queued_before_batch_reservation_prevents_new_attempts() {
    let h = Harness::new(1, vec!["gate".into()]).await;
    h.consent().await;
    h.answer().await;
    h.waive_advance().await;
    let id = h.db.list_sessions().unwrap()[0].id.clone();
    let close_task = Arc::new(Mutex::new(None));
    let task = close_task.clone();
    let weak = Arc::downgrade(&h.service);
    h.service.subscribe(Arc::new(move |snapshot| {
        if snapshot
            .ai_job
            .as_ref()
            .is_some_and(|job| job.attempt_id == "close-before-reserve")
        {
            let mut pending = task.lock().unwrap();
            if pending.is_none() {
                let service = weak.upgrade().unwrap();
                *pending = Some(tokio::spawn(async move { service.prepare_close().await }));
            }
        }
    }));
    h.service
        .generate_feedback(AiSessionCommand {
            request_id: "close-before-reserve".into(),
            session_id: id.clone(),
        })
        .await
        .unwrap();
    let pending = close_task.lock().unwrap().take().unwrap();
    pending.await.unwrap().unwrap();
    h.transport.gate.notify_one();
    h.done().await;
    assert_eq!(h.transport.calls.load(Ordering::SeqCst), 0);
    assert!(h.db.load_feedback(&id).unwrap().attempts.is_empty());
}

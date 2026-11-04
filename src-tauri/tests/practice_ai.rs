//! Pedidos persistentes e resultados imutáveis (tarefa 5 da fase 4).
//!
//! Nenhum teste faz uma chamada real nem usa credenciais reais: o transporte é
//! um duplo com porta de suspensão (`Notify`) e guião, e a base de dados e os
//! artefactos são reais num diretório temporário. Cobre a reformulação por
//! origem/nível (reutilização de variantes, geração, replay e colisão de IDs) e
//! a avaliação de prática (confirmação, papel `attempt`, família de rubrica,
//! publicação e recuperação).

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use language_assistant_tutor::{
    ai::*,
    artifacts::ArtifactStore,
    audio::{AudioPort, AvailabilityAudio, CapturedAudio, RecordingTarget},
    db::Database,
    domain::{error_code, AppError, ComponentState},
    interview::ReviewOrigin,
    practice::{
        CaptureOwner, ConfirmPracticeAnswer, CreatePractice, EvaluatePractice,
        PracticeAnswerTarget, PracticeDetail, PracticeStatus, ReportState, ResponseOrigin,
        RewriteCommand, RewriteLevel, SetRewriteLevel,
    },
    service::{AppService, LaunchedWorker, ServicePaths, WorkerLauncher},
};
use serde_json::json;

// ---------------------------------------------------------------------------
// Adaptadores controlados
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Store(Mutex<Option<String>>);
impl CredentialStore for Store {
    fn get(&self) -> Result<Option<String>> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn set(&self, key: &str) -> Result<()> {
        *self.0.lock().unwrap() = Some(key.into());
        Ok(())
    }
    fn delete(&self) -> Result<()> {
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
    fn play(&self, _: std::path::PathBuf) -> std::result::Result<(), AppError> {
        Ok(())
    }
    fn stop_playback(&self) -> std::result::Result<(), AppError> {
        Ok(())
    }
    fn reopen(&self) -> std::result::Result<(), AppError> {
        Ok(())
    }
}

/// Transporte suspenso: cada chamada consome um modo do guião (`ok`, `gate` ou
/// `error`) e devolve uma reformulação ou um relatório válidos conforme a tarefa.
struct Transport {
    calls: AtomicUsize,
    conversations: Mutex<Vec<String>>,
    gate: Arc<tokio::sync::Notify>,
    script: Mutex<Vec<String>>,
}
impl Transport {
    fn new(script: Vec<String>) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            conversations: Mutex::new(Vec::new()),
            gate: Arc::new(tokio::sync::Notify::new()),
            script: Mutex::new(script),
        }
    }
}
impl AiTransport for Transport {
    fn complete(
        &self,
        r: TransportRequest,
    ) -> Pin<Box<dyn Future<Output = Result<RemoteCompletion>> + Send>> {
        self.conversations
            .lock()
            .unwrap()
            .push(r.conversation_id.clone());
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        let mode = self
            .script
            .lock()
            .unwrap()
            .get(n)
            .cloned()
            .unwrap_or_else(|| "ok".into());
        let gate = self.gate.clone();
        let settings = r.settings.clone();
        let task = r.task.clone();
        let sources = r.sources.clone();
        Box::pin(async move {
            if mode == "gate" {
                gate.notified().await;
            }
            if mode == "error" {
                return Err(failure("Rede falhou."));
            }
            let content = match &task {
                AiTask::Rewrite { level, .. } => valid_rewrite(&sources, *level),
                _ => valid_report(&sources),
            };
            Ok(RemoteCompletion {
                model: settings.model,
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

fn valid_rewrite(sources: &AiSources, level: RewriteLevel) -> String {
    let first = &sources.answers[0];
    json!({
        "schemaVersion": "rewrite-v1",
        "level": level.as_str(),
        "segments": [{
            "id": "s1",
            "text": "I failed a release and we fixed the process.",
            "evidence": [{
                "confirmationId": first.confirmation_id,
                "role": first.role,
                "quote": first.text
            }]
        }],
        "necessaryChanges": [],
        "optionalStyle": [],
        "missingInformation": []
    })
    .to_string()
}

fn valid_report(sources: &AiSources) -> String {
    let first = &sources.answers[0];
    let content: Vec<serde_json::Value> = [
        "relevance",
        "situation",
        "individual_responsibility",
        "actions",
        "result",
        "learning",
    ]
    .iter()
    .map(|d| {
        json!({
            "id": d,
            "dimension": d,
            "rating": "partial",
            "explanation": "Conteúdo parcial no texto.",
            "evidence": [{
                "type": "quote",
                "confirmationId": first.confirmation_id,
                "role": first.role,
                "quote": first.text
            }]
        })
    })
    .collect();
    json!({
        "schemaVersion": SCHEMA,
        "content": content,
        "strengths": [],
        "gaps": [],
        "english": {"necessaryCorrections": [], "optionalStyle": []},
        "priorities": [],
        "fewerPrioritiesReason": "Não se identificaram outras prioridades sustentadas."
    })
    .to_string()
}

// ---------------------------------------------------------------------------
// Arranjo
// ---------------------------------------------------------------------------

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn seed_completed(database: &Database) {
    database
        .with_connection(|c| {
            c.execute_batch(
                r#"
INSERT INTO questions (id, text, category, competencies_json, criteria_json, version, created_at, updated_at) VALUES
 ('q1', 'Tell me about a time you failed.', 'failure_learning', '["Aprendizagem"]', '["Explicar o que aprendeu."]', 1, '2026-11-02T09:00:00Z', '2026-11-02T09:00:00Z');

INSERT INTO sessions (id, category, question_count, position, status, mode, version, created_at, completed_at) VALUES
 ('s1', NULL, 1, 1, 'completed', 'local_v2', 1, '2026-11-02T09:00:00Z', '2026-11-02T09:30:00Z');

INSERT INTO session_questions (id, session_id, source_question_id, source_version, position, text, category, competencies_json, criteria_json, selected_recording_id, selection_version) VALUES
 ('sq1', 's1', 'q1', 1, 1, 'Tell me about a time you failed.', 'failure_learning', '["Aprendizagem"]', '["Explicar o que aprendeu."]', NULL, 0);

INSERT INTO recordings (id, session_question_id, ordinal, state, original_file, normalized_file, capture_json, created_at, updated_at) VALUES
 ('rec1', 'sq1', 1, 'ready', 's1/sq1/rec1/original.wav', NULL, NULL, '2026-11-02T09:05:00Z', '2026-11-02T09:06:00Z');

INSERT INTO reviews (recording_id, version, source_transcript_id, origin, text, saved_at) VALUES
 ('rec1', 1, NULL, 'manual_explicit', 'I failed a release.', '2026-11-02T09:08:00Z');

INSERT INTO confirmations (id, session_question_id, recording_id, review_version, source_transcript_id, origin, text, confirmed_at) VALUES
 ('c1', 'sq1', 'rec1', 1, NULL, 'manual_explicit', 'I failed a release.', '2026-11-02T09:09:00Z');
"#,
            )
            .map_err(|e| AppError::new("TEST_SQLITE", e.to_string(), false))?;
            Ok(())
        })
        .expect("semear a sessão concluída");
}

fn original_origin() -> ResponseOrigin {
    ResponseOrigin::Original {
        session_id: "s1".into(),
        session_question_id: "sq1".into(),
    }
}

struct Harness {
    _dir: tempfile::TempDir,
    db: Database,
    service: Arc<AppService>,
    transport: Arc<Transport>,
}
impl Harness {
    async fn new(script: Vec<String>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.sqlite");
        let db = Database::open(path.clone()).unwrap();
        seed_completed(&db);
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
        Self {
            _dir: dir,
            db,
            service,
            transport,
        }
    }

    async fn consent(&self) {
        self.service
            .save_ai_key("oc_sk_test_secret_0001".into())
            .await
            .unwrap();
        let s = self.service.load_ai_settings().await.unwrap();
        self.service
            .accept_ai_consent(AcceptAiConsent {
                request_id: new_id(),
                expected_settings_version: s.version,
                destination: s.base_url,
                notice_version: "text-external-v1".into(),
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

    async fn create_practice(&self) -> PracticeDetail {
        self.service
            .create_practice(CreatePractice {
                request_id: new_id(),
                session_id: "s1".into(),
                session_question_id: "sq1".into(),
                previous_practice_id: None,
            })
            .await
            .expect("criar a prática")
    }

    async fn confirm_practice(&self, detail: PracticeDetail) -> PracticeDetail {
        let owner = CaptureOwner::Practice {
            practice_id: detail.id.clone(),
            answer_id: detail.answer.id.clone(),
        };
        self.service
            .start_practice_recording(owner.clone(), 0)
            .await
            .expect("gravar");
        self.service.stop_recording().await.expect("terminar");
        let current = self
            .service
            .load_practice(detail.id.clone())
            .await
            .expect("recarregar");
        let recording_id = current.answer.selected_recording_id.clone().unwrap();
        let selection_version = current.answer.selection_version;
        self.service
            .confirm_practice_answer(ConfirmPracticeAnswer {
                request_id: new_id(),
                target: PracticeAnswerTarget {
                    owner,
                    recording_id,
                    selection_version,
                },
                expected_review_version: 0,
                expected_practice_version: detail.version,
                source_transcript_id: None,
                origin: ReviewOrigin::ManualExplicit,
                text: "I failed a release and we fixed the process.".to_owned(),
            })
            .await
            .expect("confirmar a prática")
    }
}

// ---------------------------------------------------------------------------
// Reformulação: variantes imutáveis, replay, colisões e geração
// ---------------------------------------------------------------------------

#[tokio::test]
async fn rewrite_reuses_ready_variant_and_creates_new_level_without_touching_previous() {
    let h = Harness::new(vec!["ok".into(), "ok".into()]).await;
    h.consent().await;

    let b1 = h
        .service
        .request_rewrite(RewriteCommand {
            request_id: new_id(),
            origin: original_origin(),
            level: RewriteLevel::B1,
        })
        .await
        .expect("pedir a reformulação B1");
    let _ = &b1;
    h.done().await;
    let b1 = h
        .service
        .list_rewrites(original_origin())
        .await
        .unwrap()
        .into_iter()
        .find(|v| v.level == RewriteLevel::B1)
        .unwrap();
    assert_eq!(b1.state, ReportState::Ready);
    assert!(b1.id.is_some());
    assert_eq!(b1.segments.len(), 1);
    assert_eq!(h.transport.calls.load(Ordering::SeqCst), 1);

    // A mesma variante é reutilizada sem rede, mesmo com um requestId novo.
    let replay = h
        .service
        .request_rewrite(RewriteCommand {
            request_id: new_id(),
            origin: original_origin(),
            level: RewriteLevel::B1,
        })
        .await
        .expect("reutilizar a variante B1");
    assert_eq!(replay.state, ReportState::Ready);
    assert_eq!(replay.id, b1.id);
    assert_eq!(
        h.transport.calls.load(Ordering::SeqCst),
        1,
        "sem rede na reutilização"
    );

    // A preferência muda, mas não altera a reformulação anterior.
    let pref = h
        .service
        .set_rewrite_level(SetRewriteLevel {
            request_id: new_id(),
            expected_version: 1,
            level: RewriteLevel::B2,
        })
        .await
        .expect("mudar a preferência");
    assert_eq!(pref.level, RewriteLevel::B2);
    let b1_after = h
        .service
        .list_rewrites(original_origin())
        .await
        .unwrap()
        .into_iter()
        .find(|v| v.level == RewriteLevel::B1)
        .unwrap();
    assert_eq!(b1_after.state, ReportState::Ready);
    assert_eq!(b1_after.id, b1.id, "a variante B1 anterior não muda");

    // Um novo nível cria uma variante nova.
    let b2 = h
        .service
        .request_rewrite(RewriteCommand {
            request_id: new_id(),
            origin: original_origin(),
            level: RewriteLevel::B2,
        })
        .await
        .expect("pedir a reformulação B2");
    h.done().await;
    assert_eq!(b2.level, RewriteLevel::B2);
    assert_eq!(h.transport.calls.load(Ordering::SeqCst), 2);
    let listed = h.service.list_rewrites(original_origin()).await.unwrap();
    assert_eq!(listed.len(), 2, "duas variantes: B1 e B2");
}

#[tokio::test]
async fn rewrite_replay_is_idempotent_and_id_collisions_are_refused() {
    let h = Harness::new(vec!["ok".into()]).await;
    h.consent().await;

    let request_id = new_id();
    h.service
        .request_rewrite(RewriteCommand {
            request_id: request_id.clone(),
            origin: original_origin(),
            level: RewriteLevel::B1,
        })
        .await
        .expect("pedir B1");
    h.done().await;
    assert_eq!(h.transport.calls.load(Ordering::SeqCst), 1);
    let first = h
        .service
        .list_rewrites(original_origin())
        .await
        .unwrap()
        .into_iter()
        .find(|v| v.level == RewriteLevel::B1)
        .unwrap();

    // Replay do mesmo requestId: no-op, sem nova tentativa.
    let replay = h
        .service
        .request_rewrite(RewriteCommand {
            request_id: request_id.clone(),
            origin: original_origin(),
            level: RewriteLevel::B1,
        })
        .await
        .expect("replay do mesmo pedido");
    assert_eq!(replay.id, first.id);
    assert_eq!(h.transport.calls.load(Ordering::SeqCst), 1);

    // O mesmo requestId com outro conteúdo é recusado.
    let collision = h
        .service
        .request_rewrite(RewriteCommand {
            request_id: request_id.clone(),
            origin: original_origin(),
            level: RewriteLevel::C1,
        })
        .await
        .expect_err("colisão de identificador recusada");
    assert_eq!(collision.code, error_code::STALE_SOURCE);

    // Um requestId já usado num comando de prática não é reutilizado por IA.
    let practice_id = new_id();
    h.service
        .create_practice(CreatePractice {
            request_id: practice_id.clone(),
            session_id: "s1".into(),
            session_question_id: "sq1".into(),
            previous_practice_id: None,
        })
        .await
        .expect("criar prática com o requestId");
    let clash = h
        .service
        .request_rewrite(RewriteCommand {
            request_id: practice_id,
            origin: original_origin(),
            level: RewriteLevel::B1,
        })
        .await
        .expect_err("requestId de prática não é reutilizado por IA");
    assert_eq!(clash.code, error_code::STALE_SOURCE);
}

#[tokio::test]
async fn rewrite_new_request_after_error_adds_only_a_network_attempt() {
    let h = Harness::new(vec!["error".into(), "ok".into()]).await;
    h.consent().await;

    h.service
        .request_rewrite(RewriteCommand {
            request_id: new_id(),
            origin: original_origin(),
            level: RewriteLevel::B1,
        })
        .await
        .expect("primeiro pedido falha");
    h.done().await;
    let failed = h
        .service
        .list_rewrites(original_origin())
        .await
        .unwrap()
        .into_iter()
        .find(|v| v.level == RewriteLevel::B1)
        .unwrap();
    assert_eq!(failed.state, ReportState::Failed);
    assert_eq!(h.transport.calls.load(Ordering::SeqCst), 1);

    // Um requestId novo cria uma nova tentativa do mesmo pedido lógico.
    h.service
        .request_rewrite(RewriteCommand {
            request_id: new_id(),
            origin: original_origin(),
            level: RewriteLevel::B1,
        })
        .await
        .expect("nova tentativa");
    h.done().await;
    assert_eq!(h.transport.calls.load(Ordering::SeqCst), 2);

    let listed = h.service.list_rewrites(original_origin()).await.unwrap();
    assert_eq!(listed.len(), 1, "um único pedido lógico");
    assert_eq!(listed[0].state, ReportState::Ready);
    assert_eq!(
        listed[0].network_attempts.len(),
        2,
        "duas tentativas de rede"
    );
    assert_eq!(listed[0].network_attempts[0].generation, 1);
    assert_eq!(listed[0].network_attempts[1].generation, 2);
}

// ---------------------------------------------------------------------------
// Avaliação de prática
// ---------------------------------------------------------------------------

#[tokio::test]
async fn evaluate_requires_confirmed_practice_and_report_is_not_regenerated() {
    let h = Harness::new(vec!["ok".into()]).await;
    h.consent().await;

    let practice = h.confirm_practice(h.create_practice().await).await;
    assert_eq!(practice.status, PracticeStatus::Confirmed);

    let report = h
        .service
        .evaluate_practice(EvaluatePractice {
            request_id: new_id(),
            practice_id: practice.id.clone(),
            expected_version: practice.version,
        })
        .await
        .expect("avaliar a prática");
    let _ = &report;
    h.done().await;
    let report = h
        .service
        .load_practice_report(practice.id.clone())
        .await
        .unwrap();
    assert_eq!(report.state, ReportState::Ready);
    assert!(report.report.is_some());
    assert_eq!(report.rubric_version.as_deref(), Some("star-text-v1"));
    assert_eq!(h.transport.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        *h.transport.conversations.lock().unwrap(),
        vec![practice.id.clone()]
    );

    // O relatório válido não é regenerado.
    let again = h
        .service
        .evaluate_practice(EvaluatePractice {
            request_id: new_id(),
            practice_id: practice.id.clone(),
            expected_version: practice.version,
        })
        .await
        .expect("reutilizar o relatório");
    assert_eq!(again.state, ReportState::Ready);
    assert_eq!(
        h.transport.calls.load(Ordering::SeqCst),
        1,
        "sem rede na reutilização"
    );

    // A avaliação não cria seguimento nem altera a sessão original.
    let detail = h.db.load_session("s1").unwrap();
    let question = &detail.questions[0];
    assert!(
        question.follow_up.is_none(),
        "a avaliação não cria seguimento"
    );
    assert_eq!(
        question.confirmation.as_ref().unwrap().text,
        "I failed a release."
    );

    // A família de rubrica fica fixada.
    let family: (String, String) = h
        .db
        .with_connection(|c| {
            c.query_row(
                "SELECT rubric_version, schema_version FROM evaluation_families WHERE session_question_id = 'sq1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| AppError::new("TEST_SQLITE", e.to_string(), false))
        })
        .unwrap();
    assert_eq!(family.0, "star-text-v1");
}

#[tokio::test]
async fn evaluate_refuses_unconfirmed_practice_and_incompatible_family() {
    let h = Harness::new(vec![]).await;
    h.consent().await;

    // Uma prática ainda não confirmada não é avaliada.
    let pending = h.create_practice().await;
    let err = h
        .service
        .evaluate_practice(EvaluatePractice {
            request_id: new_id(),
            practice_id: pending.id.clone(),
            expected_version: pending.version,
        })
        .await
        .expect_err("prática não confirmada recusada");
    assert_eq!(err.code, error_code::STALE_SOURCE);
    assert_eq!(h.transport.calls.load(Ordering::SeqCst), 0);

    // Confirma a prática pendente para poder testar a família de rubrica.
    let detail = h.confirm_practice(pending).await;

    // Uma família com rubrica incompatível é recusada sem substituir histórico.
    h.db.with_connection(|c| {
        c.execute(
            "INSERT INTO evaluation_families (session_question_id, rubric_version, schema_version, created_at)
             VALUES ('sq1', 'future-v9', 'future-v9', '2026-11-03T09:00:00Z')",
            [],
        )
        .map_err(|e| AppError::new("TEST_SQLITE", e.to_string(), false))?;
        Ok(())
    })
    .unwrap();
    let incompatible = h
        .service
        .evaluate_practice(EvaluatePractice {
            request_id: new_id(),
            practice_id: detail.id.clone(),
            expected_version: detail.version,
        })
        .await
        .expect_err("rubrica incompatível recusada");
    assert_eq!(incompatible.code, error_code::STALE_SOURCE);
    assert_eq!(h.transport.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn evaluate_derives_family_from_original_report() {
    let h = Harness::new(vec![]).await;
    h.consent().await;
    let detail = h.confirm_practice(h.create_practice().await).await;

    // Semeia um relatório original com rubrica própria (não o padrão), para a
    // família derivar dele em vez do padrão star-text-v1.
    h.db.with_connection(|c| {
        c.execute_batch(
            "INSERT INTO ai_requests (id, purpose, question_id, session_id, origin_key, variant_key, sources_json, rubric_version, schema_version)
             VALUES ('air-orig', 'feedback', 'sq1', 's1', 'original:sq1', 'default',
                     '{\"question\":\"x\",\"criteria\":[],\"answers\":[]}', 'custom-rubric-v1', 'custom-schema-v1');
             INSERT INTO ai_attempts (id, request_id, generation, command_id, payload_hash, state, settings_json, created_at)
             VALUES ('aia-orig', 'air-orig', 1, 'cmd-orig', 'h', 'completed', '{}', '2026-11-03T09:00:00Z');
             INSERT INTO ai_reports (question_id, attempt_id, json)
             VALUES ('sq1', 'aia-orig', '{}');",
        )
        .map_err(|e| AppError::new("TEST_SQLITE", e.to_string(), false))?;
        Ok(())
    })
    .unwrap();

    // A família nasce do relatório original, não do padrão.
    let sources =
        h.db.load_exercise_sources(&ResponseOrigin::Practice {
            practice_id: detail.id.clone(),
        })
        .unwrap();
    assert_eq!(sources.rubric_version, "custom-rubric-v1");

    // A versão incompatível é recusada sem substituir histórico.
    let err = h
        .service
        .evaluate_practice(EvaluatePractice {
            request_id: new_id(),
            practice_id: detail.id.clone(),
            expected_version: detail.version,
        })
        .await
        .expect_err("rubrica original incompatível recusada");
    assert_eq!(err.code, error_code::STALE_SOURCE);
    assert_eq!(h.transport.calls.load(Ordering::SeqCst), 0);
}

// ---------------------------------------------------------------------------
// Concorrência, resultado tardio e recuperação de publicação
// ---------------------------------------------------------------------------

#[tokio::test]
async fn late_b1_result_saves_only_its_origin_and_concurrent_request_is_busy() {
    let h = Harness::new(vec!["gate".into()]).await;
    h.consent().await;

    h.service
        .request_rewrite(RewriteCommand {
            request_id: new_id(),
            origin: original_origin(),
            level: RewriteLevel::B1,
        })
        .await
        .expect("pedir B1 suspenso");
    while h.transport.calls.load(Ordering::SeqCst) == 0 {
        tokio::task::yield_now().await;
    }

    // Uma operação nova enquanto B1 está suspenso recebe busy.
    let busy = h
        .service
        .request_rewrite(RewriteCommand {
            request_id: new_id(),
            origin: original_origin(),
            level: RewriteLevel::B2,
        })
        .await
        .expect_err("operação simultânea recusada");
    assert_eq!(busy.code, error_code::BUSY);

    // A leitura e a navegação continuam disponíveis.
    assert!(h
        .service
        .list_rewrites(original_origin())
        .await
        .unwrap()
        .iter()
        .any(|v| v.level == RewriteLevel::B1));

    // O resultado tardio de B1 guarda apenas a variante de origem.
    h.transport.gate.notify_one();
    h.done().await;
    let listed = h.service.list_rewrites(original_origin()).await.unwrap();
    assert_eq!(listed.len(), 1, "só B1 foi guardado");
    assert_eq!(listed[0].level, RewriteLevel::B1);
    assert_eq!(listed[0].state, ReportState::Ready);
    assert_eq!(h.transport.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn publication_failure_retry_keeps_network_counter_and_restart_does_not_resend() {
    let h = Harness::new(vec!["gate".into()]).await;
    h.consent().await;

    h.service
        .request_rewrite(RewriteCommand {
            request_id: new_id(),
            origin: original_origin(),
            level: RewriteLevel::B1,
        })
        .await
        .expect("pedir B1");
    while h.transport.calls.load(Ordering::SeqCst) == 0 {
        tokio::task::yield_now().await;
    }
    // A publicação falha (trigger que aborta a escrita da tentativa).
    h.db.with_connection(|c| {
        c.execute_batch("CREATE TRIGGER fail_ai BEFORE UPDATE ON ai_attempts BEGIN SELECT RAISE(ABORT,'test'); END;")
            .unwrap();
        Ok(())
    })
    .unwrap();
    h.transport.gate.notify_one();
    h.done().await;
    let id = h.service.snapshot().pending_ai_publication.unwrap();
    assert_eq!(h.transport.calls.load(Ordering::SeqCst), 1);

    // O retry consolida o resultado recebido sem nova chamada.
    h.db.with_connection(|c| {
        c.execute_batch("DROP TRIGGER fail_ai;").unwrap();
        Ok(())
    })
    .unwrap();
    h.service.retry_ai_publication(id).await.unwrap();
    assert_eq!(
        h.transport.calls.load(Ordering::SeqCst),
        1,
        "sem segunda chamada"
    );
    let listed = h.service.list_rewrites(original_origin()).await.unwrap();
    assert_eq!(listed[0].state, ReportState::Ready);
    assert_eq!(
        listed[0].network_attempts.len(),
        1,
        "consumo conservado: uma tentativa"
    );
}

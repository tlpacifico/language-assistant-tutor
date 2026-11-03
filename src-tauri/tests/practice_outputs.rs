//! Voz da sugestão, comparação e consumo (tarefa 6 da fase 4).
//!
//! Nenhum teste faz chamadas reais, abre um microfone ou arranca o Kokoro: o
//! transporte, o áudio e o auxiliar são duplos controlados, e a base de dados e
//! os artefactos são reais num diretório temporário. Cobre a voz das
//! reformulações (só `spokenText`, cache indexada, repetição local, captura),
//! a comparação local (regras de §2.1) e o consumo separado por conjuntos.

use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hound::{SampleFormat, WavSpec, WavWriter};
use language_assistant_tutor::{
    ai::*,
    artifacts::{reconcile_rewrite_audio, ArtifactStore},
    audio::{AudioPort, AvailabilityAudio, CapturedAudio, RecordingTarget},
    db::Database,
    domain::{error_code, AppError, ComponentState, ComponentStatus, PlaybackTarget},
    interview::{AudioSelection, InterviewSnapshot, ReviewOrigin},
    practice::{
        CaptureOwner, ComparisonReportState, ComparisonRequest, ConfirmPracticeAnswer,
        CreatePractice, EvaluatePractice, PracticeAnswerTarget, PracticeCommand, PracticeDetail,
        ReportState, ResponseOrigin, RewriteCommand, RewriteLevel, RewriteView,
        RubricCompatibility,
    },
    service::{
        synthesis_cache_key, AppService, LaunchedWorker, ServicePaths, WorkerControl,
        WorkerLauncher,
    },
    worker::protocol::WorkerOp,
    worker::{
        HealthResult, SynthesisResult, WorkerEvent, WorkerMessage, WorkerRequest, WorkerResult,
    },
};
use serde_json::json;

const TEXT_ORIGINAL: &str = "I failed a release.";
const TEXT_FOLLOW_UP: &str = "I learned to add a rollback step.";
const QUESTION_TEXT: &str = "Tell me about a time you failed.";
const EXPLANATION: &str = "EXPLICACAO_NAO_FALADA";
const PREPARATION_QUESTION: &str = "PREPARATION_QUESTION_NOT_SPOKEN";

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

fn write_wav(path: &Path, sample_rate: u32) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let mut writer = WavWriter::create(
        path,
        WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        },
    )
    .unwrap();
    for _ in 0..(sample_rate / 2) {
        writer.write_sample(0i16).unwrap();
    }
    writer.finalize().unwrap();
}

#[derive(Default)]
struct AudioState {
    capture: Option<RecordingTarget>,
    played: Vec<PathBuf>,
    stops: usize,
}

#[derive(Default)]
struct Audio(Mutex<AudioState>);
impl AudioPort for Audio {
    fn availability(&self) -> AvailabilityAudio {
        AvailabilityAudio {
            microphone: ComponentState::ready(),
            playback: ComponentState::ready(),
        }
    }
    fn start_capture(&self, target: RecordingTarget) -> std::result::Result<(), AppError> {
        write_wav(&target.directory.join("original.wav"), 16_000);
        self.0.lock().unwrap().capture = Some(target);
        Ok(())
    }
    fn stop_capture(&self) -> std::result::Result<CapturedAudio, AppError> {
        let target = self.0.lock().unwrap().capture.take().unwrap();
        let original = target.directory.join("original.wav");
        Ok(CapturedAudio {
            id: target.id,
            meta: language_assistant_tutor::artifacts::inspect_file(&original)?,
            original,
        })
    }
    fn play(&self, path: PathBuf) -> std::result::Result<(), AppError> {
        self.0.lock().unwrap().played.push(path);
        Ok(())
    }
    fn stop_playback(&self) -> std::result::Result<(), AppError> {
        self.0.lock().unwrap().stops += 1;
        Ok(())
    }
    fn reopen(&self) -> std::result::Result<(), AppError> {
        Ok(())
    }
}

/// Auxiliar controlado: só regista os pedidos.
#[derive(Default)]
struct Worker(Mutex<Vec<WorkerRequest>>);
impl WorkerControl for Worker {
    fn submit(&self, request: WorkerRequest) -> std::result::Result<(), AppError> {
        self.0.lock().unwrap().push(request);
        Ok(())
    }
    fn restart(&self) -> std::result::Result<(), AppError> {
        Ok(())
    }
    fn shutdown(&self) -> std::result::Result<(), AppError> {
        Ok(())
    }
    fn generation(&self) -> u64 {
        1
    }
    fn is_running(&self) -> bool {
        true
    }
}

struct Launcher(Arc<Worker>);
impl WorkerLauncher for Launcher {
    fn launch(&self) -> std::result::Result<LaunchedWorker, AppError> {
        Ok(LaunchedWorker {
            control: self.0.clone(),
            events: None,
        })
    }
}

/// Transporte com guião (`ok` | `error`); devolve reformulações e relatórios válidos.
struct Transport {
    calls: AtomicUsize,
    script: Mutex<Vec<String>>,
}
impl Transport {
    fn new(script: Vec<&str>) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            script: Mutex::new(script.into_iter().map(str::to_owned).collect()),
        }
    }
}
impl AiTransport for Transport {
    fn complete(
        &self,
        r: TransportRequest,
    ) -> Pin<Box<dyn Future<Output = Result<RemoteCompletion>> + Send>> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        let mode = self
            .script
            .lock()
            .unwrap()
            .get(n)
            .cloned()
            .unwrap_or_else(|| "ok".into());
        let settings = r.settings.clone();
        let task = r.task.clone();
        let sources = r.sources.clone();
        Box::pin(async move {
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

/// Reformulação com dois segmentos e campos que nunca devem ser falados.
fn valid_rewrite(sources: &AiSources, level: RewriteLevel) -> String {
    let first = &sources.answers[0];
    let source = json!({
        "confirmationId": first.confirmation_id,
        "role": first.role,
        "quote": first.text
    });
    json!({
        "schemaVersion": "rewrite-v1",
        "level": level.as_str(),
        "segments": [
            {"id": "s1", "text": format!("Level {} first segment.", level.as_str()),
             "evidence": [source.clone()]},
            {"id": "s2", "text": "Second segment about the release.",
             "evidence": [source.clone()]}
        ],
        "necessaryChanges": [{
            "source": source.clone(),
            "segmentIds": ["s1"],
            "explanation": EXPLANATION
        }],
        "optionalStyle": [],
        "missingInformation": [{
            "dimension": "result",
            "assessedConfirmationIds": sources.answers.iter().map(|a| a.confirmation_id.clone()).collect::<Vec<_>>(),
            "preparationQuestion": PREPARATION_QUESTION,
            "explanation": EXPLANATION
        }]
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

fn sql_error(error: rusqlite::Error) -> AppError {
    AppError::new("TEST_SQLITE", error.to_string(), false)
}

/// Sessão concluída `s1` com seguimento confirmado e outra sessão `s2` (outra
/// pergunta), ambas locais, sem relatórios.
fn seed(database: &Database) {
    database
        .with_connection(|c| {
            c.execute_batch(
                r#"
INSERT INTO questions (id, text, category, competencies_json, criteria_json, version, created_at, updated_at) VALUES
 ('q1', 'Tell me about a time you failed.', 'failure_learning', '["Aprendizagem"]', '["Explicar o que aprendeu."]', 1, '2026-11-02T09:00:00Z', '2026-11-02T09:00:00Z');

INSERT INTO sessions (id, category, question_count, position, status, mode, version, created_at, completed_at) VALUES
 ('s1', NULL, 1, 1, 'completed', 'local_v2', 1, '2026-11-02T09:00:00Z', '2026-11-02T09:30:00Z'),
 ('s2', NULL, 1, 1, 'completed', 'local_v2', 1, '2026-11-02T10:00:00Z', '2026-11-02T10:30:00Z');

INSERT INTO session_questions (id, session_id, source_question_id, source_version, position, text, category, competencies_json, criteria_json, selected_recording_id, selection_version) VALUES
 ('sq1', 's1', 'q1', 1, 1, 'Tell me about a time you failed.', 'failure_learning', '["Aprendizagem"]', '["Explicar o que aprendeu."]', NULL, 0),
 ('sq3', 's2', 'q1', 1, 1, 'Tell me about a time you failed.', 'failure_learning', '["Aprendizagem"]', '["Explicar o que aprendeu."]', NULL, 0);

INSERT INTO answer_slots (id, session_question_id, session_id, role, text) VALUES
 ('fu1', 'sq1', 's1', 'follow_up', 'What did you learn from it?');

INSERT INTO recordings (id, session_question_id, ordinal, state, original_file, normalized_file, capture_json, created_at, updated_at) VALUES
 ('rec1', 'sq1', 1, 'ready', 's1/sq1/rec1/original.wav', NULL, NULL, '2026-11-02T09:05:00Z', '2026-11-02T09:06:00Z'),
 ('rec2', 'fu1', 1, 'ready', 's1/sq1/rec2/original.wav', NULL, NULL, '2026-11-02T09:15:00Z', '2026-11-02T09:16:00Z'),
 ('rec3', 'sq3', 1, 'ready', 's2/sq3/rec3/original.wav', NULL, NULL, '2026-11-02T10:05:00Z', '2026-11-02T10:06:00Z');

INSERT INTO reviews (recording_id, version, source_transcript_id, origin, text, saved_at) VALUES
 ('rec1', 1, NULL, 'manual_explicit', 'I failed a release.', '2026-11-02T09:08:00Z'),
 ('rec2', 1, NULL, 'manual_explicit', 'I learned to add a rollback step.', '2026-11-02T09:18:00Z'),
 ('rec3', 1, NULL, 'manual_explicit', 'Another question answer.', '2026-11-02T10:08:00Z');

INSERT INTO confirmations (id, session_question_id, recording_id, review_version, source_transcript_id, origin, text, confirmed_at) VALUES
 ('c1', 'sq1', 'rec1', 1, NULL, 'manual_explicit', 'I failed a release.', '2026-11-02T09:09:00Z'),
 ('c2', 'fu1', 'rec2', 1, NULL, 'manual_explicit', 'I learned to add a rollback step.', '2026-11-02T09:19:00Z'),
 ('c3', 'sq3', 'rec3', 1, NULL, 'manual_explicit', 'Another question answer.', '2026-11-02T10:09:00Z');
"#,
            )
            .map_err(sql_error)?;
            Ok(())
        })
        .expect("semear as sessões concluídas");
}

fn original_origin() -> ResponseOrigin {
    ResponseOrigin::Original {
        session_id: "s1".into(),
        session_question_id: "sq1".into(),
    }
}

fn practice_origin(id: &str) -> ResponseOrigin {
    ResponseOrigin::Practice {
        practice_id: id.to_owned(),
    }
}

struct Harness {
    _dir: tempfile::TempDir,
    root: PathBuf,
    db: Database,
    service: Arc<AppService>,
    transport: Arc<Transport>,
    audio: Arc<Audio>,
    worker: Arc<Worker>,
}

impl Harness {
    async fn new(script: Vec<&str>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("legacy");
        let path = dir.path().join("test.sqlite");
        let db = Database::open(path.clone()).unwrap();
        seed(&db);
        let transport = Arc::new(Transport::new(script));
        let audio = Arc::new(Audio::default());
        let worker = Arc::new(Worker::default());
        let service = Arc::new(
            AppService::new(
                ServicePaths {
                    database: path,
                    legacy: root.clone(),
                },
                ArtifactStore::new(dir.path().join("sessions")),
                audio.clone(),
                Arc::new(Launcher(worker.clone())),
                None,
            )
            .with_ai(transport.clone(), Arc::new(Store::default())),
        );
        service.start().await;
        let harness = Self {
            _dir: dir,
            root,
            db,
            service,
            transport,
            audio,
            worker,
        };
        harness.health_ready().await;
        harness
    }

    /// Síntese pronta no auxiliar controlado.
    async fn health_ready(&self) {
        self.service
            .on_worker_event(WorkerEvent {
                generation: 1,
                message: WorkerMessage::Completed {
                    request_id: "worker-health-1".to_owned(),
                    result: WorkerResult::Health(HealthResult {
                        transcription: ComponentState {
                            status: ComponentStatus::Missing,
                            message: None,
                        },
                        synthesis: ComponentState::ready(),
                    }),
                },
            })
            .await;
    }

    async fn consent(&self) {
        self.service
            .save_ai_key("TEST-SECRET".into())
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
            while self.service.snapshot().ai_job.is_some() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
    }

    /// Pede (e espera) uma reformulação e devolve a vista pronta.
    async fn rewrite(&self, origin: ResponseOrigin, level: RewriteLevel) -> RewriteView {
        self.service
            .request_rewrite(RewriteCommand {
                request_id: new_id(),
                origin: origin.clone(),
                level,
            })
            .await
            .expect("pedir a reformulação");
        self.done().await;
        self.service
            .list_rewrites(origin)
            .await
            .unwrap()
            .into_iter()
            .find(|view| view.level == level && view.state == ReportState::Ready)
            .expect("reformulação pronta")
    }

    fn synthesis_requests(&self) -> Vec<WorkerRequest> {
        self.worker
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|request| request.op == WorkerOp::Synthesize)
            .cloned()
            .collect()
    }

    /// Conclui a síntese pendente escrevendo o WAV no caminho pedido.
    async fn finish_synthesis(&self, snapshot: &InterviewSnapshot) -> PathBuf {
        let job = snapshot.model_job.clone().expect("síntese em curso");
        let request = self.synthesis_requests().pop().expect("pedido de síntese");
        let output = PathBuf::from(request.params["outputPath"].as_str().unwrap());
        write_wav(&output, 24_000);
        self.service
            .on_worker_event(WorkerEvent {
                generation: 1,
                message: WorkerMessage::Completed {
                    request_id: job.id,
                    result: WorkerResult::Synthesis(SynthesisResult {
                        audio_path: output.clone(),
                        sample_rate: 24_000,
                        duration_ms: 500,
                        load_ms: 1,
                        processing_ms: 2,
                    }),
                },
            })
            .await;
        output
    }

    async fn fail_synthesis(&self, snapshot: &InterviewSnapshot) {
        let job = snapshot.model_job.clone().expect("síntese em curso");
        self.service
            .on_worker_event(WorkerEvent {
                generation: 1,
                message: WorkerMessage::Failed {
                    request_id: job.id,
                    error: AppError::new(
                        error_code::WORKER_TIMEOUT,
                        "O auxiliar não respondeu.",
                        true,
                    ),
                },
            })
            .await;
    }

    fn audio_row(&self, rewrite_id: &str) -> Vec<(String, String, String, String, f64, String)> {
        self.db
            .with_connection(|c| {
                let mut st = c
                    .prepare(
                        "SELECT config_hash, relative_file, text_hash, voice, speed, state
                           FROM rewrite_audio WHERE rewrite_id = ?1 ORDER BY rowid",
                    )
                    .map_err(sql_error)?;
                let rows = st
                    .query_map([rewrite_id], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, String>(3)?,
                            r.get::<_, f64>(4)?,
                            r.get::<_, String>(5)?,
                        ))
                    })
                    .map_err(sql_error)?
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .map_err(sql_error)?;
                Ok(rows)
            })
            .unwrap()
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

    async fn confirm_practice(&self, detail: PracticeDetail, text: &str) -> PracticeDetail {
        let owner = CaptureOwner::Practice {
            practice_id: detail.id.clone(),
            answer_id: detail.answer.id.clone(),
        };
        self.service
            .start_practice_recording(owner.clone(), 0)
            .await
            .expect("gravar");
        self.service.stop_recording().await.expect("terminar");
        let current = self.service.load_practice(detail.id.clone()).await.unwrap();
        self.service
            .confirm_practice_answer(ConfirmPracticeAnswer {
                request_id: new_id(),
                target: PracticeAnswerTarget {
                    owner,
                    recording_id: current.answer.selected_recording_id.clone().unwrap(),
                    selection_version: current.answer.selection_version,
                },
                expected_review_version: 0,
                expected_practice_version: detail.version,
                source_transcript_id: None,
                origin: ReviewOrigin::ManualExplicit,
                text: text.to_owned(),
            })
            .await
            .expect("confirmar a prática")
    }
}

// ---------------------------------------------------------------------------
// Voz da sugestão
// ---------------------------------------------------------------------------

#[tokio::test]
async fn kokoro_receives_only_the_spoken_text_and_the_index_is_written_before_synthesis() {
    let h = Harness::new(vec![]).await;
    h.consent().await;
    let rewrite = h.rewrite(original_origin(), RewriteLevel::B1).await;
    let id = rewrite.id.clone().unwrap();
    let spoken = rewrite.spoken_text.clone().unwrap();
    assert_eq!(
        spoken,
        "Level B1 first segment.\n\nSecond segment about the release."
    );
    // O conteúdo existe nas listas, mas não é voz.
    assert_eq!(rewrite.necessary_changes[0].explanation, EXPLANATION);
    assert_eq!(rewrite.missing_information.len(), 1);

    let requested = h
        .service
        .play_audio(AudioSelection::Rewrite {
            rewrite_id: id.clone(),
        })
        .await
        .expect("pedir a voz da sugestão");
    assert_eq!(requested.playback, None);

    // Texto indexado como pendente antes de a síntese terminar.
    let rows = h.audio_row(&id);
    assert_eq!(rows.len(), 1);
    let config_hash = synthesis_cache_key(&spoken);
    assert_eq!(rows[0].0, config_hash);
    assert_eq!(rows[0].1, format!("rewrite-cache/{id}/{config_hash}.wav"));
    assert_eq!(rows[0].3, "af_heart");
    assert_eq!(rows[0].5, "pending");
    assert_ne!(rows[0].2, "");

    // O Kokoro recebe apenas o texto de voz, e o ficheiro vai para a cache local.
    let requests = h.synthesis_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].params["text"], spoken);
    let sent = requests[0].params.to_string();
    assert!(!sent.contains(EXPLANATION));
    assert!(!sent.contains(PREPARATION_QUESTION));
    let expected = h.root.join(&rows[0].1);
    assert_eq!(
        PathBuf::from(requests[0].params["outputPath"].as_str().unwrap()),
        expected
    );

    let output = h.finish_synthesis(&requested).await;
    assert_eq!(output, expected);
    let after = h.service.snapshot();
    assert_eq!(after.playback, Some(PlaybackTarget::Response));
    assert!(after.model_job.is_none());
    assert_eq!(h.audio.0.lock().unwrap().played, vec![expected]);
    assert_eq!(h.audio_row(&id)[0].5, "ready");
    assert_eq!(
        h.transport.calls.load(Ordering::SeqCst),
        1,
        "a voz não chama a API"
    );

    // Com o derivado válido, voltar a ouvir não sintetiza de novo.
    h.service
        .play_audio(AudioSelection::Rewrite { rewrite_id: id })
        .await
        .expect("ouvir de novo");
    assert_eq!(h.synthesis_requests().len(), 1);
    assert_eq!(h.audio.0.lock().unwrap().played.len(), 2);
}

#[tokio::test]
async fn level_and_text_change_the_cache_file() {
    let h = Harness::new(vec![]).await;
    h.consent().await;
    let b1 = h.rewrite(original_origin(), RewriteLevel::B1).await;
    let b2 = h.rewrite(original_origin(), RewriteLevel::B2).await;
    for view in [&b1, &b2] {
        let snapshot = h
            .service
            .play_audio(AudioSelection::Rewrite {
                rewrite_id: view.id.clone().unwrap(),
            })
            .await
            .unwrap();
        h.finish_synthesis(&snapshot).await;
    }
    let requests = h.synthesis_requests();
    assert_eq!(requests.len(), 2);
    assert_ne!(
        requests[0].params["outputPath"],
        requests[1].params["outputPath"]
    );
    assert_ne!(
        synthesis_cache_key(b1.spoken_text.as_deref().unwrap()),
        synthesis_cache_key(b2.spoken_text.as_deref().unwrap())
    );
    // A voz e a velocidade fazem parte da chave: mudam o ficheiro.
    let key = synthesis_cache_key("Same text");
    assert_ne!(key, synthesis_cache_key("Same text."));
    assert!(requests[0].params["outputPath"]
        .as_str()
        .unwrap()
        .contains("rewrite-cache"));
}

#[tokio::test]
async fn missing_file_or_failed_synthesis_repeats_only_local_work() {
    let h = Harness::new(vec![]).await;
    h.consent().await;
    let rewrite = h.rewrite(original_origin(), RewriteLevel::B1).await;
    let id = rewrite.id.clone().unwrap();
    let spoken = rewrite.spoken_text.clone().unwrap();
    let calls = h.transport.calls.load(Ordering::SeqCst);

    // Falha da síntese: estado failed, texto conservado.
    let first = h
        .service
        .play_audio(AudioSelection::Rewrite {
            rewrite_id: id.clone(),
        })
        .await
        .unwrap();
    h.fail_synthesis(&first).await;
    assert_eq!(h.audio_row(&id)[0].5, "failed");
    assert!(h.service.snapshot().model_job.is_none());
    let stored = h.db.load_rewrite_voice(&id).unwrap();
    assert_eq!(stored.spoken_text, spoken, "o texto continua guardado");

    // Repetir sintetiza outra vez (só trabalho local) e fica pronto.
    let second = h
        .service
        .play_audio(AudioSelection::Rewrite {
            rewrite_id: id.clone(),
        })
        .await
        .unwrap();
    let output = h.finish_synthesis(&second).await;
    assert_eq!(h.audio_row(&id)[0].5, "ready");
    assert_eq!(h.synthesis_requests().len(), 2);

    // Ficheiro em falta: nova síntese local, sem nova chamada à API.
    fs::remove_file(&output).unwrap();
    let third = h
        .service
        .play_audio(AudioSelection::Rewrite {
            rewrite_id: id.clone(),
        })
        .await
        .unwrap();
    assert!(third.model_job.is_some());
    h.finish_synthesis(&third).await;
    assert_eq!(h.synthesis_requests().len(), 3);
    assert_eq!(h.audio_row(&id)[0].5, "ready");

    // Ficheiro incompleto (WAV inválido): nunca reproduzido, volta a sintetizar.
    fs::write(&output, b"not a wav").unwrap();
    let fourth = h
        .service
        .play_audio(AudioSelection::Rewrite {
            rewrite_id: id.clone(),
        })
        .await
        .unwrap();
    assert!(fourth.model_job.is_some());
    h.finish_synthesis(&fourth).await;
    assert_eq!(h.synthesis_requests().len(), 4);

    assert_eq!(
        h.transport.calls.load(Ordering::SeqCst),
        calls,
        "nenhuma repetição usa a API"
    );
    assert_eq!(
        h.service
            .list_rewrites(original_origin())
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn capture_and_synthesis_never_overlap() {
    let h = Harness::new(vec![]).await;
    h.consent().await;
    let rewrite = h.rewrite(original_origin(), RewriteLevel::B1).await;
    let id = rewrite.id.clone().unwrap();
    let practice = h.create_practice().await;
    let owner = CaptureOwner::Practice {
        practice_id: practice.id.clone(),
        answer_id: practice.answer.id.clone(),
    };

    // Durante a síntese a captura é recusada e o microfone não abre.
    let pending = h
        .service
        .play_audio(AudioSelection::Rewrite {
            rewrite_id: id.clone(),
        })
        .await
        .unwrap();
    let busy = h
        .service
        .start_practice_recording(owner.clone(), 0)
        .await
        .expect_err("captura recusada durante a síntese");
    assert_eq!(busy.code, error_code::BUSY);
    assert!(h.audio.0.lock().unwrap().capture.is_none());
    let output = h.finish_synthesis(&pending).await;
    assert_eq!(h.audio.0.lock().unwrap().played, vec![output]);
    assert_eq!(
        h.service.snapshot().playback,
        Some(PlaybackTarget::Response)
    );

    // A captura interrompe a reprodução em curso.
    let stops = h.audio.0.lock().unwrap().stops;
    h.service
        .start_practice_recording(owner, 0)
        .await
        .expect("gravar a prática");
    assert_eq!(h.service.snapshot().playback, None);
    assert!(h.audio.0.lock().unwrap().stops > stops);

    // Durante a captura nenhuma voz é pedida nem tocada por cima do microfone.
    let before = h.synthesis_requests().len();
    let refused = h
        .service
        .play_audio(AudioSelection::Rewrite { rewrite_id: id })
        .await
        .expect_err("voz recusada durante a captura");
    assert_eq!(refused.code, error_code::BUSY);
    assert_eq!(h.synthesis_requests().len(), before);
    assert_eq!(h.audio.0.lock().unwrap().played.len(), 1);
}

#[tokio::test]
async fn practice_question_audio_uses_the_fixed_question_text() {
    let h = Harness::new(vec![]).await;
    let practice = h.create_practice().await;

    let requested = h
        .service
        .play_audio(AudioSelection::PracticeQuestion {
            practice_id: practice.id.clone(),
        })
        .await
        .expect("pedir a voz da pergunta da prática");
    let requests = h.synthesis_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].params["text"], QUESTION_TEXT);
    // A cache da pergunta é a existente, partilhada com a entrevista.
    assert_eq!(
        PathBuf::from(requests[0].params["outputPath"].as_str().unwrap()),
        h.root
            .join("question-cache")
            .join(format!("{}.wav", synthesis_cache_key(QUESTION_TEXT)))
    );
    h.finish_synthesis(&requested).await;
    assert_eq!(
        h.service.snapshot().playback,
        Some(PlaybackTarget::Question)
    );

    let unknown = h
        .service
        .play_audio(AudioSelection::PracticeQuestion {
            practice_id: new_id(),
        })
        .await
        .expect_err("prática inexistente");
    assert_eq!(unknown.code, error_code::STALE_SOURCE);
    let unknown = h
        .service
        .play_audio(AudioSelection::Rewrite {
            rewrite_id: new_id(),
        })
        .await
        .expect_err("reformulação inexistente");
    assert_eq!(unknown.code, error_code::STALE_SOURCE);
}

#[tokio::test]
async fn reconciliation_flags_missing_and_invalid_derivatives_without_deleting() {
    let h = Harness::new(vec![]).await;
    h.consent().await;
    let rewrite = h.rewrite(original_origin(), RewriteLevel::B1).await;
    let id = rewrite.id.clone().unwrap();
    let snapshot = h
        .service
        .play_audio(AudioSelection::Rewrite {
            rewrite_id: id.clone(),
        })
        .await
        .unwrap();
    let output = h.finish_synthesis(&snapshot).await;
    assert_eq!(h.audio_row(&id)[0].5, "ready");

    // Válido: nada a avisar.
    assert!(reconcile_rewrite_audio(&h.db, &h.root).unwrap().is_empty());

    // Derivado ausente: aviso, estado failed, texto intacto.
    fs::remove_file(&output).unwrap();
    let warnings = reconcile_rewrite_audio(&h.db, &h.root).unwrap();
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].code, error_code::ARTIFACT_MISSING);
    assert_eq!(h.audio_row(&id)[0].5, "failed");
    assert!(h.db.load_rewrite_voice(&id).is_ok());

    // Ficheiro incompleto: aviso e o ficheiro é conservado.
    fs::write(&output, b"truncated").unwrap();
    h.db.set_rewrite_audio_state(
        &id,
        &h.audio_row(&id)[0].0,
        language_assistant_tutor::db::RewriteAudioState::Ready,
    )
    .unwrap();
    let warnings = reconcile_rewrite_audio(&h.db, &h.root).unwrap();
    assert_eq!(warnings.len(), 1);
    assert!(output.is_file(), "nunca elimina ficheiros");
    assert_eq!(h.audio_row(&id)[0].5, "failed");

    // Caminho inválido (fora da pasta gerida): aviso, sem ler nem apagar.
    h.db.with_connection(|c| {
        c.execute(
            "UPDATE rewrite_audio SET relative_file = '../outside.wav', state = 'ready'",
            [],
        )
        .map_err(sql_error)?;
        Ok(())
    })
    .unwrap();
    let warnings = reconcile_rewrite_audio(&h.db, &h.root).unwrap();
    assert_eq!(warnings.len(), 1);
    assert_eq!(h.audio_row(&id)[0].5, "failed");

    // Um derivado válido pendente volta a ready.
    h.db.with_connection(|c| {
        c.execute(
            "UPDATE rewrite_audio SET relative_file = 'rewrite-cache/' || rewrite_id || '/' || config_hash || '.wav', state = 'pending'",
            [],
        )
        .map_err(sql_error)?;
        Ok(())
    })
    .unwrap();
    write_wav(&output, 24_000);
    assert!(reconcile_rewrite_audio(&h.db, &h.root).unwrap().is_empty());
    assert_eq!(h.audio_row(&id)[0].5, "ready");
}

// ---------------------------------------------------------------------------
// Comparação
// ---------------------------------------------------------------------------

fn request(left: ResponseOrigin, right: ResponseOrigin) -> ComparisonRequest {
    ComparisonRequest { left, right }
}

#[tokio::test]
async fn compare_original_and_practice_keeps_roles_orders_by_creation_and_creates_nothing() {
    let h = Harness::new(vec![]).await;
    let practice = h.create_practice().await;
    let confirmed = h
        .confirm_practice(practice, "I failed a release and we fixed the process.")
        .await;
    let origin_practice = practice_origin(&confirmed.id);

    // Pedido com a prática primeiro: a ordem é sempre a da criação.
    let view = h
        .service
        .compare_responses(request(origin_practice.clone(), original_origin()))
        .await
        .expect("comparar");
    assert_eq!(view.left.origin, original_origin());
    assert_eq!(view.right.origin, origin_practice);
    assert!(view.left.created_at < view.right.created_at);

    // Original: inicial e seguimento separados, cada um com a sua confirmação.
    let roles: Vec<&str> = view.left.answers.iter().map(|a| a.role.as_str()).collect();
    assert_eq!(roles, vec!["initial", "follow_up"]);
    assert_eq!(
        view.left.answers[0].confirmation.as_ref().unwrap().text,
        TEXT_ORIGINAL
    );
    assert_eq!(
        view.left.answers[1].confirmation.as_ref().unwrap().text,
        TEXT_FOLLOW_UP
    );
    assert_eq!(view.left.answers[0].recordings[0].id, "rec1");
    assert_eq!(view.left.answers[1].recordings[0].id, "rec2");
    // O áudio em falta no disco aparece como aviso, sem esconder os textos.
    assert!(!view.left.answers[0].recordings[0].warnings.is_empty());
    assert_eq!(
        view.left.confirmed_at.as_deref(),
        Some("2026-11-02T09:19:00Z")
    );
    assert_eq!(view.left.question.text, QUESTION_TEXT);

    // Prática: só a tentativa.
    assert_eq!(view.right.answers.len(), 1);
    assert_eq!(view.right.answers[0].role, "attempt");
    assert_eq!(
        view.right.answers[0].confirmation.as_ref().unwrap().text,
        "I failed a release and we fixed the process."
    );
    assert!(!view.right.answers[0].recordings.is_empty());
    assert_eq!(view.right.confirmed_at, confirmed.confirmed_at);

    // Relatório ausente: sem avaliação criada, compatibilidade desconhecida.
    assert_eq!(view.left.report_state, ComparisonReportState::NotAvailable);
    assert_eq!(view.right.report_state, ComparisonReportState::Pending);
    assert!(view.left.report.is_none() && view.right.report.is_none());
    assert_eq!(view.rubric_compatibility, RubricCompatibility::Unknown);
    assert_eq!(h.transport.calls.load(Ordering::SeqCst), 0);
    let requests: i64 =
        h.db.with_connection(|c| {
            c.query_row("SELECT count(*) FROM ai_requests", [], |r| r.get(0))
                .map_err(sql_error)
        })
        .unwrap();
    assert_eq!(requests, 0, "comparar nunca cria pedidos de avaliação");
}

#[tokio::test]
async fn compare_two_practices_of_the_same_question() {
    let h = Harness::new(vec![]).await;
    let first = h.create_practice().await;
    let first = h.confirm_practice(first, "First attempt text.").await;
    let second = h.create_practice().await;
    let second = h.confirm_practice(second, "Second attempt text.").await;

    let view = h
        .service
        .compare_responses(request(
            practice_origin(&second.id),
            practice_origin(&first.id),
        ))
        .await
        .expect("comparar duas práticas");
    assert_eq!(view.left.origin, practice_origin(&first.id));
    assert_eq!(view.right.origin, practice_origin(&second.id));
    assert_eq!(
        view.left.answers[0].confirmation.as_ref().unwrap().text,
        "First attempt text."
    );
    assert_eq!(
        view.right.answers[0].confirmation.as_ref().unwrap().text,
        "Second attempt text."
    );
}

#[tokio::test]
async fn compare_refuses_drafts_abandoned_other_questions_and_identical_origins() {
    let h = Harness::new(vec![]).await;
    let confirmed = h.create_practice().await;
    let confirmed = h.confirm_practice(confirmed, "A confirmed attempt.").await;

    // Rascunho (preparing/answering).
    let draft = h.create_practice().await;
    let err = h
        .service
        .compare_responses(request(original_origin(), practice_origin(&draft.id)))
        .await
        .expect_err("rascunho recusado");
    assert_eq!(err.code, error_code::STALE_SOURCE);

    // Abandonada.
    let abandoned = h
        .service
        .abandon_practice(PracticeCommand {
            request_id: new_id(),
            practice_id: draft.id.clone(),
            expected_version: draft.version,
        })
        .await
        .expect("abandonar");
    let err = h
        .service
        .compare_responses(request(
            practice_origin(&confirmed.id),
            practice_origin(&abandoned.id),
        ))
        .await
        .expect_err("abandonada recusada");
    assert_eq!(err.code, error_code::STALE_SOURCE);

    // Outra pergunta (sessão s2).
    let other = ResponseOrigin::Original {
        session_id: "s2".into(),
        session_question_id: "sq3".into(),
    };
    let err = h
        .service
        .compare_responses(request(other.clone(), practice_origin(&confirmed.id)))
        .await
        .expect_err("outra pergunta recusada");
    assert_eq!(err.code, error_code::INVALID_INPUT);
    let err = h
        .service
        .compare_responses(request(original_origin(), other))
        .await
        .expect_err("outra pergunta recusada");
    assert_eq!(err.code, error_code::INVALID_INPUT);

    // Origens iguais.
    let err = h
        .service
        .compare_responses(request(original_origin(), original_origin()))
        .await
        .expect_err("origens iguais recusadas");
    assert_eq!(err.code, error_code::INVALID_INPUT);

    // Prática inexistente.
    assert!(h
        .service
        .compare_responses(request(original_origin(), practice_origin(&new_id())))
        .await
        .is_err());
}

#[tokio::test]
async fn compare_reports_versions_and_rubric_compatibility() {
    let h = Harness::new(vec![]).await;
    h.consent().await;
    let practice = h.create_practice().await;
    let confirmed = h
        .confirm_practice(practice, "I failed a release and we fixed the process.")
        .await;

    // Relatório original com a rubrica padrão: a prática avaliada fica compatível.
    h.db.with_connection(|c| {
        c.execute_batch(
            "INSERT INTO ai_requests (id, purpose, question_id, session_id, origin_key, variant_key, sources_json, rubric_version, schema_version)
             VALUES ('air-orig', 'feedback', 'sq1', 's1', 'original:sq1', 'default',
                     '{\"question\":\"x\",\"criteria\":[],\"answers\":[]}', 'star-text-v1', 'ai-v3-1');
             INSERT INTO ai_attempts (id, request_id, generation, command_id, payload_hash, state, settings_json, created_at)
             VALUES ('aia-orig', 'air-orig', 1, 'cmd-orig', 'h', 'completed', '{}', '2026-11-02T09:40:00Z');
             INSERT INTO ai_reports (question_id, attempt_id, json) VALUES ('sq1', 'aia-orig', '{\"marker\":\"ORIGINAL\"}');",
        )
        .map_err(sql_error)?;
        Ok(())
    })
    .unwrap();

    h.service
        .evaluate_practice(EvaluatePractice {
            request_id: new_id(),
            practice_id: confirmed.id.clone(),
            expected_version: confirmed.version,
        })
        .await
        .expect("avaliar a prática");
    h.done().await;

    let view = h
        .service
        .compare_responses(request(original_origin(), practice_origin(&confirmed.id)))
        .await
        .unwrap();
    assert_eq!(view.left.report_state, ComparisonReportState::Ready);
    assert_eq!(view.right.report_state, ComparisonReportState::Ready);
    let left = view.left.report.as_ref().unwrap();
    assert_eq!(left.content["marker"], "ORIGINAL");
    assert_eq!(left.versions.rubric_version, "star-text-v1");
    let right = view.right.report.as_ref().unwrap();
    assert_eq!(right.versions.rubric_version, "star-text-v1");
    assert_eq!(right.versions.schema_version, "ai-v3-1");
    assert_eq!(view.rubric_compatibility, RubricCompatibility::Same);

    // Versões diferentes ficam indicadas, nunca disfarçadas de melhoria.
    h.db.with_connection(|c| {
        c.execute_batch("DROP TRIGGER ai_requests_immutable_after_reserve;")
            .map_err(sql_error)?;
        c.execute(
            "UPDATE ai_requests SET rubric_version = 'custom-rubric-v1' WHERE id = 'air-orig'",
            [],
        )
        .map_err(sql_error)?;
        Ok(())
    })
    .unwrap();
    let view = h
        .service
        .compare_responses(request(original_origin(), practice_origin(&confirmed.id)))
        .await
        .unwrap();
    assert_eq!(view.rubric_compatibility, RubricCompatibility::Different);
}

#[tokio::test]
async fn compare_lists_only_valid_suggestions_of_each_origin() {
    let h = Harness::new(vec!["ok", "error"]).await;
    h.consent().await;
    let practice = h.create_practice().await;
    let confirmed = h
        .confirm_practice(practice, "Attempt for suggestions.")
        .await;
    let rewrite = h.rewrite(original_origin(), RewriteLevel::B1).await;
    // Uma sugestão falhada da prática não é válida e não entra na comparação.
    h.service
        .request_rewrite(RewriteCommand {
            request_id: new_id(),
            origin: practice_origin(&confirmed.id),
            level: RewriteLevel::B2,
        })
        .await
        .unwrap();
    h.done().await;

    let view = h
        .service
        .compare_responses(request(original_origin(), practice_origin(&confirmed.id)))
        .await
        .unwrap();
    assert_eq!(view.left.rewrites.len(), 1);
    assert_eq!(view.left.rewrites[0].id, rewrite.id);
    assert!(view.right.rewrites.is_empty());
}

// ---------------------------------------------------------------------------
// Consumo
// ---------------------------------------------------------------------------

#[tokio::test]
async fn training_usage_uses_disjoint_sets_and_counts_each_network_attempt_once() {
    // 1.ª tentativa de reformulação falha; a repetição, a prática e a avaliação concluem.
    let h = Harness::new(vec!["error", "ok", "ok", "ok"]).await;
    h.consent().await;

    // Entrevista original: um feedback com consumo desconhecido (usage nulo).
    h.db.with_connection(|c| {
        c.execute_batch(
            "INSERT INTO ai_requests (id, purpose, question_id, session_id, origin_key, variant_key, sources_json)
             VALUES ('air-orig', 'feedback', 'sq1', 's1', 'original:sq1', 'default',
                     '{\"question\":\"x\",\"criteria\":[],\"answers\":[]}');
             INSERT INTO ai_attempts (id, request_id, generation, command_id, payload_hash, state, settings_json, usage_json, created_at)
             VALUES ('aia-orig', 'air-orig', 1, 'cmd-orig', 'h', 'interrupted', '{}', NULL, '2026-11-02T09:40:00Z');",
        )
        .map_err(sql_error)?;
        Ok(())
    })
    .unwrap();

    // Reformulação da original: tentativa falhada e repetição bem-sucedida.
    let command = |level| RewriteCommand {
        request_id: new_id(),
        origin: original_origin(),
        level,
    };
    h.service
        .request_rewrite(command(RewriteLevel::B1))
        .await
        .unwrap();
    h.done().await;
    let failed = h
        .service
        .list_rewrites(original_origin())
        .await
        .unwrap()
        .remove(0);
    assert_eq!(failed.state, ReportState::Failed);
    h.service
        .request_rewrite(command(RewriteLevel::B1))
        .await
        .unwrap();
    h.done().await;

    // Prática: reformulação da prática e avaliação.
    let practice = h.create_practice().await;
    let confirmed = h
        .confirm_practice(practice, "I failed a release and we fixed the process.")
        .await;
    h.service
        .evaluate_practice(EvaluatePractice {
            request_id: new_id(),
            practice_id: confirmed.id.clone(),
            expected_version: confirmed.version,
        })
        .await
        .unwrap();
    h.done().await;

    // Uma tentativa descartada (resultado tardio) também conta, com o seu consumo.
    h.db.with_connection(|c| {
        let request: String = c
            .query_row(
                "SELECT id FROM ai_requests WHERE purpose = 'practice_feedback'",
                [],
                |r| r.get(0),
            )
            .map_err(sql_error)?;
        c.execute(
            "INSERT INTO ai_attempts (id, request_id, generation, command_id, payload_hash, state, settings_json, usage_json, created_at)
             VALUES ('aia-disc', ?1, 9, 'cmd-disc', 'h', 'discarded', '{}',
                     '{\"warnings\":[],\"input\":10,\"output\":5,\"cache\":null,\"reported\":0.05,\"estimated\":null,\"conservative\":false,\"billingSource\":null,\"providerEstimated\":null}',
                     '2026-11-03T09:40:00Z')",
            [&request],
        )
        .map_err(sql_error)?;
        Ok(())
    })
    .unwrap();

    let usage = h.service.load_training_usage("s1".into()).await.unwrap();
    assert_eq!(usage.session_id, "s1");

    // Entrevista original: 1 tentativa, consumo desconhecido => incompleto.
    assert_eq!(usage.original_interview.network_attempt_count, 1);
    assert_eq!(usage.original_interview.totals.unknown_attempt_count, 1);
    assert!(!usage.original_interview.totals.complete);
    assert_eq!(usage.original_interview.by_purpose.len(), 1);
    assert_eq!(usage.original_interview.by_purpose[0].purpose, "feedback");

    // Prática posterior: reformulação falhada (sem consumo) + concluída + avaliação + descartada.
    let later = &usage.later_practice;
    assert_eq!(later.network_attempt_count, 4);
    let purposes: Vec<(&str, u64)> = later
        .by_purpose
        .iter()
        .map(|p| (p.purpose.as_str(), p.network_attempt_count))
        .collect();
    assert_eq!(purposes, vec![("practice_feedback", 2), ("rewrite", 2)]);
    assert!(
        !later.totals.complete,
        "a tentativa falhada sem consumo conhecido não vale zero"
    );
    assert_eq!(later.totals.unknown_attempt_count, 1);
    assert!((later.totals.known_reported_subtotal - 0.07).abs() < 1e-9);

    // Combinado: união por ids, cada rede uma vez.
    let combined = &usage.combined;
    assert_eq!(
        combined.network_attempt_count,
        usage.original_interview.network_attempt_count + later.network_attempt_count
    );
    assert_eq!(combined.network_attempt_count, 5);
    assert_eq!(combined.totals.unknown_attempt_count, 2);
    assert!(!combined.totals.complete);
    assert!(
        (combined.totals.known_reported_subtotal
            - (usage.original_interview.totals.known_reported_subtotal
                + later.totals.known_reported_subtotal))
            .abs()
            < 1e-9
    );
    let total_by_purpose: u64 = combined
        .by_purpose
        .iter()
        .map(|p| p.network_attempt_count)
        .sum();
    assert_eq!(total_by_purpose, 5);

    // O subconjunto da prática mostrado na avaliação não é somado outra vez.
    let report = h
        .service
        .load_practice_report(confirmed.id.clone())
        .await
        .unwrap();
    assert_eq!(
        report.network_attempts.len(),
        2,
        "avaliação e descartada da prática"
    );
    assert_eq!(combined.network_attempt_count, 5);

    // O áudio local não acrescenta tentativas nem custo.
    let rewrite = h.rewrite(original_origin(), RewriteLevel::B1).await;
    let snapshot = h
        .service
        .play_audio(AudioSelection::Rewrite {
            rewrite_id: rewrite.id.clone().unwrap(),
        })
        .await
        .unwrap();
    h.finish_synthesis(&snapshot).await;
    let after = h.service.load_training_usage("s1".into()).await.unwrap();
    assert_eq!(after.combined.network_attempt_count, 5);

    // Sessão sem tentativas: grupos vazios e completos; sessão inexistente é recusada.
    let empty = h.service.load_training_usage("s2".into()).await.unwrap();
    assert_eq!(empty.combined.network_attempt_count, 0);
    assert!(empty.combined.totals.complete);
    assert!(h.service.load_training_usage(new_id()).await.is_err());
}

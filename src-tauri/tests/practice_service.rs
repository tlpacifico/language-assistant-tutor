//! Ciclo local de prática (tarefa 2 da fase 4).
//!
//! Nenhum teste abre um microfone real nem arranca subprocessos: a porta de
//! áudio, o auxiliar e o arranque são substituídos por adaptadores controlados.
//! A base de dados e os artefactos são reais (diretório temporário), com sessões
//! concluídas locais e de IA semeadas por SQL direto, para cobrir criação,
//! abandono, revisão, confirmação e transcrição de uma prática individual.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use hound::{SampleFormat, WavSpec, WavWriter};

use language_assistant_tutor::artifacts::ArtifactStore;
use language_assistant_tutor::audio::{
    AudioPort, AvailabilityAudio, CapturedAudio, RecordingTarget,
};
use language_assistant_tutor::db::Database;
use language_assistant_tutor::domain::{error_code, AppError, CaptureMeta, ComponentState};
use language_assistant_tutor::interview::{Id, ReviewOrigin, Version};
use language_assistant_tutor::practice::{
    CaptureOwner, ConfirmPracticeAnswer, CreatePractice, PracticeAnswerTarget, PracticeCommand,
    PracticeDetail, PracticeStatus, SavePracticeReview,
};
use language_assistant_tutor::service::{
    AppService, LaunchedWorker, ServicePaths, WorkerControl, WorkerLauncher,
};
use language_assistant_tutor::worker::{WorkerEvent, WorkerMessage, WorkerRequest};

// ---------------------------------------------------------------------------
// Porta de áudio controlada
// ---------------------------------------------------------------------------

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
    starts: usize,
    corrupt_stop: bool,
}

#[derive(Debug, Default)]
struct FakeAudio {
    state: Mutex<AudioState>,
}

impl FakeAudio {
    fn starts(&self) -> usize {
        self.state.lock().expect("estado do áudio").starts
    }

    fn corrupt_next_stop(&self) {
        self.state.lock().expect("estado do áudio").corrupt_stop = true;
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
        let mut state = self.state.lock().expect("estado do áudio");
        write_rate_wav(&target.directory.join("original.wav"), 48_000);
        state.starts += 1;
        state.capture = Some(target);
        Ok(())
    }

    fn stop_capture(&self) -> Result<CapturedAudio, AppError> {
        let mut state = self.state.lock().expect("estado do áudio");
        let target = state.capture.take().ok_or_else(|| {
            AppError::new(
                error_code::CAPTURE_INTERRUPTED,
                "Não há nenhuma gravação em curso.",
                false,
            )
        })?;
        let original = target.directory.join("original.wav");
        if state.corrupt_stop {
            state.corrupt_stop = false;
            write_empty_wav(&original);
        }
        let bytes = std::fs::metadata(&original)
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

    fn play(&self, _path: PathBuf) -> Result<(), AppError> {
        Ok(())
    }

    fn stop_playback(&self) -> Result<(), AppError> {
        Ok(())
    }

    fn reopen(&self) -> Result<(), AppError> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Auxiliar controlado
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct WorkerState {
    requests: Vec<WorkerRequest>,
    running: bool,
}

#[derive(Debug)]
struct FakeWorker {
    state: Mutex<WorkerState>,
}

impl Default for FakeWorker {
    fn default() -> Self {
        Self {
            state: Mutex::new(WorkerState {
                requests: Vec::new(),
                running: true,
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
        state.requests.push(request);
        Ok(())
    }

    fn restart(&self) -> Result<(), AppError> {
        Ok(())
    }

    fn shutdown(&self) -> Result<(), AppError> {
        self.state.lock().expect("estado do auxiliar").running = false;
        Ok(())
    }

    fn generation(&self) -> u64 {
        1
    }

    fn is_running(&self) -> bool {
        self.state.lock().expect("estado do auxiliar").running
    }
}

struct FakeLauncher {
    worker: Arc<FakeWorker>,
    launches: AtomicUsize,
}

impl FakeLauncher {
    fn new(worker: Arc<FakeWorker>) -> Self {
        Self {
            worker,
            launches: AtomicUsize::new(0),
        }
    }
}

impl WorkerLauncher for FakeLauncher {
    fn launch(&self) -> Result<LaunchedWorker, AppError> {
        self.launches.fetch_add(1, Ordering::SeqCst);
        Ok(LaunchedWorker {
            control: self.worker.clone(),
            events: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Arranjo do serviço
// ---------------------------------------------------------------------------

fn new_request_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Semeia duas sessões concluídas (uma local, uma de IA) com confirmações.
fn seed_completed(database: &Database) {
    database
        .with_connection(|connection| {
            connection
                .execute_batch(
                    r#"
INSERT INTO questions (id, text, category, competencies_json, criteria_json, version, created_at, updated_at) VALUES
 ('q1', 'Tell me about a time you failed.', 'failure_learning', '["Aprendizagem"]', '["Explicar o que aprendeu."]', 1, '2026-11-02T09:00:00Z', '2026-11-02T09:00:00Z'),
 ('q2', 'Tell me about a time you disagreed.', 'conflict_collaboration', '["Colaboração"]', '["Explicar a divergência."]', 1, '2026-11-02T09:00:00Z', '2026-11-02T09:00:00Z'),
 ('q3', 'Tell me about a time you adapted.', 'change_adaptation', '["Adaptação"]', '["Explicar a mudança."]', 1, '2026-11-02T09:00:00Z', '2026-11-02T09:00:00Z');

INSERT INTO sessions (id, category, question_count, position, status, mode, version, created_at, completed_at) VALUES
 ('s1', NULL, 1, 1, 'completed', 'local_v2', 1, '2026-11-02T09:00:00Z', '2026-11-02T09:30:00Z'),
 ('s2', NULL, 1, 1, 'completed', 'ai_v3', 1, '2026-11-02T10:00:00Z', '2026-11-02T10:30:00Z');

INSERT INTO session_questions (id, session_id, source_question_id, source_version, position, text, category, competencies_json, criteria_json, selected_recording_id, selection_version) VALUES
 ('sq1', 's1', 'q1', 1, 1, 'Tell me about a time you failed.', 'failure_learning', '["Aprendizagem"]', '["Explicar o que aprendeu."]', NULL, 0),
 ('sq2', 's2', 'q2', 1, 1, 'Tell me about a time you disagreed.', 'conflict_collaboration', '["Colaboração"]', '["Explicar a divergência."]', NULL, 0);

INSERT INTO recordings (id, session_question_id, ordinal, state, original_file, normalized_file, capture_json, created_at, updated_at) VALUES
 ('rec1', 'sq1', 1, 'ready', 's1/sq1/rec1/original.wav', NULL, NULL, '2026-11-02T09:05:00Z', '2026-11-02T09:06:00Z'),
 ('rec2', 'sq2', 1, 'ready', 's2/sq2/rec2/original.wav', NULL, NULL, '2026-11-02T10:05:00Z', '2026-11-02T10:06:00Z');

INSERT INTO reviews (recording_id, version, source_transcript_id, origin, text, saved_at) VALUES
 ('rec1', 1, NULL, 'manual_explicit', 'I failed a release.', '2026-11-02T09:08:00Z'),
 ('rec2', 1, NULL, 'manual_explicit', 'I disagreed on the checklist.', '2026-11-02T10:08:00Z');

INSERT INTO confirmations (id, session_question_id, recording_id, review_version, source_transcript_id, origin, text, confirmed_at) VALUES
 ('c1', 'sq1', 'rec1', 1, NULL, 'manual_explicit', 'I failed a release.', '2026-11-02T09:09:00Z'),
 ('c2', 'sq2', 'rec2', 1, NULL, 'manual_explicit', 'I disagreed on the checklist.', '2026-11-02T10:09:00Z');
"#,
                )
                .map_err(|error| AppError::new("TEST_SQLITE", error.to_string(), false))?;
            Ok(())
        })
        .expect("semear as sessões concluídas");
}

struct Harness {
    _directory: tempfile::TempDir,
    database_path: PathBuf,
    legacy_root: PathBuf,
    artifacts_root: PathBuf,
    manifest: PathBuf,
    database: Database,
    audio: Arc<FakeAudio>,
    worker: Arc<FakeWorker>,
    service: Arc<AppService>,
}

impl Harness {
    async fn new() -> Self {
        let directory = tempfile::tempdir().expect("pasta temporária");
        let database_path = directory.path().join("tutor.sqlite3");
        let legacy_root = directory.path().join("phase1");
        let artifacts_root = directory.path().join("sessions");
        std::fs::create_dir_all(&legacy_root).expect("pasta da fase 1");
        let whisper = directory.path().join("whisper");
        std::fs::create_dir_all(&whisper).expect("pasta dos modelos");
        let manifest = directory.path().join("models.json");
        std::fs::write(
            &manifest,
            serde_json::to_vec(&serde_json::json!({
                "schemaVersion": 1,
                "whisperDir": whisper.display().to_string(),
            }))
            .expect("codificar manifesto"),
        )
        .expect("escrever manifesto de modelos");

        let database = Database::open(database_path.clone()).expect("abrir a base de teste");
        seed_completed(&database);

        let worker = Arc::new(FakeWorker::default());
        let launcher = Arc::new(FakeLauncher::new(worker.clone()));
        let audio = Arc::new(FakeAudio::default());
        let service = Arc::new(AppService::new(
            ServicePaths {
                database: database_path.clone(),
                legacy: legacy_root.clone(),
            },
            ArtifactStore::new(artifacts_root.clone()),
            audio.clone(),
            launcher,
            Some(manifest.clone()),
        ));
        service.start().await;

        Self {
            _directory: directory,
            database_path,
            legacy_root,
            artifacts_root,
            manifest,
            database,
            audio,
            worker,
            service,
        }
    }

    async fn create(&self, session_id: &str, session_question_id: &str) -> PracticeDetail {
        self.service
            .create_practice(CreatePractice {
                request_id: new_request_id(),
                session_id: session_id.to_owned(),
                session_question_id: session_question_id.to_owned(),
                previous_practice_id: None,
            })
            .await
            .expect("criar a prática")
    }

    fn owner_of(detail: &PracticeDetail) -> CaptureOwner {
        CaptureOwner::Practice {
            practice_id: detail.id.clone(),
            answer_id: detail.answer.id.clone(),
        }
    }

    fn target_of(
        &self,
        detail: &PracticeDetail,
        recording_id: Id,
        selection_version: Version,
    ) -> PracticeAnswerTarget {
        PracticeAnswerTarget {
            owner: Self::owner_of(detail),
            recording_id,
            selection_version,
        }
    }
}
// ---------------------------------------------------------------------------
// Criação, leitura e abandono
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_practice_is_idempotent_and_loads_from_local_and_ai_sessions() {
    let harness = Harness::new().await;

    let request_id = new_request_id();
    let first = harness
        .service
        .create_practice(CreatePractice {
            request_id: request_id.clone(),
            session_id: "s1".to_owned(),
            session_question_id: "sq1".to_owned(),
            previous_practice_id: None,
        })
        .await
        .expect("criar a partir de uma sessão local concluída");
    assert_eq!(first.status, PracticeStatus::Preparing);
    assert_eq!(first.version, 1);
    assert_eq!(first.ordinal, 1);
    assert_eq!(first.answer.role, "attempt");
    assert_eq!(first.answer.practice_id.as_deref(), Some(first.id.as_str()));
    assert_eq!(first.question.text, "Tell me about a time you failed.");

    // Repetir o mesmo pedido devolve a mesma prática, sem criar outra.
    let replay = harness
        .service
        .create_practice(CreatePractice {
            request_id,
            session_id: "s1".to_owned(),
            session_question_id: "sq1".to_owned(),
            previous_practice_id: None,
        })
        .await
        .expect("repetir a criação é idempotente");
    assert_eq!(replay.id, first.id);

    let listed = harness
        .service
        .list_practices("sq1".to_owned())
        .await
        .expect("listar as práticas");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, first.id);

    let loaded = harness
        .service
        .load_practice(first.id.clone())
        .await
        .expect("carregar a prática");
    assert_eq!(loaded, first);

    // A fotografia expõe a prática pendente e o seu dono.
    let snapshot = harness.service.snapshot();
    assert_eq!(
        snapshot
            .practice
            .active_practice
            .as_ref()
            .map(|p| p.id.clone()),
        Some(first.id.clone())
    );

    // Uma segunda prática pendente é recusada enquanto a primeira está aberta.
    let err = harness
        .service
        .create_practice(CreatePractice {
            request_id: new_request_id(),
            session_id: "s2".to_owned(),
            session_question_id: "sq2".to_owned(),
            previous_practice_id: None,
        })
        .await
        .expect_err("não se cria uma segunda prática pendente");
    assert_eq!(err.code, error_code::STALE_SOURCE);

    // Abandonada a primeira, uma sessão de IA concluída serve de origem.
    harness
        .service
        .abandon_practice(PracticeCommand {
            request_id: new_request_id(),
            practice_id: first.id.clone(),
            expected_version: first.version,
        })
        .await
        .expect("abandonar a primeira prática");
    let ai = harness
        .service
        .create_practice(CreatePractice {
            request_id: new_request_id(),
            session_id: "s2".to_owned(),
            session_question_id: "sq2".to_owned(),
            previous_practice_id: None,
        })
        .await
        .expect("criar a partir de uma sessão de IA concluída");
    assert_eq!(ai.question.text, "Tell me about a time you disagreed.");
}

#[tokio::test]
async fn create_practice_refuses_invalid_sources() {
    let harness = Harness::new().await;

    // Sessão em curso.
    harness
        .database
        .with_connection(|connection| {
            connection
                .execute(
                    "INSERT INTO sessions (id, category, question_count, position, status, mode, version, created_at, completed_at)
                     VALUES ('s-active', NULL, 1, 1, 'in_progress', 'local_v2', 1, '2026-11-02T11:00:00Z', NULL)",
                    [],
                )
                .map_err(|error| AppError::new("TEST_SQLITE", error.to_string(), false))?;
            Ok(())
        })
        .expect("semear uma sessão em curso");

    let err = harness
        .service
        .create_practice(CreatePractice {
            request_id: new_request_id(),
            session_id: "s-active".to_owned(),
            session_question_id: "sq1".to_owned(),
            previous_practice_id: None,
        })
        .await
        .expect_err("sessão em curso não é origem");
    assert_eq!(err.code, error_code::STALE_SOURCE);

    // Origem sem confirmação: uma pergunta nova ainda não confirmada.
    harness
        .database
        .with_connection(|connection| {
            connection
                .execute_batch(
                    "INSERT INTO session_questions (id, session_id, source_question_id, source_version, position, text, category, competencies_json, criteria_json, selected_recording_id, selection_version)
                     VALUES ('sq3', 's1', 'q3', 1, 2, 'Tell me about a time you adapted.', 'change_adaptation', '[\"Adaptação\"]', '[\"Explicar a mudança.\"]', NULL, 0);",
                )
                .map_err(|error| AppError::new("TEST_SQLITE", error.to_string(), false))?;
            Ok(())
        })
        .expect("semear uma pergunta não confirmada");

    let err = harness
        .service
        .create_practice(CreatePractice {
            request_id: new_request_id(),
            session_id: "s1".to_owned(),
            session_question_id: "sq3".to_owned(),
            previous_practice_id: None,
        })
        .await
        .expect_err("origem sem confirmação é recusada");
    assert_eq!(err.code, error_code::STALE_SOURCE);

    // Origem desconhecida (um "treino JSON" da fase 1 não tem linha guardada).
    let err = harness
        .service
        .create_practice(CreatePractice {
            request_id: new_request_id(),
            session_id: "s1".to_owned(),
            session_question_id: "json-attempt".to_owned(),
            previous_practice_id: None,
        })
        .await
        .expect_err("origem desconhecida é recusada");
    assert_eq!(err.code, error_code::STALE_SOURCE);
}

#[tokio::test]
async fn abandon_conserves_and_blocks_reopen() {
    let harness = Harness::new().await;
    let detail = harness.create("s1", "sq1").await;

    // Gravar e rever antes de abandonar.
    let started = harness
        .service
        .start_practice_recording(Harness::owner_of(&detail), 0)
        .await
        .expect("gravar a tentativa");
    assert!(started.recording_id.is_some());
    let stopped = harness
        .service
        .stop_recording()
        .await
        .expect("terminar a gravação");
    assert!(stopped.recording_id.is_none());

    let current = harness
        .service
        .load_practice(detail.id.clone())
        .await
        .expect("recarregar a prática");
    let recording_id = current
        .answer
        .selected_recording_id
        .clone()
        .expect("gravação selecionada");
    let selection_version = current.answer.selection_version;

    harness
        .service
        .save_practice_review(SavePracticeReview {
            request_id: new_request_id(),
            target: harness.target_of(&current, recording_id, selection_version),
            expected_review_version: 0,
            source_transcript_id: None,
            origin: ReviewOrigin::ManualExplicit,
            text: "I failed a release and we fixed it.".to_owned(),
        })
        .await
        .expect("guardar a revisão");

    let abandoned = harness
        .service
        .abandon_practice(PracticeCommand {
            request_id: new_request_id(),
            practice_id: detail.id.clone(),
            expected_version: detail.version,
        })
        .await
        .expect("abandonar a prática");
    assert_eq!(abandoned.status, PracticeStatus::Abandoned);
    assert!(abandoned.abandoned_at.is_some());
    assert!(
        !abandoned.recordings.is_empty(),
        "as gravações são conservadas"
    );
    assert!(
        abandoned.recordings.iter().any(|r| r.review.is_some()),
        "a revisão guardada é conservada"
    );

    // Uma prática abandonada não volta a ser aberta nem confirmada.
    let target = harness.target_of(
        &abandoned,
        abandoned
            .answer
            .selected_recording_id
            .clone()
            .unwrap_or_default(),
        abandoned.answer.selection_version,
    );
    let confirm = harness
        .service
        .confirm_practice_answer(ConfirmPracticeAnswer {
            request_id: new_request_id(),
            target,
            expected_review_version: 1,
            expected_practice_version: abandoned.version,
            source_transcript_id: None,
            origin: ReviewOrigin::ManualExplicit,
            text: "I failed a release and we fixed it.".to_owned(),
        })
        .await
        .expect_err("uma prática abandonada não é confirmada");
    assert_eq!(confirm.code, error_code::STALE_SOURCE);

    // O abandono liberta a vaga de prática pendente.
    let next = harness.create("s1", "sq1").await;
    assert_eq!(
        next.ordinal, 2,
        "a prática seguinte é a segunda da pergunta"
    );
}

// ---------------------------------------------------------------------------
// Gravação, revisão e confirmação
// ---------------------------------------------------------------------------

#[tokio::test]
async fn rerecording_keeps_takes_and_ordinal() {
    let harness = Harness::new().await;
    let detail = harness.create("s1", "sq1").await;

    let mut selection = 0;
    for _ in 0..2 {
        let started = harness
            .service
            .start_practice_recording(Harness::owner_of(&detail), selection)
            .await
            .expect("gravar");
        assert!(started.recording_id.is_some());
        harness
            .service
            .stop_recording()
            .await
            .expect("terminar a gravação");
        let current = harness
            .service
            .load_practice(detail.id.clone())
            .await
            .expect("recarregar");
        selection = current.answer.selection_version;
    }

    let loaded = harness
        .service
        .load_practice(detail.id.clone())
        .await
        .expect("recarregar");
    assert_eq!(
        loaded.recordings.len(),
        2,
        "as duas tomadas são conservadas"
    );
    assert_eq!(loaded.recordings[0].ordinal, 1);
    assert_eq!(loaded.recordings[1].ordinal, 2);
    assert_eq!(
        loaded.answer.selected_recording_id,
        Some(loaded.recordings[1].id.clone()),
        "a tomada mais recente fica selecionada"
    );
}

#[tokio::test]
async fn confirmation_is_immutable_and_exact_and_idempotent() {
    let harness = Harness::new().await;
    let detail = harness.create("s1", "sq1").await;

    harness
        .service
        .start_practice_recording(Harness::owner_of(&detail), 0)
        .await
        .expect("gravar");
    harness.service.stop_recording().await.expect("terminar");

    let current = harness
        .service
        .load_practice(detail.id.clone())
        .await
        .expect("recarregar");
    let recording_id = current.answer.selected_recording_id.clone().unwrap();
    let selection_version = current.answer.selection_version;
    let target = harness.target_of(&current, recording_id, selection_version);

    let text = "I failed a release and we fixed the process.";
    harness
        .service
        .save_practice_review(SavePracticeReview {
            request_id: new_request_id(),
            target: target.clone(),
            expected_review_version: 0,
            source_transcript_id: None,
            origin: ReviewOrigin::ManualExplicit,
            text: text.to_owned(),
        })
        .await
        .expect("guardar a revisão");

    let request_id = new_request_id();
    let confirmed = harness
        .service
        .confirm_practice_answer(ConfirmPracticeAnswer {
            request_id: request_id.clone(),
            target: target.clone(),
            expected_review_version: 1,
            expected_practice_version: detail.version,
            source_transcript_id: None,
            origin: ReviewOrigin::ManualExplicit,
            text: text.to_owned(),
        })
        .await
        .expect("confirmar a tentativa");
    assert_eq!(confirmed.status, PracticeStatus::Confirmed);
    assert_eq!(confirmed.version, 2);
    assert!(confirmed.confirmed_at.is_some());
    let confirmation = confirmed.confirmation.expect("confirmação guardada");
    assert_eq!(confirmation.text, text, "o texto confirmado é exato");

    // Repetir o mesmo pedido devolve a mesma confirmação.
    let replay = harness
        .service
        .confirm_practice_answer(ConfirmPracticeAnswer {
            request_id,
            target: target.clone(),
            expected_review_version: 1,
            expected_practice_version: detail.version,
            source_transcript_id: None,
            origin: ReviewOrigin::ManualExplicit,
            text: text.to_owned(),
        })
        .await
        .expect("a repetição é idempotente");
    assert_eq!(replay.confirmation.unwrap().id, confirmation.id);

    // Um pedido novo sobre uma prática já confirmada é recusado.
    let err = harness
        .service
        .confirm_practice_answer(ConfirmPracticeAnswer {
            request_id: new_request_id(),
            target,
            expected_review_version: 1,
            expected_practice_version: 2,
            source_transcript_id: None,
            origin: ReviewOrigin::ManualExplicit,
            text: "A different text.".to_owned(),
        })
        .await
        .expect_err("uma prática confirmada é imutável");
    assert_eq!(err.code, error_code::STALE_SOURCE);
}

#[tokio::test]
async fn transcription_failure_retry_reuses_the_same_file() {
    let harness = Harness::new().await;
    let detail = harness.create("s1", "sq1").await;

    harness
        .service
        .start_practice_recording(Harness::owner_of(&detail), 0)
        .await
        .expect("gravar");
    harness.service.stop_recording().await.expect("terminar");

    let current = harness
        .service
        .load_practice(detail.id.clone())
        .await
        .expect("recarregar");
    let recording_id = current.answer.selected_recording_id.clone().unwrap();
    let selection_version = current.answer.selection_version;
    let target = harness.target_of(&current, recording_id, selection_version);

    harness
        .service
        .transcribe_practice_response(target.clone())
        .await
        .expect("transcrever a tentativa");
    let first = harness
        .worker
        .requests()
        .pop()
        .expect("pedido de transcrição");
    let first_audio = PathBuf::from(first.params["audioPath"].as_str().expect("audioPath"));

    // O auxiliar perde o pedido: o derivado e o original mantêm-se.
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
    assert!(harness.service.snapshot().model_job.is_none());

    harness
        .service
        .transcribe_practice_response(target)
        .await
        .expect("repetir a transcrição");
    let retry = harness.worker.requests().pop().expect("novo pedido");
    let retry_audio = PathBuf::from(retry.params["audioPath"].as_str().expect("audioPath"));
    assert_eq!(
        retry_audio, first_audio,
        "o retry reutiliza o mesmo ficheiro"
    );
    assert_ne!(retry.id, first.id, "o identificador é novo");
}

#[tokio::test]
async fn publication_failure_keeps_previous_recording_and_audio() {
    let harness = Harness::new().await;
    let detail = harness.create("s1", "sq1").await;

    harness
        .service
        .start_practice_recording(Harness::owner_of(&detail), 0)
        .await
        .expect("primeira gravação");
    harness
        .service
        .stop_recording()
        .await
        .expect("publicar a primeira");

    let before = harness
        .service
        .load_practice(detail.id.clone())
        .await
        .expect("recarregar");
    let previous_id = before.answer.selected_recording_id.clone().unwrap();

    // A segunda captura publica um original sem amostras: a publicação falha.
    harness
        .service
        .start_practice_recording(Harness::owner_of(&detail), before.answer.selection_version)
        .await
        .expect("segunda gravação");
    let pending_id = harness.service.snapshot().recording_id.clone().unwrap();
    harness.audio.corrupt_next_stop();
    let error = harness
        .service
        .stop_recording()
        .await
        .expect_err("a publicação falha");
    assert_eq!(error.code, error_code::INVALID_AUDIO);

    let after = harness
        .service
        .load_practice(detail.id.clone())
        .await
        .expect("recarregar");
    assert_eq!(
        after.answer.selected_recording_id.as_deref(),
        Some(previous_id.as_str()),
        "a gravação anterior mantém-se selecionada"
    );
    assert_eq!(
        harness.service.snapshot().pending_recording_id.as_deref(),
        Some(pending_id.as_str()),
        "a publicação pendente fica visível"
    );

    // Reparado o ficheiro, a mesma gravação é publicada sem nova captura.
    let current = harness
        .service
        .load_practice(detail.id.clone())
        .await
        .expect("recarregar");
    let dir = harness
        .artifacts_root
        .join("s1")
        .join(&current.answer.id)
        .join(&pending_id);
    write_rate_wav(&dir.join("original.wav"), 48_000);
    harness
        .service
        .retry_save_recording(pending_id.clone())
        .await
        .expect("repetir a publicação");
    let restored = harness
        .service
        .load_practice(detail.id.clone())
        .await
        .expect("recarregar");
    assert_eq!(restored.answer.selected_recording_id, Some(pending_id));
}

#[tokio::test]
async fn old_owner_cannot_capture_the_practice_slot() {
    let harness = Harness::new().await;
    let detail = harness.create("s1", "sq1").await;

    // O dono de entrevista não é aceite como tentativa de prática.
    let err = harness
        .service
        .start_practice_recording(
            CaptureOwner::Interview {
                session_id: "s1".to_owned(),
                session_question_id: "sq1".to_owned(),
                answer_id: "sq1".to_owned(),
            },
            0,
        )
        .await
        .expect_err("dono de entrevista não abre a captura de prática");
    assert_eq!(err.code, error_code::STALE_SOURCE);
    assert_eq!(harness.audio.starts(), 0, "nenhum microfone foi aberto");

    // O slot original não pode ser usado como tentativa da prática.
    let err = harness
        .service
        .save_practice_review(SavePracticeReview {
            request_id: new_request_id(),
            target: PracticeAnswerTarget {
                owner: CaptureOwner::Practice {
                    practice_id: detail.id.clone(),
                    answer_id: "sq1".to_owned(),
                },
                recording_id: "rec1".to_owned(),
                selection_version: 0,
            },
            expected_review_version: 0,
            source_transcript_id: None,
            origin: ReviewOrigin::ManualExplicit,
            text: "nope".to_owned(),
        })
        .await
        .expect_err("o slot original não é a tentativa da prática");
    assert_eq!(err.code, error_code::STALE_SOURCE);
}

#[tokio::test]
async fn late_event_applies_only_to_the_capturing_owner() {
    let harness = Harness::new().await;
    let detail = harness.create("s1", "sq1").await;

    harness
        .service
        .start_practice_recording(Harness::owner_of(&detail), 0)
        .await
        .expect("gravar a tentativa");
    let recording_id = harness.service.snapshot().recording_id.clone().unwrap();

    // Uma interrupção atrasada de outra gravação não retira a captura atual.
    harness
        .service
        .on_audio_event(
            language_assistant_tutor::audio::AudioEvent::CaptureInterrupted {
                recording_id: Some("outra-gravacao".to_owned()),
                capture: None,
                error: AppError::new(
                    error_code::CAPTURE_INTERRUPTED,
                    "Interrupção atrasada.",
                    true,
                ),
            },
        )
        .await;
    assert_eq!(
        harness.service.snapshot().recording_id.as_deref(),
        Some(recording_id.as_str()),
        "a captura atual continua a poder ser terminada"
    );

    harness
        .service
        .stop_recording()
        .await
        .expect("terminar a captura");
}

#[tokio::test]
async fn restart_resumes_review_and_state_without_opening_the_microphone() {
    let first = Harness::new().await;
    let detail = first.create("s1", "sq1").await;

    first
        .service
        .start_practice_recording(Harness::owner_of(&detail), 0)
        .await
        .expect("gravar");
    first.service.stop_recording().await.expect("terminar");

    let current = first
        .service
        .load_practice(detail.id.clone())
        .await
        .expect("recarregar");
    let recording_id = current.answer.selected_recording_id.clone().unwrap();
    let selection_version = current.answer.selection_version;
    first
        .service
        .save_practice_review(SavePracticeReview {
            request_id: new_request_id(),
            target: first.target_of(&current, recording_id, selection_version),
            expected_review_version: 0,
            source_transcript_id: None,
            origin: ReviewOrigin::ManualExplicit,
            text: "I failed a release and we fixed it.".to_owned(),
        })
        .await
        .expect("guardar a revisão");

    let database_path = first.database_path.clone();
    let legacy_root = first.legacy_root.clone();
    let artifacts_root = first.artifacts_root.clone();
    let manifest = first.manifest.clone();
    // `first` continua vivo: o `TempDir` tem de sobreviver ao arranque novo.

    // Um arranque novo na mesma base retoma a prática sem abrir o microfone.
    let worker = Arc::new(FakeWorker::default());
    let launcher = Arc::new(FakeLauncher::new(worker));
    let audio = Arc::new(FakeAudio::default());
    let service = Arc::new(AppService::new(
        ServicePaths {
            database: database_path,
            legacy: legacy_root,
        },
        ArtifactStore::new(artifacts_root),
        audio.clone(),
        launcher,
        Some(manifest),
    ));
    service.start().await;

    assert_eq!(audio.starts(), 0, "o reinício não abre o microfone");
    let snapshot = service.snapshot();
    let active = snapshot
        .practice
        .active_practice
        .expect("a prática pendente é retomada");
    assert_eq!(active.id, detail.id);
    assert_eq!(active.recordings.len(), 1, "a gravação é retomada");
    assert!(
        active.recordings[0].review.is_some(),
        "a revisão guardada é retomada"
    );

    let loaded = service
        .load_practice(detail.id)
        .await
        .expect("carregar a prática após o reinício");
    // O arranque da primeira captura move a prática para `answering`, mesmo sem
    // apoio de história (transição incondicional, tarefa 3).
    assert_eq!(loaded.status, PracticeStatus::Answering);
    assert!(
        loaded.recordings[0]
            .review
            .as_ref()
            .is_some_and(|r| r.text == "I failed a release and we fixed it."),
        "a revisão é conservada através do reinício"
    );
}

#[tokio::test]
async fn snapshot_practice_follows_publication_and_preparation_is_readable() {
    let harness = Harness::new().await;
    let detail = harness.create("s1", "sq1").await;
    let owner = Harness::owner_of(&detail);

    let preparation = harness
        .service
        .load_preparation(owner.clone())
        .await
        .expect("ler a preparação");
    assert_eq!(preparation.answer_id, detail.answer.id);

    harness
        .service
        .start_practice_recording(owner, 0)
        .await
        .expect("gravar");
    let stopped = harness
        .service
        .stop_recording()
        .await
        .expect("terminar a gravação");
    let active = stopped
        .practice
        .active_practice
        .expect("prática pendente na fotografia");
    assert_eq!(active.recordings.len(), 1, "a publicação reflete-se na fotografia");
    assert!(stopped.current.is_none(), "sem pergunta de entrevista fictícia");
}

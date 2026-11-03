//! Histórias locais e snapshot do apoio no arranque da captura (tarefa 3).
//!
//! Nenhum teste abre um microfone real nem arranca subprocessos: a porta de
//! áudio, o auxiliar e o arranque são substituídos por adaptadores controlados;
//! as falhas de SQLite são simuladas pelo ponto de injeção do serviço. A base e
//! os artefactos são reais (diretório temporário), para cobrir CRUD de histórias,
//! seleção/snapshot do apoio, recuperação e o isolamento do marcador exclusivo.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use hound::{SampleFormat, WavSpec, WavWriter};

use language_assistant_tutor::artifacts::ArtifactStore;
use language_assistant_tutor::audio::{
    AudioPort, AvailabilityAudio, CapturedAudio, RecordingTarget,
};
use language_assistant_tutor::db::{missing_fields, validate_story_input, Database};
use language_assistant_tutor::domain::{error_code, AppError, CaptureMeta, ComponentState};
use language_assistant_tutor::interview::{ReviewOrigin, Version};
use language_assistant_tutor::practice::{
    CaptureOwner, CreateStory, LockState, MissingField, PracticeStatus, PreparationDecision,
    SelectStory, StoryInput, StoryView, UpdateStory,
};
use language_assistant_tutor::service::{
    AppService, LaunchedWorker, PreparationFault, ServicePaths, WorkerControl, WorkerLauncher,
};
use language_assistant_tutor::worker::WorkerRequest;

// ---------------------------------------------------------------------------
// Porta de áudio controlada (com falha de microfone opcional)
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

#[derive(Debug, Default)]
struct AudioState {
    capture: Option<RecordingTarget>,
    starts: usize,
    fail_start: bool,
}

#[derive(Debug, Default)]
struct FakeAudio {
    state: Mutex<AudioState>,
}

impl FakeAudio {
    fn starts(&self) -> usize {
        self.state.lock().expect("estado do áudio").starts
    }

    fn fail_next_start(&self) {
        self.state.lock().expect("estado do áudio").fail_start = true;
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
        if state.fail_start {
            state.fail_start = false;
            return Err(AppError::new(
                error_code::DEVICE_UNAVAILABLE,
                "O microfone não está disponível.",
                true,
            ));
        }
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
    running: bool,
}

#[derive(Debug)]
struct FakeWorker {
    state: Mutex<WorkerState>,
}

impl Default for FakeWorker {
    fn default() -> Self {
        Self {
            state: Mutex::new(WorkerState { running: true }),
        }
    }
}

impl WorkerControl for FakeWorker {
    fn submit(&self, _request: WorkerRequest) -> Result<(), AppError> {
        let state = self.state.lock().expect("estado do auxiliar");
        if !state.running {
            return Err(AppError::new(
                error_code::WORKER_UNAVAILABLE,
                "O auxiliar não está em execução.",
                true,
            ));
        }
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

/// Semeia duas sessões concluídas (local e IA) com confirmações.
fn seed_completed(database: &Database) {
    database
        .with_connection(|connection| {
            connection
                .execute_batch(
                    r#"
INSERT INTO questions (id, text, category, competencies_json, criteria_json, version, created_at, updated_at) VALUES
 ('q1', 'Tell me about a time you failed.', 'failure_learning', '["Aprendizagem"]', '["Explicar o que aprendeu."]', 1, '2026-11-02T09:00:00Z', '2026-11-02T09:00:00Z'),
 ('q2', 'Tell me about a time you disagreed.', 'conflict_collaboration', '["Colaboração"]', '["Explicar a divergência."]', 1, '2026-11-02T09:00:00Z', '2026-11-02T09:00:00Z');

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
    service: Arc<AppService>,
}

fn story_input(title: &str, situation: &str, competencies: &[&str]) -> StoryInput {
    StoryInput {
        title: title.to_owned(),
        situation: situation.to_owned(),
        task: String::new(),
        actions: String::new(),
        outcome: String::new(),
        competencies: competencies.iter().map(|c| c.to_string()).collect(),
    }
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
        let launcher = Arc::new(FakeLauncher::new(worker));
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
            service,
        }
    }

    /// Abre um serviço novo sobre a mesma base, sem tocar no atual.
    async fn reopen(&self) -> (Arc<AppService>, Arc<FakeAudio>) {
        let worker = Arc::new(FakeWorker::default());
        let launcher = Arc::new(FakeLauncher::new(worker));
        let audio = Arc::new(FakeAudio::default());
        let service = Arc::new(AppService::new(
            ServicePaths {
                database: self.database_path.clone(),
                legacy: self.legacy_root.clone(),
            },
            ArtifactStore::new(self.artifacts_root.clone()),
            audio.clone(),
            launcher,
            Some(self.manifest.clone()),
        ));
        service.start().await;
        (service, audio)
    }

    async fn create(
        &self,
        session_id: &str,
        session_question_id: &str,
    ) -> language_assistant_tutor::practice::PracticeDetail {
        self.service
            .create_practice(language_assistant_tutor::practice::CreatePractice {
                request_id: new_request_id(),
                session_id: session_id.to_owned(),
                session_question_id: session_question_id.to_owned(),
                previous_practice_id: None,
            })
            .await
            .expect("criar a prática")
    }

    fn owner_of(detail: &language_assistant_tutor::practice::PracticeDetail) -> CaptureOwner {
        CaptureOwner::Practice {
            practice_id: detail.id.clone(),
            answer_id: detail.answer.id.clone(),
        }
    }

    async fn create_story(&self, title: &str, situation: &str, competencies: &[&str]) -> StoryView {
        self.service
            .create_story(CreateStory {
                request_id: new_request_id(),
                input: story_input(title, situation, competencies),
            })
            .await
            .expect("criar história")
    }

    async fn select_story(
        &self,
        owner: CaptureOwner,
        expected_preparation_version: Version,
        story: Option<&StoryView>,
    ) {
        self.service
            .select_story(SelectStory {
                request_id: new_request_id(),
                owner,
                expected_preparation_version,
                story_id: story.map(|s| s.id.clone()),
                expected_story_version: story.map(|s| s.version),
            })
            .await
            .expect("selecionar o apoio");
    }
}

// ---------------------------------------------------------------------------
// Validação e limites
// ---------------------------------------------------------------------------

#[test]
fn story_input_trims_and_validates_partial_stories() {
    let input = validate_story_input(StoryInput {
        title: "  Failed release  ".to_owned(),
        situation: "  We shipped late.  ".to_owned(),
        task: "   ".to_owned(),
        actions: String::new(),
        outcome: String::new(),
        competencies: vec![
            "  Team  ".to_owned(),
            "Team".to_owned(),
            " Comunicação ".to_owned(),
        ],
    })
    .expect("uma história parcial é válida");

    assert_eq!(input.title, "Failed release", "o título é aparado");
    assert_eq!(input.situation, "We shipped late.", "as notas são aparadas");
    assert_eq!(
        input.competencies,
        vec!["Team".to_owned(), "Comunicação".to_owned()],
        "as competências são aparadas e deduplicadas exatamente"
    );

    assert_eq!(
        missing_fields(&input),
        vec![
            MissingField::Task,
            MissingField::Actions,
            MissingField::Outcome
        ],
        "os campos em falta são derivados"
    );

    let no_note = validate_story_input(StoryInput {
        title: "T".to_owned(),
        situation: String::new(),
        task: String::new(),
        actions: String::new(),
        outcome: String::new(),
        competencies: vec!["C".to_owned()],
    })
    .expect_err("é exigida pelo menos uma nota");
    assert_eq!(no_note.code, error_code::INVALID_INPUT);

    let no_competency = validate_story_input(StoryInput {
        title: "T".to_owned(),
        situation: "S".to_owned(),
        task: String::new(),
        actions: String::new(),
        outcome: String::new(),
        competencies: vec![],
    })
    .expect_err("é exigida pelo menos uma competência");
    assert_eq!(no_competency.code, error_code::INVALID_INPUT);
}

#[test]
fn story_input_enforces_byte_limits() {
    let base = || StoryInput {
        title: "t".to_owned(),
        situation: "s".to_owned(),
        task: String::new(),
        actions: String::new(),
        outcome: String::new(),
        competencies: vec!["c".to_owned()],
    };

    // Título: limite exato de 200 bytes e excesso.
    let mut ok = base();
    ok.title = "t".repeat(200);
    validate_story_input(ok).expect("200 bytes de título são aceites");
    let mut excess = base();
    excess.title = "t".repeat(201);
    assert_eq!(
        validate_story_input(excess)
            .expect_err("201 bytes excedem")
            .code,
        error_code::INVALID_INPUT
    );

    // Nota: limite exato de 16.000 bytes e excesso.
    let mut ok = base();
    ok.situation = "s".repeat(16_000);
    validate_story_input(ok).expect("16.000 bytes de nota são aceites");
    let mut excess = base();
    excess.situation = "s".repeat(16_001);
    assert_eq!(
        validate_story_input(excess)
            .expect_err("16.001 bytes excedem")
            .code,
        error_code::INVALID_INPUT
    );

    // Soma das notas: 48.000 é o limite.
    let mut ok = base();
    ok.situation = "s".repeat(12_000);
    ok.task = "t".repeat(12_000);
    ok.actions = "a".repeat(12_000);
    ok.outcome = "o".repeat(12_000);
    validate_story_input(ok).expect("48.000 bytes no total são aceites");
    let mut excess = base();
    excess.situation = "s".repeat(12_000);
    excess.task = "t".repeat(12_000);
    excess.actions = "a".repeat(12_000);
    excess.outcome = "o".repeat(12_001);
    assert_eq!(
        validate_story_input(excess)
            .expect_err("48.001 bytes excedem")
            .code,
        error_code::INVALID_INPUT
    );

    // Competência: 120 bytes exatos e excesso.
    let mut ok = base();
    ok.competencies = vec!["c".repeat(120)];
    validate_story_input(ok).expect("120 bytes de competência são aceites");
    let mut excess = base();
    excess.competencies = vec!["c".repeat(121)];
    assert_eq!(
        validate_story_input(excess)
            .expect_err("121 bytes excedem")
            .code,
        error_code::INVALID_INPUT
    );

    // Número de competências: 20 aceites, 21 recusadas.
    let twenty: Vec<String> = (0..20).map(|i| format!("c{i}")).collect();
    let mut ok = base();
    ok.competencies = twenty;
    validate_story_input(ok).expect("20 competências são aceites");
    let mut excess = base();
    excess.competencies = (0..21).map(|i| format!("c{i}")).collect();
    assert_eq!(
        validate_story_input(excess)
            .expect_err("21 competências excedem")
            .code,
        error_code::INVALID_INPUT
    );
}

// ---------------------------------------------------------------------------
// CRUD, filtro e append-only
// ---------------------------------------------------------------------------

#[tokio::test]
async fn story_crud_is_idempotent_filtered_and_append_only() {
    let harness = Harness::new().await;

    let first = harness
        .create_story("Release failure", "We shipped late.", &["Team", "Delivery"])
        .await;
    assert_eq!(first.version, 1);

    // Criação idempotente: o mesmo requestId devolve a mesma história.
    let request_id = new_request_id();
    let second = harness
        .service
        .create_story(CreateStory {
            request_id: request_id.clone(),
            input: story_input("Other", "Other note.", &["Other"]),
        })
        .await
        .expect("criar a segunda história");
    let replay = harness
        .service
        .create_story(CreateStory {
            request_id,
            input: story_input("Other", "Other note.", &["Other"]),
        })
        .await
        .expect("a criação é idempotente");
    assert_eq!(replay.id, second.id);

    // Filtro por competência, sem distinção de maiúsculas e com trim.
    let all = harness
        .service
        .list_stories(None)
        .await
        .expect("listar todas");
    assert_eq!(all.len(), 2);
    let filtered = harness
        .service
        .list_stories(Some("  team  ".to_owned()))
        .await
        .expect("filtrar por competência");
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].id, first.id);
    let none = harness
        .service
        .list_stories(Some("inexistente".to_owned()))
        .await
        .expect("filtrar sem resultados");
    assert!(none.is_empty());

    // Edição com versão esperada avança a versão.
    let edited = harness
        .service
        .update_story(UpdateStory {
            request_id: new_request_id(),
            id: first.id.clone(),
            expected_version: 1,
            input: story_input(
                "Release failure v2",
                "We shipped late and recovered.",
                &["Team"],
            ),
        })
        .await
        .expect("editar a história");
    assert_eq!(edited.version, 2);

    // Versão concorrente é recusada sem escrever.
    let stale = harness
        .service
        .update_story(UpdateStory {
            request_id: new_request_id(),
            id: first.id.clone(),
            expected_version: 1,
            input: story_input("Another", "Another note.", &["Other"]),
        })
        .await
        .expect_err("versão antiga é recusada");
    assert_eq!(stale.code, error_code::STALE_SOURCE);

    // Append-only: as duas versões coexistem e a primeira não é reescrita.
    harness
        .database
        .with_connection(|connection| {
            let count: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM story_versions WHERE story_id = ?1",
                    [&first.id],
                    |row| row.get(0),
                )
                .map_err(|error| AppError::new("TEST_SQLITE", error.to_string(), false))?;
            assert_eq!(count, 2, "duas versões guardadas");
            let first_content: String = connection
                .query_row(
                    "SELECT content_json FROM story_versions WHERE story_id = ?1 AND version = 1",
                    [&first.id],
                    |row| row.get(0),
                )
                .map_err(|error| AppError::new("TEST_SQLITE", error.to_string(), false))?;
            assert!(
                first_content.contains("We shipped late."),
                "a versão 1 conserva o conteúdo original"
            );
            Ok(())
        })
        .expect("verificar versões append-only");
}

// ---------------------------------------------------------------------------
// Snapshot e seleção
// ---------------------------------------------------------------------------

#[tokio::test]
async fn snapshot_fixes_chosen_version_and_edit_does_not_change_it() {
    let harness = Harness::new().await;
    let detail = harness.create("s1", "sq1").await;

    let story = harness
        .create_story("Original title", "Original situation.", &["Team"])
        .await;
    harness
        .select_story(Harness::owner_of(&detail), 0, Some(&story))
        .await;

    // Edita a história depois da seleção: a versão passa a 2.
    harness
        .service
        .update_story(UpdateStory {
            request_id: new_request_id(),
            id: story.id.clone(),
            expected_version: 1,
            input: story_input("Edited title", "Edited situation.", &["Team"]),
        })
        .await
        .expect("editar a história");

    harness
        .service
        .start_practice_recording(Harness::owner_of(&detail), 0)
        .await
        .expect("gravar com apoio");

    let loaded = harness
        .service
        .load_practice(detail.id.clone())
        .await
        .expect("recarregar a prática");
    assert_eq!(
        loaded.status,
        PracticeStatus::Answering,
        "o apoio de história move a prática para answering"
    );
    let snapshot = loaded.preparation.snapshot.expect("snapshot fixado");
    assert_eq!(snapshot.story_id, story.id);
    assert_eq!(snapshot.story_version, 1, "a versão escolhida fica fixa");
    assert_eq!(
        snapshot.content.title, "Original title",
        "o conteúdo é a cópia v1"
    );
    assert_eq!(
        loaded.preparation.lock_state,
        LockState::Locked,
        "a preparação fica bloqueada"
    );

    harness
        .service
        .stop_recording()
        .await
        .expect("terminar a gravação");
}

#[tokio::test]
async fn removing_support_before_capture_fixes_absence() {
    let harness = Harness::new().await;
    let detail = harness.create("s1", "sq1").await;

    let story = harness
        .create_story("To remove", "A note.", &["Team"])
        .await;
    harness
        .select_story(Harness::owner_of(&detail), 0, Some(&story))
        .await;
    // Remove o apoio antes da captura.
    harness
        .select_story(Harness::owner_of(&detail), 1, None)
        .await;

    harness
        .service
        .start_practice_recording(Harness::owner_of(&detail), 0)
        .await
        .expect("gravar sem apoio");

    let loaded = harness
        .service
        .load_practice(detail.id.clone())
        .await
        .expect("recarregar a prática");
    assert_eq!(loaded.preparation.snapshot, None, "a ausência fica fixada");
    assert_eq!(loaded.preparation.lock_state, LockState::Locked);
    assert_eq!(
        loaded.preparation.selected_story_id, None,
        "nenhuma história fica selecionada"
    );

    harness
        .service
        .stop_recording()
        .await
        .expect("terminar a gravação");
}

#[tokio::test]
async fn follow_up_shares_principal_preparation() {
    let harness = Harness::new().await;
    let story = harness
        .create_story("Shared", "A shared note.", &["Team"])
        .await;

    // Uma preparação no slot initial de uma sessão (o slot initial é criado
    // automaticamente pelo trigger de `session_questions`).
    harness
        .database
        .with_connection(|connection| {
            connection
                .execute(
                    "INSERT INTO preparations
                       (answer_id, version, selected_story_id, selected_story_version,
                        locked_capture_id, lock_state)
                     VALUES ('sq1', 1, ?1, 1, NULL, 'editable')",
                    [&story.id],
                )
                .map_err(|error| AppError::new("TEST_SQLITE", error.to_string(), false))?;
            // Um seguimento da mesma pergunta.
            connection
                .execute(
                    "INSERT INTO answer_slots
                       (id, session_question_id, session_id, role, text)
                     VALUES ('fu1', 'sq1', 's1', 'follow_up', 'Follow-up question?')",
                    [],
                )
                .map_err(|error| AppError::new("TEST_SQLITE", error.to_string(), false))?;
            Ok(())
        })
        .expect("semear seguimento e preparação");

    let principal = harness
        .database
        .load_preparation("sq1")
        .expect("preparação do initial");
    let follow_up = harness
        .database
        .load_preparation("fu1")
        .expect("preparação do seguimento");

    assert_eq!(
        principal.selected_story_id.as_deref(),
        Some(story.id.as_str())
    );
    assert_eq!(follow_up.selected_story_id, principal.selected_story_id);
    assert_eq!(
        follow_up.answer_id, "sq1",
        "o seguimento resolve o slot initial"
    );
}

#[tokio::test]
async fn known_microphone_failure_allows_choosing_another_support() {
    let harness = Harness::new().await;
    let detail = harness.create("s1", "sq1").await;

    let first = harness
        .create_story("First story", "First note.", &["Team"])
        .await;
    let second = harness
        .create_story("Second story", "Second note.", &["Team"])
        .await;
    harness
        .select_story(Harness::owner_of(&detail), 0, Some(&first))
        .await;

    // O microfone falha de forma conhecida: a preparação é libertada.
    harness.audio.fail_next_start();
    let error = harness
        .service
        .start_practice_recording(Harness::owner_of(&detail), 0)
        .await
        .expect_err("o microfone falha");
    assert_eq!(error.code, error_code::DEVICE_UNAVAILABLE);

    let loaded = harness
        .service
        .load_practice(detail.id.clone())
        .await
        .expect("recarregar a prática");
    assert_eq!(
        loaded.preparation.lock_state,
        LockState::Editable,
        "a preparação volta a ser editável"
    );

    // Escolher outro apoio e gravar com sucesso.
    harness
        .select_story(Harness::owner_of(&detail), 1, Some(&second))
        .await;
    harness
        .service
        .start_practice_recording(Harness::owner_of(&detail), 0)
        .await
        .expect("gravar com o segundo apoio");

    let loaded = harness
        .service
        .load_practice(detail.id.clone())
        .await
        .expect("recarregar a prática");
    let snapshot = loaded
        .preparation
        .snapshot
        .expect("snapshot do segundo apoio");
    assert_eq!(snapshot.story_id, second.id);
    harness
        .service
        .stop_recording()
        .await
        .expect("terminar a gravação");
}

// ---------------------------------------------------------------------------
// Falhas de SQLite e recuperação
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sqlite_failure_before_start_opens_no_microphone() {
    let harness = Harness::new().await;
    let detail = harness.create("s1", "sq1").await;

    harness
        .service
        .inject_preparation_fault(Some(PreparationFault::ReserveCandidate));
    let error = harness
        .service
        .start_practice_recording(Harness::owner_of(&detail), 0)
        .await
        .expect_err("a reserva do candidato falha");
    assert_eq!(error.code, error_code::STORAGE_WRITE_FAILED);
    assert_eq!(harness.audio.starts(), 0, "nenhum microfone foi aberto");
    harness.service.inject_preparation_fault(None);
}

#[tokio::test]
async fn sqlite_failure_after_start_keeps_capture_and_retry_consolidates() {
    let harness = Harness::new().await;
    let detail = harness.create("s1", "sq1").await;
    let story = harness
        .create_story("Before", "Before note.", &["Team"])
        .await;
    harness
        .select_story(Harness::owner_of(&detail), 0, Some(&story))
        .await;

    harness
        .service
        .inject_preparation_fault(Some(PreparationFault::MarkStarted));
    let started = harness
        .service
        .start_practice_recording(Harness::owner_of(&detail), 0)
        .await
        .expect("o microfone abre");
    let recording_id = started.recording_id.clone().expect("gravação em curso");
    assert_eq!(harness.audio.starts(), 1, "o microfone foi aberto");
    assert_eq!(
        started.practice.pending_preparation.as_deref(),
        Some(recording_id.as_str()),
        "a pendência de metadados fica visível"
    );

    // Edita a história depois do arranque.
    harness
        .service
        .update_story(UpdateStory {
            request_id: new_request_id(),
            id: story.id.clone(),
            expected_version: 1,
            input: story_input("After", "After note.", &["Team"]),
        })
        .await
        .expect("editar a história");

    // Terminar funciona e o áudio é preservado.
    harness
        .service
        .stop_recording()
        .await
        .expect("terminar a gravação");
    harness.service.inject_preparation_fault(None);

    // O retry local consolida o mesmo snapshot sem consultar a versão atual.
    harness
        .service
        .retry_preparation_publication(recording_id.clone())
        .await
        .expect("repetir a publicação dos metadados");
    let snapshot = harness.service.snapshot();
    assert_eq!(
        snapshot.practice.pending_preparation, None,
        "a pendência é resolvida"
    );

    let loaded = harness
        .service
        .load_practice(detail.id.clone())
        .await
        .expect("recarregar a prática");
    assert_eq!(loaded.status, PracticeStatus::Answering);
    assert_eq!(loaded.preparation.lock_state, LockState::Locked);
    let snapshot = loaded.preparation.snapshot.expect("snapshot consolidado");
    assert_eq!(snapshot.story_version, 1, "a versão original é conservada");
    assert_eq!(
        snapshot.content.title, "Before",
        "o conteúdo original é conservado"
    );
}

#[tokio::test]
async fn restart_with_marker_started_consolidates() {
    let harness = Harness::new().await;
    let detail = harness.create("s1", "sq1").await;
    let story = harness
        .create_story("Recover", "Recover note.", &["Team"])
        .await;
    harness
        .select_story(Harness::owner_of(&detail), 0, Some(&story))
        .await;

    // A marcação started falha depois do microfone: fica o candidato + marcador.
    harness
        .service
        .inject_preparation_fault(Some(PreparationFault::MarkStarted));
    harness
        .service
        .start_practice_recording(Harness::owner_of(&detail), 0)
        .await
        .expect("o microfone abre");
    harness
        .service
        .stop_recording()
        .await
        .expect("terminar a gravação");

    // O reinício consolida o candidato porque o marcador ficou `started`.
    let (service, audio) = harness.reopen().await;
    assert_eq!(audio.starts(), 0, "o reinício não abre o microfone");
    let loaded = service
        .load_practice(detail.id.clone())
        .await
        .expect("recarregar após o reinício");
    assert_eq!(loaded.status, PracticeStatus::Answering);
    assert_eq!(loaded.preparation.lock_state, LockState::Locked);
    assert!(
        loaded.preparation.snapshot.is_some(),
        "o snapshot candidato é consolidado"
    );
}

#[tokio::test]
async fn pending_ambiguous_shows_start_uncertain_and_resolves() {
    let harness = Harness::new().await;
    let detail = harness.create("s1", "sq1").await;
    let story = harness
        .create_story("Uncertain", "Uncertain note.", &["Team"])
        .await;
    harness
        .select_story(Harness::owner_of(&detail), 0, Some(&story))
        .await;

    // Simula uma queda entre a reserva do candidato e o microfone: gravação e
    // candidato existem, mas não há marcador nem original.
    let recording = harness
        .database
        .reserve_recording(&detail.answer.id, 0)
        .expect("reservar a gravação");
    harness
        .database
        .reserve_preparation_candidate(&detail.answer.id, &recording.id)
        .expect("reservar o candidato");

    let (service, _) = harness.reopen().await;
    let snapshot = service.snapshot();
    assert_eq!(
        snapshot.practice.pending_preparation.as_deref(),
        Some(recording.id.as_str()),
        "a pendência ambígua fica visível"
    );
    let loaded = service
        .load_practice(detail.id.clone())
        .await
        .expect("recarregar a prática");
    assert_eq!(loaded.preparation.lock_state, LockState::StartUncertain);

    // «Manter este apoio e continuar» fixa o candidato sem descartar ficheiros.
    service
        .resolve_preparation(
            recording.id.clone(),
            new_request_id(),
            PreparationDecision::Keep,
        )
        .await
        .expect("manter o apoio");
    let loaded = service
        .load_practice(detail.id.clone())
        .await
        .expect("recarregar a prática");
    assert_eq!(loaded.preparation.lock_state, LockState::Locked);
    assert!(
        loaded.preparation.snapshot.is_some(),
        "o candidato é fixado"
    );

    // A linha de captura é conservada (nada é descartado).
    let kept: bool = harness
        .database
        .with_connection(|connection| {
            connection
                .query_row(
                    "SELECT EXISTS (SELECT 1 FROM preparation_captures WHERE recording_id = ?1)",
                    [&recording.id],
                    |row| row.get(0),
                )
                .map_err(|error| AppError::new("TEST_SQLITE", error.to_string(), false))
        })
        .expect("verificar a auditoria");
    assert!(kept, "a captura de preparação não é eliminada");
}

#[tokio::test]
async fn story_marker_never_leaks_into_sources_or_answers() {
    const MARKER: &str = "ONLY_IN_STORY_742";

    let harness = Harness::new().await;

    // Uma história local com o marcador exclusivo.
    let story = harness
        .service
        .create_story(CreateStory {
            request_id: new_request_id(),
            input: StoryInput {
                title: format!("Story {MARKER}"),
                situation: format!("A situation mentioning {MARKER}."),
                task: String::new(),
                actions: String::new(),
                outcome: String::new(),
                competencies: vec![format!("{MARKER}-skill")],
            },
        })
        .await
        .expect("criar história com marcador");

    // O resolvedor de fontes IA nunca recebe a história.
    let sources = harness
        .database
        .ai_sources("s2", "sq2", "feedback")
        .expect("fontes da sessão IA");
    let serialized = serde_json::to_string(&sources).expect("serializar fontes");
    assert!(
        !serialized.contains(MARKER),
        "o marcador não entra nas fontes IA"
    );

    // O apoio é local: o snapshot guarda a história, mas a resposta nunca a
    // herda no texto confirmado nem na transcrição.
    let detail = harness.create("s1", "sq1").await;
    harness
        .select_story(Harness::owner_of(&detail), 0, Some(&story))
        .await;
    harness
        .service
        .start_practice_recording(Harness::owner_of(&detail), 0)
        .await
        .expect("gravar com apoio");
    harness
        .service
        .stop_recording()
        .await
        .expect("terminar a gravação");

    let loaded = harness
        .service
        .load_practice(detail.id.clone())
        .await
        .expect("recarregar a prática");
    assert!(
        loaded
            .preparation
            .snapshot
            .as_ref()
            .is_some_and(|s| s.content.title.contains(MARKER)),
        "o marcador vive apenas no apoio local"
    );
    assert!(
        !loaded.question.text.contains(MARKER),
        "a pergunta não herda o marcador"
    );
    assert!(
        !loaded.answer.text.contains(MARKER),
        "a resposta não herda o marcador"
    );

    // A revisão e a confirmação são o texto do utilizador, sem o marcador.
    let recording_id = loaded.answer.selected_recording_id.clone().unwrap();
    let selection_version = loaded.answer.selection_version;
    let target = language_assistant_tutor::practice::PracticeAnswerTarget {
        owner: Harness::owner_of(&loaded),
        recording_id,
        selection_version,
    };
    let answer = "I answered the question.";
    harness
        .service
        .save_practice_review(language_assistant_tutor::practice::SavePracticeReview {
            request_id: new_request_id(),
            target: target.clone(),
            expected_review_version: 0,
            source_transcript_id: None,
            origin: ReviewOrigin::ManualExplicit,
            text: answer.to_owned(),
        })
        .await
        .expect("guardar a revisão");
    let confirmed = harness
        .service
        .confirm_practice_answer(language_assistant_tutor::practice::ConfirmPracticeAnswer {
            request_id: new_request_id(),
            target,
            expected_review_version: 1,
            expected_practice_version: loaded.version,
            source_transcript_id: None,
            origin: ReviewOrigin::ManualExplicit,
            text: answer.to_owned(),
        })
        .await
        .expect("confirmar a resposta");
    assert!(
        !confirmed
            .confirmation
            .expect("confirmação")
            .text
            .contains(MARKER),
        "a confirmação não contém o marcador"
    );
}

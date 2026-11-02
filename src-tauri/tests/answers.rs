//! Regras de publicação, transcrições, revisões e confirmações na base.
//!
//! Complementa `artifacts.rs` com os casos estritamente da base: publicação
//! idempotente de um original validado, ligação do derivado normalizado,
//! operação criada e encerrada na mesma transação da transcrição, revisões
//! append-only, confirmação imutável com o texto exato que o utilizador viu e
//! avanço idempotente da sessão.

use std::fs;
use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavSpec, WavWriter};
use rusqlite::{params, Connection};
use serde_json::json;

use language_assistant_tutor::artifacts::{normalized_reference, ArtifactStore};
use language_assistant_tutor::audio::{CapturedAudio, RecordingTarget};
use language_assistant_tutor::db::Database;
use language_assistant_tutor::domain::{
    error_code, AppError, CaptureMeta, Segment, Transcript, TranscriptStatus,
};
use language_assistant_tutor::interview::{
    AdvanceSession, AnswerTarget, ConfirmAnswer, OperationTarget, RecordingState, ReviewOrigin,
    SaveReview, SessionDetail, SessionMode, SessionStatus, StartSession,
};

struct Harness {
    _directory: tempfile::TempDir,
    path: PathBuf,
    database: Database,
    artifacts: ArtifactStore,
}

fn harness() -> Harness {
    let directory = tempfile::tempdir().expect("criar o diretório temporário");
    let path = directory.path().join("tutor.sqlite3");
    let database = Database::open(path.clone()).expect("abrir a base de teste");
    let artifacts = ArtifactStore::new(directory.path().join("sessions"));
    Harness {
        _directory: directory,
        path,
        database,
        artifacts,
    }
}

impl Harness {
    fn questions(&self, count: u32) -> SessionDetail {
        self.database
            .start_session(StartSession {
                request_id: "req-answers".to_owned(),
                category: None,
                count,
            })
            .expect("iniciar a sessão de teste")
    }

    fn reserve(&self, session: &str, question: &str, expected: u64) -> (RecordingTarget, String) {
        let view = self
            .database
            .reserve_recording(question, expected)
            .expect("reservar a gravação");
        let target = self
            .artifacts
            .reserve(session, question, &view.id)
            .expect("reservar a pasta");
        (target, view.id)
    }

    /// Reserva e publica a gravação de uma pergunta, devolvendo o identificador.
    fn record(&self, session: &str, question: &str, expected: u64) -> String {
        let (target, recording_id) = self.reserve(session, question, expected);
        self.database
            .publish_recording(finalized(self, &target))
            .expect("publicar a gravação de teste");
        recording_id
    }

    fn reopen(&mut self) {
        self.database = Database::open(self.path.clone()).expect("reabrir a base de teste");
    }
}

fn write_wav(path: &Path) {
    let spec = WavSpec {
        channels: 1,
        sample_rate: 48_000,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).expect("criar o WAV de teste");
    for sample in 0..480i16 {
        writer.write_sample(sample).expect("escrever uma amostra");
    }
    writer.finalize().expect("fechar o WAV de teste");
}

fn finalized(harness: &Harness, target: &RecordingTarget) -> CapturedAudio {
    write_wav(&target.directory.join("original.wav"));
    harness
        .artifacts
        .inspect(target)
        .expect("inspecionar o original finalizado")
}

fn transcript(id: &str, text: &str) -> Transcript {
    Transcript {
        id: id.to_owned(),
        status: TranscriptStatus::Completed,
        text: text.to_owned(),
        segments: vec![Segment {
            start_ms: 100,
            end_ms: 1_200,
            text: text.to_owned(),
        }],
        model: "small".to_owned(),
        language: "en".to_owned(),
        created_at: "2026-11-02T10:00:00Z".to_owned(),
        load_ms: 5,
        processing_ms: 25,
    }
}

fn test_error(error: rusqlite::Error) -> AppError {
    AppError::new("TEST_SQLITE", error.to_string(), false)
}

fn no_speech(id: &str) -> Transcript {
    Transcript {
        id: id.to_owned(),
        status: TranscriptStatus::NoSpeech,
        text: String::new(),
        segments: Vec::new(),
        model: "small".to_owned(),
        language: "en".to_owned(),
        created_at: "2026-11-02T10:00:00Z".to_owned(),
        load_ms: 3,
        processing_ms: 12,
    }
}

fn answer_target(session: &str, question: &str, recording: &str, selection: u64) -> AnswerTarget {
    AnswerTarget {
        session_id: session.to_owned(),
        session_question_id: question.to_owned(),
        recording_id: recording.to_owned(),
        selection_version: selection,
    }
}

fn review_request(
    request_id: &str,
    target: AnswerTarget,
    expected_review_version: u64,
    source_transcript_id: Option<&str>,
    origin: ReviewOrigin,
    text: &str,
) -> SaveReview {
    SaveReview {
        request_id: request_id.to_owned(),
        target,
        expected_review_version,
        source_transcript_id: source_transcript_id.map(str::to_owned),
        origin,
        text: text.to_owned(),
    }
}

fn confirm_request(
    request_id: &str,
    target: AnswerTarget,
    expected_review_version: u64,
    source_transcript_id: Option<&str>,
    origin: ReviewOrigin,
    text: &str,
    expected_session_version: u64,
) -> ConfirmAnswer {
    ConfirmAnswer {
        request_id: request_id.to_owned(),
        target,
        expected_review_version,
        source_transcript_id: source_transcript_id.map(str::to_owned),
        origin,
        text: text.to_owned(),
        expected_session_version,
    }
}

fn advance_request(
    request_id: &str,
    session: &str,
    question: &str,
    expected_session_version: u64,
) -> AdvanceSession {
    AdvanceSession {
        request_id: request_id.to_owned(),
        session_id: session.to_owned(),
        session_question_id: question.to_owned(),
        expected_session_version,
    }
}

fn operation_target(
    session: &str,
    question: &str,
    recording: &str,
    selection: u64,
    operation: &str,
) -> OperationTarget {
    OperationTarget {
        session_id: session.to_owned(),
        session_question_id: question.to_owned(),
        recording_id: Some(recording.to_owned()),
        selection_version: selection,
        operation_id: operation.to_owned(),
        worker_generation: 0,
    }
}

/// Executa SQL que deve ser recusado e devolve o erro da base.
fn execute_fails(database: &Database, sql: &str) -> AppError {
    database
        .with_connection(|connection: &mut Connection| {
            connection.execute_batch(sql).map_err(test_error)
        })
        .expect_err("o SQL direto devia ser recusado")
}

fn table_count(database: &Database, table: &str) -> i64 {
    count(database, &format!("SELECT count(*) FROM {table}"))
}

fn execute(database: &Database, sql: &str) {
    database
        .with_connection(|connection: &mut Connection| {
            connection.execute_batch(sql).map_err(test_error)
        })
        .expect("executar o SQL de teste");
}

fn count(database: &Database, sql: &str) -> i64 {
    database
        .with_connection(|connection| {
            connection
                .query_row(sql, [], |row| row.get::<_, i64>(0))
                .map_err(test_error)
        })
        .expect("contar as linhas de teste")
}

fn text(database: &Database, sql: &str) -> String {
    database
        .with_connection(|connection| {
            connection
                .query_row(sql, [], |row| row.get::<_, String>(0))
                .map_err(test_error)
        })
        .expect("ler o texto de teste")
}

#[test]
fn publication_requires_a_valid_reserved_original() {
    let harness = harness();
    let detail = harness.questions(1);
    let session = detail.summary.id.clone();
    let question = detail.questions[0].question.id.clone();
    let (target, recording_id) = harness.reserve(&session, &question, 0);
    let valid = target.directory.join("original.wav");

    // Um identificador que não foi reservado nunca é publicado.
    write_wav(&valid);
    let stray = CapturedAudio {
        id: "unknown-recording".to_owned(),
        original: valid.clone(),
        meta: CaptureMeta {
            sample_rate: 48_000,
            channels: 1,
            duration_ms: 10,
            bytes: fs::metadata(&valid).expect("ler o original").len(),
            incomplete: false,
        },
    };
    let error = harness
        .database
        .publish_recording(stray)
        .expect_err("uma gravação não reservada é recusada");
    assert_eq!(error.code, error_code::STALE_SOURCE);

    // Sem ficheiro finalizado não há publicação nem sucesso inventado.
    fs::remove_file(&valid).expect("remover o original");
    let missing = CapturedAudio {
        id: recording_id.clone(),
        original: valid.clone(),
        meta: CaptureMeta {
            sample_rate: 48_000,
            channels: 1,
            duration_ms: 10,
            bytes: 0,
            incomplete: false,
        },
    };
    let error = harness
        .database
        .publish_recording(missing.clone())
        .expect_err("um original em falta é recusado");
    assert_eq!(error.code, error_code::ARTIFACT_MISSING);
    let detail = harness.database.load_session(&session).expect("carregar");
    assert_eq!(
        detail.questions[0].recordings[0].state,
        RecordingState::Reserved
    );

    // Um ficheiro ilegível também não é aceite.
    fs::write(&valid, b"not a wav").expect("escrever lixo no lugar do original");
    let error = harness
        .database
        .publish_recording(missing)
        .expect_err("um WAV ilegível é recusado");
    assert_eq!(error.code, error_code::INVALID_AUDIO);

    // Um ficheiro válido que não é o original da pasta reservada é recusado.
    let elsewhere = harness
        ._directory
        .path()
        .join("elsewhere")
        .join("original.wav");
    fs::create_dir_all(elsewhere.parent().expect("pasta exterior"))
        .expect("criar a pasta exterior");
    write_wav(&elsewhere);
    let foreign = CapturedAudio {
        id: recording_id.clone(),
        original: elsewhere,
        meta: CaptureMeta {
            sample_rate: 48_000,
            channels: 1,
            duration_ms: 10,
            bytes: 0,
            incomplete: false,
        },
    };
    let error = harness
        .database
        .publish_recording(foreign)
        .expect_err("o áudio de outra pasta é recusado");
    assert_eq!(error.code, error_code::STALE_SOURCE);
    let detail = harness.database.load_session(&session).expect("carregar");
    assert_eq!(
        detail.questions[0].recordings[0].state,
        RecordingState::Reserved
    );

    // O original real publica uma vez; repetir é idempotente.
    let capture = finalized(&harness, &target);
    let published = harness
        .database
        .publish_recording(capture.clone())
        .expect("publicar o original");
    assert_eq!(published.state, RecordingState::Ready);
    assert!(published.capture.is_some());
    let again = harness
        .database
        .publish_recording(capture)
        .expect("repetir a publicação");
    assert_eq!(again.state, RecordingState::Ready);
    assert_eq!(again.id, published.id);
    let detail = harness.database.load_session(&session).expect("carregar");
    assert_eq!(detail.questions[0].recordings.len(), 1);
    assert_eq!(
        detail.questions[0]
            .question
            .selected_recording_id
            .as_deref(),
        Some(recording_id.as_str())
    );
    assert_eq!(detail.questions[0].question.selection_version, 1);
    assert_eq!(
        count(&harness.database, "SELECT count(*) FROM recordings"),
        1
    );
}

#[test]
fn normalized_audio_is_linked_to_a_ready_recording() {
    let harness = harness();
    let detail = harness.questions(1);
    let session = detail.summary.id.clone();
    let question = detail.questions[0].question.id.clone();
    let (target, recording_id) = harness.reserve(&session, &question, 0);
    harness
        .database
        .publish_recording(finalized(&harness, &target))
        .expect("publicar a gravação");

    let meta = harness
        .artifacts
        .normalize(&target)
        .expect("normalizar o original");
    assert_eq!(meta.sample_rate, 16_000);
    assert_eq!(meta.channels, 1);
    assert!(target.directory.join("normalized.wav").is_file());

    let reference = normalized_reference(&session, &question, &recording_id)
        .expect("compor a referência do derivado");
    harness
        .database
        .set_normalized(&recording_id, &reference)
        .expect("ligar o derivado à gravação");
    let detail = harness.database.load_session(&session).expect("carregar");
    let recording = &detail.questions[0].recordings[0];
    assert_eq!(
        recording.normalized_file.as_deref(),
        Some(reference.as_str())
    );
    assert!(harness
        .artifacts
        .resolve(&reference)
        .expect("resolver o derivado")
        .is_file());

    // Repetir com a mesma referência é aceite; uma referência de outra
    // gravação ou um caminho fora da pasta gerida são recusados.
    harness
        .database
        .set_normalized(&recording_id, &reference)
        .expect("repetir a ligação");
    let foreign = normalized_reference(&session, &question, "other-recording")
        .expect("compor uma referência alheia");
    let error = harness
        .database
        .set_normalized(&recording_id, &foreign)
        .expect_err("uma referência alheia é recusada");
    assert_eq!(error.code, error_code::INVALID_INPUT);
    let error = harness
        .database
        .set_normalized(&recording_id, "../normalized.wav")
        .expect_err("um caminho fora da raiz é recusado");
    assert_eq!(error.code, error_code::INVALID_INPUT);

    // Só uma gravação pronta recebe um derivado; uma reserva ainda não.
    let reserved = harness
        .database
        .reserve_recording(&question, 1)
        .expect("reservar outra gravação");
    let reserved_reference = normalized_reference(&session, &question, &reserved.id)
        .expect("compor a referência da reserva");
    let error = harness
        .database
        .set_normalized(&reserved.id, &reserved_reference)
        .expect_err("uma reserva não tem derivado");
    assert_eq!(error.code, error_code::STALE_SOURCE);
    let error = harness
        .database
        .set_normalized("unknown", &reference)
        .expect_err("uma gravação desconhecida é recusada");
    assert_eq!(error.code, error_code::STALE_SOURCE);
}

#[test]
fn transcript_append_creates_and_closes_its_operation() {
    let harness = harness();
    let detail = harness.questions(2);
    let session = detail.summary.id.clone();
    let question = detail.questions[0].question.id.clone();
    let other_question = detail.questions[1].question.id.clone();
    let (target, recording_id) = harness.reserve(&session, &question, 0);
    harness
        .database
        .publish_recording(finalized(&harness, &target))
        .expect("publicar a gravação");

    let operation_target = OperationTarget {
        session_id: session.clone(),
        session_question_id: question.clone(),
        recording_id: Some(recording_id.clone()),
        selection_version: 1,
        operation_id: "op-1".to_owned(),
        worker_generation: 3,
    };
    let view = harness
        .database
        .append_transcript(
            operation_target.clone(),
            transcript("tr-1", "Automatic text."),
            json!({ "beamSize": 5 }),
        )
        .expect("guardar a transcrição");
    assert_eq!(view.transcripts.len(), 1);
    assert_eq!(view.transcripts[0].segments[0].end_ms, 1_200);
    assert_eq!(view.transcripts[0].model, "small");
    assert_eq!(
        text(
            &harness.database,
            "SELECT state FROM operations WHERE id = 'op-1'"
        ),
        "completed"
    );
    assert_eq!(
        text(
            &harness.database,
            "SELECT source_config_json FROM operations WHERE id = 'op-1'"
        ),
        r#"{"beamSize":5}"#
    );
    assert_eq!(
        count(
            &harness.database,
            "SELECT count(*) FROM operations WHERE id = 'op-1' AND finished_at IS NOT NULL"
        ),
        1
    );

    // Repetir o mesmo resultado devolve o estado atual sem duplicar linhas.
    harness
        .database
        .append_transcript(
            operation_target.clone(),
            transcript("tr-1", "Automatic text."),
            json!({ "beamSize": 5 }),
        )
        .expect("repetir a transcrição");
    assert_eq!(
        count(&harness.database, "SELECT count(*) FROM transcripts"),
        1
    );

    // O mesmo identificador com outro texto nunca reescreve o original.
    let error = harness
        .database
        .append_transcript(
            operation_target.clone(),
            transcript("tr-1", "Different text."),
            json!({}),
        )
        .expect_err("uma transcrição guardada é imutável");
    assert_eq!(error.code, error_code::STALE_SOURCE);
    assert_eq!(
        count(&harness.database, "SELECT count(*) FROM transcripts"),
        1
    );
    assert_eq!(
        text(
            &harness.database,
            "SELECT text FROM transcripts WHERE id = 'tr-1'"
        ),
        "Automatic text."
    );

    // Uma transcrição de outra pergunta, sem gravação ou de uma sessão errada
    // não encontra o alvo.
    let mut wrong_question = operation_target.clone();
    wrong_question.session_question_id = other_question;
    let error = harness
        .database
        .append_transcript(
            wrong_question,
            transcript("tr-2", "Other question."),
            json!({}),
        )
        .expect_err("uma gravação de outra pergunta é recusada");
    assert_eq!(error.code, error_code::STALE_SOURCE);

    let mut without_recording = operation_target.clone();
    without_recording.recording_id = None;
    let error = harness
        .database
        .append_transcript(
            without_recording,
            transcript("tr-3", "No recording."),
            json!({}),
        )
        .expect_err("uma transcrição sem gravação é recusada");
    assert_eq!(error.code, error_code::STALE_SOURCE);

    let mut wrong_session = operation_target;
    wrong_session.session_id = "unknown-session".to_owned();
    let error = harness
        .database
        .append_transcript(
            wrong_session,
            transcript("tr-4", "Wrong session."),
            json!({}),
        )
        .expect_err("uma sessão errada é recusada");
    assert_eq!(error.code, error_code::STALE_SOURCE);
    assert_eq!(
        count(&harness.database, "SELECT count(*) FROM transcripts"),
        1
    );
}

#[test]
fn confirmed_question_keeps_selection_and_refuses_new_captures() {
    let harness = harness();
    let detail = harness.questions(1);
    let session = detail.summary.id.clone();
    let question = detail.questions[0].question.id.clone();

    let (first, first_id) = harness.reserve(&session, &question, 0);
    harness
        .database
        .publish_recording(finalized(&harness, &first))
        .expect("publicar a primeira gravação");

    // Uma segunda gravação é reservada antes de a resposta ser confirmada.
    let (second, second_id) = harness.reserve(&session, &question, 1);

    execute(
        &harness.database,
        &format!(
            "INSERT INTO reviews
               (recording_id, version, source_transcript_id, origin, text, saved_at)
             VALUES ('{first_id}', 1, NULL, 'manual_explicit',
                     'Confirmed answer text.', '2026-11-02T10:00:00Z');
             INSERT INTO confirmations
               (id, session_question_id, recording_id, review_version,
                source_transcript_id, origin, text, confirmed_at)
             VALUES ('c-1', '{question}', '{first_id}', 1, NULL,
                     'manual_explicit', 'Confirmed answer text.',
                     '2026-11-02T10:01:00Z');"
        ),
    );

    // Confirmada, a pergunta não aceita capturas novas.
    let error = harness
        .database
        .reserve_recording(&question, 1)
        .expect_err("uma resposta confirmada é apenas de leitura");
    assert_eq!(error.code, error_code::ANSWER_CONFIRMED);
    assert!(!error.retryable);

    // Uma publicação já em curso conserva o ficheiro mas não muda a seleção.
    let published = harness
        .database
        .publish_recording(finalized(&harness, &second))
        .expect("publicar a captura já reservada");
    assert_eq!(published.state, RecordingState::Ready);
    let detail = harness.database.load_session(&session).expect("carregar");
    assert_eq!(
        detail.questions[0]
            .question
            .selected_recording_id
            .as_deref(),
        Some(first_id.as_str())
    );
    assert_eq!(detail.questions[0].question.selection_version, 1);
    let stored = &detail.questions[0].recordings;
    assert_eq!(stored.len(), 2);
    assert_eq!(stored[1].id, second_id);
    assert_eq!(stored[1].state, RecordingState::Ready);
    assert_eq!(
        count(&harness.database, "SELECT count(*) FROM recordings"),
        2
    );
}

// --- Revisão versionada, confirmação imutável e avanço idempotente (tarefa 5).

#[test]
fn save_does_not_confirm() {
    let harness = harness();
    let detail = harness.questions(1);
    let session = detail.summary.id.clone();
    let question = detail.questions[0].question.id.clone();
    let recording = harness.record(&session, &question, 0);
    let target = answer_target(&session, &question, &recording, 1);

    // A versão zero representa «ainda não há revisão guardada».
    let first = harness
        .database
        .save_review(review_request(
            "rev-1",
            target.clone(),
            0,
            None,
            ReviewOrigin::ManualExplicit,
            "  First  draft text. ",
        ))
        .expect("guardar a primeira revisão");
    assert_eq!(first.version, 1);
    assert_eq!(first.recording_id, recording);
    assert_eq!(
        first.text, "  First  draft text. ",
        "o texto exato é guardado, sem trim"
    );
    assert_eq!(first.origin, ReviewOrigin::ManualExplicit);
    assert!(first.source_transcript_id.is_none());
    assert!(first.saved_at.is_some(), "a revisão guardada tem data");

    // Guardar não confirma, não avança e não muda a sessão.
    let loaded = harness
        .database
        .load_session(&session)
        .expect("carregar a sessão");
    assert!(
        loaded.questions[0].confirmation.is_none(),
        "guardar uma revisão nunca confirma"
    );
    assert_eq!(loaded.summary.confirmed_count, 0);
    assert_eq!(loaded.summary.position, 1);
    assert_eq!(
        loaded.summary.version, 1,
        "guardar uma revisão não muda a versão da sessão"
    );
    assert_eq!(loaded.summary.status, SessionStatus::InProgress);
    assert_eq!(
        loaded.questions[0].recordings[0]
            .review
            .as_ref()
            .expect("revisão legível")
            .version,
        1
    );
    assert_eq!(table_count(&harness.database, "confirmations"), 0);

    // Guardar outra vez acrescenta uma versão; a anterior fica intacta.
    let second = harness
        .database
        .save_review(review_request(
            "rev-2",
            target.clone(),
            1,
            None,
            ReviewOrigin::ManualExplicit,
            "Second draft text.",
        ))
        .expect("guardar a segunda revisão");
    assert_eq!(second.version, 2);
    assert_eq!(table_count(&harness.database, "reviews"), 2);
    assert_eq!(
        text(
            &harness.database,
            &format!("SELECT text FROM reviews WHERE recording_id = '{recording}' AND version = 1")
        ),
        "  First  draft text. ",
        "a versão anterior nunca é reescrita"
    );

    // Texto vazio ou só com espaços é recusado antes de escrever.
    let error = harness
        .database
        .save_review(review_request(
            "rev-vazia",
            target.clone(),
            2,
            None,
            ReviewOrigin::ManualExplicit,
            " \t\n ",
        ))
        .expect_err("uma revisão vazia é recusada");
    assert_eq!(error.code, error_code::INVALID_INPUT);
    assert!(!error.retryable);

    // Uma versão esperada desatualizada também não escreve nada.
    let error = harness
        .database
        .save_review(review_request(
            "rev-antiga",
            target.clone(),
            1,
            None,
            ReviewOrigin::ManualExplicit,
            "Stale edit.",
        ))
        .expect_err("uma revisão desatualizada é recusada");
    assert_eq!(error.code, error_code::STALE_SOURCE);

    // A seleção da gravação faz parte do alvo.
    let error = harness
        .database
        .save_review(review_request(
            "rev-selecao",
            answer_target(&session, &question, &recording, 0),
            2,
            None,
            ReviewOrigin::ManualExplicit,
            "Other selection.",
        ))
        .expect_err("uma seleção antiga é recusada");
    assert_eq!(error.code, error_code::STALE_SOURCE);

    assert_eq!(table_count(&harness.database, "reviews"), 2);
    assert_eq!(table_count(&harness.database, "confirmations"), 0);
    assert_eq!(
        table_count(&harness.database, "command_receipts"),
        3,
        "só o início e as duas revisões guardadas deixam recibo"
    );
}

#[test]
fn confirmation_freezes_exact_seen_text() {
    let harness = harness();
    let detail = harness.questions(1);
    let session = detail.summary.id.clone();
    let question = detail.questions[0].question.id.clone();
    let recording = harness.record(&session, &question, 0);
    let target = answer_target(&session, &question, &recording, 1);

    // Texto vazio ou apenas com espaços não pode ser confirmado.
    let error = harness
        .database
        .confirm_answer(confirm_request(
            "conf-vazia",
            target.clone(),
            0,
            None,
            ReviewOrigin::ManualExplicit,
            "   ",
            1,
        ))
        .expect_err("uma confirmação vazia é recusada");
    assert_eq!(error.code, error_code::INVALID_INPUT);
    assert!(!error.retryable);
    assert_eq!(table_count(&harness.database, "reviews"), 0);
    assert_eq!(table_count(&harness.database, "confirmations"), 0);

    let seen = "  I led the migration  of the billing service.  ";
    let confirmation = harness
        .database
        .confirm_answer(confirm_request(
            "conf-exata",
            target.clone(),
            0,
            None,
            ReviewOrigin::ManualExplicit,
            seen,
            1,
        ))
        .expect("confirmar a resposta");
    assert_eq!(
        confirmation.text, seen,
        "a confirmação fica com o texto exato que o utilizador viu"
    );
    assert_eq!(confirmation.review_version, 1);
    assert_eq!(confirmation.session_question_id, question);
    assert_eq!(confirmation.recording_id, recording);
    assert_eq!(confirmation.origin, ReviewOrigin::ManualExplicit);
    assert!(confirmation.source_transcript_id.is_none());
    assert!(!confirmation.confirmed_at.is_empty());

    let loaded = harness
        .database
        .load_session(&session)
        .expect("carregar a sessão");
    let stored = loaded.questions[0]
        .confirmation
        .as_ref()
        .expect("confirmação guardada");
    assert_eq!(stored, &confirmation);
    assert_eq!(loaded.summary.confirmed_count, 1);
    assert_eq!(loaded.summary.position, 1, "confirmar não avança a sessão");
    assert_eq!(loaded.summary.status, SessionStatus::InProgress);
    assert_eq!(
        loaded.summary.version, 2,
        "confirmar atualiza a versão da sessão"
    );
    assert_eq!(
        text(
            &harness.database,
            &format!("SELECT text FROM reviews WHERE recording_id = '{recording}' AND version = 1")
        ),
        seen
    );

    // Nem a revisão nem a confirmação aceitam reescrita ou eliminação direta.
    let error = execute_fails(
        &harness.database,
        "UPDATE confirmations SET text = 'changed'",
    );
    assert!(
        error.message.contains("imutável"),
        "esperava a mensagem de imutabilidade: {error:?}"
    );
    let error = execute_fails(&harness.database, "DELETE FROM confirmations");
    assert!(
        error.message.contains("imutável"),
        "esperava a mensagem de imutabilidade: {error:?}"
    );
    let error = execute_fails(
        &harness.database,
        "UPDATE reviews SET text = 'changed' WHERE version = 1",
    );
    assert!(
        error.message.contains("imutável"),
        "esperava a mensagem de imutabilidade: {error:?}"
    );
    assert_eq!(
        text(&harness.database, "SELECT text FROM confirmations"),
        seen
    );
    assert_eq!(
        text(
            &harness.database,
            &format!("SELECT text FROM reviews WHERE recording_id = '{recording}' AND version = 1")
        ),
        seen
    );
}

#[test]
fn stale_review_cannot_confirm() {
    let harness = harness();
    let detail = harness.questions(1);
    let session = detail.summary.id.clone();
    let question = detail.questions[0].question.id.clone();
    let first_recording = harness.record(&session, &question, 0);
    let second_recording = harness.record(&session, &question, 1);
    let target = answer_target(&session, &question, &second_recording, 2);

    harness
        .database
        .save_review(review_request(
            "rev-1",
            target.clone(),
            0,
            None,
            ReviewOrigin::ManualExplicit,
            "First draft.",
        ))
        .expect("guardar a primeira revisão");
    harness
        .database
        .save_review(review_request(
            "rev-2",
            target.clone(),
            1,
            None,
            ReviewOrigin::ManualExplicit,
            "Final text.",
        ))
        .expect("guardar a segunda revisão");

    // Confirmar a versão que o utilizador já não vê é recusado.
    let error = harness
        .database
        .confirm_answer(confirm_request(
            "conf-antiga",
            target.clone(),
            1,
            None,
            ReviewOrigin::ManualExplicit,
            "First draft.",
            1,
        ))
        .expect_err("uma revisão desatualizada não confirma");
    assert_eq!(error.code, error_code::STALE_SOURCE);
    assert!(!error.retryable);

    // A confirmação usa apenas a gravação selecionada.
    let error = harness
        .database
        .confirm_answer(confirm_request(
            "conf-outra",
            answer_target(&session, &question, &first_recording, 2),
            2,
            None,
            ReviewOrigin::ManualExplicit,
            "Final text.",
            1,
        ))
        .expect_err("uma gravação não selecionada não confirma");
    assert_eq!(error.code, error_code::STALE_SOURCE);

    // A versão esperada da seleção também é validada.
    let error = harness
        .database
        .confirm_answer(confirm_request(
            "conf-selecao",
            answer_target(&session, &question, &second_recording, 1),
            2,
            None,
            ReviewOrigin::ManualExplicit,
            "Final text.",
            1,
        ))
        .expect_err("uma seleção desatualizada não confirma");
    assert_eq!(error.code, error_code::STALE_SOURCE);

    // A versão esperada da sessão também é validada.
    let error = harness
        .database
        .confirm_answer(confirm_request(
            "conf-sessao",
            target.clone(),
            2,
            None,
            ReviewOrigin::ManualExplicit,
            "Final text.",
            2,
        ))
        .expect_err("uma versão de sessão desatualizada não confirma");
    assert_eq!(error.code, error_code::STALE_SOURCE);

    assert_eq!(table_count(&harness.database, "reviews"), 2);
    assert_eq!(table_count(&harness.database, "confirmations"), 0);
    assert_eq!(
        harness
            .database
            .load_session(&session)
            .expect("carregar a sessão")
            .summary
            .version,
        1,
        "as recusas não mudam a versão da sessão"
    );

    // Com os valores atuais, confirma e reutiliza a revisão que já existe.
    let confirmation = harness
        .database
        .confirm_answer(confirm_request(
            "conf-correta",
            target.clone(),
            2,
            None,
            ReviewOrigin::ManualExplicit,
            "Final text.",
            1,
        ))
        .expect("confirmar a resposta atual");
    assert_eq!(
        confirmation.review_version, 2,
        "o texto não mudou e a versão guardada é reutilizada"
    );
    assert_eq!(
        table_count(&harness.database, "reviews"),
        2,
        "confirmar sem edições não cria outra revisão"
    );
    assert_eq!(confirmation.text, "Final text.");
}

#[test]
fn manual_no_speech_keeps_origin() {
    let harness = harness();
    let detail = harness.questions(2);
    let session = detail.summary.id.clone();
    let question = detail.questions[0].question.id.clone();
    let other = detail.questions[1].question.id.clone();
    let recording = harness.record(&session, &question, 0);
    let target = answer_target(&session, &question, &recording, 1);

    let view = harness
        .database
        .append_transcript(
            operation_target(&session, &question, &recording, 1, "op-sem-voz"),
            no_speech("tr-sem-voz"),
            json!({ "beamSize": 5 }),
        )
        .expect("guardar o resultado sem fala");
    assert_eq!(view.transcripts.len(), 1);
    assert_eq!(view.transcripts[0].status, TranscriptStatus::NoSpeech);
    assert_eq!(view.transcripts[0].text, "");

    let typed = "I kept the release stable by writing the rollback plan myself.";
    let draft = harness
        .database
        .save_review(review_request(
            "rev-manual",
            target.clone(),
            0,
            Some("tr-sem-voz"),
            ReviewOrigin::ManualExplicit,
            typed,
        ))
        .expect("guardar a revisão introduzida manualmente");
    assert_eq!(draft.origin, ReviewOrigin::ManualExplicit);
    assert_eq!(draft.source_transcript_id.as_deref(), Some("tr-sem-voz"));
    assert_eq!(draft.text, typed);

    let confirmation = harness
        .database
        .confirm_answer(confirm_request(
            "conf-manual",
            target.clone(),
            1,
            Some("tr-sem-voz"),
            ReviewOrigin::ManualExplicit,
            typed,
            1,
        ))
        .expect("confirmar a revisão manual");
    assert_eq!(confirmation.origin, ReviewOrigin::ManualExplicit);
    assert_eq!(
        confirmation.source_transcript_id.as_deref(),
        Some("tr-sem-voz"),
        "a origem manual fica registada sem inventar transcrição"
    );
    assert_eq!(confirmation.text, typed);

    // O resultado automático continua sem fala e sem texto fabricado.
    assert_eq!(table_count(&harness.database, "transcripts"), 1);
    assert_eq!(
        text(
            &harness.database,
            "SELECT status FROM transcripts WHERE id = 'tr-sem-voz'"
        ),
        "no_speech"
    );
    assert_eq!(
        text(
            &harness.database,
            "SELECT text FROM transcripts WHERE id = 'tr-sem-voz'"
        ),
        ""
    );

    // Uma revisão manual também é possível sem qualquer resultado automático.
    let other_recording = harness.record(&session, &other, 0);
    let confirmation = harness
        .database
        .confirm_answer(confirm_request(
            "conf-sem-transcricao",
            answer_target(&session, &other, &other_recording, 1),
            0,
            None,
            ReviewOrigin::ManualExplicit,
            "Typed from the recording.",
            2,
        ))
        .expect("confirmar sem transcrição automática");
    assert_eq!(confirmation.origin, ReviewOrigin::ManualExplicit);
    assert!(confirmation.source_transcript_id.is_none());
    assert_eq!(
        count(
            &harness.database,
            &format!("SELECT count(*) FROM transcripts WHERE recording_id = '{other_recording}'")
        ),
        0,
        "a confirmação manual não inventa transcrição"
    );
}

#[test]
fn cannot_confirm_without_original() {
    let harness = harness();
    let detail = harness.questions(2);
    let session = detail.summary.id.clone();
    let question = detail.questions[0].question.id.clone();
    let other = detail.questions[1].question.id.clone();

    // Uma reserva ainda sem original finalizado não pode ser confirmada.
    let (reserved, reserved_id) = harness.reserve(&session, &question, 0);
    let error = harness
        .database
        .confirm_answer(confirm_request(
            "conf-sem-original",
            answer_target(&session, &question, &reserved_id, 0),
            0,
            None,
            ReviewOrigin::ManualExplicit,
            "Text before recording.",
            1,
        ))
        .expect_err("sem original finalizado não há confirmação");
    assert_eq!(error.code, error_code::ANSWER_REQUIRED);
    assert!(!error.retryable);
    assert!(
        error.message.contains("gravação"),
        "a mensagem deve explicar o que falta: {error:?}"
    );
    assert_eq!(table_count(&harness.database, "reviews"), 0);
    assert_eq!(table_count(&harness.database, "confirmations"), 0);
    assert_eq!(
        harness
            .database
            .load_session(&session)
            .expect("carregar a sessão")
            .summary
            .version,
        1
    );

    // Publicado o original, a mesma confirmação é aceite.
    let published = harness
        .database
        .publish_recording(finalized(&harness, &reserved))
        .expect("publicar o original");
    assert_eq!(published.id, reserved_id);
    let confirmation = harness
        .database
        .confirm_answer(confirm_request(
            "conf-sem-original",
            answer_target(&session, &question, &reserved_id, 1),
            0,
            None,
            ReviewOrigin::ManualExplicit,
            "Text before recording.",
            1,
        ))
        .expect("confirmar depois de publicar");
    assert_eq!(confirmation.text, "Text before recording.");

    // Um original que desapareceu depois de publicado também não confirma.
    let other_recording = harness.record(&session, &other, 0);
    harness
        .database
        .with_connection(|connection| {
            connection
                .execute(
                    "UPDATE recordings
                        SET state = 'interrupted', original_file = NULL
                      WHERE id = ?1",
                    params![other_recording],
                )
                .map_err(test_error)
        })
        .expect("simular um original perdido");
    let error = harness
        .database
        .confirm_answer(confirm_request(
            "conf-original-perdido",
            answer_target(&session, &other, &other_recording, 1),
            0,
            None,
            ReviewOrigin::ManualExplicit,
            "Lost original.",
            2,
        ))
        .expect_err("um original em falta não confirma");
    assert_eq!(error.code, error_code::ANSWER_REQUIRED);
    assert_eq!(table_count(&harness.database, "confirmations"), 1);
}

#[test]
fn advance_requires_confirmation() {
    let harness = harness();
    let detail = harness.questions(2);
    let session = detail.summary.id.clone();
    let first = detail.questions[0].question.id.clone();
    let second = detail.questions[1].question.id.clone();

    // Sem resposta confirmada não há avanço.
    let error = harness
        .database
        .advance_session(advance_request("adv-sem-confirmacao", &session, &first, 1))
        .expect_err("uma pergunta sem resposta confirmada não avança");
    assert_eq!(error.code, error_code::ANSWER_REQUIRED);
    assert!(!error.retryable);
    let loaded = harness
        .database
        .load_session(&session)
        .expect("carregar a sessão");
    assert_eq!(loaded.summary.position, 1);
    assert_eq!(loaded.summary.version, 1);
    assert_eq!(table_count(&harness.database, "command_receipts"), 1);

    let recording = harness.record(&session, &first, 0);
    let confirmation = harness
        .database
        .confirm_answer(confirm_request(
            "conf-primeira",
            answer_target(&session, &first, &recording, 1),
            0,
            None,
            ReviewOrigin::ManualExplicit,
            "First answer.",
            1,
        ))
        .expect("confirmar a primeira resposta");
    assert_eq!(confirmation.review_version, 1);

    // A versão esperada da sessão protege o avanço.
    let error = harness
        .database
        .advance_session(advance_request("adv-versao", &session, &first, 1))
        .expect_err("uma versão de sessão desatualizada não avança");
    assert_eq!(error.code, error_code::STALE_SOURCE);

    // Só a pergunta da posição atual pode avançar.
    let error = harness
        .database
        .advance_session(advance_request("adv-fora", &session, &second, 2))
        .expect_err("outra pergunta não avança a sessão");
    assert_eq!(error.code, error_code::STALE_SOURCE);

    // Com a confirmação e as versões certas, avança uma posição.
    let advanced = harness
        .database
        .advance_session(advance_request("adv-primeira", &session, &first, 2))
        .expect("avançar para a segunda pergunta");
    assert_eq!(advanced.summary.position, 2);
    assert_eq!(advanced.summary.version, 3);
    assert_eq!(advanced.summary.status, SessionStatus::InProgress);
    assert_eq!(advanced.summary.confirmed_count, 1);
    assert!(advanced.summary.completed_at.is_none());
    assert_eq!(
        advanced.questions[0]
            .confirmation
            .as_ref()
            .expect("a confirmação continua guardada")
            .text,
        "First answer."
    );
    assert_eq!(
        harness
            .database
            .load_session(&session)
            .expect("recarregar a sessão"),
        advanced,
        "a posição avançada fica persistida"
    );
}

#[test]
fn replayed_advance_does_not_skip() {
    let mut harness = harness();
    let detail = harness.questions(2);
    let session = detail.summary.id.clone();
    let first = detail.questions[0].question.id.clone();
    let second = detail.questions[1].question.id.clone();
    let recording = harness.record(&session, &first, 0);
    harness
        .database
        .confirm_answer(confirm_request(
            "conf-primeira",
            answer_target(&session, &first, &recording, 1),
            0,
            None,
            ReviewOrigin::ManualExplicit,
            "First answer.",
            1,
        ))
        .expect("confirmar a primeira resposta");

    let request = advance_request("adv-repetido", &session, &first, 2);
    let first_advance = harness
        .database
        .advance_session(request.clone())
        .expect("avançar para a segunda pergunta");
    assert_eq!(first_advance.summary.position, 2);
    assert_eq!(first_advance.summary.version, 3);

    // Repetir o mesmo pedido devolve o mesmo estado, sem avançar outra vez.
    let again = harness
        .database
        .advance_session(request.clone())
        .expect("repetir o avanço devolve o estado guardado");
    assert_eq!(again, first_advance);
    assert_eq!(
        count(
            &harness.database,
            "SELECT count(*) FROM command_receipts WHERE kind = 'advance_session'"
        ),
        1
    );

    // Depois de reabrir a base, o mesmo pedido não avança nem conclui a sessão.
    harness.reopen();
    let replayed = harness
        .database
        .advance_session(request.clone())
        .expect("repetir o avanço depois de reabrir");
    assert_eq!(replayed, first_advance);
    assert_eq!(
        replayed.summary.status,
        SessionStatus::InProgress,
        "a repetição não pode saltar a segunda pergunta"
    );
    assert_eq!(replayed.summary.position, 2);

    // O mesmo requestId com outro conteúdo é uma origem desatualizada.
    let error = harness
        .database
        .advance_session(advance_request("adv-repetido", &session, &second, 3))
        .expect_err("o mesmo pedido com outro conteúdo é recusado");
    assert_eq!(error.code, error_code::STALE_SOURCE);
    let loaded = harness
        .database
        .load_session(&session)
        .expect("carregar a sessão");
    assert_eq!(loaded.summary.position, 2);
    assert_eq!(loaded.summary.version, 3);
}

#[test]
fn last_advance_completes_only_full_session() {
    let harness = harness();
    let detail = harness.questions(2);
    let session = detail.summary.id.clone();
    let first = detail.questions[0].question.id.clone();
    let second = detail.questions[1].question.id.clone();
    let first_recording = harness.record(&session, &first, 0);
    harness
        .database
        .confirm_answer(confirm_request(
            "conf-primeira",
            answer_target(&session, &first, &first_recording, 1),
            0,
            None,
            ReviewOrigin::ManualExplicit,
            "First answer.",
            1,
        ))
        .expect("confirmar a primeira resposta");
    let advanced = harness
        .database
        .advance_session(advance_request("adv-primeira", &session, &first, 2))
        .expect("avançar para a segunda pergunta");
    assert_eq!(advanced.summary.position, 2);

    // Com uma pergunta por confirmar, terminar é recusado sem escrever nada.
    let error = harness
        .database
        .advance_session(advance_request("adv-terminar", &session, &second, 3))
        .expect_err("uma sessão incompleta não termina");
    assert_eq!(error.code, error_code::ANSWER_REQUIRED);
    assert!(
        error.message.contains('1'),
        "a mensagem deve indicar quantas respostas faltam: {error:?}"
    );
    let loaded = harness
        .database
        .load_session(&session)
        .expect("carregar a sessão");
    assert_eq!(loaded.summary.position, 2);
    assert_eq!(loaded.summary.version, 3);
    assert_eq!(loaded.summary.status, SessionStatus::InProgress);
    assert!(loaded.summary.completed_at.is_none());
    assert_eq!(
        count(
            &harness.database,
            "SELECT count(*) FROM command_receipts WHERE request_id = 'adv-terminar'"
        ),
        0
    );

    // Confirmada a última pergunta, o avanço conclui a sessão.
    let second_recording = harness.record(&session, &second, 0);
    harness
        .database
        .confirm_answer(confirm_request(
            "conf-segunda",
            answer_target(&session, &second, &second_recording, 1),
            0,
            None,
            ReviewOrigin::ManualExplicit,
            "Second answer.",
            3,
        ))
        .expect("confirmar a segunda resposta");
    let completed = harness
        .database
        .advance_session(advance_request("adv-terminar", &session, &second, 4))
        .expect("terminar a sessão completa");
    assert_eq!(completed.summary.status, SessionStatus::Completed);
    assert_eq!(completed.summary.mode, SessionMode::LocalV2);
    assert_eq!(completed.summary.confirmed_count, 2);
    assert_eq!(completed.summary.position, 2);
    assert_eq!(completed.summary.version, 5);
    assert!(completed.summary.completed_at.is_some());
    assert!(
        completed
            .questions
            .iter()
            .all(|question| question.confirmation.is_some()),
        "o final tem as duas confirmações"
    );
    assert!(
        harness
            .database
            .active_session()
            .expect("ler a sessão em curso")
            .is_none(),
        "uma sessão concluída já não está em curso"
    );
    assert_eq!(
        harness
            .database
            .list_sessions()
            .expect("listar o histórico"),
        vec![completed.summary.clone()]
    );

    // Uma sessão concluída não avança outra vez.
    let error = harness
        .database
        .advance_session(advance_request("adv-depois", &session, &second, 5))
        .expect_err("uma sessão concluída não avança");
    assert_eq!(error.code, error_code::STALE_SOURCE);
}

#[test]
fn confirmation_failure_rolls_back_review_and_receipt() {
    let harness = harness();
    let detail = harness.questions(1);
    let session = detail.summary.id.clone();
    let question = detail.questions[0].question.id.clone();
    let recording = harness.record(&session, &question, 0);
    let target = answer_target(&session, &question, &recording, 1);

    let saved = harness
        .database
        .save_review(review_request(
            "rev-inicial",
            target.clone(),
            0,
            None,
            ReviewOrigin::ManualExplicit,
            "Draft answer.",
        ))
        .expect("guardar a revisão inicial");
    assert_eq!(saved.version, 1);

    // Falha injetada na confirmação, depois de a revisão nova ser inserida.
    harness
        .database
        .with_connection(|connection| {
            connection
                .execute_batch(
                    "CREATE TRIGGER test_fail_confirmation
                     BEFORE INSERT ON confirmations
                     BEGIN
                         SELECT RAISE(ABORT, 'falha injetada na confirmação');
                     END;",
                )
                .map_err(test_error)
        })
        .expect("instalar a falha injetada");

    let error = harness
        .database
        .confirm_answer(confirm_request(
            "conf-falhada",
            target.clone(),
            1,
            None,
            ReviewOrigin::ManualExplicit,
            "Final answer.",
            1,
        ))
        .expect_err("a falha injetada recusa a confirmação");
    assert_eq!(error.code, error_code::STORAGE_WRITE_FAILED);
    assert!(error.retryable);

    assert_eq!(
        table_count(&harness.database, "reviews"),
        1,
        "a revisão nova é revertida com a confirmação"
    );
    assert_eq!(table_count(&harness.database, "confirmations"), 0);
    assert_eq!(
        count(
            &harness.database,
            "SELECT count(*) FROM command_receipts WHERE request_id = 'conf-falhada'"
        ),
        0,
        "uma confirmação falhada não deixa recibo"
    );
    let loaded = harness
        .database
        .load_session(&session)
        .expect("carregar a sessão");
    assert_eq!(loaded.summary.version, 1);
    assert!(loaded.questions[0].confirmation.is_none());

    // Removida a falha, o mesmo pedido guarda a revisão exata e confirma.
    harness
        .database
        .with_connection(|connection| {
            connection
                .execute("DROP TRIGGER test_fail_confirmation", [])
                .map_err(test_error)?;
            Ok(())
        })
        .expect("remover a falha injetada");
    let confirmation = harness
        .database
        .confirm_answer(confirm_request(
            "conf-falhada",
            target,
            1,
            None,
            ReviewOrigin::ManualExplicit,
            "Final answer.",
            1,
        ))
        .expect("a mesma confirmação funciona sem a falha");
    assert_eq!(confirmation.text, "Final answer.");
    assert_eq!(confirmation.review_version, 2);
    assert_eq!(table_count(&harness.database, "reviews"), 2);
}

#[test]
fn same_request_with_other_text_is_rejected() {
    let harness = harness();
    let detail = harness.questions(1);
    let session = detail.summary.id.clone();
    let question = detail.questions[0].question.id.clone();
    let recording = harness.record(&session, &question, 0);
    let target = answer_target(&session, &question, &recording, 1);

    let first = harness
        .database
        .save_review(review_request(
            "rev-mesmo-pedido",
            target.clone(),
            0,
            None,
            ReviewOrigin::ManualExplicit,
            "First text.",
        ))
        .expect("guardar a revisão");
    assert_eq!(first.version, 1);
    let replay = harness
        .database
        .save_review(review_request(
            "rev-mesmo-pedido",
            target.clone(),
            0,
            None,
            ReviewOrigin::ManualExplicit,
            "First text.",
        ))
        .expect("repetir a revisão devolve a versão guardada");
    assert_eq!(replay, first);
    assert_eq!(table_count(&harness.database, "reviews"), 1);

    let error = harness
        .database
        .save_review(review_request(
            "rev-mesmo-pedido",
            target.clone(),
            0,
            None,
            ReviewOrigin::ManualExplicit,
            "Other text.",
        ))
        .expect_err("o mesmo pedido com outro texto é recusado");
    assert_eq!(error.code, error_code::STALE_SOURCE);
    assert_eq!(table_count(&harness.database, "reviews"), 1);

    // A confirmação repetida devolve a mesma linha; outro texto é recusado.
    let request = confirm_request(
        "conf-mesmo-pedido",
        target.clone(),
        1,
        None,
        ReviewOrigin::ManualExplicit,
        "Confirmed text.",
        1,
    );
    let confirmation = harness
        .database
        .confirm_answer(request.clone())
        .expect("confirmar a resposta");
    assert_eq!(
        confirmation.review_version, 2,
        "o texto mudou e acrescentou uma versão"
    );
    let replay = harness
        .database
        .confirm_answer(request)
        .expect("repetir a confirmação devolve a linha guardada");
    assert_eq!(replay, confirmation);
    assert_eq!(table_count(&harness.database, "confirmations"), 1);
    assert_eq!(table_count(&harness.database, "reviews"), 2);

    let error = harness
        .database
        .confirm_answer(confirm_request(
            "conf-mesmo-pedido",
            target,
            1,
            None,
            ReviewOrigin::ManualExplicit,
            "Tampered text.",
            1,
        ))
        .expect_err("o mesmo pedido com outro texto é recusado");
    assert_eq!(error.code, error_code::STALE_SOURCE);
    assert_eq!(
        text(&harness.database, "SELECT text FROM confirmations"),
        "Confirmed text."
    );
    assert_eq!(table_count(&harness.database, "reviews"), 2);
}

#[test]
fn late_transcript_cannot_change_confirmation() {
    let harness = harness();
    let detail = harness.questions(1);
    let session = detail.summary.id.clone();
    let question = detail.questions[0].question.id.clone();
    let recording = harness.record(&session, &question, 0);
    let target = answer_target(&session, &question, &recording, 1);

    let confirmation = harness
        .database
        .confirm_answer(confirm_request(
            "conf-antecipada",
            target.clone(),
            0,
            None,
            ReviewOrigin::ManualExplicit,
            "Confirmed before transcription.",
            1,
        ))
        .expect("confirmar a resposta");

    // Um resultado automático tardio não muda a confirmação nem a seleção.
    let error = harness
        .database
        .append_transcript(
            operation_target(&session, &question, &recording, 1, "op-tardia"),
            transcript("tr-tardia", "Late automatic text."),
            json!({}),
        )
        .expect_err("uma resposta confirmada não recebe transcrições");
    assert_eq!(error.code, error_code::ANSWER_CONFIRMED);
    assert_eq!(table_count(&harness.database, "transcripts"), 0);
    assert_eq!(
        count(
            &harness.database,
            "SELECT count(*) FROM operations WHERE id = 'op-tardia'"
        ),
        0,
        "a operação tardia não é registada"
    );

    // Guardar outra revisão também não reabre a confirmação.
    let error = harness
        .database
        .save_review(review_request(
            "rev-tardia",
            target,
            1,
            None,
            ReviewOrigin::ManualExplicit,
            "Changed later.",
        ))
        .expect_err("uma resposta confirmada não aceita revisões novas");
    assert_eq!(error.code, error_code::ANSWER_CONFIRMED);

    let loaded = harness
        .database
        .load_session(&session)
        .expect("carregar a sessão");
    assert_eq!(
        loaded.questions[0]
            .confirmation
            .as_ref()
            .expect("a confirmação continua guardada"),
        &confirmation
    );
    assert_eq!(table_count(&harness.database, "reviews"), 1);
    assert_eq!(loaded.summary.version, 2);
    assert_eq!(
        loaded.questions[0]
            .question
            .selected_recording_id
            .as_deref(),
        Some(recording.as_str())
    );
}

#[test]
fn advance_after_reopen_keeps_the_confirmation_pending_to_advance() {
    let mut harness = harness();
    let detail = harness.questions(2);
    let session = detail.summary.id.clone();
    let first = detail.questions[0].question.id.clone();
    let recording = harness.record(&session, &first, 0);
    let confirmation = harness
        .database
        .confirm_answer(confirm_request(
            "conf-reabrir",
            answer_target(&session, &first, &recording, 1),
            0,
            None,
            ReviewOrigin::ManualExplicit,
            "Answer kept through restart.",
            1,
        ))
        .expect("confirmar a resposta");

    // Reinício após confirmação: a confirmação recupera sem duplicar e o
    // avanço continua pendente.
    harness.reopen();
    let loaded = harness.database.load_session(&session).expect("recarregar");
    assert_eq!(
        loaded.questions[0].confirmation.as_ref(),
        Some(&confirmation)
    );
    assert_eq!(loaded.summary.position, 1);
    assert_eq!(loaded.summary.confirmed_count, 1);
    assert_eq!(table_count(&harness.database, "confirmations"), 1);

    let advanced = harness
        .database
        .advance_session(advance_request("adv-depois-reabrir", &session, &first, 2))
        .expect("avançar depois de reabrir");
    assert_eq!(advanced.summary.position, 2);
    assert_eq!(advanced.summary.confirmed_count, 1);
}

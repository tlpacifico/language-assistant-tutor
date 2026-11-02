//! Regras de publicação e de transcrições na base (tarefa 4).
//!
//! Complementa `artifacts.rs` com os casos estritamente da base: publicação
//! idempotente de um original validado, ligação do derivado normalizado,
//! operação criada e encerrada na mesma transação da transcrição, e uma
//! pergunta confirmada que conserva a seleção e recusa capturas novas.

use std::fs;
use std::path::Path;

use hound::{SampleFormat, WavSpec, WavWriter};
use rusqlite::Connection;
use serde_json::json;

use language_assistant_tutor::artifacts::{normalized_reference, ArtifactStore};
use language_assistant_tutor::audio::{CapturedAudio, RecordingTarget};
use language_assistant_tutor::db::Database;
use language_assistant_tutor::domain::{
    error_code, AppError, CaptureMeta, Segment, Transcript, TranscriptStatus,
};
use language_assistant_tutor::interview::{OperationTarget, RecordingState, StartSession};

struct Harness {
    _directory: tempfile::TempDir,
    database: Database,
    artifacts: ArtifactStore,
}

fn harness() -> Harness {
    let directory = tempfile::tempdir().expect("criar o diretório temporário");
    let path = directory.path().join("tutor.sqlite3");
    let database = Database::open(path).expect("abrir a base de teste");
    let artifacts = ArtifactStore::new(directory.path().join("sessions"));
    Harness {
        _directory: directory,
        database,
        artifacts,
    }
}

impl Harness {
    fn questions(&self, count: u32) -> language_assistant_tutor::interview::SessionDetail {
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

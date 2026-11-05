//! Migração v3 e propriedade das respostas (tarefa 1 da fase 4).
//!
//! Os testes usam diretórios temporários e SQL direto. A fixture v2 é construída
//! aplicando as migrações v1 e v2 a uma ligação crua e semeando uma sessão
//! local e uma sessão de IA com seguimento, revisões/confirmações, relatório,
//! tentativas falhadas e recibos. Depois `Database::open` migra para v3 e
//! verificam-se a preservação dos identificadores, a `foreign_key_check` vazia,
//! as constraints de propriedade e a exclusão entre prática e entrevista.

use std::path::{Path, PathBuf};

use language_assistant_tutor::db::{
    CommandIdentity, CommandIdentityLocation, Database, DATABASE_VERSION, MIGRATION_V1,
    MIGRATION_V2, MIGRATION_V4, MIGRATION_V5,
};
use language_assistant_tutor::practice::CaptureOwner;
use rusqlite::Connection;

const H64: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn temp_path() -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().expect("criar o diretório temporário");
    let path = directory.path().join("tutor.sqlite3");
    (directory, path)
}

fn raw(path: &Path) -> Connection {
    Connection::open(path).expect("abrir a base crua")
}

fn user_version(connection: &Connection) -> i64 {
    connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("ler user_version")
}

fn table_exists(connection: &Connection, name: &str) -> bool {
    connection
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [name],
            |row| row.get::<_, i64>(0),
        )
        .expect("consultar sqlite_master")
        > 0
}

/// Aplica v1 e v2 a uma ligação crua e devolve uma base de versão 2 vazia.
fn apply_v2(path: &Path) {
    let connection = raw(path);
    connection
        .execute_batch("PRAGMA foreign_keys=OFF;")
        .expect("desativar FKs");
    connection.execute_batch(MIGRATION_V1).expect("aplicar v1");
    connection.execute_batch(MIGRATION_V2).expect("aplicar v2");
    connection
        .execute_batch("PRAGMA user_version=2;")
        .expect("fixar versão 2");
    connection.close().expect("fechar a ligação crua");
}

/// Fixture v2 com sessão local, sessão de IA, seguimento, relatório e recibos.
fn v2_fixture() -> String {
    format!(
        r#"
INSERT INTO questions (id, text, category, competencies_json, criteria_json, version, created_at, updated_at)
VALUES ('q1', 'Tell me about a time you failed.', 'failure_learning',
        '["Aprendizagem"]', '["Explicar o que aprendeu."]', 1,
        '2026-11-02T09:00:00Z', '2026-11-02T09:00:00Z');

INSERT INTO sessions (id, category, question_count, position, status, mode, version, created_at, completed_at)
VALUES
 ('s1', NULL, 1, 1, 'completed', 'local_v2', 1, '2026-11-02T09:00:00Z', '2026-11-02T09:30:00Z'),
 ('s2', NULL, 1, 1, 'completed', 'ai_v3', 3, '2026-11-02T10:00:00Z', '2026-11-02T10:30:00Z');

INSERT INTO session_questions (id, session_id, source_question_id, source_version, position, text, category, competencies_json, criteria_json, selected_recording_id, selection_version)
VALUES
 ('sq1', 's1', 'q1', 1, 1, 'Tell me about a time you failed.', 'failure_learning', '["Aprendizagem"]', '["Explicar o que aprendeu."]', NULL, 0),
 ('sq2', 's2', 'q1', 1, 1, 'Tell me about a time you failed.', 'failure_learning', '["Aprendizagem"]', '["Explicar o que aprendeu."]', NULL, 0);

INSERT INTO answer_slots (id, session_question_id, session_id, role, text, selected_recording_id, selection_version)
VALUES ('fu2', 'sq2', 's2', 'follow_up', 'What did you learn?', NULL, 0);

INSERT INTO recordings (id, session_question_id, ordinal, state, original_file, normalized_file, capture_json, created_at, updated_at)
VALUES
 ('rec1', 'sq1', 1, 'ready', 's1/sq1/rec1/original.wav', NULL, NULL, '2026-11-02T09:05:00Z', '2026-11-02T09:06:00Z'),
 ('rec2', 'sq2', 1, 'ready', 's2/sq2/rec2/original.wav', NULL, NULL, '2026-11-02T10:05:00Z', '2026-11-02T10:06:00Z'),
 ('rec-fu', 'fu2', 1, 'ready', 's2/fu2/rec-fu/original.wav', NULL, NULL, '2026-11-02T10:12:00Z', '2026-11-02T10:13:00Z');

INSERT INTO transcripts (id, recording_id, status, text, segments_json, model, language, config_json, load_ms, processing_ms, created_at)
VALUES
 ('tr1', 'rec1', 'completed', 'I failed a release.', '[]', 'small', 'en', '{{}}', 1, 2, '2026-11-02T09:07:00Z'),
 ('tr2', 'rec2', 'completed', 'I missed a deadline.', '[]', 'small', 'en', '{{}}', 1, 2, '2026-11-02T10:07:00Z'),
 ('tr-fu', 'rec-fu', 'completed', 'I learned to plan better.', '[]', 'small', 'en', '{{}}', 1, 2, '2026-11-02T10:14:00Z');

INSERT INTO reviews (recording_id, version, source_transcript_id, origin, text, saved_at)
VALUES
 ('rec1', 1, 'tr1', 'automatic', 'I failed a release.', '2026-11-02T09:08:00Z'),
 ('rec2', 1, 'tr2', 'automatic', 'I missed a deadline.', '2026-11-02T10:08:00Z'),
 ('rec-fu', 1, 'tr-fu', 'automatic', 'I learned to plan better.', '2026-11-02T10:15:00Z');

INSERT INTO confirmations (id, session_question_id, recording_id, review_version, source_transcript_id, origin, text, confirmed_at)
VALUES
 ('c1', 'sq1', 'rec1', 1, 'tr1', 'automatic', 'I failed a release.', '2026-11-02T09:09:00Z'),
 ('c2', 'sq2', 'rec2', 1, 'tr2', 'automatic', 'I missed a deadline.', '2026-11-02T10:09:00Z'),
 ('c-fu', 'fu2', 'rec-fu', 1, 'tr-fu', 'automatic', 'I learned to plan better.', '2026-11-02T10:16:00Z');

INSERT INTO command_receipts (request_id, kind, payload_hash, session_id, entity_id, result_version)
VALUES
 ('req-c1', 'confirm_answer', '{H64}', 's1', 'c1', 1),
 ('req-start', 'start_session', '{H64}', 's2', 's2', 1);

INSERT INTO ai_requests (id, purpose, question_id, session_id, sources_json, prompt_version, rubric_version, schema_version)
VALUES
 ('air-fu', 'follow_up', 'sq2', 's2', '{{"question":"Tell me about a time you failed.","criteria":[],"answers":[]}}', 'v1', 'star-text-v1', 'ai-v3-1'),
 ('air-fb', 'feedback', 'sq2', 's2', '{{"question":"Tell me about a time you failed.","criteria":[],"answers":[]}}', 'v1', 'star-text-v1', 'ai-v3-1');

INSERT INTO ai_attempts (id, request_id, generation, command_id, payload_hash, state, settings_json, returned_model, usage_json, error_json, sent, created_at, finished_at)
VALUES
 ('aia-fu', 'air-fu', 1, 'cmd-fu', '{H64}', 'completed', '{{"model":"m"}}', 'm', NULL, NULL, 'received', '2026-11-02T10:10:00Z', '2026-11-02T10:11:00Z'),
 ('aia-fu2', 'air-fu', 2, 'cmd-fu2', '{H64}', 'failed', '{{"model":"m"}}', NULL, NULL, '{{"error":"x"}}', 'possibly_sent', '2026-11-02T10:12:00Z', '2026-11-02T10:13:00Z'),
 ('aia-fb', 'air-fb', 1, 'cmd-fb', '{H64}', 'completed', '{{"model":"m"}}', 'm', NULL, NULL, 'received', '2026-11-02T10:20:00Z', '2026-11-02T10:21:00Z');

INSERT INTO ai_reports (question_id, attempt_id, json) VALUES ('sq2', 'aia-fb', '{{"ok":true}}');

INSERT INTO ai_stages (question_id, stage, generation, reason) VALUES ('sq2', 'follow_up', 1, NULL);

INSERT INTO ai_receipts (id, kind, payload_hash, entity_id) VALUES ('ai-receipt', 'feedback', '{H64}', 'sq2');
"#
    )
}

/// Prepara uma base v2 povoada com a fixture, devolvendo o caminho.
fn seeded_v2(path: &Path) {
    apply_v2(path);
    let connection = raw(path);
    connection
        .execute_batch("PRAGMA foreign_keys=OFF;")
        .expect("desativar FKs");
    connection
        .execute_batch(&v2_fixture())
        .expect("semear a fixture v2");
    connection.close().expect("fechar a ligação crua");
}

/// Semeia duas perguntas com sessões concluídas e confirmações, numa base v3.
fn seed_completed_session(connection: &Connection) {
    connection
        .execute_batch(
            r#"
INSERT INTO questions (id, text, category, competencies_json, criteria_json, version, created_at, updated_at)
VALUES
 ('q1', 'Tell me about a time you failed.', 'failure_learning', '["Aprendizagem"]', '["Explicar o que aprendeu."]', 1, '2026-11-02T09:00:00Z', '2026-11-02T09:00:00Z'),
 ('q2', 'Tell me about a time you disagreed.', 'conflict_collaboration', '["Colaboração"]', '["Explicar a divergência."]', 1, '2026-11-02T09:00:00Z', '2026-11-02T09:00:00Z');
INSERT INTO sessions (id, category, question_count, position, status, mode, version, created_at, completed_at)
VALUES
 ('s1', NULL, 1, 1, 'completed', 'local_v2', 1, '2026-11-02T09:00:00Z', '2026-11-02T09:30:00Z'),
 ('s2', NULL, 1, 1, 'completed', 'local_v2', 1, '2026-11-02T10:00:00Z', '2026-11-02T10:30:00Z');
INSERT INTO session_questions (id, session_id, source_question_id, source_version, position, text, category, competencies_json, criteria_json, selected_recording_id, selection_version)
VALUES
 ('sq1', 's1', 'q1', 1, 1, 'Tell me about a time you failed.', 'failure_learning', '["Aprendizagem"]', '["Explicar o que aprendeu."]', NULL, 0),
 ('sq2', 's2', 'q2', 1, 1, 'Tell me about a time you disagreed.', 'conflict_collaboration', '["Colaboração"]', '["Explicar a divergência."]', NULL, 0);
INSERT INTO recordings (id, session_question_id, ordinal, state, original_file, normalized_file, capture_json, created_at, updated_at)
VALUES
 ('rec1', 'sq1', 1, 'ready', 's1/sq1/rec1/original.wav', NULL, NULL, '2026-11-02T09:05:00Z', '2026-11-02T09:06:00Z'),
 ('rec2', 'sq2', 1, 'ready', 's2/sq2/rec2/original.wav', NULL, NULL, '2026-11-02T10:05:00Z', '2026-11-02T10:06:00Z');
INSERT INTO transcripts (id, recording_id, status, text, segments_json, model, language, config_json, load_ms, processing_ms, created_at)
VALUES
 ('tr1', 'rec1', 'completed', 'I failed a release.', '[]', 'small', 'en', '{}', 1, 2, '2026-11-02T09:07:00Z'),
 ('tr2', 'rec2', 'completed', 'I disagreed on the checklist.', '[]', 'small', 'en', '{}', 1, 2, '2026-11-02T10:07:00Z');
INSERT INTO reviews (recording_id, version, source_transcript_id, origin, text, saved_at)
VALUES
 ('rec1', 1, 'tr1', 'automatic', 'I failed a release.', '2026-11-02T09:08:00Z'),
 ('rec2', 1, 'tr2', 'automatic', 'I disagreed on the checklist.', '2026-11-02T10:08:00Z');
INSERT INTO confirmations (id, session_question_id, recording_id, review_version, source_transcript_id, origin, text, confirmed_at)
VALUES
 ('c1', 'sq1', 'rec1', 1, 'tr1', 'automatic', 'I failed a release.', '2026-11-02T09:09:00Z'),
 ('c2', 'sq2', 'rec2', 1, 'tr2', 'automatic', 'I disagreed on the checklist.', '2026-11-02T10:09:00Z');
"#,
        )
        .expect("semear a sessão concluída");
}

/// Cria uma prática pendente com o seu slot de tentativa, na mesma transação.
fn create_practice(connection: &mut Connection, practice_id: &str, slot_id: &str) {
    let tx = connection.transaction().expect("iniciar transação");
    tx.execute(
        "INSERT INTO practices (id, session_question_id, source_session_id, ordinal, answer_id, status, version, created_at)
         VALUES (?1, 'sq1', 's1', 1, ?2, 'preparing', 1, '2026-11-02T10:00:00Z')",
        rusqlite::params![practice_id, slot_id],
    )
    .expect("inserir a prática");
    tx.execute(
        "INSERT INTO answer_slots (id, session_question_id, session_id, role, text, practice_id)
         VALUES (?1, 'sq1', 's1', 'attempt', 'Tell me about a time you failed.', ?2)",
        rusqlite::params![slot_id, practice_id],
    )
    .expect("inserir o slot de tentativa");
    tx.commit().expect("confirmar a prática");
}

#[test]
fn migrates_v0_v1_v2_preserving_all_source_ids() {
    // v2: fixture rica e comparação de todos os identificadores.
    let (_directory, path) = temp_path();
    seeded_v2(&path);

    let database = Database::open(path.clone()).expect("uma base v2 deve migrar para v3");

    database
        .with_connection(|connection| {
            assert!(
                !connection
                    .prepare("PRAGMA foreign_key_check")
                    .map_err(to_app_error)?
                    .exists([])
                    .map_err(to_app_error)?,
                "a migração não pode deixar violações de FK"
            );

            // Slots: inicial de s1, inicial de s2 e seguimento de s2, todos sem prática.
            let slots: Vec<(String, String, Option<String>)> = {
                let mut st = connection
                    .prepare(
                        "SELECT id, role, practice_id FROM answer_slots ORDER BY id",
                    )
                    .map_err(to_app_error)?;
                let rows = st
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<String>>(2)?,
                        ))
                    })
                    .map_err(to_app_error)?;
                rows.collect::<rusqlite::Result<Vec<_>>>()
                    .map_err(to_app_error)?
            };
            assert_eq!(
                slots,
                vec![
                    ("fu2".to_owned(), "follow_up".to_owned(), None),
                    ("sq1".to_owned(), "initial".to_owned(), None),
                    ("sq2".to_owned(), "initial".to_owned(), None),
                ]
            );

            for id in ["s1", "s2"] {
                let n: i64 = connection
                    .query_row(
                        "SELECT count(*) FROM sessions WHERE id = ?1",
                        [id],
                        |row| row.get(0),
                    )
                    .map_err(to_app_error)?;
                assert_eq!(n, 1, "a sessão {id} deve ser preservada");
            }
            for id in ["c1", "c2", "c-fu"] {
                let n: i64 = connection
                    .query_row(
                        "SELECT count(*) FROM confirmations WHERE id = ?1",
                        [id],
                        |row| row.get(0),
                    )
                    .map_err(to_app_error)?;
                assert_eq!(n, 1, "a confirmação {id} deve ser preservada");
            }
            for id in ["aia-fu", "aia-fu2", "aia-fb"] {
                let n: i64 = connection
                    .query_row(
                        "SELECT count(*) FROM ai_attempts WHERE id = ?1",
                        [id],
                        |row| row.get(0),
                    )
                    .map_err(to_app_error)?;
                assert_eq!(n, 1, "a tentativa {id} deve ser preservada");
            }
            for id in ["req-c1", "req-start"] {
                let n: i64 = connection
                    .query_row(
                        "SELECT count(*) FROM command_receipts WHERE request_id = ?1",
                        [id],
                        |row| row.get(0),
                    )
                    .map_err(to_app_error)?;
                assert_eq!(n, 1, "o recibo {id} deve ser preservado");
            }
            let receipts: i64 = connection
                .query_row(
                    "SELECT count(*) FROM ai_receipts WHERE id = 'ai-receipt'",
                    [],
                    |row| row.get(0),
                )
                .map_err(to_app_error)?;
            assert_eq!(receipts, 1, "o recibo de IA deve ser preservado");

            // ai_requests: origem original e variante por omissão.
            let requests: Vec<(String, String, String, Option<String>)> = {
                let mut st = connection
                    .prepare(
                        "SELECT id, origin_key, variant_key, practice_id FROM ai_requests ORDER BY id",
                    )
                    .map_err(to_app_error)?;
                let rows = st
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, Option<String>>(3)?,
                        ))
                    })
                    .map_err(to_app_error)?;
                rows.collect::<rusqlite::Result<Vec<_>>>()
                    .map_err(to_app_error)?
            };
            assert_eq!(
                requests,
                vec![
                    (
                        "air-fb".to_owned(),
                        "original:sq2".to_owned(),
                        "default".to_owned(),
                        None
                    ),
                    (
                        "air-fu".to_owned(),
                        "original:sq2".to_owned(),
                        "default".to_owned(),
                        None
                    ),
                ]
            );

            let reports: i64 = connection
                .query_row(
                    "SELECT count(*) FROM ai_reports WHERE question_id = 'sq2'",
                    [],
                    |row| row.get(0),
                )
                .map_err(to_app_error)?;
            assert_eq!(reports, 1, "o relatório deve ser preservado");

            Ok(())
        })
        .expect("verificar a migração v2→v3");

    // v1: migração de uma base com dados mínimos preserva-os.
    let (_directory, path) = temp_path();
    {
        let connection = raw(&path);
        connection
            .execute_batch("PRAGMA foreign_keys=OFF;")
            .expect("desativar FKs");
        connection.execute_batch(MIGRATION_V1).expect("aplicar v1");
        connection
            .execute_batch(
                "PRAGMA user_version=1;
                 INSERT INTO questions VALUES ('q','Question','failure_learning','[]','[]',1,'now','now');
                 INSERT INTO sessions VALUES ('s',NULL,1,1,'completed','local_v2',1,'now','done');
                 INSERT INTO session_questions VALUES ('a','s','q',1,1,'Question','failure_learning','[]','[]',NULL,0);
                 INSERT INTO recordings VALUES ('r','a',1,'ready','s/a/r/original.wav',NULL,NULL,'now','now');
                 INSERT INTO transcripts VALUES ('t','r','completed','Text','[]','small','en','{}',1,2,'now');
                 INSERT INTO reviews VALUES ('r',1,'t','automatic','Text','now');
                 INSERT INTO confirmations VALUES ('c','a','r',1,'t','automatic','Text','now');",
            )
            .expect("semear a base v1");
        connection.close().expect("fechar");
    }
    let database = Database::open(path.clone()).expect("uma base v1 deve migrar para v3");
    database
        .with_connection(|connection| {
            let version: i64 = connection
                .query_row("PRAGMA user_version", [], |row| row.get(0))
                .map_err(to_app_error)?;
            assert_eq!(version, i64::from(DATABASE_VERSION));
            let n: i64 = connection
                .query_row(
                    "SELECT count(*) FROM answer_slots WHERE id = 'a' AND role = 'initial'",
                    [],
                    |row| row.get(0),
                )
                .map_err(to_app_error)?;
            assert_eq!(n, 1, "o slot inicial deve ser criado na migração");
            Ok(())
        })
        .expect("verificar a migração v1→v3");

    // v0: base vazia migra diretamente para v3.
    let (_directory, path) = temp_path();
    let database = Database::open(path.clone()).expect("uma base vazia deve abrir em v3");
    database
        .with_connection(|connection| {
            let version: i64 = connection
                .query_row("PRAGMA user_version", [], |row| row.get(0))
                .map_err(to_app_error)?;
            assert_eq!(version, i64::from(DATABASE_VERSION));
            assert!(table_exists(connection, "practices"));
            assert!(table_exists(connection, "stories"));
            assert!(table_exists(connection, "rewrites"));
            Ok(())
        })
        .expect("verificar a migração v0→v3");
}

#[test]
fn migration_rollback_restores_v2() {
    let (_directory, path) = temp_path();
    seeded_v2(&path);

    let broken_v3 = "CREATE TABLE practices (id TEXT PRIMARY KEY NOT NULL);
                     INSERT INTO tabela_inexistente (id) VALUES (1);";
    let error = Database::open_with_migrations(
        path.clone(),
        MIGRATION_V1,
        MIGRATION_V2,
        broken_v3,
        MIGRATION_V4,
        MIGRATION_V5,
    )
    .expect_err("a migração v3 injetada deve falhar");
    assert!(error.retryable, "uma falha de migração é recuperável");

    let connection = raw(&path);
    assert_eq!(user_version(&connection), 2, "a versão só muda com sucesso");
    assert!(
        !table_exists(&connection, "practices"),
        "a tabela criada antes da falha é revertida"
    );
    assert!(
        table_exists(&connection, "answer_slots"),
        "o esquema v2 permanece"
    );
    let sessions: i64 = connection
        .query_row("SELECT count(*) FROM sessions WHERE id = 's1'", [], |row| {
            row.get(0)
        })
        .expect("os dados v2 continuam legíveis");
    assert_eq!(sessions, 1, "os dados existentes não são apagados");
}

#[test]
fn future_version_is_refused_without_write() {
    let (_directory, path) = temp_path();
    {
        let connection = raw(&path);
        connection
            .execute_batch(
                "PRAGMA user_version = 99;
                 CREATE TABLE sentinel (id TEXT PRIMARY KEY NOT NULL);
                 INSERT INTO sentinel (id) VALUES ('futuro');",
            )
            .expect("preparar uma base futura");
    }

    let error = Database::open(path.clone()).expect_err("uma base futura não deve abrir");
    assert_eq!(error.code, "DATABASE_VERSION_UNSUPPORTED");

    let connection = raw(&path);
    assert_eq!(user_version(&connection), 99, "a versão não é reescrita");
    assert!(table_exists(&connection, "sentinel"), "nada é recriado");
    assert!(
        !table_exists(&connection, "practices"),
        "nada novo é escrito"
    );
    let kept: i64 = connection
        .query_row("SELECT count(*) FROM sentinel", [], |row| row.get(0))
        .expect("os dados futuros continuam intactos");
    assert_eq!(kept, 1);
}

#[test]
fn slot_owner_cannot_cross_practice_or_source_question() {
    let (_directory, path) = temp_path();
    let database = Database::open(path).expect("abrir em v3");

    database
        .with_connection(|connection| {
            seed_completed_session(connection);
            create_practice(&mut *connection, "p1", "att1");

            // Um slot `attempt` exige `practice_id`.
            let no_practice = connection.execute(
                "INSERT INTO answer_slots (id, session_question_id, session_id, role, text)
                 VALUES ('att-x', 'sq1', 's1', 'attempt', 'texto')",
                [],
            );
            assert!(no_practice.is_err(), "tentativa sem prática é recusada");

            // Um slot inicial ou seguimento não pode ter `practice_id`.
            let initial_with_practice = connection.execute(
                "INSERT INTO answer_slots (id, session_question_id, session_id, role, text, practice_id)
                 VALUES ('bad-initial', 'sq1', 's1', 'initial', 'texto', 'p1')",
                [],
            );
            assert!(initial_with_practice.is_err(), "inicial com prática é recusada");
            let follow_up_with_practice = connection.execute(
                "INSERT INTO answer_slots (id, session_question_id, session_id, role, text, practice_id)
                 VALUES ('bad-fu', 'sq1', 's1', 'follow_up', 'texto', 'p1')",
                [],
            );
            assert!(
                follow_up_with_practice.is_err(),
                "seguimento com prática é recusado"
            );

            // A prática do slot tem de pertencer à mesma pergunta da sessão.
            let cross_question = connection.execute(
                "INSERT INTO answer_slots (id, session_question_id, session_id, role, text, practice_id)
                 VALUES ('att-cross', 'sq2', 's2', 'attempt', 'texto', 'p1')",
                [],
            );
            assert!(
                cross_question.is_err(),
                "a prática de outra pergunta é recusada no slot"
            );

            // Uma prática inexistente também é recusada.
            let foreign_practice = connection.execute(
                "INSERT INTO answer_slots (id, session_question_id, session_id, role, text, practice_id)
                 VALUES ('att-foreign', 'sq1', 's1', 'attempt', 'texto', 'p-nova')",
                [],
            );
            assert!(
                foreign_practice.is_err(),
                "prática inexistente é recusada"
            );

            Ok(())
        })
        .expect("verificar a propriedade do slot");
}

#[test]
fn multiple_attempt_slots_keep_unique_initial_and_follow_up() {
    let (_directory, path) = temp_path();
    let database = Database::open(path).expect("abrir em v3");

    database
        .with_connection(|connection| {
            seed_completed_session(connection);

            // Segunda inicial para a mesma pergunta é recusada.
            let second_initial = connection.execute(
                "INSERT INTO answer_slots (id, session_question_id, session_id, role, text)
                 VALUES ('initial-2', 'sq1', 's1', 'initial', 'texto')",
                [],
            );
            assert!(second_initial.is_err(), "segunda inicial é recusada");

            // Segundo seguimento para a mesma pergunta é recusado.
            connection
                .execute(
                    "INSERT INTO answer_slots (id, session_question_id, session_id, role, text)
                     VALUES ('fu-1', 'sq1', 's1', 'follow_up', 'What?')",
                    [],
                )
                .expect("primeiro seguimento é aceite");
            let second_follow_up = connection.execute(
                "INSERT INTO answer_slots (id, session_question_id, session_id, role, text)
                 VALUES ('fu-2', 'sq1', 's1', 'follow_up', 'Why?')",
                [],
            );
            assert!(second_follow_up.is_err(), "segundo seguimento é recusado");

            // Duas práticas da mesma pergunta podem ter duas tentativas distintas.
            {
                let tx = connection.transaction().expect("transação");
                tx.execute(
                    "INSERT INTO practices (id, session_question_id, source_session_id, ordinal, answer_id, status, version, created_at)
                     VALUES ('p1', 'sq1', 's1', 1, 'att1', 'preparing', 1, '2026-11-02T10:00:00Z')",
                    [],
                )
                .expect("prática p1");
                tx.execute(
                    "INSERT INTO answer_slots (id, session_question_id, session_id, role, text, practice_id)
                     VALUES ('att1', 'sq1', 's1', 'attempt', 'texto', 'p1')",
                    [],
                )
                .expect("tentativa de p1");
                tx.execute(
                    "INSERT INTO practices (id, session_question_id, source_session_id, ordinal, answer_id, status, version, created_at, confirmed_at)
                     VALUES ('p2', 'sq1', 's1', 2, 'att2', 'confirmed', 1, '2026-11-02T10:10:00Z', '2026-11-02T10:20:00Z')",
                    [],
                )
                .expect("prática p2 confirmada");
                tx.execute(
                    "INSERT INTO answer_slots (id, session_question_id, session_id, role, text, practice_id)
                     VALUES ('att2', 'sq1', 's1', 'attempt', 'texto', 'p2')",
                    [],
                )
                .expect("tentativa de p2");
                tx.commit().expect("confirmar");
            }

            let attempts: i64 = connection
                .query_row(
                    "SELECT count(*) FROM answer_slots WHERE role = 'attempt'",
                    [],
                    |row| row.get(0),
                )
                .expect("contar tentativas");
            assert_eq!(attempts, 2, "duas práticas têm duas tentativas");

            Ok(())
        })
        .expect("verificar a unicidade dos papéis");
}

#[test]
fn pending_practice_and_active_interview_are_mutually_exclusive() {
    let (_directory, path) = temp_path();
    let database = Database::open(path).expect("abrir em v3");

    database
        .with_connection(|connection| {
            seed_completed_session(connection);
            create_practice(&mut *connection, "p1", "att1");

            // Com uma prática pendente, não se inicia uma entrevista.
            let start_session = connection.execute(
                "INSERT INTO sessions (id, category, question_count, position, status, mode, version, created_at, completed_at)
                 VALUES ('s2', NULL, 1, 1, 'in_progress', 'local_v2', 1, '2026-11-02T11:00:00Z', NULL)",
                [],
            );
            assert!(
                start_session.is_err(),
                "prática pendente impede iniciar entrevista"
            );

            Ok(())
        })
        .expect("verificar a exclusão com prática pendente");

    // Sentido inverso: com uma entrevista em curso, não se cria prática.
    let (_directory, path) = temp_path();
    let database = Database::open(path).expect("abrir em v3");
    database
        .with_connection(|connection| {
            connection
                .execute_batch(
                    r#"
INSERT INTO questions VALUES ('q1', 'Tell me about a time you failed.', 'failure_learning', '["Aprendizagem"]', '["Explicar o que aprendeu."]', 1, '2026-11-02T09:00:00Z', '2026-11-02T09:00:00Z');
INSERT INTO sessions VALUES ('s1', NULL, 1, 1, 'in_progress', 'local_v2', 1, '2026-11-02T09:00:00Z', NULL);
INSERT INTO session_questions VALUES ('sq1', 's1', 'q1', 1, 1, 'Tell me about a time you failed.', 'failure_learning', '["Aprendizagem"]', '["Explicar o que aprendeu."]', NULL, 0);
"#,
                )
                .expect("semear entrevista em curso");

            let practice = connection.execute(
                "INSERT INTO practices (id, session_question_id, source_session_id, ordinal, answer_id, status, version, created_at)
                 VALUES ('p1', 'sq1', 's1', 1, 'att1', 'preparing', 1, '2026-11-02T10:00:00Z')",
                [],
            );
            assert!(
                practice.is_err(),
                "entrevista em curso impede criar prática"
            );

            Ok(())
        })
        .expect("verificar a exclusão com entrevista em curso");
}

#[test]
fn resolve_capture_owner_resolves_interview_and_practice() {
    let (_directory, path) = temp_path();
    let database = Database::open(path).expect("abrir em v3");

    database
        .with_connection(|connection| {
            seed_completed_session(connection);
            create_practice(&mut *connection, "p1", "att1");
            Ok(())
        })
        .expect("preparar a base");

    let interview = database
        .resolve_capture_owner(&CaptureOwner::Interview {
            session_id: "s1".into(),
            session_question_id: "sq1".into(),
            answer_id: "sq1".into(),
        })
        .expect("resolver a resposta inicial");
    assert_eq!(interview.source_session_id, "s1");
    assert_eq!(interview.session_question_id, "sq1");
    assert!(
        interview.confirmation.is_some(),
        "a inicial está confirmada"
    );
    assert!(!interview.writable, "sessão concluída é só de leitura");
    assert_eq!(interview.question.text, "Tell me about a time you failed.");

    let practice = database
        .resolve_capture_owner(&CaptureOwner::Practice {
            practice_id: "p1".into(),
            answer_id: "att1".into(),
        })
        .expect("resolver a tentativa da prática");
    assert_eq!(practice.source_session_id, "s1");
    assert_eq!(practice.session_question_id, "sq1");
    assert_eq!(practice.slot.role, "attempt");
    assert_eq!(practice.slot.practice_id.as_deref(), Some("p1"));
    assert!(practice.writable, "prática pendente é editável");

    let crossed = database.resolve_capture_owner(&CaptureOwner::Practice {
        practice_id: "p1".into(),
        answer_id: "sq1".into(),
    });
    assert!(
        crossed.is_err(),
        "o slot inicial não é tentativa da prática"
    );
}

#[test]
fn lookup_command_identity_detects_conflicts() {
    let (_directory, path) = temp_path();
    let database = Database::open(path).expect("abrir em v3");

    assert_eq!(
        database.lookup_command_identity("nada").unwrap(),
        CommandIdentity::NotFound
    );

    database
        .with_connection(|connection| {
            seed_completed_session(connection);
            connection
                .execute(
                    "INSERT INTO ai_receipts (id, kind, payload_hash, entity_id)
                     VALUES ('req-x', 'feedback', ?1, NULL)",
                    [H64],
                )
                .map_err(to_app_error)?;
            Ok(())
        })
        .expect("guardar um recibo de IA");

    let found = database.lookup_command_identity("req-x").unwrap();
    assert_eq!(
        found,
        CommandIdentity::Found {
            location: CommandIdentityLocation::AiReceipt,
            kind: Some("feedback".into()),
            payload_hash: H64.into(),
        }
    );

    // A mesma identidade noutra tabela é um conflito, não uma escolha.
    database
        .with_connection(|connection| {
            connection
                .execute(
                    "INSERT INTO ai_requests (id, purpose, question_id, session_id, origin_key, variant_key, sources_json)
                     VALUES ('air-x', 'feedback', 'sq1', 's1', 'original:sq1', 'default',
                             '{\"question\":\"x\",\"criteria\":[],\"answers\":[]}')",
                    [],
                )
                .map_err(to_app_error)?;
            connection
                .execute(
                    "INSERT INTO ai_attempts (id, request_id, generation, command_id, payload_hash, state, settings_json, created_at)
                     VALUES ('aia-x', 'air-x', 1, 'req-x', ?1, 'running', '{}', '2026-11-02T10:00:00Z')",
                    [H64],
                )
                .map_err(to_app_error)?;
            Ok(())
        })
        .expect("guardar uma tentativa com o mesmo command_id");
    assert_eq!(
        database.lookup_command_identity("req-x").unwrap(),
        CommandIdentity::Conflict
    );
}

#[test]
fn practice_slot_linkage_is_bidirectional() {
    let (_directory, path) = temp_path();
    let database = Database::open(path).expect("abrir em v3");

    // Criação normal: prática e slot coerentes.
    database
        .with_connection(|connection| {
            seed_completed_session(connection);
            create_practice(&mut *connection, "p1", "att1");
            Ok(())
        })
        .expect("a criação normal é válida");

    // Ligação cruzada: p2 e p3 trocam os answer_id; o slot de cada um aponta
    // para o outro. Só a FK composta rejeita no commit.
    database
        .with_connection(|connection| {
            let tx = connection.transaction().expect("iniciar transação");
            tx.execute(
                "INSERT INTO practices (id, session_question_id, source_session_id, ordinal, answer_id, status, version, created_at, confirmed_at)
                 VALUES ('p2', 'sq1', 's1', 2, 'att4', 'confirmed', 1, '2026-11-02T10:10:00Z', '2026-11-02T10:20:00Z')",
                [],
            )
            .expect("inserir p2");
            tx.execute(
                "INSERT INTO practices (id, session_question_id, source_session_id, ordinal, answer_id, status, version, created_at, confirmed_at)
                 VALUES ('p3', 'sq1', 's1', 3, 'att3', 'confirmed', 1, '2026-11-02T10:11:00Z', '2026-11-02T10:21:00Z')",
                [],
            )
            .expect("inserir p3");
            tx.execute(
                "INSERT INTO answer_slots (id, session_question_id, session_id, role, text, practice_id)
                 VALUES ('att3', 'sq1', 's1', 'attempt', 'texto', 'p2')",
                [],
            )
            .expect("inserir att3");
            tx.execute(
                "INSERT INTO answer_slots (id, session_question_id, session_id, role, text, practice_id)
                 VALUES ('att4', 'sq1', 's1', 'attempt', 'texto', 'p3')",
                [],
            )
            .expect("inserir att4");
            let result = tx.commit();
            assert!(
                result.is_err(),
                "a ligação cruzada entre prática e slot é rejeitada no commit"
            );
            Ok(())
        })
        .expect("verificar a FK composta");
}

#[test]
fn rewrite_origin_must_match_request() {
    let (_directory, path) = temp_path();
    let database = Database::open(path).expect("abrir em v3");

    database
        .with_connection(|connection| {
            seed_completed_session(connection);

            connection
                .execute(
                    "INSERT INTO ai_requests (id, purpose, question_id, session_id, origin_key, variant_key, sources_json)
                     VALUES ('air-rw', 'rewrite', 'sq1', 's1', 'original:sq1', 'rewrite:B1:v1:star-text-v1:ai-v3-1',
                             '{\"question\":\"x\",\"criteria\":[],\"answers\":[]}')",
                    [],
                )
                .map_err(to_app_error)?;
            connection
                .execute(
                    "INSERT INTO ai_attempts (id, request_id, generation, command_id, payload_hash, state, settings_json, created_at)
                     VALUES ('aia-rw', 'air-rw', 1, 'cmd-rw', ?1, 'completed', '{}', '2026-11-02T10:00:00Z')",
                    [H64],
                )
                .map_err(to_app_error)?;

            // Origem diferente da do pedido é rejeitada.
            let mismatched = connection.execute(
                "INSERT INTO rewrites (id, request_id, network_attempt_id, origin_key, level, json, spoken_text, created_at)
                 VALUES ('rw-bad', 'air-rw', 'aia-rw', 'practice:p1', 'B1', '{}', 'texto', '2026-11-02T10:01:00Z')",
                [],
            );
            assert!(
                mismatched.is_err(),
                "origem diferente da do pedido é rejeitada"
            );

            // Origem igual à do pedido é aceite.
            let matched = connection.execute(
                "INSERT INTO rewrites (id, request_id, network_attempt_id, origin_key, level, json, spoken_text, created_at)
                 VALUES ('rw-ok', 'air-rw', 'aia-rw', 'original:sq1', 'B1', '{}', 'texto', '2026-11-02T10:02:00Z')",
                [],
            );
            assert!(matched.is_ok(), "origem coerente é aceite");

            Ok(())
        })
        .expect("verificar a coerência da reformulação");
}

fn to_app_error(error: rusqlite::Error) -> language_assistant_tutor::domain::AppError {
    language_assistant_tutor::domain::AppError::new("TEST_SQLITE", error.to_string(), false)
}

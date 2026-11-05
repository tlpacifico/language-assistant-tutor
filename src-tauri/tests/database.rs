//! Esquema SQLite da fase 2 (secção 3 do plano técnico).
//!
//! Os testes usam diretórios temporários e SQL direto para provar as
//! invariantes que o núcleo vai depender: versão do esquema, rollback de uma
//! migração falhada, recusa de uma base futura, FKs compostas dentro da mesma
//! pergunta/gravação e triggers de imutabilidade.

use std::path::{Path, PathBuf};

use language_assistant_tutor::db::{Database, DATABASE_VERSION};
use language_assistant_tutor::domain::{error_code, AppError};
use rusqlite::Connection;

/// Grafo mínimo válido: duas perguntas de sessão, cada uma com uma gravação,
/// uma transcrição; a primeira tem revisão e confirmação.
const SEED_TWO_QUESTIONS: &str = r#"
INSERT INTO questions (id, text, category, competencies_json, criteria_json, version, created_at, updated_at)
VALUES
  ('q-a', 'Tell me about a time you disagreed with a colleague.', 'conflict_collaboration',
   '["Gestão de conflitos"]', '["Explicar a divergência."]', 1,
   '2026-11-02T09:00:00Z', '2026-11-02T09:00:00Z'),
  ('q-b', 'Describe a time you helped a team work together more effectively.', 'conflict_collaboration',
   '["Colaboração"]', '["Identificar o obstáculo."]', 1,
   '2026-11-02T09:00:00Z', '2026-11-02T09:00:00Z');

INSERT INTO sessions (id, category, question_count, position, status, mode, version, created_at, completed_at)
VALUES ('s-1', NULL, 2, 1, 'in_progress', 'local_v2', 2, '2026-11-02T09:00:00Z', NULL);

INSERT INTO session_questions
  (id, session_id, source_question_id, source_version, position, text, category,
   competencies_json, criteria_json, selected_recording_id, selection_version)
VALUES
  ('sq-a', 's-1', 'q-a', 1, 1, 'Tell me about a time you disagreed with a colleague.',
   'conflict_collaboration', '["Gestão de conflitos"]', '["Explicar a divergência."]', NULL, 0),
  ('sq-b', 's-1', 'q-b', 1, 2, 'Describe a time you helped a team work together more effectively.',
   'conflict_collaboration', '["Colaboração"]', '["Identificar o obstáculo."]', NULL, 0);

INSERT INTO recordings
  (id, session_question_id, ordinal, state, original_file, normalized_file, capture_json, created_at, updated_at)
VALUES
  ('rec-a', 'sq-a', 1, 'ready', 'sessions/s-1/sq-a/rec-a/original.wav', NULL,
   '{"sampleRate":48000,"channels":1,"durationMs":90500,"bytes":8690000,"incomplete":false}',
   '2026-11-02T09:05:00Z', '2026-11-02T09:06:00Z'),
  ('rec-b', 'sq-b', 1, 'ready', 'sessions/s-1/sq-b/rec-b/original.wav', NULL,
   '{"sampleRate":48000,"channels":1,"durationMs":71200,"bytes":6835000,"incomplete":false}',
   '2026-11-02T09:10:00Z', '2026-11-02T09:11:00Z');

INSERT INTO transcripts
  (id, recording_id, status, text, segments_json, model, language, config_json, load_ms, processing_ms, created_at)
VALUES
  ('tr-a', 'rec-a', 'completed', 'I disagreed about the release checklist.',
   '[{"startMs":0,"endMs":2400,"text":"I disagreed about the release checklist."}]',
   'small', 'en', '{"beamSize":5}', 4200, 3120, '2026-11-02T09:07:00Z'),
  ('tr-b', 'rec-b', 'completed', 'I helped the team with a shared checklist.',
   '[{"startMs":0,"endMs":2100,"text":"I helped the team with a shared checklist."}]',
   'small', 'en', '{"beamSize":5}', 4100, 3050, '2026-11-02T09:12:00Z');

INSERT INTO reviews (recording_id, version, source_transcript_id, origin, text, saved_at)
VALUES ('rec-a', 1, 'tr-a', 'automatic', 'I disagreed about the release checklist.',
        '2026-11-02T09:08:00Z');

INSERT INTO confirmations
  (id, session_question_id, recording_id, review_version, source_transcript_id, origin, text, confirmed_at)
VALUES ('c-a', 'sq-a', 'rec-a', 1, 'tr-a', 'automatic',
        'I disagreed about the release checklist.', '2026-11-02T09:09:00Z');
"#;

fn temp_database_path() -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().expect("criar o diretório temporário");
    let path = directory.path().join("tutor.sqlite3");
    (directory, path)
}

fn to_app_error(error: rusqlite::Error) -> AppError {
    AppError::new("TEST_SQLITE", error.to_string(), false)
}

fn open_raw(path: &Path) -> Connection {
    Connection::open(path).expect("abrir a base de dados de teste")
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

fn seed_two_questions(connection: &Connection) {
    connection
        .execute_batch(SEED_TWO_QUESTIONS)
        .expect("semear o grafo de teste");
}

fn expect_trigger_refusal(result: rusqlite::Result<usize>, context: &str) {
    let error = result.expect_err(context);
    assert!(
        error.to_string().contains("imut"),
        "{context}: esperava uma recusa de imutabilidade, recebi: {error}"
    );
}

#[test]
fn fresh_database_has_version_two() {
    let (_directory, path) = temp_database_path();
    let database = Database::open(path.clone()).expect("uma base nova deve abrir");

    let report = database
        .with_connection(|connection| {
            let version: i64 = connection
                .query_row("PRAGMA user_version", [], |row| row.get(0))
                .map_err(to_app_error)?;
            let foreign_keys: i64 = connection
                .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
                .map_err(to_app_error)?;
            let busy_timeout: i64 = connection
                .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
                .map_err(to_app_error)?;
            let synchronous: i64 = connection
                .query_row("PRAGMA synchronous", [], |row| row.get(0))
                .map_err(to_app_error)?;
            let journal_mode: String = connection
                .query_row("PRAGMA journal_mode", [], |row| row.get(0))
                .map_err(to_app_error)?;
            let question_count: i64 = connection
                .query_row("SELECT count(*) FROM questions", [], |row| row.get(0))
                .map_err(to_app_error)?;
            let seed_count: i64 = connection
                .query_row("SELECT count(*) FROM content_seeds", [], |row| row.get(0))
                .map_err(to_app_error)?;

            let mut tables = connection
                .prepare(
                    "SELECT name FROM sqlite_master
                     WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
                     ORDER BY name",
                )
                .map_err(to_app_error)?;
            let tables: Vec<String> = tables
                .query_map([], |row| row.get(0))
                .map_err(to_app_error)?
                .collect::<rusqlite::Result<Vec<String>>>()
                .map_err(to_app_error)?;

            let mut triggers = connection
                .prepare(
                    "SELECT name FROM sqlite_master
                     WHERE type = 'trigger' ORDER BY name",
                )
                .map_err(to_app_error)?;
            let triggers: Vec<String> = triggers
                .query_map([], |row| row.get(0))
                .map_err(to_app_error)?
                .collect::<rusqlite::Result<Vec<String>>>()
                .map_err(to_app_error)?;

            let active_index: i64 = connection
                .query_row(
                    "SELECT count(*) FROM sqlite_master
                     WHERE type = 'index' AND name = 'one_active_session'",
                    [],
                    |row| row.get(0),
                )
                .map_err(to_app_error)?;

            // Nenhuma FK por resolver: prova também que as referências para a
            // frente (session_questions -> recordings) são válidas.
            let mut violations = connection
                .prepare("PRAGMA foreign_key_check")
                .map_err(to_app_error)?;
            let violations: Vec<String> = violations
                .query_map([], |row| row.get(0))
                .map_err(to_app_error)?
                .collect::<rusqlite::Result<Vec<String>>>()
                .map_err(to_app_error)?;

            Ok((
                version,
                foreign_keys,
                busy_timeout,
                synchronous,
                journal_mode,
                question_count,
                seed_count,
                tables,
                triggers,
                active_index,
                violations,
            ))
        })
        .expect("ler o esquema criado");

    assert_eq!(report.0, i64::from(DATABASE_VERSION));
    assert_eq!(report.0, 5, "a migração deve fixar user_version = 5");
    assert_eq!(report.1, 1, "foreign_keys deve ficar ativo");
    assert_eq!(report.2, 3000, "busy_timeout deve ser 3000 ms");
    assert_eq!(report.3, 2, "synchronous deve ser FULL (2)");
    assert!(
        report.4.eq_ignore_ascii_case("delete"),
        "o journal deve manter-se DELETE, recebi {}",
        report.4
    );

    // A migração não insere perguntas (regra R1); o seed é aplicado por
    // `Database::open` depois de a migração ter sucesso, dentro da sua própria
    // transação e marcado uma única vez em `content_seeds`.
    assert_eq!(
        report.5, 20,
        "abrir a base aplica o banco inicial de 20 perguntas"
    );
    assert_eq!(report.6, 1, "o seed fica marcado uma única vez");

    assert_eq!(
        report.7,
        vec![
            "ai_attempts",
            "ai_receipts",
            "ai_reports",
            "ai_requests",
            "ai_settings",
            "ai_stages",
            "answer_slots",
            "article_artifacts",
            "article_branches",
            "article_receipts",
            "article_reports",
            "article_requests",
            "article_sessions",
            "article_turns",
            "article_versions",
            "command_receipts",
            "confirmations",
            "content_seeds",
            "evaluation_families",
            "operations",
            "practice_preferences",
            "practice_reports",
            "practices",
            "preparation_captures",
            "preparations",
            "questions",
            "recordings",
            "reviews",
            "rewrite_audio",
            "rewrites",
            "session_questions",
            "sessions",
            "stories",
            "story_versions",
            "transcripts",
        ],
        "as tabelas das fases 2, 3 e 4 e dos artigos devem existir"
    );
    assert_eq!(
        report.9, 1,
        "deve existir um índice único de sessão em curso"
    );
    assert!(report.10.is_empty(), "não pode haver violações de FK");

    assert!(
        report
            .8
            .iter()
            .any(|name| name == "transcripts_immutable_update"),
        "as transcrições não podem ser alteradas: {:?}",
        report.8
    );
    assert!(
        report
            .8
            .iter()
            .any(|name| name == "confirmations_immutable_update"),
        "as confirmações não podem ser alteradas: {:?}",
        report.8
    );
    assert!(
        report
            .8
            .iter()
            .any(|name| name == "session_questions_content_immutable"),
        "o conteúdo copiado das perguntas não pode ser alterado: {:?}",
        report.8
    );
}

#[test]
fn failed_migration_rolls_back() {
    let (_directory, path) = temp_database_path();

    // Base de versão 0 com um marcador que não pode ser perdido pela migração.
    {
        let connection = open_raw(&path);
        connection
            .execute_batch(
                "CREATE TABLE sentinel (id TEXT PRIMARY KEY NOT NULL);
                 INSERT INTO sentinel (id) VALUES ('guardado');",
            )
            .expect("preparar a base de versão 0");
        assert_eq!(user_version(&connection), 0);
    }

    // Erro injetado: a primeira instrução é válida e a segunda falha, o que
    // prova que o trabalho já feito dentro da transação é revertido.
    let broken_migration = "CREATE TABLE extra (id TEXT PRIMARY KEY NOT NULL);
         INSERT INTO tabela_inexistente (id) VALUES ('x');";

    let error = Database::open_with_schema(path.clone(), broken_migration)
        .expect_err("a migração injetada deve falhar");
    assert_eq!(error.code, error_code::DATABASE_UNAVAILABLE);
    assert!(
        error.retryable,
        "uma falha de migração deve ser recuperável"
    );

    let connection = open_raw(&path);
    assert_eq!(
        user_version(&connection),
        0,
        "a versão só muda depois de a migração inteira ter sucesso"
    );
    assert!(
        table_exists(&connection, "sentinel"),
        "o esquema existente fica"
    );
    assert!(
        !table_exists(&connection, "extra"),
        "as instruções já executadas na transação são revertidas"
    );
    let kept: i64 = connection
        .query_row("SELECT count(*) FROM sentinel", [], |row| row.get(0))
        .expect("os dados existentes continuam legíveis");
    assert_eq!(kept, 1, "os dados existentes não são apagados");
    assert!(!table_exists(&connection, "questions"));
}

#[test]
fn future_version_is_not_modified() {
    let (_directory, path) = temp_database_path();

    {
        let connection = open_raw(&path);
        connection
            .execute_batch(
                "PRAGMA user_version = 99;
                 CREATE TABLE sentinel (id TEXT PRIMARY KEY NOT NULL);
                 INSERT INTO sentinel (id) VALUES ('futuro');",
            )
            .expect("preparar uma base de versão futura");
    }

    let error = Database::open(path.clone()).expect_err("uma base futura não deve abrir");
    assert_eq!(error.code, error_code::DATABASE_VERSION_UNSUPPORTED);

    let connection = open_raw(&path);
    assert_eq!(user_version(&connection), 99, "a versão não é reescrita");
    assert!(table_exists(&connection, "sentinel"), "nada é recriado");
    assert!(
        !table_exists(&connection, "sessions"),
        "uma versão antiga não escreve um esquema desconhecido"
    );
    assert!(
        !table_exists(&connection, "content_seeds"),
        "uma versão antiga não aplica o seed"
    );
    let kept: i64 = connection
        .query_row("SELECT count(*) FROM sentinel", [], |row| row.get(0))
        .expect("os dados futuros continuam intactos");
    assert_eq!(kept, 1);
}

#[test]
fn foreign_keys_reject_cross_recording_sources() {
    let (_directory, path) = temp_database_path();
    let database = Database::open(path).expect("uma base nova deve abrir");

    database
        .with_connection(|connection| {
            seed_two_questions(connection);

            // Uma revisão guardada para a gravação B, para isolar a verificação
            // seguinte à FK composta da transcrição de origem.
            connection
                .execute(
                    "INSERT INTO reviews (recording_id, version, source_transcript_id, origin, text, saved_at)
                     VALUES ('rec-b', 1, 'tr-b', 'automatic', 'texto', '2026-11-02T09:30:00Z')",
                    [],
                )
                .expect("a revisão da gravação B é válida");

            // Uma revisão da gravação A não pode citar a transcrição da B.
            let cross_transcript = connection.execute(
                "INSERT INTO reviews (recording_id, version, source_transcript_id, origin, text, saved_at)
                 VALUES ('rec-a', 2, 'tr-b', 'automatic', 'texto', '2026-11-02T09:30:00Z')",
                [],
            );
            let error = cross_transcript.expect_err("a transcrição de outra gravação é recusada");
            assert!(
                error.to_string().contains("FOREIGN KEY"),
                "esperava uma violação de FK, recebi: {error}"
            );

            // A seleção de uma pergunta não pode apontar para a gravação de outra.
            let cross_selection = connection.execute(
                "UPDATE session_questions SET selected_recording_id = 'rec-b', selection_version = 1
                 WHERE id = 'sq-a'",
                [],
            );
            let error = cross_selection.expect_err("a gravação de outra pergunta é recusada");
            assert!(
                error.to_string().contains("FOREIGN KEY"),
                "esperava uma violação de FK, recebi: {error}"
            );

            // Uma confirmação de B não pode citar a transcrição de A, mesmo com
            // a revisão e a gravação corretas.
            let cross_confirmation = connection.execute(
                "INSERT INTO confirmations
                   (id, session_question_id, recording_id, review_version, source_transcript_id, origin, text, confirmed_at)
                 VALUES ('c-b', 'sq-b', 'rec-b', 1, 'tr-a', 'automatic', 'texto', '2026-11-02T09:31:00Z')",
                [],
            );
            let error = cross_confirmation.expect_err("a transcrição de outra pergunta é recusada");
            assert!(
                error.to_string().contains("FOREIGN KEY"),
                "esperava uma violação de FK, recebi: {error}"
            );

            // A confirmação de B também não pode usar uma versão de revisão que
            // não existe para a sua gravação.
            let cross_review = connection.execute(
                "INSERT INTO confirmations
                   (id, session_question_id, recording_id, review_version, source_transcript_id, origin, text, confirmed_at)
                 VALUES ('c-b2', 'sq-b', 'rec-b', 2, NULL, 'manual_explicit', 'texto', '2026-11-02T09:31:00Z')",
                [],
            );
            let error = cross_review.expect_err("a revisão de outra versão é recusada");
            assert!(
                error.to_string().contains("FOREIGN KEY"),
                "esperava uma violação de FK, recebi: {error}"
            );

            // O caso legítimo: a gravação, a revisão e a transcrição da própria
            // pergunta são aceites.
            connection
                .execute(
                    "UPDATE session_questions SET selected_recording_id = 'rec-a', selection_version = 1
                     WHERE id = 'sq-a'",
                    [],
                )
                .expect("a gravação da mesma pergunta deve ser selecionável");
            connection
                .execute(
                    "INSERT INTO confirmations
                       (id, session_question_id, recording_id, review_version, source_transcript_id, origin, text, confirmed_at)
                     VALUES ('c-b3', 'sq-b', 'rec-b', 1, 'tr-b', 'automatic', 'texto', '2026-11-02T09:32:00Z')",
                    [],
                )
                .expect("uma confirmação coerente deve ser aceite");

            Ok(())
        })
        .expect("as invariantes de FK são verificadas");
}

#[test]
fn triggers_protect_confirmed_text_and_transcripts() {
    let (_directory, path) = temp_database_path();
    let database = Database::open(path).expect("uma base nova deve abrir");

    database
        .with_connection(|connection| {
            seed_two_questions(connection);

            expect_trigger_refusal(
                connection.execute("UPDATE transcripts SET text = 'alterado' WHERE id = 'tr-a'", []),
                "alterar uma transcrição automática",
            );
            expect_trigger_refusal(
                connection.execute("DELETE FROM transcripts WHERE id = 'tr-a'", []),
                "eliminar uma transcrição automática",
            );

            expect_trigger_refusal(
                connection.execute("UPDATE confirmations SET text = 'alterado' WHERE id = 'c-a'", []),
                "alterar o texto confirmado",
            );
            expect_trigger_refusal(
                connection.execute("DELETE FROM confirmations WHERE id = 'c-a'", []),
                "eliminar uma confirmação",
            );

            expect_trigger_refusal(
                connection.execute(
                    "UPDATE reviews SET text = 'alterado' WHERE recording_id = 'rec-a'",
                    [],
                ),
                "reescrever uma versão de revisão",
            );
            expect_trigger_refusal(
                connection.execute("DELETE FROM reviews WHERE recording_id = 'rec-a'", []),
                "eliminar uma versão de revisão",
            );

            expect_trigger_refusal(
                connection.execute(
                    "UPDATE session_questions SET text = 'alterado' WHERE id = 'sq-a'",
                    [],
                ),
                "alterar o texto copiado da pergunta",
            );
            expect_trigger_refusal(
                connection.execute(
                    "UPDATE session_questions SET criteria_json = '[]' WHERE id = 'sq-a'",
                    [],
                ),
                "alterar os critérios copiados da pergunta",
            );

            // A seleção e o estado da gravação continuam mutáveis antes do fim.
            connection
                .execute(
                    "UPDATE session_questions SET selected_recording_id = 'rec-a', selection_version = 1
                     WHERE id = 'sq-a'",
                    [],
                )
                .expect("selecionar a gravação da pergunta deve ser permitido");
            connection
                .execute("UPDATE recordings SET state = 'publish_pending' WHERE id = 'rec-b'", [])
                .expect("o estado de uma gravação deve poder mudar");

            Ok(())
        })
        .expect("as invariantes de imutabilidade são verificadas");
}

#[test]
fn database_is_clone_send_and_sync() {
    // A ligação é partilhada entre threads e clones do serviço; o mutex interno
    // é o que torna isto possível sem expor o `Connection`.
    fn assert_shareable<T: Clone + Send + Sync>() {}
    assert_shareable::<Database>();
}

#[test]
fn only_one_session_can_be_in_progress() {
    let (_directory, path) = temp_database_path();
    let database = Database::open(path).expect("uma base nova deve abrir");

    database
        .with_connection(|connection| {
            seed_two_questions(connection);
            let duplicate = connection.execute(
                "INSERT INTO sessions (id, category, question_count, position, status, mode, version, created_at, completed_at)
                 VALUES ('s-2', 'problem_solving', 1, 1, 'in_progress', 'local_v2', 1,
                         '2026-11-02T10:00:00Z', NULL)",
                [],
            );
            let error = duplicate.expect_err("só pode existir uma sessão em curso");
            assert!(
                error.to_string().contains("UNIQUE"),
                "esperava uma violação de unicidade, recebi: {error}"
            );
            Ok(())
        })
        .expect("a unicidade da sessão em curso é verificada");
}

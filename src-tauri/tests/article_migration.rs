//! Migrações da conversa sobre artigos (v4–v6): todos os caminhos, versão
//! futura e rollback.

use std::path::{Path, PathBuf};

use language_assistant_tutor::db::{
    Database, DATABASE_VERSION, MIGRATION_V1, MIGRATION_V2, MIGRATION_V3, MIGRATION_V4,
    MIGRATION_V5, MIGRATION_V6,
};
use rusqlite::Connection;

fn temp_path() -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().expect("diretório temporário");
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

/// Aplica as migrações 1..=`upto` (até à v6) e semeia conteúdo STAR.
fn apply_upto(path: &Path, upto: u32) {
    let connection = raw(path);
    connection
        .execute_batch("PRAGMA foreign_keys=OFF;")
        .expect("desativar FKs");
    for (n, sql) in [
        MIGRATION_V1,
        MIGRATION_V2,
        MIGRATION_V3,
        MIGRATION_V4,
        MIGRATION_V5,
        MIGRATION_V6,
    ]
    .iter()
    .enumerate()
    {
        if (n as u32) < upto {
            connection.execute_batch(sql).expect("aplicar migração");
        }
    }
    connection
        .execute_batch(
            "INSERT INTO questions VALUES ('q','Question','failure_learning','[]','[]',1,'now','now');
             INSERT INTO sessions VALUES ('s',NULL,1,1,'completed','local_v2',1,'now','done');
             INSERT INTO session_questions VALUES ('a','s','q',1,1,'Question','failure_learning','[]','[]',NULL,0);",
        )
        .expect("semear conteúdo STAR");
    connection
        .execute_batch(&format!("PRAGMA user_version={upto};"))
        .expect("fixar versão");
}

fn star_dump(connection: &Connection) -> String {
    let mut out = String::new();
    // O seed de perguntas do `open` acrescenta linhas próprias; compara-se só a semente.
    for (table, id) in [
        ("questions", "q"),
        ("sessions", "s"),
        ("session_questions", "a"),
    ] {
        let mut statement = connection
            .prepare(&format!("SELECT * FROM {table} WHERE id = '{id}'"))
            .expect("preparar");
        let columns = statement.column_count();
        let rows = statement
            .query_map([], |row| {
                let mut line = String::new();
                for index in 0..columns {
                    let value: rusqlite::types::Value = row.get(index)?;
                    line.push_str(&format!("{value:?}|"));
                }
                Ok(line)
            })
            .expect("consultar");
        for row in rows {
            out.push_str(&row.expect("linha"));
            out.push('\n');
        }
    }
    out
}

fn has_failed_from(connection: &Connection) -> bool {
    connection
        .query_row(
            "SELECT count(*) FROM pragma_table_info('article_sessions') WHERE name = 'failed_from'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("pragma_table_info")
        > 0
}

const ARTICLE_TABLES: [&str; 8] = [
    "article_sessions",
    "article_branches",
    "article_turns",
    "article_versions",
    "article_requests",
    "article_reports",
    "article_artifacts",
    "article_receipts",
];

#[test]
fn every_previous_version_migrates_to_v6_keeping_star_content() {
    assert_eq!(DATABASE_VERSION, 6);
    for upto in 1..=5u32 {
        let (_directory, path) = temp_path();
        apply_upto(&path, upto);
        let before = star_dump(&raw(&path));
        let database = Database::open(path.clone()).expect("migrar para v6");
        drop(database);
        let connection = raw(&path);
        assert_eq!(user_version(&connection), 6, "v{upto}→v6");
        assert!(has_failed_from(&connection), "failed_from após v{upto}");
        for table in ARTICLE_TABLES {
            assert!(table_exists(&connection, table), "{table} após v{upto}");
        }
        assert!(
            table_exists(&connection, "article_preferences"),
            "article_preferences após v{upto}"
        );
        assert_eq!(star_dump(&connection), before, "STAR intacto após v{upto}");
        let violations: i64 = connection
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })
            .expect("foreign_key_check");
        assert_eq!(violations, 0);
    }
    // v0 → v6.
    let (_directory, path) = temp_path();
    drop(Database::open(path.clone()).expect("v0→v6"));
    let connection = raw(&path);
    assert_eq!(user_version(&connection), 6);
    assert!(has_failed_from(&connection));
    assert!(table_exists(&connection, "article_sessions"));
    assert!(table_exists(&connection, "article_preferences"));
}

#[test]
fn future_version_is_refused_without_write() {
    let (_directory, path) = temp_path();
    raw(&path)
        .execute_batch(
            "PRAGMA user_version = 99;
             CREATE TABLE sentinel (id TEXT PRIMARY KEY NOT NULL);",
        )
        .expect("base futura");
    let error = Database::open(path.clone()).expect_err("versão futura");
    assert_eq!(error.code, "DATABASE_VERSION_UNSUPPORTED");
    let connection = raw(&path);
    assert_eq!(user_version(&connection), 99);
    assert!(!table_exists(&connection, "article_sessions"));
}

#[test]
fn invalid_v4_migration_rolls_back_to_v3() {
    let (_directory, path) = temp_path();
    apply_upto(&path, 3);
    let before = star_dump(&raw(&path));
    let broken = "CREATE TABLE article_sessions (id TEXT PRIMARY KEY NOT NULL);
                  INSERT INTO tabela_inexistente (id) VALUES (1);";
    let error = Database::open_with_migrations(
        path.clone(),
        MIGRATION_V1,
        MIGRATION_V2,
        MIGRATION_V3,
        broken,
        MIGRATION_V5,
        MIGRATION_V6,
    )
    .expect_err("v4 inválida");
    assert!(error.retryable);
    let connection = raw(&path);
    assert_eq!(user_version(&connection), 3);
    assert!(!table_exists(&connection, "article_sessions"));
    assert_eq!(star_dump(&connection), before);
}

#[test]
fn invalid_v5_migration_rolls_back_to_v4_keeping_article_data() {
    let (_directory, path) = temp_path();
    apply_upto(&path, 4);
    let broken = "ALTER TABLE article_sessions ADD COLUMN failed_from TEXT;
                  INSERT INTO tabela_inexistente (id) VALUES (1);";
    let error = Database::open_with_migrations(
        path.clone(),
        MIGRATION_V1,
        MIGRATION_V2,
        MIGRATION_V3,
        MIGRATION_V4,
        broken,
        MIGRATION_V6,
    )
    .expect_err("v5 inválida");
    assert!(error.retryable);
    let connection = raw(&path);
    assert_eq!(user_version(&connection), 4);
    assert!(table_exists(&connection, "article_sessions"));
    assert!(!has_failed_from(&connection));
}

#[test]
fn preferences_default_to_autoplay_at_normal_speed_and_keep_star_data() {
    let (_directory, path) = temp_path();
    apply_upto(&path, 5);
    let before = star_dump(&raw(&path));
    let database = Database::open(path.clone()).expect("migrar para v6");
    drop(database);
    let connection = raw(&path);
    assert_eq!(user_version(&connection), 6, "v5→v6");
    let (autoplay, speed): (i64, i64) = connection
        .query_row(
            "SELECT autoplay, speed_x10 FROM article_preferences WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("ler as preferências");
    assert_eq!(autoplay, 1, "reprodução automática por omissão");
    assert_eq!(speed, 10, "velocidade 1.0× por omissão");
    assert_eq!(star_dump(&connection), before, "STAR intacto após v6");
    for table in ARTICLE_TABLES {
        assert!(table_exists(&connection, table), "{table} após v6");
    }
    assert!(table_exists(&connection, "article_preferences"));
}

#[test]
fn invalid_v6_migration_rolls_back_to_v5_keeping_star_and_article_data() {
    let (_directory, path) = temp_path();
    apply_upto(&path, 5);
    // Uma conversa de artigo nua é válida: `active_branch_id` não tem chave
    // estrangeira, pelo que uma linha sem ramo passa.
    raw(&path)
        .execute_batch(
            "INSERT INTO article_sessions
               (id, title, article_text, state, version, generation,
                active_branch_id, created_at, updated_at)
             VALUES ('as1', NULL, 'Article.', 'draft', 1, 1, 'branch-1', 'now', 'now');",
        )
        .expect("semear a sessão de artigo");
    let before = star_dump(&raw(&path));
    let broken = "CREATE TABLE article_preferences (id INTEGER PRIMARY KEY CHECK (id = 1));
                  INSERT INTO tabela_inexistente (id) VALUES (1);";
    let error = Database::open_with_migrations(
        path.clone(),
        MIGRATION_V1,
        MIGRATION_V2,
        MIGRATION_V3,
        MIGRATION_V4,
        MIGRATION_V5,
        broken,
    )
    .expect_err("v6 inválida");
    assert!(error.retryable);
    let connection = raw(&path);
    assert_eq!(user_version(&connection), 5, "a versão só muda com sucesso");
    assert_eq!(star_dump(&connection), before, "STAR intacto após rollback");
    let articles: i64 = connection
        .query_row(
            "SELECT count(*) FROM article_sessions WHERE id = 'as1'",
            [],
            |row| row.get(0),
        )
        .expect("contar a sessão de artigo");
    assert_eq!(articles, 1, "a conversa de artigo sobrevive ao rollback");
    assert!(
        !table_exists(&connection, "article_preferences"),
        "a tabela criada antes da falha é revertida"
    );
}

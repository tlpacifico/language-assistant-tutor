//! Migração v4 (conversa sobre artigos): todos os caminhos, versão futura e rollback.

use std::path::{Path, PathBuf};

use language_assistant_tutor::db::{
    Database, DATABASE_VERSION, MIGRATION_V1, MIGRATION_V2, MIGRATION_V3, MIGRATION_V4,
    MIGRATION_V5,
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

/// Aplica as migrações 1..=`upto` (até à v4) e semeia conteúdo STAR.
fn apply_upto(path: &Path, upto: u32) {
    let connection = raw(path);
    connection
        .execute_batch("PRAGMA foreign_keys=OFF;")
        .expect("desativar FKs");
    for (n, sql) in [MIGRATION_V1, MIGRATION_V2, MIGRATION_V3, MIGRATION_V4]
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
fn every_previous_version_migrates_to_v5_keeping_star_content() {
    assert_eq!(DATABASE_VERSION, 5);
    for upto in 1..=4u32 {
        let (_directory, path) = temp_path();
        apply_upto(&path, upto);
        let before = star_dump(&raw(&path));
        let database = Database::open(path.clone()).expect("migrar para v4");
        drop(database);
        let connection = raw(&path);
        assert_eq!(user_version(&connection), 5, "v{upto}→v5");
        assert!(has_failed_from(&connection), "failed_from após v{upto}");
        for table in ARTICLE_TABLES {
            assert!(table_exists(&connection, table), "{table} após v{upto}");
        }
        assert_eq!(star_dump(&connection), before, "STAR intacto após v{upto}");
        let violations: i64 = connection
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })
            .expect("foreign_key_check");
        assert_eq!(violations, 0);
    }
    // v0 → v5.
    let (_directory, path) = temp_path();
    drop(Database::open(path.clone()).expect("v0→v5"));
    let connection = raw(&path);
    assert_eq!(user_version(&connection), 5);
    assert!(has_failed_from(&connection));
    assert!(table_exists(&connection, "article_sessions"));
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
    )
    .expect_err("v5 inválida");
    assert!(error.retryable);
    let connection = raw(&path);
    assert_eq!(user_version(&connection), 4);
    assert!(table_exists(&connection, "article_sessions"));
    assert!(!has_failed_from(&connection));
}

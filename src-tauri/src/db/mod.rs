//! Ligação SQLite, migrações e execução síncrona protegida.
//!
//! A base de dados é a autoridade das sessões da fase 2. Há uma única ligação
//! `rusqlite::Connection` dentro de `Database`, partilhada por um `Mutex` num
//! `Arc`; todas as operações são síncronas e correm em `spawn_blocking` a partir
//! dos comandos. Um guard nunca atravessa um `await`.
//!
//! Na abertura, `user_version` é lida antes de qualquer alteração persistente:
//! uma base criada por uma versão mais recente é recusada sem escrita. Só depois
//! se ativam os pragmas e, quando a versão é 0, se aplica a migração dentro de
//! uma transação. Uma falha reverte tudo; a recuperação nunca apaga nem recria a
//! base.

use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::domain::{error_code, AppError};
use crate::interview::Result;

/// Versão do esquema escrita em `PRAGMA user_version`.
pub const DATABASE_VERSION: u32 = 1;

/// Esquema inicial da fase 2, aplicado numa única transação.
pub const MIGRATION_V1: &str = include_str!("migrations/001_guided_interview.sql");

/// Ligação SQLite partilhável. Os clones referem a mesma ligação protegida.
#[derive(Clone)]
pub struct Database {
    connection: Arc<Mutex<Connection>>,
}

impl Database {
    /// Abre (ou cria) a base de dados e aplica a migração em falta.
    pub fn open(path: PathBuf) -> Result<Self> {
        Self::open_with_schema(path, MIGRATION_V1)
    }

    /// Abre a base com um esquema explícito.
    ///
    /// A abertura real usa sempre [`MIGRATION_V1`]; os testes de rollback
    /// injetam um esquema inválido para provar que a transação reverte.
    pub fn open_with_schema(path: PathBuf, schema: &str) -> Result<Self> {
        let mut connection = Connection::open(&path).map_err(open_failed)?;

        // A versão é lida antes de qualquer alteração persistente.
        let version = read_user_version(&connection)?;
        if version > DATABASE_VERSION {
            return Err(AppError::new(
                error_code::DATABASE_VERSION_UNSUPPORTED,
                format!(
                    "A base de dados foi criada por uma versão mais recente da aplicação \
                     (versão {version}); esta aplicação suporta a versão {DATABASE_VERSION}. \
                     Atualize a aplicação para a abrir."
                ),
                false,
            ));
        }

        configure(&connection)?;

        if version < DATABASE_VERSION {
            migrate(&mut connection, schema)?;
        }

        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    /// Executa uma operação síncrona com a ligação protegida.
    ///
    /// Nunca deve ser chamado diretamente num comando async: a chamada corre
    /// dentro de `spawn_blocking` e o guard não atravessa um `await`.
    pub fn with_connection<T>(&self, f: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T> {
        let mut guard = self.connection.lock().map_err(|_| {
            AppError::new(
                error_code::DATABASE_UNAVAILABLE,
                "A base de dados não está disponível. Repita a abertura.",
                true,
            )
        })?;
        f(&mut guard)
    }
}

impl fmt::Debug for Database {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Database").finish_non_exhaustive()
    }
}

fn read_user_version(connection: &Connection) -> Result<u32> {
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(open_failed)?;
    Ok(u32::try_from(version).unwrap_or(u32::MAX))
}

fn configure(connection: &Connection) -> Result<()> {
    connection
        .execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 3000;
             PRAGMA synchronous = FULL;
             PRAGMA journal_mode = DELETE;",
        )
        .map_err(open_failed)
}

fn migrate(connection: &mut Connection, schema: &str) -> Result<()> {
    let transaction = connection.transaction().map_err(migration_failed)?;
    transaction
        .execute_batch(schema)
        .map_err(migration_failed)?;
    // A versão só é escrita no fim, dentro da mesma transação: uma falha
    // anterior reverte o esquema, os dados e a versão.
    transaction
        .execute_batch(&format!("PRAGMA user_version = {DATABASE_VERSION};"))
        .map_err(migration_failed)?;
    transaction.commit().map_err(migration_failed)?;
    Ok(())
}

fn open_failed(error: rusqlite::Error) -> AppError {
    AppError::new(
        error_code::DATABASE_UNAVAILABLE,
        format!("Não foi possível abrir a base de dados: {error}"),
        true,
    )
}

fn migration_failed(error: rusqlite::Error) -> AppError {
    AppError::new(
        error_code::DATABASE_UNAVAILABLE,
        format!("Não foi possível preparar a base de dados; a migração foi revertida: {error}"),
        true,
    )
}

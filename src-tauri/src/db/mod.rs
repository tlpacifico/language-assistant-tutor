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
//! base. Depois de a migração estar aplicada, `open` garante o banco inicial de
//! perguntas: a marca `questions-v1` em `content_seeds` torna o seed único.
//! `questions` reúne o banco editável, `sessions` a criação atómica das
//! entrevistas guiadas, com as cópias fixas das perguntas, os avanços e os
//! recibos dos pedidos, `answers` as gravações, publicações, transcrições
//! automáticas, revisões append-only e confirmações imutáveis, e `operations` o
//! registo dos pedidos ao auxiliar, do estado `running` ao encerramento.

use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::domain::{error_code, AppError};
use crate::interview::Result;

pub(crate) mod ai;
mod answers;
mod articles;
mod operations;
mod outputs;
mod practice;
mod questions;
mod rewrites;
mod sessions;
mod stories;

pub use ai::{CommandIdentity, CommandIdentityLocation};
pub(crate) use answers::PendingRecording;
pub use outputs::{RewriteAudioRow, RewriteAudioState, RewriteVoice};
pub use questions::*;
pub use rewrites::ExerciseSources;
pub use stories::{missing_fields, validate_story_input, PreparationReservation};

/// Versão do esquema escrita em `PRAGMA user_version`.
pub const DATABASE_VERSION: u32 = 5;

/// Esquema inicial da fase 2, aplicado numa única transação.
pub const MIGRATION_V1: &str = include_str!("migrations/001_guided_interview.sql");

/// Migração da fase 3 (tutor de IA).
pub const MIGRATION_V2: &str = include_str!("migrations/002_ai_tutor.sql");

/// Migração da fase 4 (melhorar e repetir).
pub const MIGRATION_V3: &str = include_str!("migrations/003_improve_repeat.sql");

/// Migração da conversa sobre artigos.
pub const MIGRATION_V4: &str = include_str!("migrations/004_article_conversation.sql");

/// Origem da falha recuperável da conversa sobre artigos.
pub const MIGRATION_V5: &str = include_str!("migrations/005_article_failed_from.sql");

/// Ligação SQLite partilhável. Os clones referem a mesma ligação protegida.
#[derive(Clone)]
pub struct Database {
    connection: Arc<Mutex<Connection>>,
}

impl Database {
    /// Abre (ou cria) a base de dados, aplica a migração em falta e garante o
    /// banco inicial de perguntas.
    ///
    /// O seed é idempotente: numa base já semeada, ou já editada pelo
    /// utilizador, nada é escrito. Uma falha do seed devolve o erro e reverte
    /// apenas o seed; o esquema migrado mantém-se.
    pub fn open(path: PathBuf) -> Result<Self> {
        let database = Self::open_with_schema(path, MIGRATION_V1)?;
        database.ensure_question_seed()?;
        Ok(database)
    }

    /// Abre a base com um esquema explícito, sem aplicar o banco de perguntas.
    ///
    /// A abertura real usa sempre [`MIGRATION_V1`], [`MIGRATION_V2`] e
    /// [`MIGRATION_V3`]; os testes de rollback injetam um esquema inválido para
    /// provar que a transação reverte.
    pub fn open_with_schema(path: PathBuf, schema: &str) -> Result<Self> {
        Self::open_with_migrations(
            path,
            schema,
            MIGRATION_V2,
            MIGRATION_V3,
            MIGRATION_V4,
            MIGRATION_V5,
        )
    }

    /// Abre a base com as cinco migrações explícitas, sem aplicar o banco de perguntas.
    ///
    /// Suporta qualquer versão anterior até v5, cada abertura numa única
    /// transação. A versão é lida antes de qualquer escrita; uma versão futura é
    /// recusada sem alterar nada. As chaves estrangeiras ficam desativadas
    /// apenas durante a transação de migração e voltam a ativar-se em todos os
    /// caminhos; `foreign_key_check` é verificada antes do commit. Uma falha
    /// reverte dados e `user_version`.
    pub fn open_with_migrations(
        path: PathBuf,
        v1: &str,
        v2: &str,
        v3: &str,
        v4: &str,
        v5: &str,
    ) -> Result<Self> {
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
            connection
                .execute_batch("PRAGMA foreign_keys=OFF;")
                .map_err(migration_failed)?;
            let result = (|| {
                let tx = connection.transaction().map_err(migration_failed)?;
                if version == 0 {
                    tx.execute_batch(v1).map_err(migration_failed)?;
                }
                if version <= 1 {
                    tx.execute_batch(v2).map_err(migration_failed)?;
                }
                if version <= 2 {
                    tx.execute_batch(v3).map_err(migration_failed)?;
                }
                if version <= 3 {
                    tx.execute_batch(v4).map_err(migration_failed)?;
                }
                if version <= 4 {
                    tx.execute_batch(v5).map_err(migration_failed)?;
                }
                let bad = tx
                    .prepare("PRAGMA foreign_key_check")
                    .map_err(migration_failed)?
                    .exists([])
                    .map_err(migration_failed)?;
                if bad {
                    return Err(AppError::new(
                        error_code::DATABASE_UNAVAILABLE,
                        "A migração contém referências inválidas.",
                        true,
                    ));
                }
                tx.execute_batch(&format!("PRAGMA user_version={DATABASE_VERSION};"))
                    .map_err(migration_failed)?;
                tx.commit().map_err(migration_failed)
            })();
            connection
                .execute_batch("PRAGMA foreign_keys=ON;")
                .map_err(migration_failed)?;
            result?;
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

//! Operações do auxiliar registadas na base de dados.
//!
//! A spec exige que cada operação tenha «identidade, alvo, geração, estado,
//! erro e tempos» (§8) e que um trabalho interrompido passe a estado recuperável
//! (§9). Por isso a linha é criada em `running` quando o pedido é submetido ao
//! auxiliar e encerrada com o resultado (`completed`/`no_speech`, escrito na
//! mesma transação da transcrição), com uma falha explícita ou como
//! `interrupted` quando a aplicação fecha ou é encerrada abruptamente.
//!
//! Sem a linha em `running`, [`Database::interrupt_running_operations`] nunca
//! encontrava nada e o relatório de recuperação ficava sempre vazio.

use rusqlite::{params, TransactionBehavior};
use serde_json::Value;

use super::questions::utc_now;
use super::Database;
use crate::domain::{error_code, AppError, OperationKind};
use crate::interview::{Id, OperationTarget, Result};

impl Database {
    /// Regista um pedido ao auxiliar como `running`, antes de o submeter.
    ///
    /// O alvo e a geração ficam fixados na linha: um resultado tardio é
    /// comparado com estes valores por [`Database::append_transcript`]. Um
    /// `worker_generation` zero significa que não existe encarnação do auxiliar
    /// a que o pedido pertença e é guardado como nulo.
    pub fn begin_operation(
        &self,
        kind: OperationKind,
        target: &OperationTarget,
        configuration: Option<&Value>,
    ) -> Result<()> {
        let kind = match kind {
            OperationKind::Transcribe => "transcribe",
            OperationKind::Synthesize => "synthesize",
        };
        let generation = match target.worker_generation {
            0 => None,
            value => Some(i64::try_from(value).map_err(|_| invalid_generation())?),
        };
        let config_json = configuration
            .map(|value| serde_json::to_string(value).map_err(configuration_failed))
            .transpose()?;
        let now = utc_now();
        self.with_connection(|connection| {
            connection
                .execute(
                    "INSERT INTO operations
                       (id, kind, state, session_id, session_question_id,
                        recording_id, worker_generation, source_config_json,
                        error_json, created_at, finished_at)
                     VALUES (?1, ?2, 'running', ?3, ?4, ?5, ?6, ?7, NULL, ?8, NULL)",
                    params![
                        target.operation_id,
                        kind,
                        target.session_id,
                        target.session_question_id,
                        target.recording_id,
                        generation,
                        config_json,
                        now
                    ],
                )
                .map_err(write_failed)?;
            Ok(())
        })
    }

    /// Encerra uma operação em curso como concluída.
    ///
    /// Uma linha já encerrada não é reescrita: repetir o encerramento é seguro e
    /// nunca muda o estado de outra operação.
    pub fn complete_operation(&self, operation_id: &str) -> Result<()> {
        self.close_operation(operation_id, "completed")
    }

    /// Encerra uma operação em curso sem resultado, ficando recuperável.
    pub fn interrupt_operation(&self, operation_id: &str) -> Result<()> {
        self.close_operation(operation_id, "interrupted")
    }

    /// Encerra uma operação em curso como falhada e guarda o erro observado.
    pub fn fail_operation(&self, operation_id: &str, error: &AppError) -> Result<()> {
        let error_json = serde_json::to_string(error).map_err(configuration_failed)?;
        let now = utc_now();
        self.with_connection(|connection| {
            connection
                .execute(
                    "UPDATE operations
                        SET state = 'failed', error_json = ?2, finished_at = ?3
                      WHERE id = ?1 AND state = 'running'",
                    params![operation_id, error_json, now],
                )
                .map_err(write_failed)?;
            Ok(())
        })
    }

    /// Encerra todas as operações ainda em curso, devolvendo os identificadores.
    ///
    /// No arranque nada está a correr: uma linha `running` só pode vir de um
    /// encerramento abrupto e fica `interrupted`, sem duplicar trabalho.
    pub(crate) fn interrupt_running_operations(&self) -> Result<Vec<Id>> {
        let now = utc_now();
        self.with_connection(|connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(write_failed)?;
            let ids: Vec<Id> = {
                let mut statement = transaction
                    .prepare(
                        "SELECT id FROM operations
                          WHERE state = 'running'
                          ORDER BY created_at, rowid",
                    )
                    .map_err(read_failed)?;
                let mut rows = statement.query([]).map_err(read_failed)?;
                let mut ids = Vec::new();
                while let Some(row) = rows.next().map_err(read_failed)? {
                    ids.push(row.get("id").map_err(read_failed)?);
                }
                ids
            };
            for id in &ids {
                transaction
                    .execute(
                        "UPDATE operations
                            SET state = 'interrupted', finished_at = ?2
                          WHERE id = ?1 AND state = 'running'",
                        params![id, now],
                    )
                    .map_err(write_failed)?;
            }
            transaction.commit().map_err(write_failed)?;
            Ok(ids)
        })
    }

    fn close_operation(&self, operation_id: &str, state: &str) -> Result<()> {
        let now = utc_now();
        self.with_connection(|connection| {
            connection
                .execute(
                    "UPDATE operations
                        SET state = ?2, error_json = NULL, finished_at = ?3
                      WHERE id = ?1 AND state = 'running'",
                    params![operation_id, state, now],
                )
                .map_err(write_failed)?;
            Ok(())
        })
    }
}

fn invalid_generation() -> AppError {
    AppError::new(
        error_code::INVALID_INPUT,
        "A geração do auxiliar indicada no pedido não é válida.",
        false,
    )
}

fn configuration_failed(error: serde_json::Error) -> AppError {
    AppError::new(
        error_code::INVALID_INPUT,
        format!("Não foi possível preparar a configuração da operação: {error}."),
        false,
    )
}

fn read_failed(error: rusqlite::Error) -> AppError {
    AppError::new(
        error_code::DATABASE_UNAVAILABLE,
        format!("Não foi possível ler as operações guardadas: {error}"),
        false,
    )
}

fn write_failed(error: rusqlite::Error) -> AppError {
    AppError::new(
        error_code::STORAGE_WRITE_FAILED,
        format!("Não foi possível guardar a operação; nenhuma alteração foi aplicada: {error}"),
        true,
    )
}

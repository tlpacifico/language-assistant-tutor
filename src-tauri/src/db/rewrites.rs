//! Fontes de exercício para reformulação e avaliação de prática (fase 4).
//!
//! Nesta tarefa só a leitura das fontes: [`Database::load_exercise_sources`]
//! resolve a pergunta fixa, os critérios e as confirmações de uma origem
//! confirmada, sem chamar o resolvedor de feedback da entrevista (que recusa
//! originais locais) e sem incluir `Preparation`/`Story`. A persistência de
//! pedidos e resultados pertence à tarefa 5, que estende este ficheiro.

use rusqlite::{Connection, OptionalExtension};

use super::answers::load_confirmation;
use super::questions::json_list;
use super::Database;
use crate::ai::{AiSource, AiSources, SCHEMA};
use crate::domain::{error_code, AppError};
use crate::interview::Result;
use crate::practice::ResponseOrigin;

/// Fontes de um exercício, resolvidas a partir de uma origem confirmada.
///
/// Contém a identidade da origem, as fontes de IA (`question`, `criteria`,
/// `answers`) e as versões da família de avaliação. **Não** contém preparação
/// nem história; os DTOs de transporte serializam apenas `AiSources`.
#[derive(Debug, Clone)]
pub struct ExerciseSources {
    pub origin: ResponseOrigin,
    pub sources: AiSources,
    pub rubric_version: String,
    pub schema_version: String,
}

impl Database {
    /// Resolve as fontes de exercício de uma origem confirmada, sem rede.
    ///
    /// A origem original exige sessão concluída e confirmação inicial; inclui a
    /// confirmação do seguimento quando existe. A origem de prática exige
    /// `status = confirmed` e devolve só a confirmação da tentativa (papel
    /// `attempt`). Nunca inclui notas de história, preparação ou relatórios.
    pub fn load_exercise_sources(&self, origin: &ResponseOrigin) -> Result<ExerciseSources> {
        self.with_connection(|connection| match origin {
            ResponseOrigin::Original {
                session_id,
                session_question_id,
            } => load_original_sources(connection, session_id, session_question_id),
            ResponseOrigin::Practice { practice_id } => {
                load_practice_sources(connection, practice_id)
            }
        })
    }
}

fn load_original_sources(
    connection: &Connection,
    session_id: &str,
    session_question_id: &str,
) -> Result<ExerciseSources> {
    let status: Option<String> = connection
        .query_row(
            "SELECT status FROM sessions WHERE id = ?1",
            [session_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(read_failed)?;
    if status.as_deref() != Some("completed") {
        return Err(session_not_completed());
    }
    let (question_session_id, question_text, criteria_json): (String, String, String) = connection
        .query_row(
            "SELECT session_id, text, criteria_json FROM session_questions WHERE id = ?1",
            [session_question_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(read_failed)?
        .ok_or_else(question_not_found)?;
    if question_session_id != session_id {
        return Err(question_mismatch());
    }
    let criteria = json_list(&criteria_json, "critérios")?;

    let initial = load_confirmation(connection, session_question_id)?.ok_or_else(not_confirmed)?;
    let mut answers = vec![AiSource {
        confirmation_id: initial.id,
        role: "initial".into(),
        text: initial.text,
    }];

    let follow_up_slot: Option<String> = connection
        .query_row(
            "SELECT id FROM answer_slots WHERE session_question_id = ?1 AND role = 'follow_up'",
            [session_question_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(read_failed)?;
    if let Some(slot) = follow_up_slot {
        if let Some(follow_up) = load_confirmation(connection, &slot)? {
            answers.push(AiSource {
                confirmation_id: follow_up.id,
                role: "follow_up".into(),
                text: follow_up.text,
            });
        }
    }

    Ok(ExerciseSources {
        origin: ResponseOrigin::Original {
            session_id: session_id.to_owned(),
            session_question_id: session_question_id.to_owned(),
        },
        sources: AiSources {
            question: question_text,
            criteria,
            answers,
        },
        rubric_version: "star-text-v1".into(),
        schema_version: SCHEMA.into(),
    })
}

fn load_practice_sources(connection: &Connection, practice_id: &str) -> Result<ExerciseSources> {
    let row: Option<(String, String, String, String)> = connection
        .query_row(
            "SELECT p.status, p.session_question_id, p.source_session_id, a.id
               FROM practices p
               JOIN answer_slots a ON a.practice_id = p.id
              WHERE p.id = ?1",
            [practice_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .map_err(read_failed)?;
    let Some((status, session_question_id, _source_session_id, answer_id)) = row else {
        return Err(practice_not_found());
    };
    if status != "confirmed" {
        return Err(practice_not_confirmed());
    }
    let (question_text, criteria_json): (String, String) = connection
        .query_row(
            "SELECT text, criteria_json FROM session_questions WHERE id = ?1",
            [&session_question_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(read_failed)?
        .ok_or_else(question_not_found)?;
    let criteria = json_list(&criteria_json, "critérios")?;
    let confirmation =
        load_confirmation(connection, &answer_id)?.ok_or_else(practice_not_confirmed)?;

    Ok(ExerciseSources {
        origin: ResponseOrigin::Practice {
            practice_id: practice_id.to_owned(),
        },
        sources: AiSources {
            question: question_text,
            criteria,
            answers: vec![AiSource {
                confirmation_id: confirmation.id,
                role: "attempt".into(),
                text: confirmation.text,
            }],
        },
        rubric_version: "star-text-v1".into(),
        schema_version: SCHEMA.into(),
    })
}

fn read_failed(error: rusqlite::Error) -> AppError {
    AppError::new(
        error_code::DATABASE_UNAVAILABLE,
        format!("Não foi possível ler as fontes do exercício: {error}"),
        false,
    )
}

fn session_not_completed() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "Só pode reformular ou avaliar a partir de uma sessão concluída.",
        false,
    )
}

fn question_not_found() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "A pergunta indicada não pertence a uma sessão guardada.",
        false,
    )
}

fn question_mismatch() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "A pergunta indicada não pertence à sessão de origem.",
        false,
    )
}

fn not_confirmed() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "Confirme a resposta antes de reformular ou avaliar.",
        false,
    )
}

fn practice_not_found() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "A prática indicada não existe.",
        false,
    )
}

fn practice_not_confirmed() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "Só pode reformular ou avaliar uma prática já confirmada.",
        false,
    )
}

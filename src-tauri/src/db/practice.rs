//! Consultas de leitura das práticas e resolvedor de propriedade das respostas.
//!
//! Nesta tarefa não há escrita nem rede: só o resolvedor interno
//! [`Database::resolve_capture_owner`], que traduz um [`CaptureOwner`] num alvo
//! concreto, e as leituras mínimas de apoio. As mutações e os comandos Tauri
//! pertencem às tarefas seguintes.

use rusqlite::{Connection, OptionalExtension};

use super::questions::{json_list, parse_category};
use super::{answers, Database};
use crate::domain::{error_code, AppError};
use crate::interview::{AnswerSlot, Confirmation, Id, Result, Version};
use crate::practice::{ArtifactParent, CaptureOwner, QuestionView, ResolvedAnswer};

impl Database {
    /// Resolve um dono de resposta no alvo concreto, sem rede nem escrita.
    ///
    /// Para uma origem de entrevista, o `answerId` identifica o slot (inicial ou
    /// seguimento) e valida-se a sessão e a pergunta; para uma prática, o
    /// `practiceId` tem de corresponder ao slot de tentativa. Um slot `attempt`
    /// nunca é aceite como alvo de entrevista, e uma prática não é aceite só
    /// porque o slot pertence à mesma sessão histórica.
    pub fn resolve_capture_owner(&self, owner: &CaptureOwner) -> Result<ResolvedAnswer> {
        self.with_connection(|connection| resolve_capture_owner_in(connection, owner))
    }
}

/// Linha mínima de um slot de resposta.
struct StoredSlot {
    id: Id,
    session_question_id: Id,
    session_id: Id,
    role: String,
    text: String,
    selected_recording_id: Option<Id>,
    selection_version: Version,
    practice_id: Option<Id>,
}

/// Linha mínima de uma prática.
struct StoredPractice {
    session_question_id: Id,
    source_session_id: Id,
    version: Version,
    status: String,
}

fn resolve_capture_owner_in(
    connection: &Connection,
    owner: &CaptureOwner,
) -> Result<ResolvedAnswer> {
    let answer_id = match owner {
        CaptureOwner::Interview { answer_id, .. } => answer_id,
        CaptureOwner::Practice { answer_id, .. } => answer_id,
    };
    let slot = load_slot(connection, answer_id)?.ok_or_else(owner_not_found)?;

    let (source_session_id, session_question_id, owner_version, writable) = match owner {
        CaptureOwner::Interview {
            session_id,
            session_question_id,
            ..
        } => {
            if slot.session_id != *session_id
                || slot.session_question_id != *session_question_id
                || slot.role == "attempt"
                || slot.practice_id.is_some()
            {
                return Err(owner_mismatch());
            }
            let (status, version): (String, i64) = connection
                .query_row(
                    "SELECT status, version FROM sessions WHERE id = ?1",
                    [session_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(read_failed)?;
            let confirmed: bool = connection
                .query_row(
                    "SELECT EXISTS (
                        SELECT 1 FROM confirmations WHERE session_question_id = ?1
                     )",
                    [answer_id],
                    |row| row.get(0),
                )
                .map_err(read_failed)?;
            (
                session_id.clone(),
                session_question_id.clone(),
                version_from_i64(version)?,
                !confirmed && status == "in_progress",
            )
        }
        CaptureOwner::Practice { practice_id, .. } => {
            if slot.practice_id.as_deref() != Some(practice_id.as_str()) || slot.role != "attempt" {
                return Err(owner_mismatch());
            }
            let practice = load_practice(connection, practice_id)?.ok_or_else(owner_not_found)?;
            if practice.session_question_id != slot.session_question_id
                || practice.source_session_id != slot.session_id
            {
                return Err(owner_mismatch());
            }
            let writable = matches!(practice.status.as_str(), "preparing" | "answering");
            (
                practice.source_session_id,
                practice.session_question_id,
                practice.version,
                writable,
            )
        }
    };

    let question = load_question(connection, &session_question_id)?.ok_or_else(owner_not_found)?;
    let confirmation: Option<Confirmation> = answers::load_confirmation(connection, answer_id)?;

    let slot = AnswerSlot {
        id: slot.id,
        session_question_id: slot.session_question_id,
        role: slot.role,
        text: slot.text,
        selected_recording_id: slot.selected_recording_id,
        selection_version: slot.selection_version,
        practice_id: slot.practice_id,
    };

    Ok(ResolvedAnswer {
        owner: owner.clone(),
        slot,
        source_session_id: source_session_id.clone(),
        session_question_id,
        question,
        confirmation,
        owner_version,
        writable,
        artifact_parent: ArtifactParent {
            source_session_id,
            answer_id: answer_id.clone(),
        },
    })
}

fn load_slot(connection: &Connection, answer_id: &str) -> Result<Option<StoredSlot>> {
    let row = connection
        .query_row(
            "SELECT id, session_question_id, session_id, role, text,
                    selected_recording_id, selection_version, practice_id
               FROM answer_slots
              WHERE id = ?1",
            [answer_id],
            |row| {
                Ok((
                    row.get::<_, Id>("id")?,
                    row.get::<_, Id>("session_question_id")?,
                    row.get::<_, Id>("session_id")?,
                    row.get::<_, String>("role")?,
                    row.get::<_, String>("text")?,
                    row.get::<_, Option<Id>>("selected_recording_id")?,
                    row.get::<_, i64>("selection_version")?,
                    row.get::<_, Option<Id>>("practice_id")?,
                ))
            },
        )
        .optional()
        .map_err(read_failed)?;
    let Some((
        id,
        session_question_id,
        session_id,
        role,
        text,
        selected_recording_id,
        selection_version,
        practice_id,
    )) = row
    else {
        return Ok(None);
    };
    Ok(Some(StoredSlot {
        id,
        session_question_id,
        session_id,
        role,
        text,
        selected_recording_id,
        selection_version: version_from_i64(selection_version)?,
        practice_id,
    }))
}

fn load_practice(connection: &Connection, practice_id: &str) -> Result<Option<StoredPractice>> {
    let row = connection
        .query_row(
            "SELECT session_question_id, source_session_id, version, status
               FROM practices
              WHERE id = ?1",
            [practice_id],
            |row| {
                Ok((
                    row.get::<_, Id>("session_question_id")?,
                    row.get::<_, Id>("source_session_id")?,
                    row.get::<_, i64>("version")?,
                    row.get::<_, String>("status")?,
                ))
            },
        )
        .optional()
        .map_err(read_failed)?;
    let Some((session_question_id, source_session_id, version, status)) = row else {
        return Ok(None);
    };
    Ok(Some(StoredPractice {
        session_question_id,
        source_session_id,
        version: version_from_i64(version)?,
        status,
    }))
}

fn load_question(
    connection: &Connection,
    session_question_id: &str,
) -> Result<Option<QuestionView>> {
    let row = connection
        .query_row(
            "SELECT text, category, competencies_json, criteria_json
               FROM session_questions
              WHERE id = ?1",
            [session_question_id],
            |row| {
                Ok((
                    row.get::<_, String>("text")?,
                    row.get::<_, String>("category")?,
                    row.get::<_, String>("competencies_json")?,
                    row.get::<_, String>("criteria_json")?,
                ))
            },
        )
        .optional()
        .map_err(read_failed)?;
    let Some((text, category_name, competencies_json, criteria_json)) = row else {
        return Ok(None);
    };
    let category = parse_category(&category_name)
        .ok_or_else(|| read_corrupt(&format!("categoria desconhecida «{category_name}»")))?;
    Ok(Some(QuestionView {
        text,
        category,
        competencies: json_list(&competencies_json, "competências")?,
        criteria: json_list(&criteria_json, "critérios")?,
    }))
}

fn version_from_i64(value: i64) -> Result<Version> {
    Version::try_from(value).map_err(|_| read_corrupt("versão inválida"))
}

fn owner_not_found() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "A resposta indicada não existe. Atualize o estado antes de continuar.",
        false,
    )
}

fn owner_mismatch() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "A resposta indicada não pertence ao alvo pedido; nada foi aplicado.",
        false,
    )
}

fn read_failed(error: rusqlite::Error) -> AppError {
    AppError::new(
        error_code::DATABASE_UNAVAILABLE,
        format!("Não foi possível ler as práticas guardadas: {error}"),
        false,
    )
}

fn read_corrupt(detail: &str) -> AppError {
    AppError::new(
        error_code::DATABASE_UNAVAILABLE,
        format!("Não foi possível ler as práticas guardadas: {detail}."),
        false,
    )
}

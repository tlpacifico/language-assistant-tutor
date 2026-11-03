//! Consultas de leitura das práticas e resolvedor de propriedade das respostas.
//!
//! Nesta tarefa não há escrita nem rede: só o resolvedor interno
//! [`Database::resolve_capture_owner`], que traduz um [`CaptureOwner`] num alvo
//! concreto, e as leituras mínimas de apoio. As mutações e os comandos Tauri
//! pertencem às tarefas seguintes.

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use uuid::Uuid;

use super::ai::{lookup_command_identity, CommandIdentity, CommandIdentityLocation};
use super::answers::{self, request_hash};
use super::questions::{json_list, parse_category, utc_now};
use super::stories::load_preparation_in;
use super::Database;
use crate::domain::{error_code, AppError};
use crate::interview::{
    validate_review_text, AnswerSlot, AnswerTarget, Confirmation, Id, RecordingState,
    RecordingView, Result, ReviewOrigin, Version,
};
use crate::practice::{
    ArtifactParent, CaptureOwner, ConfirmPracticeAnswer, CreatePractice, PracticeCommand,
    PracticeDetail, PracticeStatus, QuestionView, ResolvedAnswer,
};

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

    /// Cria uma prática a partir de uma pergunta confirmada de uma sessão concluída.
    ///
    /// Idempotente por `requestId`: repetir o mesmo pedido devolve a prática já
    /// criada. Recusa sessão em curso, origem sem confirmação, predecessor de
    /// outra pergunta e uma segunda prática pendente. Nada é escrito nas recusas.
    pub fn create_practice(&self, request: CreatePractice) -> Result<PracticeDetail> {
        let request_id = request.request_id.trim().to_owned();
        if request_id.is_empty() {
            return Err(invalid_practice_request());
        }
        let payload_hash = request_hash(&CreatePracticePayload {
            session_id: &request.session_id,
            session_question_id: &request.session_question_id,
            previous_practice_id: request.previous_practice_id.as_deref(),
        })?;
        let now = utc_now();

        self.with_connection(|connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(write_failed)?;

            if let Some(practice_id) =
                require_fresh_command(&transaction, &request_id, "create_practice", &payload_hash)?
            {
                let detail = load_practice_detail(&transaction, &practice_id)?
                    .ok_or_else(|| read_corrupt("a prática indicada pelo recibo não existe"))?;
                transaction.commit().map_err(write_failed)?;
                return Ok(detail);
            }

            let session_status: Option<String> = transaction
                .query_row(
                    "SELECT status FROM sessions WHERE id = ?1",
                    [&request.session_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(read_failed)?;
            match session_status.as_deref() {
                None => return Err(source_session_not_found()),
                Some(status) if status != "completed" => return Err(source_not_completed()),
                _ => {}
            }

            let source: Option<(String, String)> = transaction
                .query_row(
                    "SELECT text, session_id FROM session_questions WHERE id = ?1",
                    [&request.session_question_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(read_failed)?;
            let Some((question_text, question_session_id)) = source else {
                return Err(source_question_not_found());
            };
            if question_session_id != request.session_id {
                return Err(source_question_mismatch());
            }
            let confirmed: bool = transaction
                .query_row(
                    "SELECT EXISTS (
                        SELECT 1 FROM confirmations WHERE session_question_id = ?1
                     )",
                    [&request.session_question_id],
                    |row| row.get(0),
                )
                .map_err(read_failed)?;
            if !confirmed {
                return Err(source_not_confirmed());
            }

            if let Some(previous) = &request.previous_practice_id {
                let valid: bool = transaction
                    .query_row(
                        "SELECT EXISTS (
                            SELECT 1 FROM practices
                             WHERE id = ?1 AND session_question_id = ?2 AND status = 'confirmed'
                         )",
                        params![previous, request.session_question_id],
                        |row| row.get(0),
                    )
                    .map_err(read_failed)?;
                if !valid {
                    return Err(predecessor_invalid());
                }
            }

            let active_session: bool = transaction
                .query_row(
                    "SELECT EXISTS (SELECT 1 FROM sessions WHERE status = 'in_progress')",
                    [],
                    |row| row.get(0),
                )
                .map_err(read_failed)?;
            if active_session {
                return Err(active_session_blocks_practice());
            }
            let pending_practice: bool = transaction
                .query_row(
                    "SELECT EXISTS (
                        SELECT 1 FROM practices WHERE status IN ('preparing', 'answering')
                     )",
                    [],
                    |row| row.get(0),
                )
                .map_err(read_failed)?;
            if pending_practice {
                return Err(pending_practice_exists());
            }

            let ordinal: i64 = transaction
                .query_row(
                    "SELECT COALESCE(MAX(ordinal), 0) + 1
                       FROM practices
                      WHERE session_question_id = ?1",
                    [&request.session_question_id],
                    |row| row.get(0),
                )
                .map_err(read_failed)?;
            let practice_id = Uuid::new_v4().to_string();
            let answer_id = Uuid::new_v4().to_string();

            transaction
                .execute(
                    "INSERT INTO practices
                       (id, session_question_id, source_session_id, ordinal,
                        previous_practice_id, answer_id, status, version, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'preparing', 1, ?7)",
                    params![
                        practice_id,
                        request.session_question_id,
                        request.session_id,
                        ordinal,
                        request.previous_practice_id,
                        answer_id,
                        now
                    ],
                )
                .map_err(|error| {
                    if is_pending_practice_conflict(&error) {
                        pending_practice_exists()
                    } else {
                        write_failed(error)
                    }
                })?;
            transaction
                .execute(
                    "INSERT INTO answer_slots
                       (id, session_question_id, session_id, role, text, practice_id)
                     VALUES (?1, ?2, ?3, 'attempt', ?4, ?5)",
                    params![
                        answer_id,
                        request.session_question_id,
                        request.session_id,
                        question_text,
                        practice_id
                    ],
                )
                .map_err(write_failed)?;

            insert_practice_receipt(
                &transaction,
                &request_id,
                "create_practice",
                &payload_hash,
                &practice_id,
            )?;

            let detail = load_practice_detail(&transaction, &practice_id)?
                .ok_or_else(|| read_corrupt("a prática recém-criada não foi encontrada"))?;
            transaction.commit().map_err(write_failed)?;
            Ok(detail)
        })
    }

    /// Lê uma prática completa, com a pergunta fixa, a tentativa e os áudios.
    pub fn load_practice(&self, practice_id: &str) -> Result<PracticeDetail> {
        self.with_connection(|connection| {
            load_practice_detail(connection, practice_id)?.ok_or_else(practice_not_found)
        })
    }

    /// Lista as práticas de uma pergunta da sessão, pela ordem de criação.
    pub fn list_practices(&self, session_question_id: &str) -> Result<Vec<PracticeDetail>> {
        self.with_connection(|connection| {
            let mut statement = connection
                .prepare("SELECT id FROM practices WHERE session_question_id = ?1 ORDER BY ordinal")
                .map_err(read_failed)?;
            let ids: Vec<Id> = statement
                .query_map([session_question_id], |row| row.get(0))
                .map_err(read_failed)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(read_failed)?;
            drop(statement);
            ids.iter()
                .map(|id| {
                    load_practice_detail(connection, id)?
                        .ok_or_else(|| read_corrupt("uma prática listada não foi encontrada"))
                })
                .collect()
        })
    }

    /// A única prática pendente (preparação ou resposta), se existir.
    pub fn pending_practice(&self) -> Result<Option<PracticeDetail>> {
        self.with_connection(|connection| {
            let id: Option<Id> = connection
                .query_row(
                    "SELECT id FROM practices WHERE status IN ('preparing', 'answering')",
                    [],
                    |row| row.get(0),
                )
                .optional()
                .map_err(read_failed)?;
            match id {
                Some(id) => load_practice_detail(connection, &id),
                None => Ok(None),
            }
        })
    }

    /// Abandona uma prática pendente, conservando revisões e gravações.
    ///
    /// Só uma prática ainda não confirmada nem abandonada pode ser abandonada;
    /// o estado terminal é imutável. Idempotente por `requestId`.
    pub fn abandon_practice(&self, request: PracticeCommand) -> Result<PracticeDetail> {
        let request_id = request.request_id.trim().to_owned();
        if request_id.is_empty() {
            return Err(invalid_practice_request());
        }
        let payload_hash = request_hash(&PracticeCommandPayload {
            practice_id: &request.practice_id,
            expected_version: request.expected_version,
        })?;
        let now = utc_now();

        self.with_connection(|connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(write_failed)?;

            if let Some(practice_id) =
                require_fresh_command(&transaction, &request_id, "abandon_practice", &payload_hash)?
            {
                let detail = load_practice_detail(&transaction, &practice_id)?
                    .ok_or_else(|| read_corrupt("a prática indicada pelo recibo não existe"))?;
                transaction.commit().map_err(write_failed)?;
                return Ok(detail);
            }

            let updated = transaction
                .execute(
                    "UPDATE practices
                        SET status = 'abandoned', abandoned_at = ?3, version = version + 1
                      WHERE id = ?1 AND version = ?2
                        AND status IN ('preparing', 'answering')",
                    params![
                        request.practice_id,
                        version_to_i64(request.expected_version)?,
                        now
                    ],
                )
                .map_err(write_failed)?;
            if updated != 1 {
                return Err(abandon_refused());
            }

            insert_practice_receipt(
                &transaction,
                &request_id,
                "abandon_practice",
                &payload_hash,
                &request.practice_id,
            )?;

            let detail = load_practice_detail(&transaction, &request.practice_id)?
                .ok_or_else(|| read_corrupt("a prática abandonada não foi encontrada"))?;
            transaction.commit().map_err(write_failed)?;
            Ok(detail)
        })
    }

    /// Confirma a tentativa de uma prática, tornando-a imutável, sem tocar na sessão.
    ///
    /// Reutiliza as validações partilhadas de revisão/gravação e escreve a
    /// confirmação imutável, avança a versão da prática e regista o recibo.
    pub fn confirm_practice_answer(
        &self,
        request: ConfirmPracticeAnswer,
    ) -> Result<PracticeDetail> {
        let request_id = request.request_id.trim().to_owned();
        if request_id.is_empty() {
            return Err(invalid_practice_request());
        }
        validate_review_text(&request.text)?;
        let payload_hash = request_hash(&ConfirmPracticePayload {
            target: &request.target,
            expected_review_version: request.expected_review_version,
            expected_practice_version: request.expected_practice_version,
            source_transcript_id: request.source_transcript_id.as_deref(),
            origin: request.origin,
            text: &request.text,
        })?;
        let now = utc_now();

        self.with_connection(|connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(write_failed)?;

            if let Some(practice_id) =
                require_fresh_command(&transaction, &request_id, "confirm_practice", &payload_hash)?
            {
                let detail = load_practice_detail(&transaction, &practice_id)?
                    .ok_or_else(|| read_corrupt("a prática indicada pelo recibo não existe"))?;
                transaction.commit().map_err(write_failed)?;
                return Ok(detail);
            }

            let resolved = resolve_capture_owner_in(&transaction, &request.target.owner)?;
            let (practice_id, answer_id, source_session_id) = match &resolved.owner {
                CaptureOwner::Practice {
                    practice_id,
                    answer_id,
                } => (
                    practice_id.clone(),
                    answer_id.clone(),
                    resolved.source_session_id.clone(),
                ),
                _ => return Err(owner_not_practice()),
            };
            if resolved.owner_version != request.expected_practice_version {
                return Err(stale_practice());
            }
            if !resolved.writable {
                return Err(practice_not_writable());
            }

            let answer_target = AnswerTarget {
                answer_id: None,
                session_id: source_session_id.clone(),
                session_question_id: answer_id.clone(),
                recording_id: request.target.recording_id.clone(),
                selection_version: request.target.selection_version,
            };
            let target = answers::require_review_target(&transaction, &answer_target, None)?;
            answers::require_selected_recording(&transaction, &answer_target, &target)?;
            answers::require_transcript_owner(
                &transaction,
                &request.target.recording_id,
                request.source_transcript_id.as_deref(),
            )?;

            let current = answers::load_review(&transaction, &request.target.recording_id)?;
            let expected = current.as_ref().map_or(0, |review| review.version);
            if expected != request.expected_review_version {
                return Err(stale_review());
            }
            let review_version = match &current {
                Some(review)
                    if review.text == request.text
                        && review.origin == request.origin
                        && review.source_transcript_id == request.source_transcript_id =>
                {
                    review.version
                }
                _ => {
                    let version = expected.checked_add(1).ok_or_else(invalid_version)?;
                    answers::insert_review(
                        &transaction,
                        &request.target.recording_id,
                        version,
                        request.source_transcript_id.as_deref(),
                        request.origin,
                        &request.text,
                        &now,
                    )?;
                    version
                }
            };

            let confirmation_id = Uuid::new_v4().to_string();
            transaction
                .execute(
                    "INSERT INTO confirmations
                       (id, session_question_id, recording_id, review_version,
                        source_transcript_id, origin, text, confirmed_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        confirmation_id,
                        answer_id,
                        request.target.recording_id,
                        version_to_i64(review_version)?,
                        request.source_transcript_id.as_deref(),
                        request.origin.as_str(),
                        request.text,
                        now
                    ],
                )
                .map_err(write_failed)?;

            let updated = transaction
                .execute(
                    "UPDATE practices
                        SET status = 'confirmed', confirmed_at = ?3, version = version + 1
                      WHERE id = ?1 AND version = ?2
                        AND status IN ('preparing', 'answering')",
                    params![
                        practice_id,
                        version_to_i64(request.expected_practice_version)?,
                        now
                    ],
                )
                .map_err(write_failed)?;
            if updated != 1 {
                return Err(stale_practice());
            }

            insert_practice_receipt(
                &transaction,
                &request_id,
                "confirm_practice",
                &payload_hash,
                &practice_id,
            )?;

            let detail = load_practice_detail(&transaction, &practice_id)?
                .ok_or_else(|| read_corrupt("a prática confirmada não foi encontrada"))?;
            transaction.commit().map_err(write_failed)?;
            Ok(detail)
        })
    }

    /// A gravação selecionada e finalizada da tentativa de uma prática.
    ///
    /// Valida que a gravação é a selecionada no slot de tentativa e que a versão
    /// de seleção coincide; devolve a vista pronta para transcrição.
    pub fn practice_selected_recording(
        &self,
        slot_id: &str,
        recording_id: &str,
        selection_version: Version,
    ) -> Result<RecordingView> {
        self.with_connection(|connection| {
            let selected: Option<Id> = connection
                .query_row(
                    "SELECT selected_recording_id FROM answer_slots WHERE id = ?1",
                    [slot_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(read_failed)?;
            if selected.as_deref() != Some(recording_id) {
                return Err(stale_selection());
            }
            let stored_version: i64 = connection
                .query_row(
                    "SELECT selection_version FROM answer_slots WHERE id = ?1",
                    [slot_id],
                    |row| row.get(0),
                )
                .map_err(read_failed)?;
            let stored_version = Version::try_from(stored_version)
                .map_err(|_| read_corrupt("versão de seleção inválida"))?;
            if stored_version != selection_version {
                return Err(stale_selection());
            }
            let recordings = answers::load_recordings(connection, slot_id)?;
            let recording = recordings
                .into_iter()
                .find(|recording| recording.id == recording_id)
                .ok_or_else(recording_not_found)?;
            if recording.state != RecordingState::Ready {
                return Err(recording_required());
            }
            Ok(recording)
        })
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

pub(super) fn resolve_capture_owner_in(
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

/// Detalhe completo de uma prática: pergunta fixa, tentativa, áudios e confirmação.
fn load_practice_detail(
    connection: &Connection,
    practice_id: &str,
) -> Result<Option<PracticeDetail>> {
    let row = connection
        .query_row(
            "SELECT p.id, p.session_question_id, p.source_session_id, p.ordinal,
                    p.previous_practice_id, p.status, p.version, p.created_at,
                    p.confirmed_at, p.abandoned_at,
                    a.id, a.text, a.selected_recording_id, a.selection_version,
                    q.text, q.category, q.competencies_json, q.criteria_json
               FROM practices p
               JOIN answer_slots a ON a.practice_id = p.id
               JOIN session_questions q ON q.id = p.session_question_id
              WHERE p.id = ?1",
            [practice_id],
            |row| {
                Ok((
                    row.get::<_, Id>(0)?,
                    row.get::<_, Id>(1)?,
                    row.get::<_, Id>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<Id>>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, Id>(10)?,
                    row.get::<_, String>(11)?,
                    row.get::<_, Option<Id>>(12)?,
                    row.get::<_, i64>(13)?,
                    row.get::<_, String>(14)?,
                    row.get::<_, String>(15)?,
                    row.get::<_, String>(16)?,
                    row.get::<_, String>(17)?,
                ))
            },
        )
        .optional()
        .map_err(read_failed)?;

    let Some((
        id,
        session_question_id,
        source_session_id,
        ordinal,
        previous_practice_id,
        status,
        version,
        created_at,
        confirmed_at,
        abandoned_at,
        answer_id,
        answer_text,
        selected_recording_id,
        selection_version,
        question_text,
        category_name,
        competencies_json,
        criteria_json,
    )) = row
    else {
        return Ok(None);
    };

    let category = parse_category(&category_name)
        .ok_or_else(|| read_corrupt(&format!("categoria desconhecida «{category_name}»")))?;
    let status = parse_status(&status)?;
    let recordings = answers::load_recordings(connection, &answer_id)?;
    let confirmation = answers::load_confirmation(connection, &answer_id)?;

    Ok(Some(PracticeDetail {
        id,
        source_session_id,
        session_question_id: session_question_id.clone(),
        ordinal: u32::try_from(ordinal).map_err(|_| read_corrupt("ordinal de prática inválido"))?,
        previous_practice_id,
        status,
        version: version_from_i64(version)?,
        created_at,
        confirmed_at,
        abandoned_at,
        question: QuestionView {
            text: question_text,
            category,
            competencies: json_list(&competencies_json, "competências")?,
            criteria: json_list(&criteria_json, "critérios")?,
        },
        answer: AnswerSlot {
            id: answer_id.clone(),
            session_question_id,
            role: "attempt".into(),
            text: answer_text,
            selected_recording_id,
            selection_version: version_from_i64(selection_version)?,
            practice_id: Some(practice_id.to_owned()),
        },
        recordings,
        confirmation,
        preparation: load_preparation_in(connection, &answer_id)?,
    }))
}

fn parse_status(value: &str) -> Result<PracticeStatus> {
    match value {
        "preparing" => Ok(PracticeStatus::Preparing),
        "answering" => Ok(PracticeStatus::Answering),
        "confirmed" => Ok(PracticeStatus::Confirmed),
        "abandoned" => Ok(PracticeStatus::Abandoned),
        other => Err(read_corrupt(&format!(
            "estado de prática desconhecido «{other}»"
        ))),
    }
}

/// Preflight da tarefa 1 para comandos de prática: o mesmo `requestId` com o
/// mesmo conteúdo devolve o recibo para repetir o efeito; qualquer conflito ou
/// conteúdo diferente é recusado em vez de escolhido arbitrariamente.
///
/// Os recibos de prática vivem em `ai_receipts` (kind/hash/entity), como previsto
/// em §2.2, sem alargar o CHECK de `command_receipts`.
fn require_fresh_command(
    connection: &Connection,
    request_id: &str,
    kind: &str,
    payload_hash: &str,
) -> Result<Option<String>> {
    match lookup_command_identity(connection, request_id)? {
        CommandIdentity::NotFound => Ok(None),
        CommandIdentity::Conflict => Err(stale_request()),
        CommandIdentity::Found {
            location: CommandIdentityLocation::AiReceipt,
            kind: Some(found_kind),
            payload_hash: found_hash,
        } => {
            if found_kind == kind && found_hash == payload_hash {
                let entity = find_practice_receipt(connection, request_id)?
                    .ok_or_else(|| read_corrupt("o recibo indicado não existe na base"))?;
                Ok(Some(entity))
            } else {
                Err(stale_request())
            }
        }
        CommandIdentity::Found { .. } => Err(stale_request()),
    }
}

/// Recibo de prática: `requestId` como chave e `entity_id` como a prática criada.
fn insert_practice_receipt(
    connection: &Connection,
    request_id: &str,
    kind: &str,
    payload_hash: &str,
    practice_id: &str,
) -> Result<()> {
    connection
        .execute(
            "INSERT INTO ai_receipts (id, kind, payload_hash, entity_id)
             VALUES (?1, ?2, ?3, ?4)",
            params![request_id, kind, payload_hash, practice_id],
        )
        .map_err(write_failed)?;
    Ok(())
}

fn find_practice_receipt(connection: &Connection, request_id: &str) -> Result<Option<Id>> {
    connection
        .query_row(
            "SELECT entity_id FROM ai_receipts WHERE id = ?1",
            [request_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(read_failed)
}

/// Campos do pedido de criação sem o `requestId`, pela ordem fixa do tipo.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CreatePracticePayload<'a> {
    session_id: &'a str,
    session_question_id: &'a str,
    previous_practice_id: Option<&'a str>,
}

/// Campos do comando de abandono sem o `requestId`.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PracticeCommandPayload<'a> {
    practice_id: &'a str,
    expected_version: Version,
}

/// Campos do pedido de confirmação sem o `requestId`.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfirmPracticePayload<'a> {
    target: &'a crate::practice::PracticeAnswerTarget,
    expected_review_version: Version,
    expected_practice_version: Version,
    source_transcript_id: Option<&'a str>,
    origin: ReviewOrigin,
    text: &'a str,
}

fn version_to_i64(version: Version) -> Result<i64> {
    i64::try_from(version).map_err(|_| invalid_version())
}

fn is_pending_practice_conflict(error: &rusqlite::Error) -> bool {
    match error {
        rusqlite::Error::SqliteFailure(failure, message) => {
            failure.code == rusqlite::ErrorCode::ConstraintViolation
                && message
                    .as_deref()
                    .is_none_or(|message| message.contains("one_pending_practice"))
        }
        _ => false,
    }
}

fn invalid_practice_request() -> AppError {
    AppError::new(
        error_code::INVALID_INPUT,
        "O pedido de prática não tem identificador. Repita a operação a partir do treino.",
        false,
    )
}

fn invalid_version() -> AppError {
    AppError::new(
        error_code::INVALID_INPUT,
        "A versão indicada no pedido excede o intervalo guardável.",
        false,
    )
}

fn source_session_not_found() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "A sessão de origem indicada não existe. Atualize o histórico antes de praticar.",
        false,
    )
}

fn source_not_completed() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "Só pode praticar a partir de uma sessão concluída.",
        false,
    )
}

fn source_question_not_found() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "A pergunta indicada não pertence a uma sessão guardada; não há prática a criar.",
        false,
    )
}

fn source_question_mismatch() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "A pergunta indicada não pertence à sessão de origem.",
        false,
    )
}

fn source_not_confirmed() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "Só pode praticar uma pergunta já confirmada na sessão.",
        false,
    )
}

fn predecessor_invalid() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "A prática anterior tem de estar confirmada e ser da mesma pergunta.",
        false,
    )
}

fn active_session_blocks_practice() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "Termine a entrevista em curso antes de praticar.",
        false,
    )
}

fn pending_practice_exists() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "Já existe uma prática pendente. Confirme-a ou abandone-a antes de criar outra.",
        false,
    )
}

fn practice_not_found() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "A prática indicada não existe. Atualize o histórico antes de a abrir.",
        false,
    )
}

fn stale_practice() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "A prática mudou entretanto. Atualize o estado antes de continuar.",
        false,
    )
}

fn practice_not_writable() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "Esta prática já não aceita alterações; nada foi aplicado.",
        false,
    )
}

fn owner_not_practice() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "O alvo indicado não é uma tentativa de prática.",
        false,
    )
}

fn abandon_refused() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "Só uma prática ainda pendente pode ser abandonada.",
        false,
    )
}

fn stale_selection() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "A seleção da resposta mudou entretanto. Atualize a prática antes de continuar.",
        false,
    )
}

fn recording_not_found() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "A gravação indicada não pertence a esta prática.",
        false,
    )
}

fn recording_required() -> AppError {
    AppError::new(
        error_code::ANSWER_REQUIRED,
        "Esta prática ainda não tem uma gravação finalizada selecionada.",
        false,
    )
}

fn stale_review() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "A revisão desta resposta mudou entretanto. Confirme exatamente a versão que está a ver.",
        false,
    )
}

fn stale_request() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "Este pedido já foi usado com outro conteúdo. Repita a operação a partir do estado atual.",
        false,
    )
}

fn write_failed(error: rusqlite::Error) -> AppError {
    AppError::new(
        error_code::STORAGE_WRITE_FAILED,
        format!("Não foi possível guardar a prática; nenhuma alteração foi aplicada: {error}"),
        true,
    )
}

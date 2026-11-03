//! Histórias locais e snapshot do apoio no arranque da captura (fase 4).
//!
//! As histórias são exclusivamente locais: nenhum campo, identificador, nota ou
//! competência de história entra em `AiSources`, DTOs de transporte ou bodies
//! HTTP. O resolvedor de fontes IA (`db/ai.rs`) continua a ler apenas
//! confirmações; este módulo nunca lhe entrega `StoryInput`/`PreparationView`.
//!
//! O apoio de uma resposta vive em `preparations` (seleção e bloqueio) e
//! `preparation_captures` (cópia fixada por gravação). A ordem não finge
//! atomicidade: reserva SQLite → microfone → marcador de ficheiro → SQLite. Uma
//! escrita que falhe depois do arranque não pára a captura; o serviço expõe a
//! pendência e permite retry local. A recuperação no arranque distingue
//! `started` (consolida), `failed` (liberta) e `start_uncertain` (exige
//! resolução explícita), sem descartar ficheiros.

use std::collections::HashSet;

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use uuid::Uuid;

use super::ai::{lookup_command_identity, CommandIdentity, CommandIdentityLocation};
use super::answers::request_hash;
use super::practice::resolve_capture_owner_in;
use super::questions::{json_array, json_list, utc_now};
use super::Database;
use crate::domain::{error_code, AppError};
use crate::interview::{Id, Result, Version};
use crate::practice::{
    CreateStory, LockState, MissingField, PreparationDecision, PreparationView, SelectStory,
    StoryInput, StorySnapshot, StoryView, SupportKind, UpdateStory,
};

/// Título: 1 a 200 bytes.
const TITLE_MAX_BYTES: usize = 200;
/// Cada nota (situação, tarefa, ações, desfecho): até 16.000 bytes.
const NOTE_MAX_BYTES: usize = 16_000;
/// Soma das notas: até 48.000 bytes.
const NOTES_TOTAL_MAX_BYTES: usize = 48_000;
/// Competências: 1 a 20, cada uma até 120 bytes.
const COMPETENCY_MAX_BYTES: usize = 120;
const COMPETENCIES_MAX: usize = 20;

/// Reserva de preparação produzida antes de abrir o microfone.
///
/// `Locked` significa que o apoio já foi fixado numa captura anterior e a
/// gravação nova prossegue sem criar outro candidato; `Reserved` significa que
/// um candidato imutável (snapshot ou ausência) foi gravado em SQLite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreparationReservation {
    Locked,
    Reserved { snapshot: Option<StorySnapshot> },
}

/// Linha de `preparations` lida da base.
struct StoredPreparation {
    version: Version,
    selected_story_id: Option<Id>,
    selected_story_version: Option<Version>,
    locked_capture_id: Option<Id>,
    lock_state: String,
}

/// Linha de `preparation_captures` lida da base.
struct StoredCapture {
    answer_id: Id,
}

/// Localização de uma captura de preparação pendente, para a recuperação.
pub(crate) struct PreparationCaptureLocation {
    pub(crate) recording_id: Id,
    pub(crate) session_id: Id,
    pub(crate) session_question_id: Id,
}

/// Valida e normaliza uma história, aparando as pontas de todos os campos.
///
/// Título 1…200 bytes; cada nota até 16.000 bytes e a soma até 48.000; 1…20
/// competências até 120 bytes, deduplicadas exatamente após o trim. Exige pelo
/// menos uma nota e uma competência não vazias. Não exige resultado nem STAR
/// completo: a completude é um rótulo derivado (`missing_fields`).
pub fn validate_story_input(input: StoryInput) -> Result<StoryInput> {
    let title = input.title.trim().to_owned();
    let situation = input.situation.trim().to_owned();
    let task = input.task.trim().to_owned();
    let actions = input.actions.trim().to_owned();
    let outcome = input.outcome.trim().to_owned();

    if title.is_empty() {
        return Err(invalid_story("Escreva um título para a história."));
    }
    if title.len() > TITLE_MAX_BYTES {
        return Err(invalid_story(format!(
            "O título excede {TITLE_MAX_BYTES} bytes."
        )));
    }

    let notes = [&situation, &task, &actions, &outcome];
    if notes.iter().all(|note| note.is_empty()) {
        return Err(invalid_story(
            "Escreva pelo menos uma nota: situação, tarefa, ações ou desfecho.",
        ));
    }
    for note in notes {
        if note.len() > NOTE_MAX_BYTES {
            return Err(invalid_story(format!(
                "Uma nota excede {NOTE_MAX_BYTES} bytes."
            )));
        }
    }
    let total: usize = notes.iter().map(|note| note.len()).sum();
    if total > NOTES_TOTAL_MAX_BYTES {
        return Err(invalid_story(format!(
            "As notas ultrapassam em conjunto {NOTES_TOTAL_MAX_BYTES} bytes."
        )));
    }

    let mut competencies: Vec<String> = Vec::with_capacity(input.competencies.len());
    let mut seen = HashSet::with_capacity(input.competencies.len());
    for competency in input.competencies {
        let trimmed = competency.trim().to_owned();
        if trimmed.is_empty() {
            return Err(invalid_story("As competências não podem estar vazias."));
        }
        if trimmed.len() > COMPETENCY_MAX_BYTES {
            return Err(invalid_story(format!(
                "Uma competência excede {COMPETENCY_MAX_BYTES} bytes."
            )));
        }
        if seen.insert(trimmed.clone()) {
            competencies.push(trimmed);
        }
    }
    if competencies.is_empty() {
        return Err(invalid_story("Indique pelo menos uma competência."));
    }
    if competencies.len() > COMPETENCIES_MAX {
        return Err(invalid_story(format!(
            "Indique no máximo {COMPETENCIES_MAX} competências distintas."
        )));
    }

    Ok(StoryInput {
        title,
        situation,
        task,
        actions,
        outcome,
        competencies,
    })
}

/// Campos STAR em falta numa história validada.
pub fn missing_fields(input: &StoryInput) -> Vec<MissingField> {
    let mut fields = Vec::new();
    if input.situation.is_empty() {
        fields.push(MissingField::Situation);
    }
    if input.task.is_empty() {
        fields.push(MissingField::Task);
    }
    if input.actions.is_empty() {
        fields.push(MissingField::Actions);
    }
    if input.outcome.is_empty() {
        fields.push(MissingField::Outcome);
    }
    fields
}

impl Database {
    /// Lista as histórias locais, opcionalmente filtradas por competência.
    ///
    /// O filtro é normalizado com trim e comparação sem distinção de maiúsculas.
    pub fn list_stories(&self, competency: Option<&str>) -> Result<Vec<StoryView>> {
        let filter = competency
            .map(|value| value.trim().to_lowercase())
            .filter(|value| !value.is_empty());
        self.with_connection(|connection| {
            let mut statement = connection
                .prepare("SELECT id FROM stories ORDER BY rowid")
                .map_err(read_failed)?;
            let ids: Vec<Id> = statement
                .query_map([], |row| row.get(0))
                .map_err(read_failed)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(read_failed)?;
            drop(statement);
            let mut stories = Vec::new();
            for id in ids {
                let view = load_story_view(connection, &id)?
                    .ok_or_else(|| read_corrupt("uma história listada não foi encontrada"))?;
                if let Some(filter) = &filter {
                    if !view
                        .competencies
                        .iter()
                        .any(|c| c.to_lowercase() == *filter)
                    {
                        continue;
                    }
                }
                stories.push(view);
            }
            Ok(stories)
        })
    }

    /// Cria uma história local, idempotente por `requestId`.
    pub fn create_story(&self, request: CreateStory) -> Result<StoryView> {
        let request_id = request.request_id.trim().to_owned();
        if request_id.is_empty() {
            return Err(invalid_story_request());
        }
        let input = validate_story_input(request.input)?;
        let payload_hash = request_hash(&input)?;
        let now = utc_now();

        self.with_connection(|connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(write_failed)?;

            if let Some(story_id) =
                require_story_command(&transaction, &request_id, "create_story", &payload_hash)?
            {
                let view = load_story_view(&transaction, &story_id)?
                    .ok_or_else(|| read_corrupt("a história indicada pelo recibo não existe"))?;
                transaction.commit().map_err(write_failed)?;
                return Ok(view);
            }

            let id = Uuid::new_v4().to_string();
            let competencies = json_array(&input.competencies)?;
            let content = serde_json::to_string(&input).map_err(serialize_failed)?;
            transaction
                .execute(
                    "INSERT INTO stories
                       (id, version, title, situation, task, actions, outcome,
                        competencies_json, created_at, updated_at)
                     VALUES (?1, 1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
                    params![
                        id,
                        input.title,
                        input.situation,
                        input.task,
                        input.actions,
                        input.outcome,
                        competencies,
                        now
                    ],
                )
                .map_err(write_failed)?;
            transaction
                .execute(
                    "INSERT INTO story_versions (story_id, version, content_json, created_at)
                     VALUES (?1, 1, ?2, ?3)",
                    params![id, content, now],
                )
                .map_err(write_failed)?;
            insert_story_receipt(
                &transaction,
                &request_id,
                "create_story",
                &payload_hash,
                &id,
            )?;

            let view = load_story_view(&transaction, &id)?
                .ok_or_else(|| read_corrupt("a história recém-criada não foi encontrada"))?;
            transaction.commit().map_err(write_failed)?;
            Ok(view)
        })
    }

    /// Edita uma história com controlo otimista de versão, append-only.
    ///
    /// A edição acrescenta uma versão em `story_versions`; nenhuma versão
    /// anterior é alterada nem eliminada. Não existem comandos de eliminação.
    pub fn update_story(&self, request: UpdateStory) -> Result<StoryView> {
        let request_id = request.request_id.trim().to_owned();
        if request_id.is_empty() {
            return Err(invalid_story_request());
        }
        let input = validate_story_input(request.input)?;
        let payload_hash = request_hash(&UpdateStoryPayload {
            id: &request.id,
            expected_version: request.expected_version,
            input: &input,
        })?;
        let now = utc_now();
        let expected = version_to_i64(request.expected_version)?;

        self.with_connection(|connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(write_failed)?;

            if let Some(story_id) =
                require_story_command(&transaction, &request_id, "update_story", &payload_hash)?
            {
                let view = load_story_view(&transaction, &story_id)?
                    .ok_or_else(|| read_corrupt("a história indicada pelo recibo não existe"))?;
                transaction.commit().map_err(write_failed)?;
                return Ok(view);
            }

            let competencies = json_array(&input.competencies)?;
            let updated = transaction
                .execute(
                    "UPDATE stories
                        SET title = ?1, situation = ?2, task = ?3, actions = ?4,
                            outcome = ?5, competencies_json = ?6,
                            version = version + 1, updated_at = ?7
                      WHERE id = ?8 AND version = ?9",
                    params![
                        input.title,
                        input.situation,
                        input.task,
                        input.actions,
                        input.outcome,
                        competencies,
                        now,
                        request.id,
                        expected
                    ],
                )
                .map_err(write_failed)?;
            if updated != 1 {
                return Err(stale_story());
            }
            let new_version: i64 = transaction
                .query_row(
                    "SELECT version FROM stories WHERE id = ?1",
                    [&request.id],
                    |row| row.get(0),
                )
                .map_err(read_failed)?;
            let content = serde_json::to_string(&input).map_err(serialize_failed)?;
            transaction
                .execute(
                    "INSERT INTO story_versions (story_id, version, content_json, created_at)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![request.id, new_version, content, now],
                )
                .map_err(write_failed)?;
            insert_story_receipt(
                &transaction,
                &request_id,
                "update_story",
                &payload_hash,
                &request.id,
            )?;

            let view = load_story_view(&transaction, &request.id)?
                .ok_or_else(|| read_corrupt("a história editada não foi encontrada"))?;
            transaction.commit().map_err(write_failed)?;
            Ok(view)
        })
    }

    /// Seleciona (ou remove) o apoio de história de uma resposta.
    ///
    /// Resolve o dono para o alvo concreto; o apoio de um seguimento resolve a
    /// preparação do slot `initial` da mesma pergunta. Só é permitido antes da
    /// primeira captura bem-sucedida e com a preparação editável.
    pub fn select_story(&self, request: SelectStory) -> Result<PreparationView> {
        let request_id = request.request_id.trim().to_owned();
        if request_id.is_empty() {
            return Err(invalid_story_request());
        }
        let payload_hash = request_hash(&SelectStoryPayload {
            owner: &request.owner,
            expected_preparation_version: request.expected_preparation_version,
            story_id: request.story_id.as_deref(),
            expected_story_version: request.expected_story_version,
        })?;
        let expected_preparation = version_to_i64(request.expected_preparation_version)?;

        self.with_connection(|connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(write_failed)?;

            if let Some(answer_id) =
                require_story_command(&transaction, &request_id, "select_story", &payload_hash)?
            {
                let view = load_preparation_in(&transaction, &answer_id)?;
                transaction.commit().map_err(write_failed)?;
                return Ok(view);
            }

            let resolved = resolve_capture_owner_in(&transaction, &request.owner)?;
            if !resolved.writable {
                return Err(story_not_writable());
            }
            let answer_id = preparation_target(&transaction, &resolved.slot.id)?;

            let preparation = load_preparation_row(&transaction, &answer_id)?;
            match &preparation {
                Some(row) => {
                    if row.lock_state != "editable" {
                        return Err(preparation_locked());
                    }
                    if row.version != request.expected_preparation_version {
                        return Err(stale_preparation());
                    }
                }
                None => {
                    if request.expected_preparation_version != 0 {
                        return Err(stale_preparation());
                    }
                    if has_evidence(&transaction, &answer_id)? {
                        return Err(historical_none_blocked());
                    }
                }
            }

            match (&request.story_id, &request.expected_story_version) {
                (None, None) => {
                    set_preparation_selection(
                        &transaction,
                        &answer_id,
                        None,
                        None,
                        expected_preparation,
                    )?;
                }
                (Some(story_id), Some(expected_story_version)) => {
                    let current: Option<i64> = transaction
                        .query_row(
                            "SELECT version FROM stories WHERE id = ?1",
                            [story_id],
                            |row| row.get(0),
                        )
                        .optional()
                        .map_err(read_failed)?;
                    let Some(current) = current else {
                        return Err(story_not_found());
                    };
                    if current != version_to_i64(*expected_story_version)? {
                        return Err(stale_story());
                    }
                    set_preparation_selection(
                        &transaction,
                        &answer_id,
                        Some(story_id),
                        Some(*expected_story_version),
                        expected_preparation,
                    )?;
                }
                _ => return Err(invalid_story_selection()),
            }

            insert_story_receipt(
                &transaction,
                &request_id,
                "select_story",
                &payload_hash,
                &answer_id,
            )?;
            let view = load_preparation_in(&transaction, &answer_id)?;
            transaction.commit().map_err(write_failed)?;
            Ok(view)
        })
    }

    /// Lê a preparação observável de uma resposta (inicial, seguimento ou tentativa).
    ///
    /// O apoio do seguimento resolve a preparação do slot `initial`; alvos sem
    /// linha e já com áudio publicado ou confirmação ficam com ausência
    /// histórica bloqueada, sem snapshot inventado.
    pub fn load_preparation(&self, answer_id: &str) -> Result<PreparationView> {
        self.with_connection(|connection| load_preparation_in(connection, answer_id))
    }

    /// A resposta tem uma preparação pendente de resolução, a resolver antes de
    /// gravar, abandonar ou confirmar.
    pub fn preparation_pending(&self, answer_id: &str) -> Result<bool> {
        self.with_connection(|connection| preparation_pending_for_answer(connection, answer_id))
    }

    /// Reserva o candidato imutável da preparação, antes de abrir o microfone.
    ///
    /// Se a transação falhar, o microfone não é aberto. Devolve o snapshot
    /// escolhido (ou `None` para ausência fixada) para o marcador de ficheiro;
    /// `Locked` significa que o apoio já estava fixado e a gravação prossegue.
    pub fn reserve_preparation_candidate(
        &self,
        answer_id: &str,
        recording_id: &str,
    ) -> Result<PreparationReservation> {
        let now = utc_now();
        self.with_connection(|connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(write_failed)?;

            let target = preparation_target(&transaction, answer_id)?;
            let preparation = load_preparation_row(&transaction, &target)?;
            let lock_state = preparation
                .as_ref()
                .map_or("editable", |row| row.lock_state.as_str());
            match lock_state {
                "editable" => {}
                "locked" => {
                    transaction.commit().map_err(write_failed)?;
                    return Ok(PreparationReservation::Locked);
                }
                _ => return Err(preparation_unresolved()),
            }

            let snapshot = match preparation
                .as_ref()
                .map(|row| (row.selected_story_id.as_deref(), row.selected_story_version))
            {
                Some((Some(story_id), Some(story_version))) => {
                    let content = load_story_version_input(&transaction, story_id, story_version)?
                        .ok_or_else(|| {
                            read_corrupt("a versão selecionada da história não foi encontrada")
                        })?;
                    Some(StorySnapshot {
                        story_id: story_id.to_owned(),
                        story_version,
                        content,
                    })
                }
                _ => None,
            };
            let snapshot_json = snapshot
                .as_ref()
                .map(|snapshot| serde_json::to_string(snapshot).map_err(serialize_failed))
                .transpose()?;
            let preparation_version = preparation.as_ref().map_or(0, |row| row.version);

            transaction
                .execute(
                    "INSERT INTO preparation_captures
                       (recording_id, answer_id, preparation_version, snapshot_json, state, created_at)
                     VALUES (?1, ?2, ?3, ?4, 'candidate', ?5)",
                    params![
                        recording_id,
                        target,
                        version_to_i64(preparation_version)?,
                        snapshot_json,
                        now
                    ],
                )
                .map_err(write_failed)?;
            if preparation.is_some() {
                transaction
                    .execute(
                        "UPDATE preparations
                            SET lock_state = 'pending_start', locked_capture_id = ?2
                          WHERE answer_id = ?1",
                        params![target, recording_id],
                    )
                    .map_err(write_failed)?;
            } else {
                transaction
                    .execute(
                        "INSERT INTO preparations
                           (answer_id, version, selected_story_id, selected_story_version,
                            locked_capture_id, lock_state)
                         VALUES (?1, 0, NULL, NULL, ?2, 'pending_start')",
                        params![target, recording_id],
                    )
                    .map_err(write_failed)?;
            }

            transaction.commit().map_err(write_failed)?;
            Ok(PreparationReservation::Reserved { snapshot })
        })
    }

    /// Marca a captura como iniciada: `started` no candidato, `locked` na
    /// preparação e a prática passa de `preparing` a `answering` em qualquer
    /// arranque bem-sucedido (com ou sem apoio). Idempotente para a recuperação.
    pub fn mark_preparation_started(&self, recording_id: &str) -> Result<()> {
        self.with_connection(|connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(write_failed)?;
            let capture = load_capture_row(&transaction, recording_id)?
                .ok_or_else(preparation_capture_missing)?;

            transaction
                .execute(
                    "UPDATE preparation_captures SET state = 'started' WHERE recording_id = ?1",
                    [recording_id],
                )
                .map_err(write_failed)?;
            transaction
                .execute(
                    "UPDATE preparations
                        SET lock_state = 'locked'
                      WHERE answer_id = ?1 AND locked_capture_id = ?2",
                    params![capture.answer_id, recording_id],
                )
                .map_err(write_failed)?;

            // A transição `preparing → answering` é incondicional: qualquer
            // arranque bem-sucedido de uma tentativa move a prática.
            if let Some(practice_id) = slot_practice_id(&transaction, &capture.answer_id)? {
                transaction
                    .execute(
                        "UPDATE practices SET status = 'answering'
                          WHERE id = ?1 AND status = 'preparing'",
                        [practice_id],
                    )
                    .map_err(write_failed)?;
            }
            transaction.commit().map_err(write_failed)?;
            Ok(())
        })
    }

    /// Marca a captura como falhada e liberta a preparação, conservando auditoria.
    pub fn mark_preparation_failed(&self, recording_id: &str) -> Result<()> {
        self.with_connection(|connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(write_failed)?;
            let capture = load_capture_row(&transaction, recording_id)?
                .ok_or_else(preparation_capture_missing)?;
            transaction
                .execute(
                    "UPDATE preparation_captures SET state = 'failed' WHERE recording_id = ?1",
                    [recording_id],
                )
                .map_err(write_failed)?;
            transaction
                .execute(
                    "UPDATE preparations
                        SET lock_state = 'editable', locked_capture_id = NULL
                      WHERE answer_id = ?1 AND locked_capture_id = ?2",
                    params![capture.answer_id, recording_id],
                )
                .map_err(write_failed)?;
            transaction.commit().map_err(write_failed)?;
            Ok(())
        })
    }

    /// Marca a captura como incerta, à espera de resolução explícita.
    pub fn mark_preparation_uncertain(&self, recording_id: &str) -> Result<()> {
        self.with_connection(|connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(write_failed)?;
            let capture = load_capture_row(&transaction, recording_id)?
                .ok_or_else(preparation_capture_missing)?;
            transaction
                .execute(
                    "UPDATE preparation_captures SET state = 'uncertain' WHERE recording_id = ?1",
                    [recording_id],
                )
                .map_err(write_failed)?;
            transaction
                .execute(
                    "UPDATE preparations
                        SET lock_state = 'start_uncertain'
                      WHERE answer_id = ?1 AND locked_capture_id = ?2",
                    params![capture.answer_id, recording_id],
                )
                .map_err(write_failed)?;
            transaction.commit().map_err(write_failed)?;
            Ok(())
        })
    }

    /// Resolve explicitamente uma preparação incerta, idempotente por `requestId`.
    ///
    /// `Keep` fixa o candidato (mesmo snapshot, sem consultar a versão atual);
    /// `DidNotStart` regista a decisão e liberta a seleção, conservando auditoria.
    pub fn resolve_preparation(
        &self,
        recording_id: &str,
        request_id: &str,
        decision: PreparationDecision,
    ) -> Result<PreparationView> {
        let request_id = request_id.trim().to_owned();
        if request_id.is_empty() {
            return Err(invalid_story_request());
        }
        let payload_hash = request_hash(&ResolvePreparationPayload {
            recording_id,
            decision,
        })?;

        self.with_connection(|connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(write_failed)?;

            if let Some(answer_id) = require_story_command(
                &transaction,
                &request_id,
                "resolve_preparation",
                &payload_hash,
            )? {
                let view = load_preparation_in(&transaction, &answer_id)?;
                transaction.commit().map_err(write_failed)?;
                return Ok(view);
            }

            let capture = load_capture_row(&transaction, recording_id)?
                .ok_or_else(preparation_capture_missing)?;
            match decision {
                PreparationDecision::Keep => {
                    transaction
                        .execute(
                            "UPDATE preparation_captures SET state = 'started'
                              WHERE recording_id = ?1",
                            [recording_id],
                        )
                        .map_err(write_failed)?;
                    transaction
                        .execute(
                            "UPDATE preparations SET lock_state = 'locked'
                              WHERE answer_id = ?1 AND locked_capture_id = ?2",
                            params![capture.answer_id, recording_id],
                        )
                        .map_err(write_failed)?;
                    // Transição `preparing → answering` incondicional.
                    if let Some(practice_id) = slot_practice_id(&transaction, &capture.answer_id)? {
                        transaction
                            .execute(
                                "UPDATE practices SET status = 'answering'
                                  WHERE id = ?1 AND status = 'preparing'",
                                [practice_id],
                            )
                            .map_err(write_failed)?;
                    }
                }
                PreparationDecision::DidNotStart => {
                    transaction
                        .execute(
                            "UPDATE preparation_captures SET state = 'failed'
                              WHERE recording_id = ?1",
                            [recording_id],
                        )
                        .map_err(write_failed)?;
                    transaction
                        .execute(
                            "UPDATE preparations
                                SET lock_state = 'editable', locked_capture_id = NULL
                              WHERE answer_id = ?1 AND locked_capture_id = ?2",
                            params![capture.answer_id, recording_id],
                        )
                        .map_err(write_failed)?;
                }
            }
            insert_story_receipt(
                &transaction,
                &request_id,
                "resolve_preparation",
                &payload_hash,
                &capture.answer_id,
            )?;

            let view = load_preparation_in(&transaction, &capture.answer_id)?;
            transaction.commit().map_err(write_failed)?;
            Ok(view)
        })
    }

    /// Capturas de preparação ainda por consolidar, com a localização dos ficheiros.
    pub(crate) fn pending_preparation_captures(&self) -> Result<Vec<PreparationCaptureLocation>> {
        self.with_connection(|connection| {
            let mut statement = connection
                .prepare(
                    "SELECT pc.recording_id, sq.session_id, r.session_question_id
                       FROM preparation_captures pc
                       JOIN recordings r ON r.id = pc.recording_id
                       JOIN answer_slots sq ON sq.id = r.session_question_id
                      WHERE pc.state IN ('candidate', 'started')
                      ORDER BY pc.created_at, pc.recording_id",
                )
                .map_err(read_failed)?;
            let mut rows = statement.query([]).map_err(read_failed)?;
            let mut captures = Vec::new();
            while let Some(row) = rows.next().map_err(read_failed)? {
                captures.push(PreparationCaptureLocation {
                    recording_id: row.get("recording_id").map_err(read_failed)?,
                    session_id: row.get("session_id").map_err(read_failed)?,
                    session_question_id: row.get("session_question_id").map_err(read_failed)?,
                });
            }
            Ok(captures)
        })
    }
    /// Localização (sessão, slot) de uma gravação de preparação, para o retry local.
    pub fn preparation_capture_location(&self, recording_id: &str) -> Result<Option<(Id, Id)>> {
        self.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT sq.session_id, r.session_question_id
                       FROM recordings r
                       JOIN answer_slots sq ON sq.id = r.session_question_id
                      WHERE r.id = ?1",
                    [recording_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(read_failed)
        })
    }
}

/// Preflight de um `requestId` de história: mesma tabela/kind/hash devolve a
/// entidade original; qualquer conflito ou conteúdo diferente é recusado.
fn require_story_command(
    connection: &Connection,
    request_id: &str,
    kind: &str,
    payload_hash: &str,
) -> Result<Option<Id>> {
    match lookup_command_identity(connection, request_id)? {
        CommandIdentity::NotFound => Ok(None),
        CommandIdentity::Conflict => Err(stale_request()),
        CommandIdentity::Found {
            location: CommandIdentityLocation::AiReceipt,
            kind: Some(found_kind),
            payload_hash: found_hash,
        } => {
            if found_kind == kind && found_hash == payload_hash {
                let entity: Option<Id> = connection
                    .query_row(
                        "SELECT entity_id FROM ai_receipts WHERE id = ?1",
                        [request_id],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(read_failed)?;
                Ok(entity)
            } else {
                Err(stale_request())
            }
        }
        CommandIdentity::Found { .. } => Err(stale_request()),
    }
}

fn insert_story_receipt(
    connection: &Connection,
    request_id: &str,
    kind: &str,
    payload_hash: &str,
    entity_id: &str,
) -> Result<()> {
    connection
        .execute(
            "INSERT INTO ai_receipts (id, kind, payload_hash, entity_id)
             VALUES (?1, ?2, ?3, ?4)",
            params![request_id, kind, payload_hash, entity_id],
        )
        .map_err(write_failed)?;
    Ok(())
}

/// Alvo da preparação: um seguimento resolve o slot `initial` da mesma pergunta.
fn preparation_target(connection: &Connection, answer_id: &str) -> Result<Id> {
    let row: Option<(String, Id)> = connection
        .query_row(
            "SELECT role, session_question_id FROM answer_slots WHERE id = ?1",
            [answer_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(read_failed)?;
    let Some((role, session_question_id)) = row else {
        return Err(answer_not_found());
    };
    Ok(if role == "follow_up" {
        session_question_id
    } else {
        answer_id.to_owned()
    })
}

/// Lê a preparação de um alvo já resolvido, com o snapshot fixado quando bloqueado.
pub(super) fn load_preparation_in(
    connection: &Connection,
    answer_id: &str,
) -> Result<PreparationView> {
    let target = preparation_target(connection, answer_id)?;
    let row = load_preparation_row(connection, &target)?;
    let Some(preparation) = row else {
        return if has_evidence(connection, &target)? {
            Ok(PreparationView {
                answer_id: target,
                version: 0,
                lock_state: LockState::Locked,
                selected_story_id: None,
                selected_story_version: None,
                snapshot: None,
                support: SupportKind::HistoricalNone,
                locked_capture_id: None,
            })
        } else {
            Ok(PreparationView {
                answer_id: target,
                version: 0,
                lock_state: LockState::Editable,
                selected_story_id: None,
                selected_story_version: None,
                snapshot: None,
                support: SupportKind::None,
                locked_capture_id: None,
            })
        };
    };

    let snapshot = match preparation.locked_capture_id.as_deref() {
        Some(recording_id) => load_snapshot(connection, recording_id)?,
        None => None,
    };
    let support = if snapshot.is_some() || preparation.selected_story_id.is_some() {
        SupportKind::Story
    } else {
        SupportKind::None
    };
    let lock_state = parse_lock_state(&preparation.lock_state)?;

    Ok(PreparationView {
        answer_id: target,
        version: preparation.version,
        lock_state,
        selected_story_id: preparation.selected_story_id,
        selected_story_version: preparation.selected_story_version,
        snapshot,
        support,
        locked_capture_id: preparation.locked_capture_id,
    })
}

fn load_preparation_row(
    connection: &Connection,
    answer_id: &str,
) -> Result<Option<StoredPreparation>> {
    let row = connection
        .query_row(
            "SELECT version, selected_story_id, selected_story_version,
                    locked_capture_id, lock_state
               FROM preparations
              WHERE answer_id = ?1",
            [answer_id],
            |row| {
                Ok((
                    row.get::<_, i64>("version")?,
                    row.get::<_, Option<Id>>("selected_story_id")?,
                    row.get::<_, Option<i64>>("selected_story_version")?,
                    row.get::<_, Option<Id>>("locked_capture_id")?,
                    row.get::<_, String>("lock_state")?,
                ))
            },
        )
        .optional()
        .map_err(read_failed)?;
    let Some((version, selected_story_id, selected_story_version, locked_capture_id, lock_state)) =
        row
    else {
        return Ok(None);
    };
    Ok(Some(StoredPreparation {
        version: Version::try_from(version)
            .map_err(|_| read_corrupt("versão de preparação inválida"))?,
        selected_story_id,
        selected_story_version: selected_story_version
            .map(Version::try_from)
            .transpose()
            .map_err(|_| read_corrupt("versão de história inválida na preparação"))?,
        locked_capture_id,
        lock_state,
    }))
}

fn load_capture_row(connection: &Connection, recording_id: &str) -> Result<Option<StoredCapture>> {
    let row = connection
        .query_row(
            "SELECT answer_id
               FROM preparation_captures
              WHERE recording_id = ?1",
            [recording_id],
            |row| row.get::<_, Id>("answer_id"),
        )
        .optional()
        .map_err(read_failed)?;
    let Some(answer_id) = row else {
        return Ok(None);
    };
    Ok(Some(StoredCapture { answer_id }))
}

fn load_snapshot(connection: &Connection, recording_id: &str) -> Result<Option<StorySnapshot>> {
    let snapshot_json: Option<String> = connection
        .query_row(
            "SELECT snapshot_json FROM preparation_captures WHERE recording_id = ?1",
            [recording_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(read_failed)?
        .flatten();
    let Some(snapshot_json) = snapshot_json else {
        return Ok(None);
    };
    serde_json::from_str(&snapshot_json)
        .map_err(|error| read_corrupt(&format!("snapshot de preparação ilegível: {error}")))
}

fn load_story_view(connection: &Connection, id: &str) -> Result<Option<StoryView>> {
    let row = connection
        .query_row(
            "SELECT id, version, title, situation, task, actions, outcome,
                    competencies_json, created_at, updated_at
               FROM stories
              WHERE id = ?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, Id>("id")?,
                    row.get::<_, i64>("version")?,
                    row.get::<_, String>("title")?,
                    row.get::<_, String>("situation")?,
                    row.get::<_, String>("task")?,
                    row.get::<_, String>("actions")?,
                    row.get::<_, String>("outcome")?,
                    row.get::<_, String>("competencies_json")?,
                    row.get::<_, String>("created_at")?,
                    row.get::<_, String>("updated_at")?,
                ))
            },
        )
        .optional()
        .map_err(read_failed)?;
    let Some((
        id,
        version,
        title,
        situation,
        task,
        actions,
        outcome,
        competencies_json,
        created_at,
        updated_at,
    )) = row
    else {
        return Ok(None);
    };
    let input = StoryInput {
        title,
        situation,
        task,
        actions,
        outcome,
        competencies: json_list(&competencies_json, "competências da história")?,
    };
    let missing = missing_fields(&input);
    Ok(Some(StoryView {
        id,
        version: Version::try_from(version)
            .map_err(|_| read_corrupt("versão de história inválida"))?,
        title: input.title,
        situation: input.situation,
        task: input.task,
        actions: input.actions,
        outcome: input.outcome,
        competencies: input.competencies,
        missing_fields: missing,
        created_at,
        updated_at,
    }))
}

/// Conteúdo de uma versão concreta de uma história, para fixar o snapshot.
fn load_story_version_input(
    connection: &Connection,
    story_id: &str,
    version: Version,
) -> Result<Option<StoryInput>> {
    let content_json: Option<String> = connection
        .query_row(
            "SELECT content_json FROM story_versions WHERE story_id = ?1 AND version = ?2",
            params![story_id, version_to_i64(version)?],
            |row| row.get(0),
        )
        .optional()
        .map_err(read_failed)?;
    let Some(content_json) = content_json else {
        return Ok(None);
    };
    serde_json::from_str(&content_json)
        .map_err(|error| read_corrupt(&format!("versão de história ilegível: {error}")))
}

/// A resposta já tem evidência de captura (áudio publicado ou confirmação).
fn has_evidence(connection: &Connection, answer_id: &str) -> Result<bool> {
    let confirmed: bool = connection
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM confirmations WHERE session_question_id = ?1)",
            [answer_id],
            |row| row.get(0),
        )
        .map_err(read_failed)?;
    if confirmed {
        return Ok(true);
    }
    connection
        .query_row(
            "SELECT EXISTS (
                SELECT 1 FROM recordings WHERE session_question_id = ?1 AND state = 'ready'
             )",
            [answer_id],
            |row| row.get(0),
        )
        .map_err(read_failed)
}

fn set_preparation_selection(
    connection: &Connection,
    answer_id: &str,
    story_id: Option<&str>,
    story_version: Option<Version>,
    expected_version: i64,
) -> Result<()> {
    let story_version = story_version.map(version_to_i64).transpose()?;
    let existed: bool = connection
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM preparations WHERE answer_id = ?1)",
            [answer_id],
            |row| row.get(0),
        )
        .map_err(read_failed)?;
    if existed {
        let updated = connection
            .execute(
                "UPDATE preparations
                    SET selected_story_id = ?1, selected_story_version = ?2,
                        version = version + 1
                  WHERE answer_id = ?3 AND version = ?4",
                params![story_id, story_version, answer_id, expected_version],
            )
            .map_err(write_failed)?;
        if updated != 1 {
            return Err(stale_preparation());
        }
    } else {
        if expected_version != 0 {
            return Err(stale_preparation());
        }
        connection
            .execute(
                "INSERT INTO preparations
                   (answer_id, version, selected_story_id, selected_story_version,
                    locked_capture_id, lock_state)
                 VALUES (?1, 1, ?2, ?3, NULL, 'editable')",
                params![answer_id, story_id, story_version],
            )
            .map_err(write_failed)?;
    }
    Ok(())
}

fn slot_practice_id(connection: &Connection, answer_id: &str) -> Result<Option<Id>> {
    let practice_id: Option<Option<Id>> = connection
        .query_row(
            "SELECT practice_id FROM answer_slots WHERE id = ?1",
            [answer_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(read_failed)?;
    Ok(practice_id.flatten())
}

/// A preparação de uma resposta está pendente de resolução (`pending_start` ou
/// `start_uncertain`).
pub(super) fn preparation_pending_for_answer(
    connection: &Connection,
    answer_id: &str,
) -> Result<bool> {
    let lock: Option<String> = connection
        .query_row(
            "SELECT lock_state FROM preparations WHERE answer_id = ?1",
            [answer_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(read_failed)?;
    Ok(matches!(
        lock.as_deref(),
        Some("pending_start" | "start_uncertain")
    ))
}

/// A tentativa de uma prática tem uma preparação pendente de resolução.
pub(super) fn practice_preparation_pending(
    connection: &Connection,
    practice_id: &str,
) -> Result<bool> {
    let answer_id: Option<Id> = connection
        .query_row(
            "SELECT answer_id FROM practices WHERE id = ?1",
            [practice_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(read_failed)?;
    let Some(answer_id) = answer_id else {
        return Ok(false);
    };
    preparation_pending_for_answer(connection, &answer_id)
}

/// Erro de pendência de preparação, partilhado por gravação, abandono e confirmação.
pub(super) fn preparation_unresolved() -> AppError {
    AppError::new(
        error_code::BUSY,
        "Há uma preparação pendente para resolver ou retomar. Resolva-a antes de continuar.",
        true,
    )
}

fn parse_lock_state(value: &str) -> Result<LockState> {
    match value {
        "editable" => Ok(LockState::Editable),
        "pending_start" => Ok(LockState::PendingStart),
        "locked" => Ok(LockState::Locked),
        "start_uncertain" => Ok(LockState::StartUncertain),
        other => Err(read_corrupt(&format!(
            "estado de bloqueio de preparação desconhecido «{other}»"
        ))),
    }
}

fn version_to_i64(version: Version) -> Result<i64> {
    i64::try_from(version).map_err(|_| invalid_version())
}

/// Campos do pedido de edição sem o `requestId`.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateStoryPayload<'a> {
    id: &'a str,
    expected_version: Version,
    input: &'a StoryInput,
}

/// Campos do pedido de seleção sem o `requestId`.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SelectStoryPayload<'a> {
    owner: &'a crate::practice::CaptureOwner,
    expected_preparation_version: Version,
    story_id: Option<&'a str>,
    expected_story_version: Option<Version>,
}

/// Campos da resolução sem o `requestId`.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ResolvePreparationPayload<'a> {
    recording_id: &'a str,
    decision: PreparationDecision,
}

fn invalid_story(message: impl Into<String>) -> AppError {
    AppError::new(error_code::INVALID_INPUT, message, false)
}

fn invalid_story_request() -> AppError {
    AppError::new(
        error_code::INVALID_INPUT,
        "O pedido de história não tem identificador. Repita a operação.",
        false,
    )
}

fn invalid_version() -> AppError {
    AppError::new(
        error_code::INVALID_INPUT,
        "A versão indicada excede o intervalo guardável.",
        false,
    )
}

fn story_not_found() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "A história indicada não existe. Atualize o banco de histórias.",
        false,
    )
}

fn stale_story() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "A história mudou entretanto. Atualize o banco de histórias antes de continuar.",
        false,
    )
}

fn story_not_writable() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "Esta resposta já não aceita um apoio de história; nada foi aplicado.",
        false,
    )
}

fn preparation_locked() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "O apoio desta resposta já está fixado e não pode ser alterado.",
        false,
    )
}

fn stale_preparation() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "A preparação mudou entretanto. Atualize o estado antes de continuar.",
        false,
    )
}

fn historical_none_blocked() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "Esta resposta já tem áudio ou confirmação e não aceita um apoio de história novo.",
        false,
    )
}

fn invalid_story_selection() -> AppError {
    AppError::new(
        error_code::INVALID_INPUT,
        "Indique a história e a versão esperada, ou remova o apoio por completo.",
        false,
    )
}

fn answer_not_found() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "A resposta indicada não existe. Atualize o estado antes de continuar.",
        false,
    )
}

fn preparation_capture_missing() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "A captura de preparação indicada não existe. Atualize o estado antes de continuar.",
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

fn read_failed(error: rusqlite::Error) -> AppError {
    AppError::new(
        error_code::DATABASE_UNAVAILABLE,
        format!("Não foi possível ler as histórias guardadas: {error}"),
        false,
    )
}

fn read_corrupt(detail: &str) -> AppError {
    AppError::new(
        error_code::DATABASE_UNAVAILABLE,
        format!("Não foi possível ler as histórias guardadas: {detail}."),
        false,
    )
}

fn serialize_failed(error: serde_json::Error) -> AppError {
    AppError::new(
        error_code::STORAGE_WRITE_FAILED,
        format!("Não foi possível preparar a história: {error}"),
        false,
    )
}

fn write_failed(error: rusqlite::Error) -> AppError {
    AppError::new(
        error_code::STORAGE_WRITE_FAILED,
        format!("Não foi possível guardar a história; nenhuma alteração foi aplicada: {error}"),
        true,
    )
}

//! Gravações, respostas, revisões e confirmações das entrevistas guiadas.
//!
//! A base é a autoridade das linhas; os ficheiros são geridos por
//! `artifacts::ArtifactStore`. Uma gravação é reservada antes da captura, o
//! `original.wav` finalizado é validado com Hound e a publicação liga o
//! ficheiro já guardado à linha reservada — é idempotente pelo identificador e
//! nunca duplica registos. A seleção só avança para uma gravação de ordinal
//! superior e apenas enquanto a pergunta não estiver confirmada; uma publicação
//! atrasada fica guardada e legível sem substituir a seleção mais recente.
//!
//! A transcrição automática é guardada na mesma transação que encerra a sua
//! operação: um resultado só fecha o pedido que o originou, um resultado tardio
//! não é aplicado e uma transcrição já gravada nunca é reescrita.
//!
//! Guardar uma revisão acrescenta uma versão nova (`n+1`) e nunca reescreve nem
//! elimina versões anteriores; não confirma nem avança a sessão. Confirmar é uma
//! única transação: valida a versão da revisão, a seleção da gravação e a versão
//! da sessão, insere a revisão exata que o utilizador viu quando o texto mudou,
//! grava a confirmação imutável, incrementa `sessions.version` e escreve o
//! recibo. Qualquer falha reverte as duas escritas e o recibo. O avanço da
//! sessão pertence a `sessions.rs`, que reutiliza estes auxiliares.
//!
//! A divisão entre ficheiros e base não tem transação comum. A recuperação
//! (`artifacts::reconcile`) inspeciona as reservas pendentes e publica os
//! originais fechados; nenhuma falha de escrita apaga o áudio.

use std::ffi::OsStr;
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Row, TransactionBehavior};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::questions::utc_now;
use super::Database;
use crate::artifacts::{
    inspect_file, normalized_reference, original_reference, validate_reference,
};
use crate::audio::{CapturedAudio, ORIGINAL_FILE};
use crate::domain::{error_code, AppError, CaptureMeta, Transcript, TranscriptStatus};
use crate::interview::{
    validate_review_text, AnswerTarget, ConfirmAnswer, Confirmation, Id, OperationTarget,
    RecordingState, RecordingView, Result, ReviewDraft, ReviewOrigin, SaveReview, Version,
};

/// Reserva que ainda pode ter um original por publicar.
pub(crate) struct PendingRecording {
    pub(crate) id: Id,
    pub(crate) session_id: Id,
    pub(crate) session_question_id: Id,
}

/// Localização de uma gravação, para reconhecer pastas órfãs no disco.
pub(crate) struct RecordingLocation {
    pub(crate) id: Id,
    pub(crate) session_id: Id,
    pub(crate) session_question_id: Id,
}

/// Estado da pergunta da sessão relevante para gravar e publicar.
struct AnswerQuestion {
    session_id: Id,
    selection_version: Version,
    confirmed: bool,
}

/// Linha de gravação com a sessão a que pertence.
struct StoredRecording {
    session_id: Id,
    session_question_id: Id,
    ordinal: i64,
    state: RecordingState,
}

/// Operação já registada para um resultado de transcrição.
struct StoredOperation {
    state: String,
    session_id: Option<Id>,
    session_question_id: Option<Id>,
    recording_id: Option<Id>,
    worker_generation: Option<i64>,
}

/// Transcrição já guardada com um identificador.
struct StoredTranscript {
    recording_id: Id,
    status: String,
    text: String,
}

/// Estado da pergunta relevante para guardar, confirmar e avançar.
struct ReviewTarget {
    session_id: Id,
    selection_version: Version,
    selected_recording_id: Option<Id>,
    confirmed: bool,
    session_version: Version,
}

/// Revisão lida da base, antes de ser convertida no DTO.
struct StoredReviewRow {
    recording_id: Id,
    version: i64,
    source_transcript_id: Option<Id>,
    origin: String,
    text: String,
    saved_at: String,
}

/// Confirmação lida da base, antes de ser convertida no DTO.
struct StoredConfirmationRow {
    id: Id,
    session_question_id: Id,
    recording_id: Id,
    review_version: i64,
    source_transcript_id: Option<Id>,
    origin: String,
    text: String,
    confirmed_at: String,
}

/// Recibo de um pedido já aplicado, partilhado por `sessions.rs`.
pub(super) struct Receipt {
    pub(super) kind: String,
    pub(super) payload_hash: String,
    pub(super) session_id: Option<Id>,
    pub(super) entity_id: Option<Id>,
    pub(super) result_version: Option<Version>,
}

impl Database {
    /// Reserva a próxima gravação de uma pergunta da sessão, antes da captura.
    ///
    /// O identificador e o ordinal são gerados aqui e a linha fica em
    /// `reserved`. Uma seleção esperada desatualizada devolve `STALE_SOURCE`;
    /// uma pergunta já confirmada devolve `ANSWER_CONFIRMED`. Nada é escrito
    /// nas recusas.
    pub fn reserve_recording(
        &self,
        session_question_id: &str,
        expected_selection: Version,
    ) -> Result<RecordingView> {
        let now = utc_now();
        self.with_connection(|connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(write_failed)?;

            let question = find_answer_question(&transaction, session_question_id)?
                .ok_or_else(question_not_found)?;
            if question.selection_version != expected_selection {
                return Err(stale_selection());
            }
            if question.confirmed {
                return Err(answer_confirmed());
            }

            let ordinal: i64 = transaction
                .query_row(
                    "SELECT COALESCE(MAX(ordinal), 0) + 1
                       FROM recordings
                      WHERE session_question_id = ?1",
                    [session_question_id],
                    |row| row.get(0),
                )
                .map_err(read_failed)?;
            let id = Uuid::new_v4().to_string();
            transaction
                .execute(
                    "INSERT INTO recordings
                       (id, session_question_id, ordinal, state, original_file,
                        normalized_file, capture_json, created_at, updated_at)
                     VALUES (?1, ?2, ?3, 'reserved', NULL, NULL, NULL, ?4, ?4)",
                    params![id, session_question_id, ordinal, now],
                )
                .map_err(write_failed)?;

            let view = load_recording_view(&transaction, &id)?
                .ok_or_else(|| read_corrupt("a reserva recém-criada não foi encontrada"))?;
            transaction.commit().map_err(write_failed)?;
            Ok(view)
        })
    }

    /// Publica um original finalizado, ligando-o à linha já reservada.
    ///
    /// O WAV é validado com Hound antes de a linha mudar. A publicação é
    /// idempotente pelo identificador: repeti-la devolve o estado atual. A
    /// seleção só avança para um ordinal superior e numa pergunta ainda não
    /// confirmada; as gravações anteriores ficam prontas e legíveis.
    pub fn publish_recording(&self, capture: CapturedAudio) -> Result<RecordingView> {
        // A validação é feita fora da transação: nenhum I/O de ficheiro com a
        // ligação bloqueada. O fecho real vem do ficheiro; `incomplete` vem da
        // captura, que é a única a saber se houve uma interrupção.
        let measured = inspect_file(&capture.original)?;
        let meta = CaptureMeta {
            incomplete: capture.meta.incomplete,
            ..measured
        };
        let capture_json = serde_json::to_string(&meta).map_err(configuration_failed)?;
        let now = utc_now();

        self.with_connection(|connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(write_failed)?;

            let recording = find_stored_recording(&transaction, &capture.id)?
                .ok_or_else(recording_not_found)?;
            if !original_matches(&capture, &recording) {
                return Err(foreign_audio());
            }
            if recording.state == RecordingState::Ready {
                let view = load_recording_view(&transaction, &capture.id)?
                    .ok_or_else(recording_not_found)?;
                transaction.commit().map_err(write_failed)?;
                return Ok(view);
            }

            let reference = original_reference(
                &recording.session_id,
                &recording.session_question_id,
                &capture.id,
            )?;
            transaction
                .execute(
                    "UPDATE recordings
                        SET state = 'ready', original_file = ?2,
                            capture_json = ?3, updated_at = ?4
                      WHERE id = ?1",
                    params![capture.id, reference, capture_json, now],
                )
                .map_err(write_failed)?;

            // Só avança a seleção: um ordinal superior, numa pergunta ainda não
            // confirmada. A subconsulta garante que uma publicação atrasada de
            // uma gravação antiga nunca substitui a seleção mais recente.
            transaction
                .execute(
                    "UPDATE session_questions
                        SET selected_recording_id = ?1,
                            selection_version = selection_version + 1
                      WHERE id = ?2
                        AND NOT EXISTS (
                            SELECT 1 FROM confirmations c
                             WHERE c.session_question_id = ?2
                        )
                        AND ?3 > COALESCE(
                            (SELECT r.ordinal
                               FROM recordings r
                              WHERE r.id = session_questions.selected_recording_id),
                            0)",
                    params![capture.id, recording.session_question_id, recording.ordinal],
                )
                .map_err(write_failed)?;

            let view =
                load_recording_view(&transaction, &capture.id)?.ok_or_else(recording_not_found)?;
            transaction.commit().map_err(write_failed)?;
            Ok(view)
        })
    }

    /// Guarda um resultado automático e encerra a operação que o originou.
    ///
    /// O alvo tem de pertencer à sessão e à pergunta, a versão da seleção tem
    /// de ser a atual e a resposta não pode estar confirmada. Se a operação
    /// ainda não estiver registada, é criada já encerrada na mesma transação;
    /// uma operação já terminada recusa o resultado. Uma transcrição com o
    /// mesmo identificador e o mesmo texto é devolvida sem escrever; com outro
    /// texto, nada é reescrito.
    pub fn append_transcript(
        &self,
        target: OperationTarget,
        transcript: Transcript,
        configuration: Value,
    ) -> Result<RecordingView> {
        let recording_id = target
            .recording_id
            .clone()
            .ok_or_else(transcript_without_recording)?;
        let config_json = serde_json::to_string(&configuration).map_err(configuration_failed)?;
        let segments_json =
            serde_json::to_string(&transcript.segments).map_err(configuration_failed)?;
        let status = match transcript.status {
            TranscriptStatus::Completed => "completed",
            TranscriptStatus::NoSpeech => "no_speech",
        };
        let generation = match target.worker_generation {
            0 => None,
            value => Some(i64::try_from(value).map_err(|_| invalid_generation())?),
        };
        let now = utc_now();
        let load_ms = i64::try_from(transcript.load_ms).map_err(|_| invalid_duration())?;
        let processing_ms =
            i64::try_from(transcript.processing_ms).map_err(|_| invalid_duration())?;

        self.with_connection(|connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(write_failed)?;

            let question = find_answer_question(&transaction, &target.session_question_id)?
                .ok_or_else(question_not_found)?;
            if question.session_id != target.session_id {
                return Err(stale_target());
            }
            if question.confirmed {
                return Err(answer_confirmed());
            }
            if question.selection_version != target.selection_version {
                return Err(stale_target());
            }

            let owner: Option<String> = transaction
                .query_row(
                    "SELECT session_question_id FROM recordings WHERE id = ?1",
                    [&recording_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(read_failed)?;
            if owner.as_deref() != Some(target.session_question_id.as_str()) {
                return Err(transcript_unknown_recording());
            }

            if let Some(existing) = find_transcript(&transaction, &transcript.id)? {
                if existing.recording_id == recording_id
                    && existing.status == status
                    && existing.text == transcript.text
                {
                    let view = load_recording_view(&transaction, &recording_id)?
                        .ok_or_else(recording_not_found)?;
                    transaction.commit().map_err(write_failed)?;
                    return Ok(view);
                }
                return Err(transcript_already_saved());
            }

            match find_operation(&transaction, &target.operation_id)? {
                Some(operation) => {
                    if operation.state != "running" {
                        return Err(operation_closed());
                    }
                    if operation
                        .session_id
                        .as_deref()
                        .is_some_and(|stored| stored != target.session_id)
                        || operation
                            .session_question_id
                            .as_deref()
                            .is_some_and(|stored| stored != target.session_question_id)
                        || operation
                            .recording_id
                            .as_deref()
                            .is_some_and(|stored| stored != recording_id)
                        || operation
                            .worker_generation
                            .is_some_and(|stored| Some(stored) != generation)
                    {
                        return Err(operation_mismatch());
                    }
                    transaction
                        .execute(
                            "UPDATE operations
                                SET state = ?2, finished_at = ?3, error_json = NULL
                              WHERE id = ?1 AND state = 'running'",
                            params![target.operation_id, status, now],
                        )
                        .map_err(write_failed)?;
                }
                None => {
                    transaction
                        .execute(
                            "INSERT INTO operations
                               (id, kind, state, session_id, session_question_id,
                                recording_id, worker_generation, source_config_json,
                                error_json, created_at, finished_at)
                             VALUES (?1, 'transcribe', ?2, ?3, ?4, ?5, ?6, ?7, NULL, ?8, ?8)",
                            params![
                                target.operation_id,
                                status,
                                target.session_id,
                                target.session_question_id,
                                recording_id,
                                generation,
                                config_json,
                                now
                            ],
                        )
                        .map_err(write_failed)?;
                }
            }

            transaction
                .execute(
                    "INSERT INTO transcripts
                       (id, recording_id, status, text, segments_json, model, language,
                        config_json, load_ms, processing_ms, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                    params![
                        transcript.id,
                        recording_id,
                        status,
                        transcript.text,
                        segments_json,
                        transcript.model,
                        transcript.language,
                        config_json,
                        load_ms,
                        processing_ms,
                        transcript.created_at
                    ],
                )
                .map_err(write_failed)?;

            let view = load_recording_view(&transaction, &recording_id)?
                .ok_or_else(recording_not_found)?;
            transaction.commit().map_err(write_failed)?;
            Ok(view)
        })
    }

    /// Regista a referência do derivado normalizado de uma gravação publicada.
    ///
    /// A referência tem de ser exatamente a canónica desta gravação, relativa à
    /// raiz gerida; qualquer desvio é `INVALID_INPUT`. Uma gravação ainda não
    /// publicada ou desconhecida devolve `STALE_SOURCE`.
    pub fn set_normalized(&self, recording_id: &str, relative_path: &str) -> Result<()> {
        validate_reference(relative_path)?;
        let now = utc_now();
        self.with_connection(|connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(write_failed)?;

            let recording = find_stored_recording(&transaction, recording_id)?
                .ok_or_else(recording_not_found)?;
            let expected = normalized_reference(
                &recording.session_id,
                &recording.session_question_id,
                recording_id,
            )?;
            if expected != relative_path {
                return Err(invalid_normalized_reference());
            }
            if recording.state != RecordingState::Ready {
                return Err(normalized_before_publication());
            }

            transaction
                .execute(
                    "UPDATE recordings SET normalized_file = ?2, updated_at = ?3 WHERE id = ?1",
                    params![recording_id, relative_path, now],
                )
                .map_err(write_failed)?;
            transaction.commit().map_err(write_failed)?;
            Ok(())
        })
    }

    /// Guarda uma versão nova da revisão de uma resposta, sem confirmar.
    ///
    /// As revisões são só de acréscimo: a versão `n+1` junta-se às anteriores e
    /// nenhuma versão guardada é reescrita. Guardar não confirma, não avança a
    /// sessão e não muda `sessions.version`. A versão de revisão esperada, a
    /// seleção da gravação e a versão de seleção têm de corresponder ao estado
    /// atual; uma pergunta já confirmada devolve `ANSWER_CONFIRMED`. Texto vazio
    /// ou só com espaços é `INVALID_INPUT`; fora isso, o texto é guardado
    /// exatamente como recebido.
    pub fn save_review(&self, request: SaveReview) -> Result<ReviewDraft> {
        let request_id = request.request_id.trim().to_owned();
        if request_id.is_empty() {
            return Err(invalid_review_request());
        }
        validate_review_text(&request.text)?;
        let payload_hash = request_hash(&SaveReviewPayload {
            target: &request.target,
            expected_review_version: request.expected_review_version,
            source_transcript_id: request.source_transcript_id.as_deref(),
            origin: request.origin,
            text: &request.text,
        })?;
        let now = utc_now();

        self.with_connection(|connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(write_failed)?;

            // O recibo é consultado antes das versões: repetir um pedido
            // guardado devolve a mesma revisão sem escrever outra.
            if let Some(receipt) = find_receipt(&transaction, &request_id)? {
                return replay_save_review(&transaction, &receipt, &payload_hash);
            }

            let target = require_review_target(&transaction, &request.target, None)?;
            require_selected_recording(&transaction, &request.target, &target)?;
            require_transcript_owner(
                &transaction,
                &request.target.recording_id,
                request.source_transcript_id.as_deref(),
            )?;

            let current = load_review(&transaction, &request.target.recording_id)?;
            let expected = current.as_ref().map_or(0, |review| review.version);
            if expected != request.expected_review_version {
                return Err(stale_review());
            }
            let version = expected.checked_add(1).ok_or_else(invalid_version)?;
            insert_review(
                &transaction,
                &request.target.recording_id,
                version,
                request.source_transcript_id.as_deref(),
                request.origin,
                &request.text,
                &now,
            )?;
            insert_receipt(
                &transaction,
                &request_id,
                "save_review",
                &payload_hash,
                Some(&request.target.session_id),
                Some(&request.target.recording_id),
                Some(version),
            )?;

            let draft = load_review(&transaction, &request.target.recording_id)?
                .ok_or_else(|| read_corrupt("a revisão recém-guardada não foi encontrada"))?;
            transaction.commit().map_err(write_failed)?;
            Ok(draft)
        })
    }

    /// Confirma a resposta num único commit, com o texto exato que o utilizador viu.
    ///
    /// Valida, dentro de uma transação `BEGIN IMMEDIATE`, a versão de revisão
    /// esperada, a seleção da gravação e a versão esperada da sessão; exige uma
    /// gravação finalizada e não confirmada. Se o texto mudou em relação à
    /// revisão guardada, acrescenta uma versão a essa revisão e confirma essa
    /// versão; caso contrário reutiliza a versão existente. A confirmação fica
    /// ligada à pergunta, à gravação e à versão da revisão, a versão da sessão
    /// avança e o recibo é escrito na mesma transação. Uma repetição do mesmo
    /// pedido devolve a confirmação guardada antes de recusar por já confirmado.
    pub fn confirm_answer(&self, request: ConfirmAnswer) -> Result<Confirmation> {
        let request_id = request.request_id.trim().to_owned();
        if request_id.is_empty() {
            return Err(invalid_confirmation_request());
        }
        validate_review_text(&request.text)?;
        let payload_hash = request_hash(&ConfirmAnswerPayload {
            target: &request.target,
            expected_review_version: request.expected_review_version,
            source_transcript_id: request.source_transcript_id.as_deref(),
            origin: request.origin,
            text: &request.text,
            expected_session_version: request.expected_session_version,
        })?;
        let now = utc_now();

        self.with_connection(|connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(write_failed)?;

            if let Some(receipt) = find_receipt(&transaction, &request_id)? {
                return replay_confirmation(&transaction, &receipt, &payload_hash);
            }

            let target = require_review_target(
                &transaction,
                &request.target,
                Some(request.expected_session_version),
            )?;
            require_selected_recording(&transaction, &request.target, &target)?;
            require_transcript_owner(
                &transaction,
                &request.target.recording_id,
                request.source_transcript_id.as_deref(),
            )?;

            let current = load_review(&transaction, &request.target.recording_id)?;
            let expected = current.as_ref().map_or(0, |review| review.version);
            if expected != request.expected_review_version {
                return Err(stale_review());
            }

            // Só uma revisão diferente do estado guardado acrescenta versão; o
            // texto confirmado é sempre o que o pedido trouxe, sem `trim`.
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
                    insert_review(
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
                        request.target.session_question_id,
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
                    "UPDATE sessions
                        SET version = version + 1
                      WHERE id = ?1 AND version = ?2 AND status = 'in_progress'",
                    params![
                        target.session_id,
                        version_to_i64(request.expected_session_version)?
                    ],
                )
                .map_err(write_failed)?;
            if updated != 1 {
                return Err(stale_session());
            }
            let session_version = request
                .expected_session_version
                .checked_add(1)
                .ok_or_else(invalid_version)?;
            insert_receipt(
                &transaction,
                &request_id,
                "confirm_answer",
                &payload_hash,
                Some(&target.session_id),
                Some(&confirmation_id),
                Some(session_version),
            )?;

            let confirmation =
                load_confirmation(&transaction, &request.target.session_question_id)?.ok_or_else(
                    || read_corrupt("a confirmação recém-guardada não foi encontrada"),
                )?;
            transaction.commit().map_err(write_failed)?;
            Ok(confirmation)
        })
    }

    /// Gravações que podem ter um original ainda por publicar.
    pub(crate) fn pending_recordings(&self) -> Result<Vec<PendingRecording>> {
        self.with_connection(|connection| {
            let mut statement = connection
                .prepare(
                    "SELECT r.id, sq.session_id, r.session_question_id
                       FROM recordings r
                       JOIN session_questions sq ON sq.id = r.session_question_id
                      WHERE r.state IN ('reserved', 'capturing', 'publish_pending')
                      ORDER BY r.created_at, r.rowid",
                )
                .map_err(read_failed)?;
            let mut rows = statement.query([]).map_err(read_failed)?;
            let mut pending = Vec::new();
            while let Some(row) = rows.next().map_err(read_failed)? {
                pending.push(PendingRecording {
                    id: row.get("id").map_err(read_failed)?,
                    session_id: row.get("session_id").map_err(read_failed)?,
                    session_question_id: row.get("session_question_id").map_err(read_failed)?,
                });
            }
            Ok(pending)
        })
    }

    /// Todas as gravações guardadas, para reconhecer pastas órfãs.
    pub(crate) fn recording_locations(&self) -> Result<Vec<RecordingLocation>> {
        self.with_connection(|connection| {
            let mut statement = connection
                .prepare(
                    "SELECT r.id, sq.session_id, r.session_question_id
                       FROM recordings r
                       JOIN session_questions sq ON sq.id = r.session_question_id",
                )
                .map_err(read_failed)?;
            let mut rows = statement.query([]).map_err(read_failed)?;
            let mut locations = Vec::new();
            while let Some(row) = rows.next().map_err(read_failed)? {
                locations.push(RecordingLocation {
                    id: row.get("id").map_err(read_failed)?,
                    session_id: row.get("session_id").map_err(read_failed)?,
                    session_question_id: row.get("session_question_id").map_err(read_failed)?,
                });
            }
            Ok(locations)
        })
    }

    /// Marca uma reserva sem original como captura interrompida.
    ///
    /// Uma gravação já pronta nunca é alterada por esta via.
    pub(crate) fn mark_interrupted(&self, recording_id: &str) -> Result<()> {
        let now = utc_now();
        self.with_connection(|connection| {
            connection
                .execute(
                    "UPDATE recordings
                        SET state = 'interrupted', updated_at = ?2
                      WHERE id = ?1 AND state <> 'ready'",
                    params![recording_id, now],
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
}

/// Gravações de uma pergunta da sessão, pela ordem de captura.
pub(super) fn load_recordings(
    connection: &Connection,
    session_question_id: &str,
) -> Result<Vec<RecordingView>> {
    let mut statement = connection
        .prepare(
            "SELECT id FROM recordings
              WHERE session_question_id = ?1
              ORDER BY ordinal",
        )
        .map_err(read_failed)?;
    let mut rows = statement
        .query([session_question_id])
        .map_err(read_failed)?;
    let mut ids = Vec::new();
    while let Some(row) = rows.next().map_err(read_failed)? {
        ids.push(row.get::<_, Id>("id").map_err(read_failed)?);
    }
    drop(rows);
    drop(statement);

    let mut recordings = Vec::with_capacity(ids.len());
    for id in ids {
        recordings.push(
            load_recording_view(connection, &id)?
                .ok_or_else(|| read_corrupt("uma gravação da pergunta não foi encontrada"))?,
        );
    }
    Ok(recordings)
}

/// Lê uma gravação com os resultados automáticos e a revisão guardada.
fn load_recording_view(
    connection: &Connection,
    recording_id: &str,
) -> Result<Option<RecordingView>> {
    let row = connection
        .query_row(
            "SELECT id, session_question_id, ordinal, state, original_file,
                    normalized_file, capture_json
               FROM recordings
              WHERE id = ?1",
            [recording_id],
            |row| {
                Ok((
                    row.get::<_, Id>("id")?,
                    row.get::<_, Id>("session_question_id")?,
                    row.get::<_, i64>("ordinal")?,
                    row.get::<_, String>("state")?,
                    row.get::<_, Option<String>>("original_file")?,
                    row.get::<_, Option<String>>("normalized_file")?,
                    row.get::<_, Option<String>>("capture_json")?,
                ))
            },
        )
        .optional()
        .map_err(read_failed)?;
    let Some((id, session_question_id, ordinal, state, original_file, normalized_file, capture)) =
        row
    else {
        return Ok(None);
    };

    let ordinal =
        u32::try_from(ordinal).map_err(|_| read_corrupt("ordinal de gravação inválido"))?;
    let state = parse_state(&state)?;
    let capture =
        match capture {
            Some(json) => Some(serde_json::from_str::<CaptureMeta>(&json).map_err(|error| {
                read_corrupt(&format!("metadados da gravação ilegíveis: {error}"))
            })?),
            None => None,
        };

    let transcripts = load_transcripts(connection, &id)?;
    let review = load_review(connection, &id)?;
    Ok(Some(RecordingView {
        id,
        session_question_id,
        ordinal,
        state,
        capture,
        original_file,
        normalized_file,
        transcripts,
        review,
        warnings: Vec::new(),
    }))
}

fn load_transcripts(connection: &Connection, recording_id: &str) -> Result<Vec<Transcript>> {
    let mut statement = connection
        .prepare(
            "SELECT id, status, text, segments_json, model, language, load_ms,
                    processing_ms, created_at
               FROM transcripts
              WHERE recording_id = ?1
              ORDER BY created_at, rowid",
        )
        .map_err(read_failed)?;
    let mut rows = statement.query([recording_id]).map_err(read_failed)?;
    let mut transcripts = Vec::new();
    while let Some(row) = rows.next().map_err(read_failed)? {
        let status: String = row.get("status").map_err(read_failed)?;
        let status = match status.as_str() {
            "completed" => TranscriptStatus::Completed,
            "no_speech" => TranscriptStatus::NoSpeech,
            other => {
                return Err(read_corrupt(&format!(
                    "estado de transcrição desconhecido «{other}»"
                )))
            }
        };
        let segments_json: String = row.get("segments_json").map_err(read_failed)?;
        transcripts.push(Transcript {
            id: row.get("id").map_err(read_failed)?,
            status,
            text: row.get("text").map_err(read_failed)?,
            segments: serde_json::from_str(&segments_json).map_err(|error| {
                read_corrupt(&format!("segmentos da transcrição ilegíveis: {error}"))
            })?,
            model: row.get("model").map_err(read_failed)?,
            language: row.get("language").map_err(read_failed)?,
            created_at: row.get("created_at").map_err(read_failed)?,
            load_ms: duration_from_row(row, "load_ms")?,
            processing_ms: duration_from_row(row, "processing_ms")?,
        });
    }
    Ok(transcripts)
}

fn load_review(connection: &Connection, recording_id: &str) -> Result<Option<ReviewDraft>> {
    let row = connection
        .query_row(
            "SELECT recording_id, version, source_transcript_id, origin, text, saved_at
               FROM reviews
              WHERE recording_id = ?1
              ORDER BY version DESC
              LIMIT 1",
            [recording_id],
            stored_review,
        )
        .optional()
        .map_err(read_failed)?;
    row.map(StoredReviewRow::into_draft).transpose()
}

/// Lê uma versão concreta de uma revisão, para repetir um pedido já guardado.
fn load_review_version(
    connection: &Connection,
    recording_id: &str,
    version: Version,
) -> Result<Option<ReviewDraft>> {
    let row = connection
        .query_row(
            "SELECT recording_id, version, source_transcript_id, origin, text, saved_at
               FROM reviews
              WHERE recording_id = ?1 AND version = ?2",
            params![recording_id, version_to_i64(version)?],
            stored_review,
        )
        .optional()
        .map_err(read_failed)?;
    row.map(StoredReviewRow::into_draft).transpose()
}

fn stored_review(row: &Row<'_>) -> rusqlite::Result<StoredReviewRow> {
    Ok(StoredReviewRow {
        recording_id: row.get("recording_id")?,
        version: row.get("version")?,
        source_transcript_id: row.get("source_transcript_id")?,
        origin: row.get("origin")?,
        text: row.get("text")?,
        saved_at: row.get("saved_at")?,
    })
}

impl StoredReviewRow {
    fn into_draft(self) -> Result<ReviewDraft> {
        Ok(ReviewDraft {
            recording_id: self.recording_id,
            source_transcript_id: self.source_transcript_id,
            origin: parse_review_origin(&self.origin)?,
            text: self.text,
            version: Version::try_from(self.version)
                .map_err(|_| read_corrupt("versão de revisão inválida"))?,
            saved_at: Some(self.saved_at),
        })
    }
}

/// Confirmação guardada de uma pergunta da sessão, se existir.
pub(super) fn load_confirmation(
    connection: &Connection,
    session_question_id: &str,
) -> Result<Option<Confirmation>> {
    let row = connection
        .query_row(
            "SELECT id, session_question_id, recording_id, review_version,
                    source_transcript_id, origin, text, confirmed_at
               FROM confirmations
              WHERE session_question_id = ?1",
            [session_question_id],
            stored_confirmation,
        )
        .optional()
        .map_err(read_failed)?;
    row.map(StoredConfirmationRow::into_dto).transpose()
}

fn load_confirmation_by_id(connection: &Connection, id: &str) -> Result<Option<Confirmation>> {
    let row = connection
        .query_row(
            "SELECT id, session_question_id, recording_id, review_version,
                    source_transcript_id, origin, text, confirmed_at
               FROM confirmations
              WHERE id = ?1",
            [id],
            stored_confirmation,
        )
        .optional()
        .map_err(read_failed)?;
    row.map(StoredConfirmationRow::into_dto).transpose()
}

fn stored_confirmation(row: &Row<'_>) -> rusqlite::Result<StoredConfirmationRow> {
    Ok(StoredConfirmationRow {
        id: row.get("id")?,
        session_question_id: row.get("session_question_id")?,
        recording_id: row.get("recording_id")?,
        review_version: row.get("review_version")?,
        source_transcript_id: row.get("source_transcript_id")?,
        origin: row.get("origin")?,
        text: row.get("text")?,
        confirmed_at: row.get("confirmed_at")?,
    })
}

impl StoredConfirmationRow {
    fn into_dto(self) -> Result<Confirmation> {
        Ok(Confirmation {
            id: self.id,
            session_question_id: self.session_question_id,
            recording_id: self.recording_id,
            review_version: Version::try_from(self.review_version)
                .map_err(|_| read_corrupt("versão de revisão confirmada inválida"))?,
            source_transcript_id: self.source_transcript_id,
            origin: parse_review_origin(&self.origin)?,
            text: self.text,
            confirmed_at: self.confirmed_at,
        })
    }
}

fn parse_review_origin(value: &str) -> Result<ReviewOrigin> {
    match value {
        "automatic" => Ok(ReviewOrigin::Automatic),
        "manual_explicit" => Ok(ReviewOrigin::ManualExplicit),
        other => Err(read_corrupt(&format!(
            "origem de revisão desconhecida «{other}»"
        ))),
    }
}

fn find_answer_question(
    connection: &Connection,
    session_question_id: &str,
) -> Result<Option<AnswerQuestion>> {
    let row = connection
        .query_row(
            "SELECT sq.session_id, sq.selection_version,
                    EXISTS (
                        SELECT 1 FROM confirmations c
                         WHERE c.session_question_id = sq.id
                    ) AS confirmed
               FROM session_questions sq
              WHERE sq.id = ?1",
            [session_question_id],
            |row| {
                Ok((
                    row.get::<_, Id>("session_id")?,
                    row.get::<_, i64>("selection_version")?,
                    row.get::<_, i64>("confirmed")?,
                ))
            },
        )
        .optional()
        .map_err(read_failed)?;
    let Some((session_id, selection_version, confirmed)) = row else {
        return Ok(None);
    };
    let selection_version = Version::try_from(selection_version)
        .map_err(|_| read_corrupt("versão de seleção inválida"))?;
    Ok(Some(AnswerQuestion {
        session_id,
        selection_version,
        confirmed: confirmed != 0,
    }))
}

/// Estado da pergunta e da sessão relevante para guardar e confirmar.
fn find_review_target(
    connection: &Connection,
    session_question_id: &str,
) -> Result<Option<ReviewTarget>> {
    let row = connection
        .query_row(
            "SELECT sq.session_id, sq.selection_version, sq.selected_recording_id,
                    EXISTS (
                        SELECT 1 FROM confirmations c
                         WHERE c.session_question_id = sq.id
                    ) AS confirmed,
                    s.version AS session_version
               FROM session_questions sq
               JOIN sessions s ON s.id = sq.session_id
              WHERE sq.id = ?1",
            [session_question_id],
            |row| {
                Ok((
                    row.get::<_, Id>("session_id")?,
                    row.get::<_, i64>("selection_version")?,
                    row.get::<_, Option<Id>>("selected_recording_id")?,
                    row.get::<_, i64>("confirmed")?,
                    row.get::<_, i64>("session_version")?,
                ))
            },
        )
        .optional()
        .map_err(read_failed)?;
    let Some((session_id, selection_version, selected_recording_id, confirmed, session_version)) =
        row
    else {
        return Ok(None);
    };

    Ok(Some(ReviewTarget {
        session_id,
        selection_version: Version::try_from(selection_version)
            .map_err(|_| read_corrupt("versão de seleção inválida"))?,
        selected_recording_id,
        confirmed: confirmed != 0,
        session_version: Version::try_from(session_version)
            .map_err(|_| read_corrupt("versão de sessão inválida"))?,
    }))
}

/// Valida a pergunta, a sessão, a confirmação e a versão de seleção do alvo.
fn require_review_target(
    connection: &Connection,
    request: &AnswerTarget,
    expected_session: Option<Version>,
) -> Result<ReviewTarget> {
    let target = find_review_target(connection, &request.session_question_id)?
        .ok_or_else(question_not_found)?;
    if target.session_id != request.session_id {
        return Err(stale_target());
    }
    if target.confirmed {
        return Err(answer_confirmed());
    }
    if expected_session.is_some_and(|version| target.session_version != version) {
        return Err(stale_session());
    }
    if target.selection_version != request.selection_version {
        return Err(stale_target());
    }
    Ok(target)
}

/// Exige que a gravação do alvo seja a selecionada e tenha um original finalizado.
fn require_selected_recording(
    connection: &Connection,
    request: &AnswerTarget,
    question: &ReviewTarget,
) -> Result<()> {
    let Some(selected) = question.selected_recording_id.as_deref() else {
        return Err(recording_required());
    };
    if selected != request.recording_id {
        return Err(stale_target());
    }

    let recording = find_stored_recording(connection, &request.recording_id)?
        .ok_or_else(recording_not_found)?;
    if recording.session_question_id != request.session_question_id {
        return Err(stale_target());
    }
    if recording.state != RecordingState::Ready {
        return Err(recording_required());
    }
    Ok(())
}

/// A transcrição de origem, quando indicada, tem de pertencer à gravação.
fn require_transcript_owner(
    connection: &Connection,
    recording_id: &str,
    source_transcript_id: Option<&str>,
) -> Result<()> {
    let Some(transcript_id) = source_transcript_id else {
        return Ok(());
    };
    let owner: Option<Id> = connection
        .query_row(
            "SELECT recording_id FROM transcripts WHERE id = ?1",
            [transcript_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(read_failed)?;
    if owner.as_deref() != Some(recording_id) {
        return Err(stale_transcript());
    }
    Ok(())
}

/// Acrescenta uma versão de revisão; as anteriores nunca são alteradas.
fn insert_review(
    connection: &Connection,
    recording_id: &str,
    version: Version,
    source_transcript_id: Option<&str>,
    origin: ReviewOrigin,
    text: &str,
    now: &str,
) -> Result<()> {
    connection
        .execute(
            "INSERT INTO reviews
               (recording_id, version, source_transcript_id, origin, text, saved_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                recording_id,
                version_to_i64(version)?,
                source_transcript_id,
                origin.as_str(),
                text,
                now
            ],
        )
        .map_err(write_failed)?;
    Ok(())
}

/// Recibo de um pedido, se já tiver sido aplicado.
pub(super) fn find_receipt(connection: &Connection, request_id: &str) -> Result<Option<Receipt>> {
    connection
        .query_row(
            "SELECT kind, payload_hash, session_id, entity_id, result_version
               FROM command_receipts
              WHERE request_id = ?1",
            [request_id],
            |row| {
                let result_version: Option<i64> = row.get("result_version")?;
                Ok((
                    row.get::<_, String>("kind")?,
                    row.get::<_, String>("payload_hash")?,
                    row.get::<_, Option<Id>>("session_id")?,
                    row.get::<_, Option<Id>>("entity_id")?,
                    result_version,
                ))
            },
        )
        .optional()
        .map_err(read_failed)?
        .map(
            |(kind, payload_hash, session_id, entity_id, result_version)| {
                let result_version = result_version
                    .map(Version::try_from)
                    .transpose()
                    .map_err(|_| read_corrupt("versão de resultado inválida no recibo"))?;
                Ok(Receipt {
                    kind,
                    payload_hash,
                    session_id,
                    entity_id,
                    result_version,
                })
            },
        )
        .transpose()
}

/// Escreve o recibo na transação da mutação que o originou.
pub(super) fn insert_receipt(
    connection: &Connection,
    request_id: &str,
    kind: &str,
    payload_hash: &str,
    session_id: Option<&str>,
    entity_id: Option<&str>,
    result_version: Option<Version>,
) -> Result<()> {
    let result_version = result_version.map(version_to_i64).transpose()?;
    connection
        .execute(
            "INSERT INTO command_receipts
               (request_id, kind, payload_hash, session_id, entity_id, result_version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                request_id,
                kind,
                payload_hash,
                session_id,
                entity_id,
                result_version
            ],
        )
        .map_err(write_failed)?;
    Ok(())
}

/// Hash SHA-256 da serialização canónica de um pedido, sem o `requestId`.
pub(super) fn request_hash<T: Serialize>(payload: &T) -> Result<String> {
    let canonical = serde_json::to_vec(payload).map_err(|error| {
        AppError::new(
            error_code::INVALID_INPUT,
            format!("Não foi possível preparar o pedido de resposta: {error}"),
            false,
        )
    })?;
    let digest = Sha256::digest(&canonical);
    let mut hash = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hash.push_str(&format!("{byte:02x}"));
    }
    Ok(hash)
}

fn replay_save_review(
    connection: &Connection,
    receipt: &Receipt,
    payload_hash: &str,
) -> Result<ReviewDraft> {
    if receipt.kind != "save_review" || receipt.payload_hash != payload_hash {
        return Err(stale_request());
    }
    let recording_id = receipt
        .entity_id
        .as_deref()
        .ok_or_else(|| read_corrupt("o recibo da revisão não indica a gravação"))?;
    let version = receipt
        .result_version
        .ok_or_else(|| read_corrupt("o recibo da revisão não indica a versão"))?;
    load_review_version(connection, recording_id, version)?
        .ok_or_else(|| read_corrupt("a revisão indicada pelo recibo não existe na base"))
}

fn replay_confirmation(
    connection: &Connection,
    receipt: &Receipt,
    payload_hash: &str,
) -> Result<Confirmation> {
    if receipt.kind != "confirm_answer" || receipt.payload_hash != payload_hash {
        return Err(stale_request());
    }
    let confirmation_id = receipt
        .entity_id
        .as_deref()
        .ok_or_else(|| read_corrupt("o recibo da confirmação não indica o registo criado"))?;
    load_confirmation_by_id(connection, confirmation_id)?
        .ok_or_else(|| read_corrupt("a confirmação indicada pelo recibo não existe na base"))
}

/// Campos do pedido de revisão sem o `requestId`, pela ordem fixa do tipo.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SaveReviewPayload<'a> {
    target: &'a AnswerTarget,
    expected_review_version: Version,
    source_transcript_id: Option<&'a str>,
    origin: ReviewOrigin,
    text: &'a str,
}

/// Campos do pedido de confirmação sem o `requestId`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfirmAnswerPayload<'a> {
    target: &'a AnswerTarget,
    expected_review_version: Version,
    source_transcript_id: Option<&'a str>,
    origin: ReviewOrigin,
    text: &'a str,
    expected_session_version: Version,
}

fn version_to_i64(version: Version) -> Result<i64> {
    i64::try_from(version).map_err(|_| invalid_version())
}

fn find_stored_recording(
    connection: &Connection,
    recording_id: &str,
) -> Result<Option<StoredRecording>> {
    let row = connection
        .query_row(
            "SELECT sq.session_id, r.session_question_id, r.ordinal, r.state
               FROM recordings r
               JOIN session_questions sq ON sq.id = r.session_question_id
              WHERE r.id = ?1",
            [recording_id],
            |row| {
                Ok((
                    row.get::<_, Id>("session_id")?,
                    row.get::<_, Id>("session_question_id")?,
                    row.get::<_, i64>("ordinal")?,
                    row.get::<_, String>("state")?,
                ))
            },
        )
        .optional()
        .map_err(read_failed)?;
    let Some((session_id, session_question_id, ordinal, state)) = row else {
        return Ok(None);
    };
    Ok(Some(StoredRecording {
        session_id,
        session_question_id,
        ordinal,
        state: parse_state(&state)?,
    }))
}

fn find_transcript(
    connection: &Connection,
    transcript_id: &str,
) -> Result<Option<StoredTranscript>> {
    connection
        .query_row(
            "SELECT recording_id, status, text FROM transcripts WHERE id = ?1",
            [transcript_id],
            |row| {
                Ok(StoredTranscript {
                    recording_id: row.get("recording_id")?,
                    status: row.get("status")?,
                    text: row.get("text")?,
                })
            },
        )
        .optional()
        .map_err(read_failed)
}

fn find_operation(connection: &Connection, operation_id: &str) -> Result<Option<StoredOperation>> {
    connection
        .query_row(
            "SELECT state, session_id, session_question_id, recording_id,
                    worker_generation
               FROM operations
              WHERE id = ?1",
            [operation_id],
            |row| {
                Ok(StoredOperation {
                    state: row.get("state")?,
                    session_id: row.get("session_id")?,
                    session_question_id: row.get("session_question_id")?,
                    recording_id: row.get("recording_id")?,
                    worker_generation: row.get("worker_generation")?,
                })
            },
        )
        .optional()
        .map_err(read_failed)
}

/// Confirma que o ficheiro a publicar é o `original.wav` da pasta reservada.
///
/// A base não conhece a raiz dos artefactos, mas os três últimos componentes do
/// caminho bastam para recusar áudio que pertença a outra gravação.
fn original_matches(capture: &CapturedAudio, recording: &StoredRecording) -> bool {
    let directory = capture.original.parent();
    capture.original.file_name() == Some(OsStr::new(ORIGINAL_FILE))
        && directory.and_then(Path::file_name) == Some(OsStr::new(&capture.id))
        && directory.and_then(Path::parent).and_then(Path::file_name)
            == Some(OsStr::new(&recording.session_question_id))
}

fn parse_state(value: &str) -> Result<RecordingState> {
    match value {
        "reserved" => Ok(RecordingState::Reserved),
        "capturing" => Ok(RecordingState::Capturing),
        "publish_pending" => Ok(RecordingState::PublishPending),
        "ready" => Ok(RecordingState::Ready),
        "interrupted" => Ok(RecordingState::Interrupted),
        other => Err(read_corrupt(&format!(
            "estado de gravação desconhecido «{other}»"
        ))),
    }
}

fn duration_from_row(row: &Row<'_>, column: &str) -> Result<u64> {
    let value: i64 = row.get(column).map_err(read_failed)?;
    u64::try_from(value).map_err(|_| read_corrupt(&format!("«{column}» inválido «{value}»")))
}

fn foreign_audio() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "O áudio indicado não pertence à pasta reservada para esta gravação; nada foi publicado.",
        false,
    )
}

fn question_not_found() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "A pergunta da sessão indicada não existe. Atualize a entrevista antes de gravar.",
        false,
    )
}

fn invalid_review_request() -> AppError {
    AppError::new(
        error_code::INVALID_INPUT,
        "O pedido de revisão não tem identificador. Repita o pedido a partir da entrevista.",
        false,
    )
}

fn invalid_confirmation_request() -> AppError {
    AppError::new(
        error_code::INVALID_INPUT,
        "O pedido de confirmação não tem identificador. Repita a confirmação a partir da entrevista.",
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

/// A versão da revisão esperada já não é a mais recente.
fn stale_review() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "A revisão desta resposta mudou entretanto. Confirme exatamente a versão que está a ver.",
        false,
    )
}

fn stale_session() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "A entrevista mudou entretanto. Atualize o estado antes de confirmar a resposta.",
        false,
    )
}

fn stale_transcript() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "A transcrição indicada não pertence a esta gravação; a revisão não foi guardada.",
        false,
    )
}

/// Sem uma gravação finalizada e selecionada não há resposta a guardar.
fn recording_required() -> AppError {
    AppError::new(
        error_code::ANSWER_REQUIRED,
        "Esta pergunta ainda não tem uma gravação finalizada selecionada. Grave a resposta \
         antes de a guardar ou confirmar.",
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

fn recording_not_found() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "A gravação indicada não foi reservada nesta entrevista. Grave novamente.",
        false,
    )
}

fn stale_selection() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "A seleção da resposta mudou entretanto. Atualize a entrevista antes de gravar outra vez.",
        false,
    )
}

fn stale_target() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "O resultado pertence a uma versão antiga da resposta e não foi aplicado.",
        false,
    )
}

fn answer_confirmed() -> AppError {
    AppError::new(
        error_code::ANSWER_CONFIRMED,
        "A resposta desta pergunta já foi confirmada e é apenas de leitura.",
        false,
    )
}

fn transcript_without_recording() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "A transcrição não indica a gravação que a originou; o resultado foi ignorado.",
        false,
    )
}

fn transcript_unknown_recording() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "A transcrição não pertence à pergunta da sessão indicada; o resultado foi ignorado.",
        false,
    )
}

fn transcript_already_saved() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "Já existe uma transcrição com este identificador e outro texto; nada foi alterado.",
        false,
    )
}

fn operation_closed() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "Este pedido já terminou e o resultado não é aplicado outra vez.",
        false,
    )
}

fn operation_mismatch() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "O resultado pertence a outro pedido ou a outra geração do auxiliar e foi ignorado.",
        false,
    )
}

fn normalized_before_publication() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "Só uma gravação já publicada pode receber o áudio normalizado.",
        false,
    )
}

fn invalid_normalized_reference() -> AppError {
    AppError::new(
        error_code::INVALID_INPUT,
        "A referência do áudio normalizado não corresponde a esta gravação.",
        false,
    )
}

fn invalid_generation() -> AppError {
    AppError::new(
        error_code::INVALID_INPUT,
        "A geração do auxiliar indicada no resultado não é válida.",
        false,
    )
}

fn invalid_duration() -> AppError {
    AppError::new(
        error_code::INVALID_INPUT,
        "Uma duração da transcrição excede o intervalo guardável.",
        false,
    )
}

fn configuration_failed(error: serde_json::Error) -> AppError {
    AppError::new(
        error_code::INVALID_INPUT,
        format!("Não foi possível preparar a configuração guardada com o resultado: {error}."),
        false,
    )
}

fn read_failed(error: rusqlite::Error) -> AppError {
    AppError::new(
        error_code::DATABASE_UNAVAILABLE,
        format!("Não foi possível ler as gravações guardadas: {error}"),
        false,
    )
}

fn write_failed(error: rusqlite::Error) -> AppError {
    AppError::new(
        error_code::STORAGE_WRITE_FAILED,
        format!("Não foi possível guardar a gravação; nenhuma alteração foi aplicada: {error}"),
        true,
    )
}

fn read_corrupt(detail: &str) -> AppError {
    AppError::new(
        error_code::DATABASE_UNAVAILABLE,
        format!("Não foi possível ler as gravações guardadas: {detail}."),
        false,
    )
}

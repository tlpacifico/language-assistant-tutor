//! Gravações, publicações e transcrições automáticas das entrevistas guiadas.
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
//! não é aplicado e uma transcrição já gravada nunca é reescrita. Guardar uma
//! revisão pertence à tarefa 5; esta camada só a lê, sem a alterar.
//!
//! A divisão entre ficheiros e base não tem transação comum. A recuperação
//! (`artifacts::reconcile`) inspeciona as reservas pendentes e publica os
//! originais fechados; nenhuma falha de escrita apaga o áudio.

use std::ffi::OsStr;
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Row, TransactionBehavior};
use serde_json::Value;
use uuid::Uuid;

use super::questions::utc_now;
use super::Database;
use crate::artifacts::{
    inspect_file, normalized_reference, original_reference, validate_reference,
};
use crate::audio::{CapturedAudio, ORIGINAL_FILE};
use crate::domain::{error_code, AppError, CaptureMeta, Transcript, TranscriptStatus};
use crate::interview::{
    Id, OperationTarget, RecordingState, RecordingView, Result, ReviewDraft, ReviewOrigin, Version,
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
            |row| {
                Ok((
                    row.get::<_, Id>("recording_id")?,
                    row.get::<_, i64>("version")?,
                    row.get::<_, Option<Id>>("source_transcript_id")?,
                    row.get::<_, String>("origin")?,
                    row.get::<_, String>("text")?,
                    row.get::<_, String>("saved_at")?,
                ))
            },
        )
        .optional()
        .map_err(read_failed)?;
    let Some((recording_id, version, source_transcript_id, origin, text, saved_at)) = row else {
        return Ok(None);
    };

    let version =
        Version::try_from(version).map_err(|_| read_corrupt("versão de revisão inválida"))?;
    let origin = match origin.as_str() {
        "automatic" => ReviewOrigin::Automatic,
        "manual_explicit" => ReviewOrigin::ManualExplicit,
        other => {
            return Err(read_corrupt(&format!(
                "origem de revisão desconhecida «{other}»"
            )))
        }
    };
    Ok(Some(ReviewDraft {
        recording_id,
        source_transcript_id,
        origin,
        text,
        version,
        saved_at: Some(saved_at),
    }))
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

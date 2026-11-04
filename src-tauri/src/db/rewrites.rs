//! Fontes de exercício, reformulação, avaliação de prática e preferência de
//! nível (fase 4, §2.2/§2.4/§2.5/§2.6 do plano).
//!
//! [`Database::load_exercise_sources`] resolve a pergunta fixa, os critérios e
//! as confirmações de uma origem confirmada, sem chamar o resolvedor de feedback
//! da entrevista (que recusa originais locais) e sem incluir `Preparation`/
//! `Story`. A rubrica e o esquema derivam da família de avaliação
//! (`evaluation_families`), do relatório original existente ou do padrão
//! `star-text-v1`.
//!
//! [`Database::request_rewrite`] e [`Database::evaluate_practice`] reservam o
//! pedido lógico (`ai_requests`) e a tentativa de rede (`ai_attempts`) com
//! origem/finalidade/variante/geração, devolvendo `None` quando já existe um
//! resultado válido ou quando o `requestId` é um replay. A gravação dos
//! resultados imutáveis pertence a [`Database::finish_ai`] (`db/ai.rs`), que faz
//! dispatch por finalidade e escreve em `rewrites`/`practice_reports`.

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use uuid::Uuid;

use super::ai::{
    lookup_command_identity, CommandIdentity, CommandIdentityLocation, ReservedAttempt,
};
use super::answers::{load_confirmation, request_hash};
use super::questions::{json_list, utc_now};
use super::Database;
use crate::ai::{
    aggregate_usage, build_body, AiSettingsView, AiSource, AiSources, AiTask, NormalizedUsage,
    PRACTICE_FEEDBACK_PROMPT_VERSION, REWRITE_PROMPT_VERSION, REWRITE_RUBRIC_VERSION,
    REWRITE_SCHEMA, SCHEMA,
};
use crate::domain::{error_code, AppError};
use crate::interview::{Result, Version};
use crate::practice::{
    EvaluatePractice, NetworkAttemptState, NetworkAttemptView, PracticeReportView, ReportState,
    ReportVersions, ResponseOrigin, RewriteCommand, RewriteLevel, RewritePreference, RewriteView,
    SetRewriteLevel,
};

/// Fontes de um exercício, resolvidas a partir de uma origem confirmada.
///
/// Contém a identidade da origem, a pergunta principal e a sessão de origem, as
/// fontes de IA (`question`, `criteria`, `answers`) e as versões da família de
/// avaliação. **Não** contém preparação nem história; os DTOs de transporte
/// serializam apenas `AiSources`.
#[derive(Debug, Clone)]
pub struct ExerciseSources {
    pub origin: ResponseOrigin,
    /// Pergunta principal (`session_questions.id`).
    pub question_id: String,
    /// Sessão de origem (`sessions.id`).
    pub session_id: String,
    pub sources: AiSources,
    pub rubric_version: String,
    pub schema_version: String,
}

impl Database {
    /// Resolve as fontes de exercício de uma origem confirmada, sem rede.
    ///
    /// A origem original exige sessão concluída ou pergunta IA encerrada e confirmação inicial; inclui a
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

    /// Reserva (ou reutiliza) uma reformulação para uma origem e um nível.
    ///
    /// Devolve `None` quando a variante já tem resultado válido (sem rede) ou
    /// quando o `requestId` é um replay; caso contrário devolve a tentativa
    /// reservada pronta a correr no transporte. Recusa um `requestId` já usado
    /// noutro caminho (prática, configuração, outra finalidade de IA).
    pub fn request_rewrite(
        &self,
        command: &RewriteCommand,
        settings: &AiSettingsView,
    ) -> Result<Option<ReservedAttempt>> {
        let request_id = command.request_id.trim().to_owned();
        if request_id.is_empty() {
            return Err(invalid_request());
        }
        let sources = self.load_exercise_sources(&command.origin)?;
        let task = AiTask::Rewrite {
            level: command.level,
            rubric_version: REWRITE_RUBRIC_VERSION.into(),
        };
        build_body(settings, &task, &sources.sources)?;
        let (practice_id, origin_key) = match &command.origin {
            ResponseOrigin::Original {
                session_question_id,
                ..
            } => (None, format!("original:{session_question_id}")),
            ResponseOrigin::Practice { practice_id } => {
                (Some(practice_id.clone()), format!("practice:{practice_id}"))
            }
        };
        let variant_key = rewrite_variant_key(command.level);
        let payload_hash = request_hash(&RewritePayload {
            origin: &command.origin,
            level: command.level,
        })?;

        self.with_connection(|connection| {
            let tx = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(write_failed)?;

            // Uma variante válida já existente é reutilizada sem rede.
            if let Some(request_id) = existing_request(&tx, "rewrite", &origin_key, &variant_key)? {
                let has_result: bool = tx
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM rewrites WHERE request_id = ?1)",
                        [&request_id],
                        |r| r.get(0),
                    )
                    .map_err(write_failed)?;
                if has_result {
                    return Ok(None);
                }
            }

            let reserved = reserve_attempt(
                &tx,
                &request_id,
                &payload_hash,
                "rewrite",
                &sources.question_id,
                &sources.session_id,
                practice_id.as_deref(),
                &origin_key,
                &variant_key,
                &sources.sources,
                settings,
                REWRITE_PROMPT_VERSION,
                REWRITE_RUBRIC_VERSION,
                REWRITE_SCHEMA,
                &task,
            )?;
            tx.commit().map_err(write_failed)?;
            Ok(reserved)
        })
    }

    /// Lista as reformulações de uma origem, uma por nível já pedido.
    pub fn list_rewrites(&self, origin: &ResponseOrigin) -> Result<Vec<RewriteView>> {
        let origin_key = origin_key_of(origin);
        self.with_connection(|connection| {
            let mut statement = connection
                .prepare(
                    "SELECT id, variant_key FROM ai_requests
                      WHERE purpose = 'rewrite' AND origin_key = ?1
                      ORDER BY rowid",
                )
                .map_err(read_failed)?;
            let rows: Vec<(String, String)> = statement
                .query_map([&origin_key], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(read_failed)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(read_failed)?;
            drop(statement);
            rows.iter()
                .map(|(id, variant_key)| load_rewrite_view(connection, origin, id, variant_key))
                .collect()
        })
    }

    /// Reserva (ou reutiliza) a avaliação de uma prática confirmada.
    ///
    /// Devolve `None` quando a prática já tem relatório válido ou quando o
    /// `requestId` é um replay. A rubrica da família nasce do relatório original
    /// existente ou de `star-text-v1` na primeira avaliação; uma versão
    /// incompatível é recusada sem substituir relatórios antigos.
    pub fn evaluate_practice(
        &self,
        command: &EvaluatePractice,
        settings: &AiSettingsView,
    ) -> Result<Option<ReservedAttempt>> {
        let request_id = command.request_id.trim().to_owned();
        if request_id.is_empty() {
            return Err(invalid_request());
        }
        let origin = ResponseOrigin::Practice {
            practice_id: command.practice_id.clone(),
        };
        let sources = self.load_exercise_sources(&origin)?;
        if sources.rubric_version != "star-text-v1" || sources.schema_version != SCHEMA {
            return Err(incompatible_rubric(&sources.rubric_version));
        }
        let task = AiTask::PracticeFeedback {
            rubric_version: sources.rubric_version.clone(),
        };
        build_body(settings, &task, &sources.sources)?;
        let origin_key = format!("practice:{}", command.practice_id);
        let variant_key = "default".to_string();
        let payload_hash = request_hash(&EvaluatePayload {
            practice_id: &command.practice_id,
            expected_version: command.expected_version,
        })?;

        self.with_connection(|connection| {
            let tx = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(write_failed)?;

            let stored: Option<i64> = tx
                .query_row(
                    "SELECT version FROM practices WHERE id = ?1",
                    [&command.practice_id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(write_failed)?;
            let Some(stored) = stored else {
                return Err(practice_not_found());
            };
            if stored as u64 != command.expected_version {
                return Err(stale_practice());
            }

            let has_report: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM practice_reports WHERE practice_id = ?1)",
                    [&command.practice_id],
                    |r| r.get(0),
                )
                .map_err(write_failed)?;
            if has_report {
                return Ok(None);
            }

            let reserved = reserve_attempt(
                &tx,
                &request_id,
                &payload_hash,
                "practice_feedback",
                &sources.question_id,
                &sources.session_id,
                Some(&command.practice_id),
                &origin_key,
                &variant_key,
                &sources.sources,
                settings,
                PRACTICE_FEEDBACK_PROMPT_VERSION,
                &sources.rubric_version,
                &sources.schema_version,
                &task,
            )?;
            tx.commit().map_err(write_failed)?;
            Ok(reserved)
        })
    }

    /// Relatório de avaliação de uma prática, com tentativas e consumo.
    pub fn load_practice_report(&self, practice_id: &str) -> Result<PracticeReportView> {
        self.with_connection(|connection| {
            let origin = ResponseOrigin::Practice {
                practice_id: practice_id.to_owned(),
            };
            let origin_key = format!("practice:{practice_id}");
            let request_id: Option<String> = connection
                .query_row(
                    "SELECT id FROM ai_requests WHERE purpose = 'practice_feedback' AND origin_key = ?1",
                    [&origin_key],
                    |r| r.get(0),
                )
                .optional()
                .map_err(read_failed)?;
            let attempts = match request_id {
                Some(id) => load_network_attempts(connection, &id, &origin)?,
                None => Vec::new(),
            };
            let usage = aggregate_usage(
                &attempts
                    .iter()
                    .map(|a| a.usage.clone())
                    .collect::<Vec<_>>(),
            );

            let report: Option<(String, String, String, String)> = connection
                .query_row(
                    "SELECT rubric_version, prompt_version, schema_version, json
                       FROM practice_reports
                      WHERE practice_id = ?1",
                    [practice_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .optional()
                .map_err(read_failed)?;

            match report {
                Some((rubric, prompt, schema, json)) => Ok(PracticeReportView {
                    practice_id: practice_id.to_owned(),
                    state: ReportState::Ready,
                    report: Some(
                        serde_json::from_str(&json)
                            .map_err(|_| read_corrupt("o relatório de prática é ilegível"))?,
                    ),
                    rubric_version: Some(rubric),
                    prompt_version: Some(prompt),
                    schema_version: Some(schema),
                    usage,
                    network_attempts: attempts,
                }),
                None => {
                    let state = if attempts
                        .iter()
                        .any(|a| a.state == NetworkAttemptState::Running)
                    {
                        ReportState::Running
                    } else if attempts.is_empty() {
                        ReportState::Pending
                    } else {
                        ReportState::Failed
                    };
                    Ok(PracticeReportView {
                        practice_id: practice_id.to_owned(),
                        state,
                        report: None,
                        rubric_version: None,
                        prompt_version: None,
                        schema_version: None,
                        usage,
                        network_attempts: attempts,
                    })
                }
            }
        })
    }

    /// Preferência local de nível, com B1 por omissão quando não há linha.
    pub fn load_rewrite_preference(&self) -> Result<RewritePreference> {
        self.with_connection(|connection| load_preference_in(connection))
    }

    /// Altera a preferência local de nível, idempotente por `requestId`.
    ///
    /// A mudança não altera reformulações anteriores nem envia nada à rede.
    pub fn set_rewrite_level(&self, request: SetRewriteLevel) -> Result<RewritePreference> {
        let request_id = request.request_id.trim().to_owned();
        if request_id.is_empty() {
            return Err(invalid_request());
        }
        let payload_hash = request_hash(&SetLevelPayload {
            expected_version: request.expected_version,
            level: request.level,
        })?;
        let now = utc_now();

        self.with_connection(|connection| {
            let tx = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(write_failed)?;

            match lookup_command_identity(&tx, &request_id)? {
                CommandIdentity::NotFound => {}
                CommandIdentity::Conflict => return Err(stale_request()),
                CommandIdentity::Found {
                    location: CommandIdentityLocation::AiReceipt,
                    kind: Some(kind),
                    payload_hash: found,
                } if kind == "set_rewrite_level" && found == payload_hash => {
                    return load_preference_in(&tx);
                }
                CommandIdentity::Found { .. } => return Err(stale_request()),
            }

            let current: Option<(String, i64)> = tx
                .query_row(
                    "SELECT rewrite_level, version FROM practice_preferences WHERE id = 1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(write_failed)?;

            let (version, level) = match current {
                None => {
                    if request.expected_version != 1 {
                        return Err(stale_preference());
                    }
                    tx.execute(
                        "INSERT INTO practice_preferences (id, rewrite_level, version, updated_at)
                         VALUES (1, ?1, 2, ?2)",
                        params![request.level.as_str(), now],
                    )
                    .map_err(write_failed)?;
                    (2, request.level)
                }
                Some((_, stored_version)) => {
                    if stored_version as u64 != request.expected_version {
                        return Err(stale_preference());
                    }
                    let next = stored_version + 1;
                    tx.execute(
                        "UPDATE practice_preferences
                            SET rewrite_level = ?1, version = ?2, updated_at = ?3
                          WHERE id = 1",
                        params![request.level.as_str(), next, now],
                    )
                    .map_err(write_failed)?;
                    (next, request.level)
                }
            };

            tx.execute(
                "INSERT INTO ai_receipts (id, kind, payload_hash, entity_id)
                 VALUES (?1, 'set_rewrite_level', ?2, NULL)",
                params![request_id, payload_hash],
            )
            .map_err(write_failed)?;

            tx.commit().map_err(write_failed)?;
            Ok(RewritePreference {
                version: version as u64,
                level,
                updated_at: now,
            })
        })
    }
}

/// Compõe a variante de reformulação: `rewrite:<nível>:<prompt>:<rubrica>:<esquema>`.
fn rewrite_variant_key(level: RewriteLevel) -> String {
    format!(
        "rewrite:{}:{}:{}:{}",
        level.as_str(),
        REWRITE_PROMPT_VERSION,
        REWRITE_RUBRIC_VERSION,
        REWRITE_SCHEMA
    )
}

fn level_from_variant_key(key: &str) -> Option<RewriteLevel> {
    let level = key.strip_prefix("rewrite:")?.split(':').next()?;
    parse_level(level)
}

fn parse_level(value: &str) -> Option<RewriteLevel> {
    match value {
        "B1" => Some(RewriteLevel::B1),
        "B2" => Some(RewriteLevel::B2),
        "C1" => Some(RewriteLevel::C1),
        _ => None,
    }
}

fn origin_key_of(origin: &ResponseOrigin) -> String {
    match origin {
        ResponseOrigin::Original {
            session_question_id,
            ..
        } => format!("original:{session_question_id}"),
        ResponseOrigin::Practice { practice_id } => format!("practice:{practice_id}"),
    }
}

/// O pedido lógico já existente para uma finalidade/variante, se existir.
fn existing_request(
    connection: &Connection,
    purpose: &str,
    origin_key: &str,
    variant_key: &str,
) -> Result<Option<String>> {
    connection
        .query_row(
            "SELECT id FROM ai_requests WHERE purpose = ?1 AND origin_key = ?2 AND variant_key = ?3",
            params![purpose, origin_key, variant_key],
            |r| r.get(0),
        )
        .optional()
        .map_err(write_failed)
}

/// Reserva (ou reconhece o replay de) um pedido lógico e uma tentativa de rede.
///
/// Devolve `None` quando o `requestId` já foi aplicado com o mesmo conteúdo
/// (replay); recusa reutilização noutra tabela/kind ou com conteúdo diferente.
#[allow(clippy::too_many_arguments)]
fn reserve_attempt(
    tx: &rusqlite::Transaction<'_>,
    command_id: &str,
    payload_hash: &str,
    purpose: &str,
    question_id: &str,
    session_id: &str,
    practice_id: Option<&str>,
    origin_key: &str,
    variant_key: &str,
    sources: &AiSources,
    settings: &AiSettingsView,
    prompt_version: &str,
    rubric_version: &str,
    schema_version: &str,
    task: &AiTask,
) -> Result<Option<ReservedAttempt>> {
    match lookup_command_identity(tx, command_id)? {
        CommandIdentity::NotFound => {}
        CommandIdentity::Conflict => return Err(stale_request()),
        CommandIdentity::Found {
            location: CommandIdentityLocation::AiAttempt,
            payload_hash: found,
            ..
        } => {
            if found == payload_hash {
                return Ok(None);
            }
            return Err(stale_request());
        }
        CommandIdentity::Found { .. } => return Err(stale_request()),
    }

    let (request_id, generation) = match existing_request(tx, purpose, origin_key, variant_key)? {
        Some(id) => {
            let current: i64 = tx
                .query_row(
                    "SELECT generation FROM ai_requests WHERE id = ?1",
                    [&id],
                    |r| r.get(0),
                )
                .map_err(write_failed)?;
            let next = current + 1;
            tx.execute(
                "UPDATE ai_requests SET generation = ?2 WHERE id = ?1",
                params![id, next],
            )
            .map_err(write_failed)?;
            (id, next)
        }
        None => {
            let id = Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO ai_requests
                 (id, purpose, question_id, session_id, practice_id, origin_key, variant_key,
                  sources_json, generation, prompt_version, rubric_version, schema_version)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9, ?10, ?11)",
                params![
                    id,
                    purpose,
                    question_id,
                    session_id,
                    practice_id,
                    origin_key,
                    variant_key,
                    serde_json::to_string(sources)
                        .map_err(|_| failure_source("fontes inválidas"))?,
                    prompt_version,
                    rubric_version,
                    schema_version
                ],
            )
            .map_err(write_failed)?;
            (id, 1)
        }
    };

    let attempt_id = Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO ai_attempts
         (id, request_id, generation, command_id, payload_hash, state, settings_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'running', ?6, ?7)",
        params![
            attempt_id,
            request_id,
            generation,
            command_id,
            payload_hash,
            serde_json::to_string(settings).map_err(|_| failure_source("configuração inválida"))?,
            utc_now()
        ],
    )
    .map_err(write_failed)?;

    Ok(Some(ReservedAttempt {
        id: attempt_id,
        request_id,
        question_id: question_id.to_owned(),
        session_id: session_id.to_owned(),
        practice_id: practice_id.map(str::to_owned),
        purpose: purpose.to_owned(),
        task: task.clone(),
        generation: generation as u64,
        origin_key: origin_key.to_owned(),
        variant_key: variant_key.to_owned(),
        prompt_version: prompt_version.to_owned(),
        rubric_version: rubric_version.to_owned(),
        schema_version: schema_version.to_owned(),
        sources: sources.clone(),
        settings: settings.clone(),
    }))
}

/// Versões da família de avaliação de uma pergunta, sem inventar dados.
fn family_versions(connection: &Connection, session_question_id: &str) -> Result<(String, String)> {
    let family: Option<(String, String)> = connection
        .query_row(
            "SELECT rubric_version, schema_version FROM evaluation_families
              WHERE session_question_id = ?1",
            [session_question_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(read_failed)?;
    if let Some(versions) = family {
        return Ok(versions);
    }
    let report: Option<(String, String)> = connection
        .query_row(
            "SELECT r.rubric_version, r.schema_version
               FROM ai_reports rep
               JOIN ai_attempts a ON a.id = rep.attempt_id
               JOIN ai_requests r ON r.id = a.request_id
              WHERE rep.question_id = ?1",
            [session_question_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(read_failed)?;
    if let Some(versions) = report {
        return Ok(versions);
    }
    Ok(("star-text-v1".into(), SCHEMA.into()))
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
    if status.as_deref() != Some("completed") {
        let mode: String = connection
            .query_row("SELECT mode FROM sessions WHERE id=?1", [session_id], |r| {
                r.get(0)
            })
            .map_err(read_failed)?;
        if status.as_deref() != Some("in_progress") || mode != "ai_v3" {
            return Err(session_not_completed());
        }
        super::ai::require_advance(connection, session_question_id)?;
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

    let (rubric_version, schema_version) = family_versions(connection, session_question_id)?;

    Ok(ExerciseSources {
        origin: ResponseOrigin::Original {
            session_id: session_id.to_owned(),
            session_question_id: session_question_id.to_owned(),
        },
        question_id: session_question_id.to_owned(),
        session_id: session_id.to_owned(),
        sources: AiSources {
            question: question_text,
            criteria,
            answers,
        },
        rubric_version,
        schema_version,
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
    let Some((status, session_question_id, source_session_id, answer_id)) = row else {
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

    let (rubric_version, schema_version) = family_versions(connection, &session_question_id)?;

    Ok(ExerciseSources {
        origin: ResponseOrigin::Practice {
            practice_id: practice_id.to_owned(),
        },
        question_id: session_question_id.clone(),
        session_id: source_session_id,
        sources: AiSources {
            question: question_text,
            criteria,
            answers: vec![AiSource {
                confirmation_id: confirmation.id,
                role: "attempt".into(),
                text: confirmation.text,
            }],
        },
        rubric_version,
        schema_version,
    })
}

/// Vista de uma reformulação de uma origem, num nível.
fn load_rewrite_view(
    connection: &Connection,
    origin: &ResponseOrigin,
    request_id: &str,
    variant_key: &str,
) -> Result<RewriteView> {
    let level = level_from_variant_key(variant_key)
        .ok_or_else(|| read_corrupt("a variante da reformulação não tem nível"))?;
    let (prompt_version, rubric_version, schema_version): (String, String, String) = connection
        .query_row(
            "SELECT prompt_version, rubric_version, schema_version FROM ai_requests WHERE id = ?1",
            [request_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(read_failed)?;
    let result: Option<(String, String, String)> = connection
        .query_row(
            "SELECT id, json, spoken_text FROM rewrites WHERE request_id = ?1",
            [request_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(read_failed)?;
    let attempts = load_network_attempts(connection, request_id, origin)?;
    let usage = aggregate_usage(&attempts.iter().map(|a| a.usage.clone()).collect::<Vec<_>>());

    let versions = ReportVersions {
        rubric_version,
        prompt_version,
        schema_version,
    };

    match result {
        Some((id, json, spoken_text)) => {
            let content: serde_json::Value = serde_json::from_str(&json)
                .map_err(|_| read_corrupt("a reformulação guardada é ilegível"))?;
            Ok(RewriteView {
                id: Some(id),
                origin: origin.clone(),
                level,
                state: ReportState::Ready,
                segments: serde_json::from_value(content["segments"].clone())
                    .map_err(|_| read_corrupt("segmentos da reformulação inválidos"))?,
                spoken_text: Some(spoken_text),
                necessary_changes: serde_json::from_value(content["necessaryChanges"].clone())
                    .map_err(|_| read_corrupt("alterações necessárias inválidas"))?,
                optional_style: serde_json::from_value(content["optionalStyle"].clone())
                    .map_err(|_| read_corrupt("alterações de estilo inválidas"))?,
                missing_information: serde_json::from_value(content["missingInformation"].clone())
                    .map_err(|_| read_corrupt("lacunas da reformulação inválidas"))?,
                versions,
                usage,
                network_attempts: attempts,
            })
        }
        None => {
            let state = if attempts
                .iter()
                .any(|a| a.state == NetworkAttemptState::Running)
            {
                ReportState::Running
            } else if attempts.is_empty() {
                ReportState::Pending
            } else {
                ReportState::Failed
            };
            Ok(RewriteView {
                id: None,
                origin: origin.clone(),
                level,
                state,
                segments: Vec::new(),
                spoken_text: None,
                necessary_changes: Vec::new(),
                optional_style: Vec::new(),
                missing_information: Vec::new(),
                versions,
                usage,
                network_attempts: attempts,
            })
        }
    }
}

/// Tentativas de rede de um pedido lógico, pela ordem de geração.
fn load_network_attempts(
    connection: &Connection,
    request_id: &str,
    origin: &ResponseOrigin,
) -> Result<Vec<NetworkAttemptView>> {
    let mut statement = connection
        .prepare(
            "SELECT a.id, a.request_id, r.purpose, a.generation, a.state, a.usage_json,
                    a.created_at, a.finished_at
               FROM ai_attempts a
               JOIN ai_requests r ON r.id = a.request_id
              WHERE a.request_id = ?1
              ORDER BY a.generation",
        )
        .map_err(read_failed)?;
    let rows = statement
        .query_map([request_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, Option<String>>(7)?,
            ))
        })
        .map_err(read_failed)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(read_failed)?;
    drop(statement);

    rows.into_iter()
        .map(
            |(
                network_attempt_id,
                request_id,
                purpose,
                generation,
                state,
                usage_json,
                created_at,
                finished_at,
            )| {
                let usage = usage_json
                    .as_deref()
                    .and_then(|s| serde_json::from_str::<NormalizedUsage>(s).ok())
                    .unwrap_or_default();
                Ok(NetworkAttemptView {
                    network_attempt_id,
                    request_id,
                    purpose,
                    origin: origin.clone(),
                    state: parse_attempt_state(&state)?,
                    generation: generation as u64,
                    usage,
                    created_at,
                    finished_at,
                })
            },
        )
        .collect()
}

fn parse_attempt_state(value: &str) -> Result<NetworkAttemptState> {
    match value {
        "running" => Ok(NetworkAttemptState::Running),
        "completed" => Ok(NetworkAttemptState::Completed),
        "failed" => Ok(NetworkAttemptState::Failed),
        "interrupted" => Ok(NetworkAttemptState::Interrupted),
        "discarded" => Ok(NetworkAttemptState::Discarded),
        _ => Err(read_corrupt("estado de tentativa de rede desconhecido")),
    }
}

fn load_preference_in(connection: &Connection) -> Result<RewritePreference> {
    let row: Option<(String, i64, String)> = connection
        .query_row(
            "SELECT rewrite_level, version, updated_at FROM practice_preferences WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(read_failed)?;
    match row {
        Some((level, version, updated_at)) => Ok(RewritePreference {
            version: Version::try_from(version)
                .map_err(|_| read_corrupt("versão de preferência inválida"))?,
            level: parse_level(&level)
                .ok_or_else(|| read_corrupt("nível de preferência inválido"))?,
            updated_at,
        }),
        // Sem linha: B1 é o padrão, sem inventar dados nem criar a linha.
        None => Ok(RewritePreference {
            version: 1,
            level: RewriteLevel::B1,
            updated_at: String::new(),
        }),
    }
}

/// Campos do pedido de reformulação sem o `requestId`.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct RewritePayload<'a> {
    origin: &'a ResponseOrigin,
    level: RewriteLevel,
}

/// Campos do pedido de avaliação sem o `requestId`.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct EvaluatePayload<'a> {
    practice_id: &'a str,
    expected_version: Version,
}

/// Campos do pedido de nível sem o `requestId`.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SetLevelPayload {
    expected_version: Version,
    level: RewriteLevel,
}

fn failure_source(what: &str) -> AppError {
    AppError::new(
        error_code::INVALID_INPUT,
        format!("Pedido inválido: {what}."),
        false,
    )
}

fn read_failed(error: rusqlite::Error) -> AppError {
    AppError::new(
        error_code::DATABASE_UNAVAILABLE,
        format!("Não foi possível ler as fontes do exercício: {error}"),
        false,
    )
}

fn write_failed(error: rusqlite::Error) -> AppError {
    AppError::new(
        error_code::STORAGE_WRITE_FAILED,
        format!("Não foi possível guardar o pedido; nenhuma alteração foi aplicada: {error}"),
        true,
    )
}

fn read_corrupt(detail: &str) -> AppError {
    AppError::new(
        error_code::DATABASE_UNAVAILABLE,
        format!("Não foi possível ler os dados guardados: {detail}."),
        false,
    )
}

fn invalid_request() -> AppError {
    AppError::new(
        error_code::INVALID_INPUT,
        "O pedido não tem identificador. Repita a operação a partir do treino.",
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

fn stale_practice() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "A prática mudou entretanto. Atualize o estado antes de a avaliar.",
        false,
    )
}

fn stale_preference() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "A preferência de nível mudou entretanto. Volte a carregá-la.",
        false,
    )
}

fn incompatible_rubric(rubric: &str) -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        format!(
            "A rubrica da família ({rubric}) não é executável nesta versão; \
             a avaliação não foi substituída. Consulte o relatório existente."
        ),
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

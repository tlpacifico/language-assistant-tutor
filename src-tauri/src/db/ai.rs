//! Persistência IA. As colunas locais legadas `session_question_id` em áudio
//! referem um slot de resposta em v2; aqui `question_id` é sempre a principal.
use super::{answers, questions::utc_now, Database};
use crate::ai::*;
use crate::interview::{QuestionDetail, SessionQuestion};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;
pub fn sql_error(_: rusqlite::Error) -> crate::domain::AppError {
    failure("Não foi possível guardar ou ler os dados de IA. Repita a operação.")
}
pub fn hash<T: serde::Serialize>(v: &T) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(v).map_err(|_| failure("Pedido inválido."))?)
    ))
}
pub(super) fn load_follow_up(
    c: &Connection,
    q: &SessionQuestion,
) -> Result<(Option<Box<QuestionDetail>>, Option<String>)> {
    let stage: Option<String> = c
        .query_row(
            "SELECT stage FROM ai_stages WHERE question_id=?1",
            [&q.id],
            |r| r.get(0),
        )
        .optional()
        .map_err(sql_error)?;
    let slot: Option<(String, String, Option<String>, i64)> = c
        .query_row(
            "SELECT id,
text,
selected_recording_id,
selection_version
 FROM answer_slots
 WHERE session_question_id=?1
 AND role='follow_up'",
            [&q.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .map_err(sql_error)?;
    let follow = if let Some((id, text, selected, version)) = slot {
        let mut sq = q.clone();
        sq.id = id.clone();
        sq.text = text;
        sq.selected_recording_id = selected;
        sq.selection_version = version as u64;
        Some(Box::new(QuestionDetail {
            initial_answer_id: q.id.clone(),
            follow_up: None,
            follow_up_stage: None,
            recordings: answers::load_recordings(c, &id)?,
            confirmation: answers::load_confirmation(c, &id)?.map(|mut c| {
                c.session_question_id = q.id.clone();
                c
            }),
            question: sq,
        }))
    } else {
        None
    };
    Ok((follow, stage))
}
pub(super) fn require_advance(c: &Connection, q: &str) -> Result<()> {
    let mode: String = c
        .query_row(
            "SELECT s.mode
 FROM sessions s
 JOIN session_questions q ON q.session_id=s.id
 WHERE q.id=?1",
            [q],
            |r| r.get(0),
        )
        .map_err(sql_error)?;
    if mode == "ai_v3" {
        let stage: Option<String> = c
            .query_row(
                "SELECT stage FROM ai_stages WHERE question_id=?1",
                [q],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql_error)?;
        let closed = matches!(stage.as_deref(), Some("not_needed" | "waived"))
            || stage.as_deref() == Some("follow_up")
                && c.query_row(
                    "SELECT EXISTS(SELECT 1
 FROM confirmations c
 JOIN answer_slots a ON a.id=c.session_question_id
 WHERE a.session_question_id=?1
 AND a.role='follow_up')",
                    [q],
                    |r| r.get::<_, bool>(0),
                )
                .map_err(sql_error)?;
        if !closed {
            return Err(failure(
                "Resolva o seguimento: peça, repita ou dispense explicitamente antes de avançar.",
            ));
        }
    }
    Ok(())
}
#[derive(Clone)]
pub struct ReservedAttempt {
    pub id: String,
    pub request_id: String,
    pub question_id: String,
    pub session_id: String,
    pub purpose: String,
    pub generation: u64,
    pub sources: AiSources,
    pub settings: AiSettingsView,
}
impl Database {
    pub fn ai_settings(&self) -> Result<AiSettingsView> {
        self.with_connection(|c| {
            let raw: Option<String> = c
                .query_row("SELECT json FROM ai_settings WHERE id=1", [], |r| r.get(0))
                .optional()
                .map_err(sql_error)?;
            match raw {
                Some(s) => {
                    serde_json::from_str(&s).map_err(|_| failure("Configuração IA inválida."))
                }
                None => Ok(AiSettingsView::default()),
            }
        })
    }
    pub fn ai_receipt_matches(&self, id: &str, kind: &str, payload: &str) -> Result<bool> {
        if id.trim().is_empty() {
            return Err(failure("Pedido sem identificador."));
        }
        self.with_connection(|c| {
            let old: Option<(String, String)> = c
                .query_row(
                    "SELECT kind,payload_hash FROM ai_receipts WHERE id=?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(sql_error)?;
            match old {
                Some((k, h)) if k == kind && h == payload => Ok(true),
                Some(_) => Err(failure("Identificador reutilizado com outro conteúdo.")),
                None => Ok(false),
            }
        })
    }
    pub fn put_ai_settings(
        &self,
        expected: u64,
        v: &AiSettingsView,
        receipt: (&str, &str, &str),
    ) -> Result<()> {
        self.with_connection(|c| {
            let tx = c.transaction().map_err(sql_error)?;
            let current: Option<i64> = tx
                .query_row("SELECT version FROM ai_settings WHERE id=1", [], |r| {
                    r.get(0)
                })
                .optional()
                .map_err(sql_error)?;
            if current.unwrap_or(1) as u64 != expected {
                return Err(failure("A configuração mudou; volte a carregá-la."));
            }
            let mut public = v.clone();
            public.credential_configured = false;
            tx.execute(
                "INSERT INTO ai_settings
 VALUES (1,
?1,
?2)
 ON CONFLICT (id) DO UPDATE
 SET version=excluded.version,
json=excluded.json",
                params![v.version as i64, serde_json::to_string(&public).unwrap()],
            )
            .map_err(sql_error)?;
            tx.execute(
                "INSERT INTO ai_receipts VALUES(?1,?2,?3,NULL)",
                params![receipt.0, receipt.1, receipt.2],
            )
            .map_err(sql_error)?;
            tx.commit().map_err(sql_error)
        })
    }
    pub fn ai_sources(&self, session: &str, question: &str, purpose: &str) -> Result<AiSources> {
        let d = self.load_session(session)?;
        let q = d
            .questions
            .iter()
            .find(|q| q.question.id == question)
            .ok_or_else(|| failure("Pergunta inválida."))?;
        if d.summary.mode != crate::interview::SessionMode::AiV3 {
            return Err(failure("Esta sessão é local."));
        }
        if purpose == "follow_up"
            && (d.summary.status != crate::interview::SessionStatus::InProgress
                || q.question.position != d.summary.position)
        {
            return Err(failure("A pergunta já não está corrente."));
        }
        let c = q
            .confirmation
            .as_ref()
            .ok_or_else(|| failure("Confirme primeiro a resposta."))?;
        let mut answers = vec![AiSource {
            confirmation_id: c.id.clone(),
            role: "initial".into(),
            text: c.text.clone(),
        }];
        if purpose == "feedback" {
            if d.summary.status != crate::interview::SessionStatus::Completed {
                return Err(failure("Termine a entrevista antes de pedir feedback."));
            }
            if let Some(c) = q.follow_up.as_ref().and_then(|f| f.confirmation.as_ref()) {
                answers.push(AiSource {
                    confirmation_id: c.id.clone(),
                    role: "follow_up".into(),
                    text: c.text.clone(),
                });
            }
        }
        Ok(AiSources {
            question: q.question.text.clone(),
            criteria: q.question.criteria.clone(),
            answers,
        })
    }
    pub fn reserve_ai(
        &self,
        command: &AiQuestionCommand,
        purpose: &str,
        settings: &AiSettingsView,
    ) -> Result<Option<ReservedAttempt>> {
        let replay = self.with_connection(|c| {
            let previous: Option<String> = c
                .query_row(
                    "SELECT payload_hash FROM ai_attempts WHERE command_id=?1",
                    [&command.request_id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(sql_error)?;
            match previous {
                Some(h) if h == hash(&(command, purpose))? => Ok(true),
                Some(_) => Err(failure("Identificador reutilizado.")),
                None => Ok(false),
            }
        })?;
        if replay {
            return Ok(None);
        }
        let sources =
            self.ai_sources(&command.session_id, &command.session_question_id, purpose)?;
        build_body(settings, &AiTask::from_purpose(purpose), &sources)?;
        let payload = hash(&(command, purpose))?;
        self.with_connection(|c| {
            let tx = c.transaction().map_err(sql_error)?;
            if command.request_id.trim().is_empty() {
                return Err(failure("Pedido sem identificador."));
            }
            let replay: Option<String> = tx
                .query_row(
                    "SELECT payload_hash FROM ai_attempts WHERE command_id=?1",
                    [&command.request_id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(sql_error)?;
            if let Some(h) = replay {
                if h != payload {
                    return Err(failure("Identificador reutilizado com outro conteúdo."));
                }
                return Ok(None);
            }
            let version: i64 = tx
                .query_row(
                    "SELECT version FROM sessions WHERE id=?1",
                    [&command.session_id],
                    |r| r.get(0),
                )
                .map_err(sql_error)?;
            if purpose == "follow_up" && version as u64 != command.expected_session_version {
                return Err(failure("A sessão mudou; atualize a entrevista."));
            }
            let (generation, stage): (i64, String) = tx
                .query_row(
                    "SELECT generation,stage FROM ai_stages WHERE question_id=?1",
                    [&command.session_question_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(sql_error)?
                .unwrap_or((0, "not_requested".into()));
            if purpose == "follow_up" && !matches!(stage.as_str(), "not_requested" | "failed") {
                return Err(failure("O seguimento já foi resolvido ou está em curso."));
            }
            if purpose == "feedback"
                && tx
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM ai_reports WHERE question_id=?1)",
                        [&command.session_question_id],
                        |r| r.get::<_, bool>(0),
                    )
                    .map_err(sql_error)?
            {
                return Ok(None);
            }
            let req = Uuid::new_v4().to_string();
            tx.execute(
                "INSERT OR IGNORE INTO ai_requests(id,
 purpose,
 question_id,
 session_id,
 practice_id,
 origin_key,
 variant_key,
 sources_json)
 VALUES (?1,
 ?2,
 ?3,
 ?4,
 NULL,
 'original:' || ?3,
 'default',
 ?5)",
                params![
                    req,
                    purpose,
                    command.session_question_id,
                    command.session_id,
                    serde_json::to_string(&sources).unwrap()
                ],
            )
            .map_err(sql_error)?;
            let request_id: String = tx
                .query_row(
                    "SELECT id FROM ai_requests WHERE purpose=?1 AND question_id=?2",
                    params![purpose, command.session_question_id],
                    |r| r.get(0),
                )
                .map_err(sql_error)?;
            let id = Uuid::new_v4().to_string();
            let generation = generation + 1;
            tx.execute(
                "INSERT INTO ai_attempts(id,
request_id,
generation,
command_id,
payload_hash,
state,
settings_json,
created_at)
 VALUES (?1,
?2,
?3,
?4,
?5,
'running',
?6,
?7)",
                params![
                    id,
                    request_id,
                    generation,
                    command.request_id,
                    payload,
                    serde_json::to_string(settings).unwrap(),
                    utc_now()
                ],
            )
            .map_err(sql_error)?;
            if purpose == "follow_up" {
                tx.execute(
                    "INSERT INTO ai_stages
 VALUES (?1,
'running',
?2,
NULL)
 ON CONFLICT (question_id) DO UPDATE
 SET stage='running',
generation=excluded.generation,
reason=NULL",
                    params![command.session_question_id, generation],
                )
                .map_err(sql_error)?;
            }
            tx.commit().map_err(sql_error)?;
            Ok(Some(ReservedAttempt {
                id,
                request_id,
                question_id: command.session_question_id.clone(),
                session_id: command.session_id.clone(),
                purpose: purpose.into(),
                generation: generation as u64,
                sources,
                settings: settings.clone(),
            }))
        })
    }
    pub fn waive_ai(&self, request: &WaiveFollowUp) -> Result<()> {
        let previous = self.with_connection(|c| {
            let h: Option<String> = c
                .query_row(
                    "SELECT payload_hash FROM ai_receipts WHERE id=?1",
                    [&request.command.request_id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(sql_error)?;
            match h {
                Some(h) if h == hash(request)? => Ok(true),
                Some(_) => Err(failure("Identificador reutilizado.")),
                None => Ok(false),
            }
        })?;
        if previous {
            return Ok(());
        }
        let d = self.load_session(&request.command.session_id)?;
        if d.summary.mode != crate::interview::SessionMode::AiV3
            || d.summary.status != crate::interview::SessionStatus::InProgress
        {
            return Err(failure("Sessão IA não está em curso."));
        }
        let q = d
            .questions
            .iter()
            .find(|q| {
                q.question.id == request.command.session_question_id
                    && q.question.position == d.summary.position
            })
            .ok_or_else(|| failure("Pergunta desatualizada."))?;
        if q.confirmation.is_none() {
            return Err(failure("Confirme a resposta primeiro."));
        }
        let payload = hash(request)?;
        self.with_connection(|c| {
            let tx = c.transaction().map_err(sql_error)?;
            let existing: Option<String> = tx
                .query_row(
                    "SELECT payload_hash FROM ai_receipts WHERE id=?1",
                    [&request.command.request_id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(sql_error)?;
            if let Some(h) = existing {
                if h == payload {
                    return Ok(());
                }
                return Err(failure("Pedido reutilizado."));
            }
            if d.summary.version != request.command.expected_session_version
                || !matches!(
                    request.reason.as_str(),
                    "unavailable" | "request_failed" | "user_cancelled_wait"
                )
                || q.follow_up.is_some()
                || matches!(q.follow_up_stage.as_deref(), Some("waived" | "not_needed"))
            {
                return Err(failure("Não é possível dispensar esta etapa."));
            }
            tx.execute(
                "INSERT INTO ai_stages
 VALUES (?1,
'waived',
1,
?2)
 ON CONFLICT (question_id) DO UPDATE
 SET stage='waived',
generation=generation+1,
reason=excluded.reason",
                params![q.question.id, request.reason],
            )
            .map_err(sql_error)?;
            tx.execute(
                "INSERT INTO ai_receipts VALUES(?1,'waive',?2,?3)",
                params![request.command.request_id, payload, q.question.id],
            )
            .map_err(sql_error)?;
            tx.execute(
                "UPDATE sessions SET version=version+1 WHERE id=?1",
                [&d.summary.id],
            )
            .map_err(sql_error)?;
            tx.commit().map_err(sql_error)
        })
    }
    pub fn finish_ai(&self, a: &ReservedAttempt, result: &Result<RemoteCompletion>) -> Result<()> {
        let usage = result.as_ref().map(|r| r.usage.clone()).unwrap_or_default();
        let validated = result.as_ref().map_err(Clone::clone).and_then(|r| {
            if r.invalid {
                return Err(failure("A resposta foi truncada ou recusada."));
            }
            if a.purpose == "follow_up" {
                validate_follow_up(&r.content)
            } else {
                validate_feedback(&r.content, &a.sources)
            }
        });
        self.with_connection(|c| {
            let tx = c.transaction().map_err(sql_error)?;
            let state: String = tx
                .query_row("SELECT state FROM ai_attempts WHERE id=?1", [&a.id], |r| {
                    r.get(0)
                })
                .map_err(sql_error)?;
            if state != "running" {
                return Ok(());
            }
            let current = if a.purpose == "follow_up" {
                tx.query_row(
                    "SELECT stage='running' AND generation=?2 FROM ai_stages WHERE question_id=?1",
                    params![a.question_id, a.generation as i64],
                    |r| r.get::<_, bool>(0),
                )
                .optional()
                .map_err(sql_error)?
                .unwrap_or(false)
            } else {
                true
            };
            let final_state = if !current {
                "discarded"
            } else if validated.is_ok() {
                "completed"
            } else {
                "failed"
            };
            tx.execute(
                "UPDATE ai_attempts
 SET state=?2,
usage_json=?3,
error_json=?4,
sent=?5,
finished_at=?6,
returned_model=?7
 WHERE id=?1",
                params![
                    a.id,
                    final_state,
                    serde_json::to_string(&usage).unwrap(),
                    validated
                        .as_ref()
                        .err()
                        .map(|e| serde_json::to_string(e).unwrap()),
                    if result.is_ok() {
                        "received"
                    } else {
                        "possibly_sent"
                    },
                    utc_now(),
                    result.as_ref().ok().map(|r| r.model.as_str())
                ],
            )
            .map_err(sql_error)?;
            if current {
                match &validated {
                    Ok(v) if a.purpose == "follow_up" => {
                        let stage = if v["decision"] == "ask" {
                            let id = Uuid::new_v4().to_string();
                            tx.execute(
                                "INSERT INTO answer_slots(id,
session_question_id,
session_id,
role,
text)
 VALUES (?1,
?2,
?3,
'follow_up',
?4)",
                                params![id, a.question_id, a.session_id, v["question"].as_str()],
                            )
                            .map_err(sql_error)?;
                            "follow_up"
                        } else {
                            "not_needed"
                        };
                        tx.execute(
                            "UPDATE ai_stages SET stage=?2,reason=?3 WHERE question_id=?1",
                            params![a.question_id, stage, v["reason"].as_str()],
                        )
                        .map_err(sql_error)?;
                        tx.execute(
                            "UPDATE sessions SET version=version+1 WHERE id=?1",
                            [&a.session_id],
                        )
                        .map_err(sql_error)?;
                    }
                    Ok(v) => {
                        tx.execute(
                            "INSERT INTO ai_reports VALUES(?1,?2,?3)",
                            params![a.question_id, a.id, serde_json::to_string(v).unwrap()],
                        )
                        .map_err(sql_error)?;
                    }
                    Err(e) if a.purpose == "follow_up" => {
                        tx.execute(
                            "UPDATE ai_stages SET stage='failed',reason=?2 WHERE question_id=?1",
                            params![a.question_id, e.message],
                        )
                        .map_err(sql_error)?;
                    }
                    _ => {}
                }
            }
            tx.commit().map_err(sql_error)
        })
    }
    pub fn recover_ai(&self) -> Result<()> {
        self.with_connection(|c| {
            let tx = c.transaction().map_err(sql_error)?;
            tx.execute(
                "UPDATE ai_attempts
 SET state='interrupted',
finished_at=?1
 WHERE state='running'",
                [utc_now()],
            )
            .map_err(sql_error)?;
            tx.execute(
                "UPDATE ai_stages
 SET stage='failed',
generation=generation+1,
                        reason='Pedido interrompido; repetir pode gerar custo.'
 WHERE stage='running'",
                [],
            )
            .map_err(sql_error)?;
            tx.commit().map_err(sql_error)
        })
    }
    pub fn load_feedback(&self, session: &str) -> Result<FeedbackView> {
        let d = self.load_session(session)?;
        let mut reports = Vec::new();
        let mut attempts = Vec::new();
        let mut usages = Vec::new();
        self.with_connection(|c| {
            let mut st = c
                .prepare(
                    "SELECT r.question_id,
r.json
 FROM ai_reports r
 JOIN session_questions q ON q.id=r.question_id
 WHERE q.session_id=?1
 ORDER BY q.position",
                )
                .map_err(sql_error)?;
            for row in st
                .query_map([session], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })
                .map_err(sql_error)?
            {
                let (q, raw) = row.map_err(sql_error)?;
                let report: Value =
                    serde_json::from_str(&raw).map_err(|_| failure("Relatório ilegível."))?;
                let source = d.questions.iter().find(|v| v.question.id == q).unwrap();
                reports.push(json!({
                "questionId":q,
                "position":source.question.position,
                "questionText":source.question.text,
                "report":report,
                "sources":self_sources(source)}
                ));
            }
            let mut st = c
                .prepare(
                    "SELECT a.id,
a.state,
a.usage_json,
a.error_json,
a.created_at,
a.settings_json,
r.purpose,
r.question_id,
a.returned_model,
r.prompt_version,
r.rubric_version,
r.schema_version
 FROM ai_attempts a
 JOIN ai_requests r ON r.id=a.request_id
 WHERE r.session_id=?1
 ORDER BY a.rowid",
                )
                .map_err(sql_error)?;
            let mut rows = st.query([session]).map_err(sql_error)?;
            while let Some(row) = rows.next().map_err(sql_error)? {
                let raw: Option<String> = row.get(2).map_err(sql_error)?;
                let u = raw
                    .and_then(|s| serde_json::from_str::<NormalizedUsage>(&s).ok())
                    .unwrap_or_default();
                usages.push(u.clone());
                let settings: Value =
                    serde_json::from_str(&row.get::<_, String>(5).map_err(sql_error)?)
                        .unwrap_or(Value::Null);
                attempts.push(json!({
                "id":row.get::<_,
                String>(0).map_err(sql_error)?,
                "state":row.get::<_,
                String>(1).map_err(sql_error)?,
                "usage":u,
                "error":row.get::<_,
                Option<String>>(3).map_err(sql_error)?,
                "createdAt":row.get::<_,
                String>(4).map_err(sql_error)?,
                "requestedModel":settings["model"],
                "returnedModel":row.get::<_,
                Option<String>>(8).map_err(sql_error)?,
                "promptVersion":row.get::<_,
                String>(9).map_err(sql_error)?,
                "rubricVersion":row.get::<_,
                String>(10).map_err(sql_error)?,
                "schemaVersion":row.get::<_,
                String>(11).map_err(sql_error)?,
                "settings":settings,
                "purpose":row.get::<_,
                String>(6).map_err(sql_error)?,
                "questionId":row.get::<_,
                String>(7).map_err(sql_error)?}
                ));
            }
            Ok(())
        })?;
        let state = if d.summary.mode == crate::interview::SessionMode::LocalV2 {
            "local"
        } else if reports.len() == d.summary.count as usize {
            "ready"
        } else if attempts
            .iter()
            .any(|a| a["state"] == "running" && a["purpose"] == "feedback")
        {
            "running"
        } else if !reports.is_empty() {
            "partial"
        } else if attempts.iter().any(|a| a["purpose"] == "feedback") {
            "failed"
        } else {
            "pending"
        };
        let mut priorities = Vec::new();
        for section in ["gaps", "necessaryCorrections"] {
            for r in &reports {
                let observations = if section == "gaps" {
                    r["report"][section].as_array()
                } else {
                    r["report"]["english"][section].as_array()
                };
                if let Some(obs) = observations {
                    for p in r["report"]["priorities"].as_array().unwrap() {
                        if obs.iter().any(|o| o["id"] == p["observationId"]) && priorities.len() < 3
                        {
                            priorities.push(json!({
                            "questionId":r["questionId"],
                            "priority":p}
                            ));
                        }
                    }
                }
            }
        }
        Ok(FeedbackView {
            session_id: session.into(),
            state: state.into(),
            reports,
            priorities,
            usage: aggregate_usage(&usages),
            attempts,
        })
    }
}
fn self_sources(q: &QuestionDetail) -> Vec<Value> {
    let mut v = Vec::new();
    if let Some(c) = &q.confirmation {
        v.push(json!({
        "confirmationId":c.id,
        "role":"initial",
        "text":c.text}
        ));
    }
    if let Some(c) = q.follow_up.as_ref().and_then(|f| f.confirmation.as_ref()) {
        v.push(json!({
        "confirmationId":c.id,
        "role":"follow_up",
        "text":c.text}
        ));
    }
    v
}

/// Local onde um identificador de comando já foi visto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandIdentityLocation {
    AiAttempt,
    AiReceipt,
    CommandReceipt,
}

/// Identidade de um `requestId` já aplicado, para o preflight de comandos.
///
/// `Found` indica a única ocorrência e o seu conteúdo; `Conflict` indica mais de
/// uma ocorrência histórica, que é recusada em vez de escolhida arbitrariamente.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandIdentity {
    NotFound,
    Found {
        location: CommandIdentityLocation,
        kind: Option<String>,
        payload_hash: String,
    },
    Conflict,
}

/// Consulta o `requestId` em `ai_attempts.command_id`, `ai_receipts.id` e
/// `command_receipts.request_id`, dentro da transação de quem reserva/muta.
///
/// Mais de uma ocorrência é devolvida como [`CommandIdentity::Conflict`]. A
/// decisão de reutilizar o efeito original (mesma tabela/kind/hash) ou recusar
/// (conteúdo diferente) pertence ao chamador, que conhece o kind/hash esperados.
pub(crate) fn lookup_command_identity(
    connection: &Connection,
    request_id: &str,
) -> Result<CommandIdentity> {
    let attempt_hash: Option<String> = connection
        .query_row(
            "SELECT payload_hash FROM ai_attempts WHERE command_id = ?1",
            [request_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(sql_error)?;
    let receipt: Option<(String, String)> = connection
        .query_row(
            "SELECT kind, payload_hash FROM ai_receipts WHERE id = ?1",
            [request_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(sql_error)?;
    let command: Option<(String, String)> = connection
        .query_row(
            "SELECT kind, payload_hash FROM command_receipts WHERE request_id = ?1",
            [request_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(sql_error)?;

    let mut occurrences = 0usize;
    if attempt_hash.is_some() {
        occurrences += 1;
    }
    if receipt.is_some() {
        occurrences += 1;
    }
    if command.is_some() {
        occurrences += 1;
    }
    if occurrences > 1 {
        return Ok(CommandIdentity::Conflict);
    }
    if let Some(hash) = attempt_hash {
        return Ok(CommandIdentity::Found {
            location: CommandIdentityLocation::AiAttempt,
            kind: None,
            payload_hash: hash,
        });
    }
    if let Some((kind, hash)) = receipt {
        return Ok(CommandIdentity::Found {
            location: CommandIdentityLocation::AiReceipt,
            kind: Some(kind),
            payload_hash: hash,
        });
    }
    if let Some((kind, hash)) = command {
        return Ok(CommandIdentity::Found {
            location: CommandIdentityLocation::CommandReceipt,
            kind: Some(kind),
            payload_hash: hash,
        });
    }
    Ok(CommandIdentity::NotFound)
}

impl Database {
    /// Preflight de um identificador de comando, fora de uma transação.
    ///
    /// Consulta a identidade de um `requestId` já aplicado sem escrever. A
    /// variante transacional é [`lookup_command_identity`], usada pelos
    /// escritores antes de reservar ou mutar.
    pub fn lookup_command_identity(&self, request_id: &str) -> Result<CommandIdentity> {
        self.with_connection(|c| lookup_command_identity(c, request_id))
    }
}

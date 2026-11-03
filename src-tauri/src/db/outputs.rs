//! Saídas da fase 4 (tarefa 6): índice da voz das reformulações, comparação
//! local entre origens confirmadas e consumo de treino separado.
//!
//! Tudo aqui é local: nenhuma função chama a rede nem cria avaliações. O texto
//! de voz de uma reformulação vive em `rewrites.spoken_text` (imutável); a
//! tabela `rewrite_audio` só indexa derivados recriáveis.

use std::collections::{BTreeMap, HashSet};

use rusqlite::{params, OptionalExtension};

use super::Database;
use crate::ai::{aggregate_usage, NormalizedUsage};
use crate::domain::{error_code, AppError};
use crate::interview::{Id, QuestionDetail, Result, SessionDetail, SessionMode, SessionQuestion};
use crate::practice::{
    ComparisonAnswer, ComparisonEntry, ComparisonReport, ComparisonReportState, ComparisonRequest,
    ComparisonView, PracticeDetail, PurposeUsage, QuestionView, ReportState, ReportVersions,
    ResponseOrigin, RubricCompatibility, TrainingUsageView, UsageGroup,
};

/// Texto de voz de uma reformulação e a pergunta a que pertence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewriteVoice {
    pub rewrite_id: Id,
    pub spoken_text: String,
    pub session_id: Id,
    pub session_question_id: Id,
}

/// Estado de um derivado de voz.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RewriteAudioState {
    Pending,
    Ready,
    Failed,
}

impl RewriteAudioState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::Failed => "failed",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "ready" => Some(Self::Ready),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

/// Linha do índice `rewrite_audio`.
#[derive(Debug, Clone, PartialEq)]
pub struct RewriteAudioRow {
    pub rewrite_id: Id,
    pub config_hash: String,
    pub relative_file: String,
    pub text_hash: String,
    pub voice: String,
    pub speed: f64,
    pub state: RewriteAudioState,
}

impl Database {
    /// Texto de voz de uma reformulação guardada, sem rede.
    pub fn load_rewrite_voice(&self, rewrite_id: &str) -> Result<RewriteVoice> {
        self.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT w.id, w.spoken_text, r.session_id, r.question_id
                       FROM rewrites w
                       JOIN ai_requests r ON r.id = w.request_id
                      WHERE w.id = ?1",
                    [rewrite_id],
                    |row| {
                        Ok(RewriteVoice {
                            rewrite_id: row.get(0)?,
                            spoken_text: row.get(1)?,
                            session_id: row.get(2)?,
                            session_question_id: row.get(3)?,
                        })
                    },
                )
                .optional()
                .map_err(read_failed)?
                .ok_or_else(|| {
                    AppError::new(
                        error_code::STALE_SOURCE,
                        "A reformulação indicada não existe. Atualize a página e tente novamente.",
                        false,
                    )
                })
        })
    }

    /// Regista (ou atualiza) um derivado de voz. Os metadados são deriváveis; o
    /// texto da reformulação nunca é alterado por esta escrita.
    pub fn record_rewrite_audio(&self, row: &RewriteAudioRow) -> Result<()> {
        self.with_connection(|connection| {
            connection
                .execute(
                    "INSERT INTO rewrite_audio
                       (rewrite_id, config_hash, relative_file, text_hash, voice, speed, state)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT(rewrite_id, config_hash) DO UPDATE SET
                       relative_file = excluded.relative_file,
                       text_hash = excluded.text_hash,
                       voice = excluded.voice,
                       speed = excluded.speed,
                       state = excluded.state",
                    params![
                        row.rewrite_id,
                        row.config_hash,
                        row.relative_file,
                        row.text_hash,
                        row.voice,
                        row.speed,
                        row.state.as_str()
                    ],
                )
                .map_err(write_failed)?;
            Ok(())
        })
    }

    /// Muda o estado de um derivado já registado.
    pub fn set_rewrite_audio_state(
        &self,
        rewrite_id: &str,
        config_hash: &str,
        state: RewriteAudioState,
    ) -> Result<()> {
        self.with_connection(|connection| {
            connection
                .execute(
                    "UPDATE rewrite_audio SET state = ?3
                      WHERE rewrite_id = ?1 AND config_hash = ?2",
                    params![rewrite_id, config_hash, state.as_str()],
                )
                .map_err(write_failed)?;
            Ok(())
        })
    }

    /// Todos os derivados de voz registados, para a reconciliação.
    pub fn list_rewrite_audio(&self) -> Result<Vec<RewriteAudioRow>> {
        self.with_connection(|connection| {
            let mut statement = connection
                .prepare(
                    "SELECT rewrite_id, config_hash, relative_file, text_hash, voice, speed, state
                       FROM rewrite_audio ORDER BY rowid",
                )
                .map_err(read_failed)?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, f64>(5)?,
                        row.get::<_, String>(6)?,
                    ))
                })
                .map_err(read_failed)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(read_failed)?;
            rows.into_iter()
                .map(
                    |(rewrite_id, config_hash, relative_file, text_hash, voice, speed, state)| {
                        Ok(RewriteAudioRow {
                            rewrite_id,
                            config_hash,
                            relative_file,
                            text_hash,
                            voice,
                            speed,
                            state: RewriteAudioState::parse(&state)
                                .ok_or_else(|| corrupt("estado de voz desconhecido"))?,
                        })
                    },
                )
                .collect()
        })
    }

    // -- comparação ---------------------------------------------------------

    /// Compara localmente duas origens confirmadas da mesma pergunta.
    ///
    /// Recusa origens iguais, rascunhos, práticas abandonadas, sessões em curso
    /// e perguntas diferentes. Não cria avaliações nem texto analítico: devolve
    /// os dados fixos, por ordem de criação, e a compatibilidade de rubrica.
    pub fn compare_responses(&self, request: &ComparisonRequest) -> Result<ComparisonView> {
        if request.left == request.right {
            return Err(AppError::new(
                error_code::INVALID_INPUT,
                "Escolha duas respostas diferentes para comparar.",
                false,
            ));
        }
        let left = self.load_exercise_sources(&request.left)?;
        let right = self.load_exercise_sources(&request.right)?;
        if left.question_id != right.question_id {
            return Err(AppError::new(
                error_code::INVALID_INPUT,
                "As respostas pertencem a perguntas diferentes e não podem ser comparadas.",
                false,
            ));
        }

        let mut entries = vec![
            (0u8, self.comparison_entry(&request.left)?),
            (1u8, self.comparison_entry(&request.right)?),
        ];
        // Ordem pela criação, independentemente da ordem pedida; o empate
        // mantém a ordem pedida.
        entries.sort_by(|a, b| a.1.created_at.cmp(&b.1.created_at).then(a.0.cmp(&b.0)));
        let mut entries = entries.into_iter().map(|(_, entry)| entry);
        let left = entries.next().expect("duas entradas");
        let right = entries.next().expect("duas entradas");

        let rubric_compatibility = match (&left.report, &right.report) {
            (Some(a), Some(b)) => {
                if a.versions.rubric_version == b.versions.rubric_version
                    && a.versions.schema_version == b.versions.schema_version
                {
                    RubricCompatibility::Same
                } else {
                    RubricCompatibility::Different
                }
            }
            _ => RubricCompatibility::Unknown,
        };
        Ok(ComparisonView {
            left,
            right,
            rubric_compatibility,
        })
    }

    fn comparison_entry(&self, origin: &ResponseOrigin) -> Result<ComparisonEntry> {
        let rewrites: Vec<_> = self
            .list_rewrites(origin)?
            .into_iter()
            .filter(|view| view.state == ReportState::Ready)
            .collect();
        match origin {
            ResponseOrigin::Original {
                session_id,
                session_question_id,
            } => {
                let detail = self.load_session(session_id)?;
                original_entry(self, origin, &detail, session_question_id, rewrites)
            }
            ResponseOrigin::Practice { practice_id } => {
                let detail = self.load_practice(practice_id)?;
                let report = self.load_practice_report(practice_id)?;
                Ok(practice_entry(origin, detail, report, rewrites))
            }
        }
    }

    // -- consumo ------------------------------------------------------------

    /// Consumo de treino de uma sessão, separado entre a entrevista original
    /// (seguimento e feedback) e a prática posterior (reformulação e avaliação
    /// de prática). Os conjuntos são disjuntos por `ai_attempts.id` e o total
    /// combinado conta cada tentativa de rede uma única vez. Consumo desconhecido
    /// (`usage_json` nulo ou sem custo) conta como incompleto, nunca como zero.
    pub fn load_training_usage(&self, session_id: &str) -> Result<TrainingUsageView> {
        self.with_connection(|connection| {
            let exists: bool = connection
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sessions WHERE id = ?1)",
                    [session_id],
                    |row| row.get(0),
                )
                .map_err(read_failed)?;
            if !exists {
                return Err(AppError::new(
                    error_code::STALE_SOURCE,
                    "A sessão indicada não existe. Atualize o histórico antes de a abrir.",
                    false,
                ));
            }
            let mut statement = connection
                .prepare(
                    "SELECT a.id, r.purpose, a.usage_json
                       FROM ai_attempts a
                       JOIN ai_requests r ON r.id = a.request_id
                      WHERE r.session_id = ?1
                      ORDER BY a.rowid",
                )
                .map_err(read_failed)?;
            let rows = statement
                .query_map([session_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                })
                .map_err(read_failed)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(read_failed)?;

            let mut seen = HashSet::new();
            let mut original = Vec::new();
            let mut later = Vec::new();
            let mut combined = Vec::new();
            for (id, purpose, usage_json) in rows {
                if !seen.insert(id) {
                    continue;
                }
                let usage = usage_json
                    .as_deref()
                    .and_then(|raw| serde_json::from_str::<NormalizedUsage>(raw).ok())
                    .unwrap_or_default();
                let item = (purpose.clone(), usage);
                match purpose.as_str() {
                    "follow_up" | "feedback" => original.push(item.clone()),
                    _ => later.push(item.clone()),
                }
                combined.push(item);
            }
            Ok(TrainingUsageView {
                session_id: session_id.to_owned(),
                original_interview: usage_group(&original),
                later_practice: usage_group(&later),
                combined: usage_group(&combined),
            })
        })
    }
}

fn usage_group(items: &[(String, NormalizedUsage)]) -> UsageGroup {
    let all: Vec<NormalizedUsage> = items.iter().map(|(_, usage)| usage.clone()).collect();
    let mut by_purpose: BTreeMap<&str, Vec<NormalizedUsage>> = BTreeMap::new();
    for (purpose, usage) in items {
        by_purpose
            .entry(purpose.as_str())
            .or_default()
            .push(usage.clone());
    }
    UsageGroup {
        totals: aggregate_usage(&all),
        network_attempt_count: items.len() as u64,
        by_purpose: by_purpose
            .into_iter()
            .map(|(purpose, usages)| PurposeUsage {
                purpose: purpose.to_owned(),
                usage: aggregate_usage(&usages),
                network_attempt_count: usages.len() as u64,
            })
            .collect(),
    }
}

fn question_view(question: &SessionQuestion) -> QuestionView {
    QuestionView {
        text: question.text.clone(),
        category: question.category,
        competencies: question.competencies.clone(),
        criteria: question.criteria.clone(),
    }
}

fn original_entry(
    database: &Database,
    origin: &ResponseOrigin,
    detail: &SessionDetail,
    session_question_id: &str,
    rewrites: Vec<crate::practice::RewriteView>,
) -> Result<ComparisonEntry> {
    let main: &QuestionDetail = detail
        .questions
        .iter()
        .find(|question| question.question.id == session_question_id)
        .ok_or_else(|| {
            AppError::new(
                error_code::STALE_SOURCE,
                "A pergunta indicada não pertence a esta entrevista.",
                false,
            )
        })?;
    let mut answers = vec![ComparisonAnswer {
        role: "initial".into(),
        recordings: main.recordings.clone(),
        confirmation: main.confirmation.clone(),
    }];
    let mut confirmed_at = main
        .confirmation
        .as_ref()
        .map(|confirmation| confirmation.confirmed_at.clone());
    if let Some(follow_up) = main.follow_up.as_deref() {
        if follow_up.confirmation.is_some() {
            confirmed_at = follow_up
                .confirmation
                .as_ref()
                .map(|confirmation| confirmation.confirmed_at.clone());
            answers.push(ComparisonAnswer {
                role: "follow_up".into(),
                recordings: follow_up.recordings.clone(),
                confirmation: follow_up.confirmation.clone(),
            });
        }
    }

    let (report, report_state) = original_report(
        database,
        session_question_id,
        detail.summary.mode == SessionMode::LocalV2,
    )?;
    Ok(ComparisonEntry {
        origin: origin.clone(),
        created_at: detail.summary.created_at.clone(),
        confirmed_at,
        question: question_view(&main.question),
        answers,
        report,
        report_state,
        rewrites,
        preparation: database.load_preparation(&main.initial_answer_id)?,
    })
}

/// Relatório original (com versões) e o seu estado; nunca cria uma avaliação.
fn original_report(
    database: &Database,
    session_question_id: &str,
    local_only: bool,
) -> Result<(Option<ComparisonReport>, ComparisonReportState)> {
    database.with_connection(|connection| {
        let stored: Option<(String, String, String, String)> = connection
            .query_row(
                "SELECT rep.json, r.rubric_version, r.prompt_version, r.schema_version
                   FROM ai_reports rep
                   JOIN ai_attempts a ON a.id = rep.attempt_id
                   JOIN ai_requests r ON r.id = a.request_id
                  WHERE rep.question_id = ?1",
                [session_question_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .map_err(read_failed)?;
        if let Some((json, rubric_version, prompt_version, schema_version)) = stored {
            let content = serde_json::from_str(&json)
                .map_err(|_| corrupt("o relatório original é ilegível"))?;
            return Ok((
                Some(ComparisonReport {
                    content,
                    versions: ReportVersions {
                        rubric_version,
                        prompt_version,
                        schema_version,
                    },
                }),
                ComparisonReportState::Ready,
            ));
        }
        let mut statement = connection
            .prepare(
                "SELECT a.state FROM ai_attempts a
                   JOIN ai_requests r ON r.id = a.request_id
                  WHERE r.purpose = 'feedback' AND r.origin_key = ?1",
            )
            .map_err(read_failed)?;
        let states: Vec<String> = statement
            .query_map([format!("original:{session_question_id}")], |row| {
                row.get(0)
            })
            .map_err(read_failed)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(read_failed)?;
        let state = if states.iter().any(|state| state == "running") {
            ComparisonReportState::Running
        } else if !states.is_empty() {
            ComparisonReportState::Failed
        } else if local_only {
            ComparisonReportState::NotAvailable
        } else {
            ComparisonReportState::Pending
        };
        Ok((None, state))
    })
}

fn practice_entry(
    origin: &ResponseOrigin,
    detail: PracticeDetail,
    report: crate::practice::PracticeReportView,
    rewrites: Vec<crate::practice::RewriteView>,
) -> ComparisonEntry {
    let (comparison_report, report_state) = match (report.state, report.report) {
        (ReportState::Ready, Some(content)) => (
            Some(ComparisonReport {
                content,
                versions: ReportVersions {
                    rubric_version: report.rubric_version.unwrap_or_default(),
                    prompt_version: report.prompt_version.unwrap_or_default(),
                    schema_version: report.schema_version.unwrap_or_default(),
                },
            }),
            ComparisonReportState::Ready,
        ),
        (ReportState::Running, _) => (None, ComparisonReportState::Running),
        (ReportState::Failed, _) => (None, ComparisonReportState::Failed),
        _ => (None, ComparisonReportState::Pending),
    };
    ComparisonEntry {
        origin: origin.clone(),
        created_at: detail.created_at.clone(),
        confirmed_at: detail.confirmed_at.clone(),
        question: detail.question.clone(),
        answers: vec![ComparisonAnswer {
            role: "attempt".into(),
            recordings: detail.recordings.clone(),
            confirmation: detail.confirmation.clone(),
        }],
        report: comparison_report,
        report_state,
        rewrites,
        preparation: detail.preparation,
    }
}

fn read_failed(error: rusqlite::Error) -> AppError {
    AppError::new(
        error_code::STORAGE_WRITE_FAILED,
        format!("Não foi possível ler os dados locais: {error}."),
        true,
    )
}

fn write_failed(error: rusqlite::Error) -> AppError {
    AppError::new(
        error_code::STORAGE_WRITE_FAILED,
        format!("Não foi possível guardar os dados locais: {error}."),
        true,
    )
}

fn corrupt(detail: &str) -> AppError {
    AppError::new(
        error_code::STORAGE_WRITE_FAILED,
        format!("Dados locais inválidos: {detail}."),
        false,
    )
}

//! Persistência das conversas sobre artigos (esquema v4).
//!
//! Toda a escrita corre numa transação imediata com recibo por `requestId`:
//! repetir um comando devolve o resultado já guardado sem o aplicar outra vez.
//! Fontes, turnos, versões e relatórios são imutáveis (triggers); a eliminação é
//! lógica primeiro e só depois, sem ficheiros pendentes, remove as linhas.

use std::collections::{HashMap, HashSet};

use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use uuid::Uuid;

use super::questions::utc_now;
use super::Database;
use crate::article::{
    transition, validate_answer_text, validate_article, ArticleAudioView, ArticleAuthorization,
    ArticleCommand, ArticleContext, ArticleCreate, ArticleError, ArticleEvent, ArticleMutation,
    ArticleOutcome, ArticlePlayback, ArticleReport, ArticleRequestKind, ArticleSessionView,
    ArticleSnapshot, ArticleSpeaker, ArticleState, ArticleSummary, ArticleTicket, ArticleTurnView,
    ArticleUsageView, ArticleVersionSource, ArticleVersionView, ContextRole, ContextTurn,
    ARTICLE_NOTICE_VERSION, MAX_TURNS_PER_BRANCH, SUGGEST_FINISH_AFTER,
};
use crate::domain::{error_code, AppError};
use crate::interview::Result;

struct SessionRow {
    id: String,
    title: Option<String>,
    text: String,
    state: ArticleState,
    paused_from: Option<ArticleState>,
    failed_from: Option<ArticleState>,
    version: u64,
    generation: u64,
    branch: String,
    auth: Option<ArticleAuthorization>,
    error: Option<ArticleError>,
    deleted: bool,
}

/// Turno projetado num ramo, com a versão efetiva.
pub(crate) struct ProjectedTurn {
    pub id: String,
    pub branch_id: String,
    pub ordinal: u32,
    pub speaker: ArticleSpeaker,
    pub version_id: String,
    pub source: Option<ArticleVersionSource>,
    pub text: String,
    pub audio: bool,
    /// Fala já confirmada (tutor, revista ou transcrição dentro dos limites).
    pub sendable: bool,
}

impl Database {
    /// Cria uma sessão de artigo em rascunho, com o texto fixo.
    pub fn create_article(&self, input: ArticleCreate) -> Result<ArticleSnapshot> {
        validate_article(&input)?;
        let request_id = input.request_id.trim().to_owned();
        if request_id.is_empty() {
            return Err(invalid("O pedido não tem identificador."));
        }
        let title = input
            .title
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_owned);
        let text = input.text.trim().to_owned();
        let now = utc_now();
        self.with_connection(|connection| {
            let tx = begin(connection)?;
            if let Some((session_id, operation)) = receipt(&tx, &request_id)? {
                if operation != "create" {
                    return Err(reused_request());
                }
                return replay(&tx, &session_id, snapshot);
            }
            let active: bool = tx
                .query_row(
                    "SELECT count(*) > 0 FROM article_sessions
                     WHERE state <> 'completed' AND deleted_at IS NULL",
                    [],
                    |row| row.get(0),
                )
                .map_err(read_failed)?;
            if active {
                return Err(AppError::new(
                    error_code::ACTIVE_SESSION_EXISTS,
                    "Já existe uma conversa sobre um artigo em curso. Conclua-a ou elimine-a primeiro.",
                    false,
                ));
            }
            let session_id = Uuid::new_v4().to_string();
            let branch_id = Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO article_sessions
                   (id, title, article_text, state, version, generation,
                    active_branch_id, created_at, updated_at)
                 VALUES (?1, ?2, ?3, 'draft', 1, 1, ?4, ?5, ?5)",
                params![session_id, title, text, branch_id, now],
            )
            .map_err(write_failed)?;
            tx.execute(
                "INSERT INTO article_branches (id, session_id, created_at) VALUES (?1, ?2, ?3)",
                params![branch_id, session_id, now],
            )
            .map_err(write_failed)?;
            insert_receipt(&tx, &request_id, &session_id, "create", 1, &now)?;
            let result = snapshot(&tx, &session_id)?;
            tx.commit().map_err(write_failed)?;
            Ok(result)
        })
    }

    /// Contexto textual exato do ramo ativo (só falas confirmadas), para os pedidos à IA.
    pub fn article_context(&self, session_id: &str) -> Result<ArticleContext> {
        self.with_connection(|connection| {
            let session = load_session(connection, session_id)?;
            if session.deleted {
                return Err(deleted());
            }
            let turns = project_branch(connection, &session.branch)?;
            Ok(context_of(&session, &turns))
        })
    }

    /// Fotografia do estado persistido; sem efeitos externos.
    pub fn article_snapshot(&self, session_id: &str) -> Result<ArticleSnapshot> {
        self.with_connection(|connection| snapshot(connection, session_id))
    }

    /// Aplica uma mutação tipada, com recibo idempotente e versão otimista.
    pub fn apply_article(
        &self,
        command: &ArticleCommand,
        change: ArticleMutation,
    ) -> Result<ArticleSnapshot> {
        let request_id = command.request_id.trim().to_owned();
        if request_id.is_empty() {
            return Err(invalid("O comando não tem identificador."));
        }
        let now = utc_now();
        self.with_connection(|connection| {
            let tx = begin(connection)?;
            if let Some((session_id, operation)) = receipt(&tx, &request_id)? {
                if session_id != command.session_id || operation != operation_name(&change) {
                    return Err(reused_request());
                }
                return replay(&tx, &session_id, |tx, id| {
                    build_snapshot(tx, load_session(tx, id)?)
                });
            }
            let session = load_session(&tx, &command.session_id)?;
            if session.deleted {
                return Err(deleted());
            }
            if session.version != command.expected_version {
                return Err(AppError::new(
                    error_code::STALE_SOURCE,
                    "A conversa mudou entretanto. Atualize antes de repetir.",
                    false,
                ));
            }
            let operation = operation_name(&change);
            apply(&tx, &session, change, &now)?;
            insert_receipt(
                &tx,
                &request_id,
                &session.id,
                operation,
                session.version + 1,
                &now,
            )?;
            let result = build_snapshot(&tx, load_session(&tx, &session.id)?)?;
            tx.commit().map_err(write_failed)?;
            Ok(result)
        })
    }

    /// Já existe um recibo para este identificador (comando repetido).
    pub fn article_replayed(&self, request_id: &str) -> Result<bool> {
        self.with_connection(|connection| Ok(receipt(connection, request_id.trim())?.is_some()))
    }

    /// Caminhos relativos dos ficheiros ainda por remover de uma sessão eliminada.
    pub fn pending_article_artifacts(&self, session_id: &str) -> Result<Vec<String>> {
        self.with_connection(|connection| {
            let mut statement = connection
                .prepare(
                    "SELECT path FROM article_artifacts
                     WHERE session_id = ?1 AND state = 'pending_delete' ORDER BY path",
                )
                .map_err(read_failed)?;
            let rows = statement
                .query_map([session_id], |row| row.get::<_, String>(0))
                .map_err(read_failed)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(read_failed)?;
            Ok(rows)
        })
    }

    /// Regista que o ficheiro foi removido do disco.
    pub fn mark_article_artifact_removed(&self, path: &str) -> Result<()> {
        self.with_connection(|connection| {
            connection
                .execute("DELETE FROM article_artifacts WHERE path = ?1", [path])
                .map_err(write_failed)?;
            Ok(())
        })
    }

    /// Remove as linhas de uma sessão eliminada quando já não há ficheiros.
    ///
    /// Devolve `false` enquanto houver ficheiros por remover; repetir é inofensivo.
    pub fn purge_article_if_clean(&self, session_id: &str) -> Result<bool> {
        self.with_connection(|connection| {
            let tx = begin(connection)?;
            let deleted: Option<bool> = tx
                .query_row(
                    "SELECT deleted_at IS NOT NULL FROM article_sessions WHERE id = ?1",
                    [session_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(read_failed)?;
            match deleted {
                None => return Ok(true),
                Some(false) => {
                    return Err(invalid("A conversa não foi eliminada."));
                }
                Some(true) => {}
            }
            let remaining: i64 = tx
                .query_row(
                    "SELECT count(*) FROM article_artifacts WHERE session_id = ?1",
                    [session_id],
                    |row| row.get(0),
                )
                .map_err(read_failed)?;
            if remaining > 0 {
                return Ok(false);
            }
            for table in [
                "article_reports",
                "article_requests",
                "article_versions",
                "article_turns",
                "article_branches",
            ] {
                tx.execute(
                    &format!("DELETE FROM {table} WHERE session_id = ?1"),
                    [session_id],
                )
                .map_err(write_failed)?;
            }
            tx.execute("DELETE FROM article_sessions WHERE id = ?1", [session_id])
                .map_err(write_failed)?;
            // Recibos: só o da criação sobrevive (repetir a criação não duplica a
            // sessão) e é podado ao fim de 30 dias; os restantes saem já.
            tx.execute(
                "DELETE FROM article_receipts
                 WHERE (session_id = ?1 AND operation <> 'create')
                    OR (operation = 'create'
                        AND created_at < strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-30 days')
                        AND session_id NOT IN (SELECT id FROM article_sessions))",
                [session_id],
            )
            .map_err(write_failed)?;
            tx.commit().map_err(write_failed)?;
            Ok(true)
        })
    }
}

fn begin(connection: &mut Connection) -> Result<Transaction<'_>> {
    connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(write_failed)
}

fn invalid(message: &str) -> AppError {
    AppError::new(error_code::INVALID_INPUT, message, false)
}

fn deleted() -> AppError {
    AppError::new("ARTICLE_DELETED", "Esta conversa foi eliminada.", false)
}

fn read_failed(error: rusqlite::Error) -> AppError {
    AppError::new(
        error_code::DATABASE_UNAVAILABLE,
        format!("Não foi possível ler a conversa guardada: {error}"),
        false,
    )
}

fn write_failed(error: rusqlite::Error) -> AppError {
    AppError::new(
        error_code::STORAGE_WRITE_FAILED,
        format!("Não foi possível guardar a conversa; nenhuma alteração foi aplicada: {error}"),
        true,
    )
}

/// Sessão e operação já registadas para um `requestId`.
fn receipt(connection: &Connection, request_id: &str) -> Result<Option<(String, String)>> {
    connection
        .query_row(
            "SELECT session_id, operation FROM article_receipts WHERE request_id = ?1",
            [request_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(read_failed)
}

fn reused_request() -> AppError {
    invalid("O identificador do pedido já foi usado noutra operação ou conversa.")
}

/// Repete um recibo: devolve o resultado atual da sessão, ou `ARTICLE_DELETED`
/// se a conversa já foi eliminada e limpa (os recibos sobrevivem à limpeza para
/// que repetir uma criação não gere uma sessão nova).
fn replay(
    connection: &Connection,
    session_id: &str,
    build: impl FnOnce(&Connection, &str) -> Result<ArticleSnapshot>,
) -> Result<ArticleSnapshot> {
    let exists: bool = connection
        .query_row(
            "SELECT count(*) > 0 FROM article_sessions WHERE id = ?1",
            [session_id],
            |row| row.get(0),
        )
        .map_err(read_failed)?;
    if !exists {
        return Err(deleted());
    }
    build(connection, session_id)
}

fn insert_receipt(
    tx: &Transaction<'_>,
    request_id: &str,
    session_id: &str,
    operation: &str,
    version_after: u64,
    now: &str,
) -> Result<()> {
    tx.execute(
        "INSERT INTO article_receipts (request_id, session_id, operation, version_after, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![request_id, session_id, operation, version_after as i64, now],
    )
    .map_err(write_failed)?;
    Ok(())
}

fn state_to_str(state: ArticleState) -> String {
    serde_json::to_value(state)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn state_from_str(value: &str) -> Result<ArticleState> {
    serde_json::from_value(serde_json::Value::String(value.to_owned())).map_err(|_| {
        AppError::new(
            error_code::DATABASE_UNAVAILABLE,
            "Estado de conversa desconhecido na base de dados.",
            false,
        )
    })
}

fn load_session(connection: &Connection, session_id: &str) -> Result<SessionRow> {
    let row = connection
        .query_row(
            "SELECT id, title, article_text, state, paused_from, version, generation,
                    active_branch_id, auth_destination, auth_notice_version,
                    error_json, deleted_at IS NOT NULL, failed_from
             FROM article_sessions WHERE id = ?1",
            [session_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, Option<String>>(10)?,
                    row.get::<_, bool>(11)?,
                    row.get::<_, Option<String>>(12)?,
                ))
            },
        )
        .optional()
        .map_err(read_failed)?;
    let Some((
        id,
        title,
        text,
        state,
        paused,
        version,
        generation,
        branch,
        dest,
        notice,
        err,
        del,
        failed,
    )) = row
    else {
        return Err(AppError::new(
            error_code::STALE_SOURCE,
            "A conversa indicada não existe.",
            false,
        ));
    };
    Ok(SessionRow {
        id,
        title,
        text,
        state: state_from_str(&state)?,
        paused_from: paused.as_deref().map(state_from_str).transpose()?,
        failed_from: failed.as_deref().map(state_from_str).transpose()?,
        version: version as u64,
        generation: generation as u64,
        branch,
        auth: match (dest, notice) {
            (Some(destination), Some(notice_version)) => Some(ArticleAuthorization {
                destination,
                notice_version,
            }),
            _ => None,
        },
        error: err.and_then(|json| serde_json::from_str(&json).ok()),
        deleted: del,
    })
}

/// Projeção do percurso de um ramo: prefixo ancestral até à bifurcação mais os
/// turnos do próprio ramo, com a versão efetiva de cada turno.
///
/// Faz um número constante de consultas (ramos, turnos, versões e áudios da
/// sessão) e resolve a cadeia em memória, porque corre dentro de transações
/// imediatas que bloqueiam a base.
pub(crate) fn project_branch(
    connection: &Connection,
    branch_id: &str,
) -> Result<Vec<ProjectedTurn>> {
    let session_id: String = connection
        .query_row(
            "SELECT session_id FROM article_branches WHERE id = ?1",
            [branch_id],
            |row| row.get(0),
        )
        .map_err(read_failed)?;

    let mut parents: HashMap<String, (Option<String>, Option<String>)> = HashMap::new();
    for row in collect(
        connection,
        "SELECT id, parent_branch_id, fork_turn_id FROM article_branches WHERE session_id = ?1",
        &session_id,
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        },
    )? {
        parents.insert(row.0, (row.1, row.2));
    }

    let rows = collect(
        connection,
        "SELECT id, branch_id, ordinal, speaker FROM article_turns WHERE session_id = ?1",
        &session_id,
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u32>(2)?,
                row.get::<_, String>(3)?,
            ))
        },
    )?;
    let ordinals: HashMap<&str, u32> = rows.iter().map(|r| (r.0.as_str(), r.2)).collect();

    // Cadeia da folha à raiz, com o limite de ordinal de cada ramo.
    let mut chain: Vec<(String, Option<u32>)> = Vec::new();
    let mut current = branch_id.to_owned();
    let mut limit: Option<u32> = None;
    loop {
        chain.push((current.clone(), limit));
        match parents.get(&current) {
            Some((Some(parent), Some(fork))) if chain.len() <= parents.len() => {
                let ordinal = *ordinals
                    .get(fork.as_str())
                    .ok_or_else(|| read_failed(rusqlite::Error::QueryReturnedNoRows))?;
                limit = Some(limit.map_or(ordinal, |l| l.min(ordinal)));
                current = parent.clone();
            }
            _ => break,
        }
    }

    // Versão mais recente de cada (turno, ramo).
    type Latest = (i64, String, Option<String>, String);
    let mut latest: HashMap<(String, String), Latest> = HashMap::new();
    for (id, turn, branch, seq, source, text) in collect(
        connection,
        "SELECT id, turn_id, branch_id, seq, source, text FROM article_versions
         WHERE session_id = ?1",
        &session_id,
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
            ))
        },
    )? {
        let entry = latest
            .entry((turn, branch))
            .or_insert((0, id.clone(), None, String::new()));
        if seq > entry.0 {
            *entry = (seq, id, source, text);
        }
    }

    let with_audio: HashSet<String> = collect(
        connection,
        "SELECT DISTINCT turn_id FROM article_artifacts
         WHERE session_id = ?1 AND state = 'present' AND turn_id IS NOT NULL",
        &session_id,
        |row| row.get::<_, String>(0),
    )?
    .into_iter()
    .collect();

    let mut turns: Vec<ProjectedTurn> = Vec::new();
    for (id, branch, ordinal, speaker) in rows {
        let in_path = chain
            .iter()
            .any(|(b, limit)| *b == branch && limit.is_none_or(|l| ordinal <= l));
        if !in_path {
            continue;
        }
        let speaker = if speaker == "tutor" {
            ArticleSpeaker::Tutor
        } else {
            ArticleSpeaker::User
        };
        let version = chain
            .iter()
            .find_map(|(candidate, _)| latest.get(&(id.clone(), candidate.clone())));
        let (version_id, source, text, sendable) = match version {
            Some((_, vid, source, text)) => {
                let sendable = match source.as_deref() {
                    // Fala do tutor.
                    None => true,
                    Some("manual_review") => true,
                    // A transcrição automática só segue se couber nos limites.
                    Some(_) => validate_answer_text(text).is_ok(),
                };
                (
                    vid.clone(),
                    source.as_deref().map(|s| {
                        if s == "manual_review" {
                            ArticleVersionSource::ManualReview
                        } else {
                            ArticleVersionSource::AutomaticSent
                        }
                    }),
                    text.clone(),
                    sendable,
                )
            }
            // Turno do utilizador sem texto ainda (gravação em curso) ou vazio.
            None => (String::new(), None, String::new(), false),
        };
        turns.push(ProjectedTurn {
            audio: with_audio.contains(&id),
            id,
            branch_id: branch,
            ordinal,
            speaker,
            version_id,
            source,
            text,
            sendable,
        });
    }
    turns.sort_by_key(|t| t.ordinal);
    Ok(turns)
}

fn collect<T>(
    connection: &Connection,
    sql: &str,
    session_id: &str,
    map: impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>> {
    let mut statement = connection.prepare(sql).map_err(read_failed)?;
    let rows = statement
        .query_map([session_id], map)
        .map_err(read_failed)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(read_failed)?;
    Ok(rows)
}

/// Contexto enviado à IA: só falas confirmadas. Texto por rever (transcrição
/// excessiva ou vazia) fica de fora, tal como do limite de tamanho.
fn context_of(session: &SessionRow, turns: &[ProjectedTurn]) -> ArticleContext {
    let sent: Vec<&ProjectedTurn> = turns.iter().filter(|t| t.sendable).collect();
    ArticleContext {
        article: session.text.clone(),
        title: session.title.clone(),
        turns: sent
            .iter()
            .map(|t| ContextTurn {
                role: match t.speaker {
                    ArticleSpeaker::Tutor => ContextRole::Tutor,
                    ArticleSpeaker::User => ContextRole::User,
                },
                text: t.text.clone(),
            })
            .collect(),
        branch_id: session.branch.clone(),
        source_version_ids: sent.iter().map(|t| t.version_id.clone()).collect(),
        turn_ids: sent.iter().map(|t| t.id.clone()).collect(),
    }
}

/// Número de respostas do utilizador já enviadas ou enviáveis no percurso.
fn answer_count(turns: &[ProjectedTurn]) -> usize {
    turns
        .iter()
        .filter(|t| t.speaker == ArticleSpeaker::User && t.sendable)
        .count()
}

/// Estado a que uma nova autorização regressa, conforme a última fala.
fn resume_target(turns: &[ProjectedTurn]) -> ArticleState {
    match turns.last() {
        None => ArticleState::AwaitingTutor,
        Some(t) if t.speaker == ArticleSpeaker::Tutor => ArticleState::Ready,
        Some(t) if t.sendable => ArticleState::AwaitingTutor,
        // Texto por rever (transcrição excessiva): continua por rever.
        Some(t) if !t.version_id.is_empty() => ArticleState::NeedsReview,
        // Gravação abandonada ou transcrição vazia: grava-se de novo.
        Some(_) => ArticleState::Ready,
    }
}

fn snapshot(connection: &Connection, session_id: &str) -> Result<ArticleSnapshot> {
    let session = load_session(connection, session_id)?;
    if session.deleted {
        return Err(deleted());
    }
    build_snapshot(connection, session)
}

/// Fotografia sem recusar sessões eliminadas: resultado do próprio comando de
/// eliminação e das suas repetições.
fn build_snapshot(connection: &Connection, session: SessionRow) -> Result<ArticleSnapshot> {
    let projected = project_branch(connection, &session.branch)?;
    let answers = answer_count(&projected);
    let context = context_of(&session, &projected);
    // Um turno do utilizador sem texto só aparece quando aguarda texto manual.
    let awaiting_review = session.state == ArticleState::NeedsReview
        || (session.state == ArticleState::Paused
            && session.paused_from == Some(ArticleState::NeedsReview));
    let last = projected.len().saturating_sub(1);
    let turns = projected
        .iter()
        .enumerate()
        .filter(|(index, t)| {
            t.speaker == ArticleSpeaker::Tutor
                || !t.text.is_empty()
                || (awaiting_review && *index == last)
        })
        .map(|(_, t)| t)
        .enumerate()
        // Posição visível (1, 2, 3…): turnos escondidos não deixam lacunas.
        .map(|(position, t)| ArticleTurnView {
            id: t.id.clone(),
            branch_id: t.branch_id.clone(),
            index: position as u32 + 1,
            speaker: t.speaker,
            version: ArticleVersionView {
                id: t.version_id.clone(),
                source: t.source,
                text: t.text.clone(),
            },
            audio_available: t.audio,
            from_previous_branch: t.branch_id != session.branch,
        })
        .collect();
    let report: Option<ArticleReport> = connection
        .query_row(
            "SELECT report_json FROM article_reports
             WHERE session_id = ?1 AND branch_id = ?2 ORDER BY created_at DESC, rowid DESC LIMIT 1",
            params![session.id, session.branch],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(read_failed)?
        .and_then(|json| serde_json::from_str(&json).ok());
    Ok(ArticleSnapshot {
        session: ArticleSessionView {
            id: session.id.clone(),
            title: session.title.clone(),
            article_text: session.text.clone(),
            version: session.version,
            state: session.state,
            active_branch_id: session.branch.clone(),
            authorization: session.auth.clone(),
            answers: answers as u32,
            suggest_finish: answers >= SUGGEST_FINISH_AFTER,
            limit_reached: answers >= MAX_TURNS_PER_BRANCH || !context.has_room_for_next_answer(),
            error: session.error.clone(),
            usage: usage_view(connection, &session.id)?,
        },
        turns,
        report,
        audio: ArticleAudioView {
            playback: ArticlePlayback::Idle,
            turn_id: None,
        },
    })
}

/// Caminho exclusivo da sessão, relativo à pasta de dados.
pub(crate) fn artifact_path_ok(session_id: &str, path: &str) -> bool {
    let prefix = format!("articles/{session_id}/");
    path.starts_with(&prefix)
        && path.len() > prefix.len()
        && !path.contains("..")
        && !path.contains('\\')
        && !path.contains(':')
}

fn next_ordinal(turns: &[ProjectedTurn]) -> u32 {
    turns.iter().map(|t| t.ordinal).max().unwrap_or(0) + 1
}

fn next_seq(tx: &Transaction<'_>, turn_id: &str, branch_id: &str) -> Result<i64> {
    tx.query_row(
        "SELECT coalesce(max(seq), 0) + 1 FROM article_versions
         WHERE turn_id = ?1 AND branch_id = ?2",
        params![turn_id, branch_id],
        |row| row.get(0),
    )
    .map_err(read_failed)
}

fn insert_version(
    tx: &Transaction<'_>,
    session: &SessionRow,
    turn_id: &str,
    source: Option<&str>,
    text: &str,
    now: &str,
) -> Result<()> {
    let seq = next_seq(tx, turn_id, &session.branch)?;
    tx.execute(
        "INSERT INTO article_versions (id, session_id, turn_id, branch_id, seq, source, text, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            Uuid::new_v4().to_string(),
            session.id,
            turn_id,
            session.branch,
            seq,
            source,
            text,
            now
        ],
    )
    .map_err(write_failed)?;
    Ok(())
}

/// O turno indicado tem de ser o turno atual do utilizador (o último do
/// percurso): uma transcrição atrasada ou uma correção antiga não reescreve
/// respostas anteriores.
fn require_current_turn(turns: &[ProjectedTurn], turn_id: &str) -> Result<()> {
    match turns.last() {
        Some(t) if t.speaker == ArticleSpeaker::User && t.id == turn_id => Ok(()),
        _ => Err(AppError::new(
            error_code::STALE_SOURCE,
            "O turno indicado já não é o turno atual da conversa.",
            false,
        )),
    }
}

/// Nome da operação guardado no recibo; liga o `requestId` a uma só operação.
fn operation_name(change: &ArticleMutation) -> &'static str {
    match change {
        ArticleMutation::Authorize { .. } => "authorize",
        ArticleMutation::Revoke => "revoke",
        ArticleMutation::Pause => "pause",
        ArticleMutation::Resume => "resume",
        ArticleMutation::AddTutorTurn { .. } => "tutor_turn",
        ArticleMutation::StartRecording { .. } => "start_recording",
        ArticleMutation::StopRecording => "stop_recording",
        ArticleMutation::RecordTranscript { .. } => "transcript",
        ArticleMutation::ManualText { .. } => "manual_text",
        ArticleMutation::Finish => "finish",
        ArticleMutation::Complete => "complete",
        ArticleMutation::Fail { .. } => "fail",
        ArticleMutation::Retry => "retry",
        ArticleMutation::Delete => "delete",
    }
}

/// Aplica a mutação e atualiza a sessão.
fn apply(
    tx: &Transaction<'_>,
    session: &SessionRow,
    change: ArticleMutation,
    now: &str,
) -> Result<()> {
    let operation = operation_name(&change);
    let mut state = session.state;
    let mut generation = session.generation;
    let mut paused_from = session.paused_from;
    let mut failed_from = session.failed_from;
    let mut error = None;
    let mut auth = session.auth.clone();
    // Uma só projeção por operação, reutilizada por todas as verificações.
    let turns = project_branch(tx, &session.branch)?;
    match change {
        ArticleMutation::Authorize {
            destination,
            notice_version,
        } => {
            if destination.trim().is_empty() || notice_version != ARTICLE_NOTICE_VERSION {
                return Err(invalid("A autorização não corresponde ao aviso atual."));
            }
            // Feedback final falhado antes da revogação: a nova autorização retoma o relatório.
            let resume_to = if session.failed_from == Some(ArticleState::Finishing) {
                ArticleState::Finishing
            } else {
                resume_target(&turns)
            };
            state = transition(state, ArticleEvent::Authorize { resume_to })?;
            auth = Some(ArticleAuthorization {
                destination,
                notice_version,
            });
        }
        ArticleMutation::Revoke => {
            state = transition(state, ArticleEvent::Revoke)?;
            auth = None;
            generation += 1;
            // Só a intenção de terminar sobrevive à revogação.
            failed_from = session
                .failed_from
                .filter(|from| *from == ArticleState::Finishing);
        }
        ArticleMutation::Pause => {
            paused_from = Some(state);
            state = transition(state, ArticleEvent::Pause)?;
            error = session.error.clone();
        }
        ArticleMutation::Resume => {
            let target = session
                .paused_from
                .ok_or_else(|| invalid("A conversa não está em pausa."))?;
            state = transition(state, ArticleEvent::Resume(target))?;
            paused_from = None;
            error = session.error.clone();
        }
        ArticleMutation::AddTutorTurn { text } => {
            let text = text.trim();
            if text.is_empty() {
                return Err(invalid("A fala do tutor está vazia."));
            }
            state = transition(state, ArticleEvent::TutorReplied)?;
            let turn_id = insert_turn(tx, session, next_ordinal(&turns), "tutor", now)?;
            insert_version(tx, session, &turn_id, None, text, now)?;
        }
        ArticleMutation::StartRecording {
            turn_id,
            audio_path,
        } => {
            if turn_id.trim().is_empty() || !artifact_path_ok(&session.id, &audio_path) {
                return Err(invalid("O ficheiro de áudio não pertence a esta conversa."));
            }
            let context = context_of(session, &turns);
            if answer_count(&turns) >= MAX_TURNS_PER_BRANCH || !context.has_room_for_next_answer() {
                return Err(invalid(
                    "A conversa atingiu o limite. Termine para obter o feedback.",
                ));
            }
            state = transition(state, ArticleEvent::StartRecording)?;
            tx.execute(
                "INSERT INTO article_turns (id, session_id, branch_id, ordinal, speaker, created_at)
                 VALUES (?1, ?2, ?3, ?4, 'user', ?5)",
                params![turn_id, session.id, session.branch, next_ordinal(&turns), now],
            )
            .map_err(write_failed)?;
            tx.execute(
                "INSERT INTO article_artifacts (id, session_id, turn_id, kind, path, state, created_at)
                 VALUES (?1, ?2, ?3, 'answer_audio', ?4, 'present', ?5)",
                params![Uuid::new_v4().to_string(), session.id, turn_id, audio_path, now],
            )
            .map_err(write_failed)?;
        }
        ArticleMutation::StopRecording => {
            state = transition(state, ArticleEvent::UserStop)?;
        }
        ArticleMutation::RecordTranscript { turn_id, text } => {
            require_current_turn(&turns, &turn_id)?;
            let text = text.map(|t| t.trim().to_owned()).filter(|t| !t.is_empty());
            let sendable = text
                .as_deref()
                .is_some_and(|t| validate_answer_text(t).is_ok());
            state = transition(
                state,
                if sendable {
                    ArticleEvent::TranscriptReady
                } else {
                    ArticleEvent::TranscriptFailed
                },
            )?;
            if let Some(text) = &text {
                // A transcrição automática fica sempre guardada, mesmo se excessiva.
                insert_version(tx, session, &turn_id, Some("automatic_sent"), text, now)?;
            }
        }
        ArticleMutation::ManualText { turn_id, text } => {
            validate_answer_text(&text)?;
            require_current_turn(&turns, &turn_id)?;
            state = transition(state, ArticleEvent::ManualText)?;
            insert_version(
                tx,
                session,
                &turn_id,
                Some("manual_review"),
                text.trim(),
                now,
            )?;
        }
        ArticleMutation::Finish => {
            state = transition(state, ArticleEvent::Finish)?;
            generation += 1;
        }
        ArticleMutation::Complete => {
            state = transition(state, ArticleEvent::ReportReady)?;
        }
        ArticleMutation::Fail { error: failure } => {
            // Guarda de onde veio a falha para o `Retry` repetir o passo certo.
            failed_from = Some(state);
            let event = if state == ArticleState::Recording {
                ArticleEvent::CaptureInterrupted
            } else {
                ArticleEvent::RequestFailed
            };
            state = transition(state, event)?;
            error = Some(failure);
        }
        ArticleMutation::Retry => {
            let from = session
                .failed_from
                .ok_or_else(|| invalid("Não há nenhuma falha para repetir."))?;
            state = transition(state, ArticleEvent::Retry(from))?;
            failed_from = None;
        }
        ArticleMutation::Delete => {
            // O ficheiro em escrita não pode ser marcado para remoção.
            if session.state == ArticleState::Recording {
                return Err(invalid("Pare a gravação antes de eliminar a conversa."));
            }
            tx.execute(
                "UPDATE article_artifacts SET state = 'pending_delete' WHERE session_id = ?1",
                [&session.id],
            )
            .map_err(write_failed)?;
            tx.execute(
                "UPDATE article_sessions SET deleted_at = ?2, generation = generation + 1,
                        version = version + 1, updated_at = ?2 WHERE id = ?1",
                params![session.id, now],
            )
            .map_err(write_failed)?;
            return Ok(());
        }
    }
    // A origem da falha só se mantém enquanto a falha existe (incl. em pausa).
    if !matches!(operation, "fail" | "pause" | "resume" | "revoke") {
        failed_from = None;
    }
    tx.execute(
        "UPDATE article_sessions
         SET state = ?2, paused_from = ?3, failed_from = ?4, generation = ?5,
             version = version + 1, auth_destination = ?6, auth_notice_version = ?7,
             error_json = ?8, updated_at = ?9
         WHERE id = ?1",
        params![
            session.id,
            state_to_str(state),
            paused_from.map(state_to_str),
            failed_from.map(state_to_str),
            generation as i64,
            auth.as_ref().map(|a| a.destination.clone()),
            auth.as_ref().map(|a| a.notice_version.clone()),
            error.and_then(|e| serde_json::to_string(&e).ok()),
            now
        ],
    )
    .map_err(write_failed)?;
    Ok(())
}

fn insert_turn(
    tx: &Transaction<'_>,
    session: &SessionRow,
    ordinal: u32,
    speaker: &str,
    now: &str,
) -> Result<String> {
    let id = Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO article_turns (id, session_id, branch_id, ordinal, speaker, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, session.id, session.branch, ordinal, speaker, now],
    )
    .map_err(write_failed)?;
    Ok(id)
}

// -- pedidos ao fornecedor, listagem e consumo ------------------------------

impl Database {
    /// Conversas não eliminadas, da mais recente para a mais antiga.
    pub fn list_articles(&self) -> Result<Vec<ArticleSummary>> {
        self.with_connection(|connection| {
            let mut statement = connection
                .prepare(
                    "SELECT id, title, state, updated_at FROM article_sessions
                     WHERE deleted_at IS NULL ORDER BY updated_at DESC, rowid DESC",
                )
                .map_err(read_failed)?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })
                .map_err(read_failed)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(read_failed)?;
            rows.into_iter()
                .map(|(id, title, state, updated_at)| {
                    Ok(ArticleSummary {
                        id,
                        title,
                        state: state_from_str(&state)?,
                        updated_at,
                    })
                })
                .collect()
        })
    }

    /// Aplica uma mutação iniciada pelo núcleo (resultado de pedido, falha),
    /// sem versão esperada nem recibo: a validade vem do estado e da geração.
    pub fn apply_article_system(
        &self,
        session_id: &str,
        change: ArticleMutation,
    ) -> Result<ArticleSnapshot> {
        let now = utc_now();
        self.with_connection(|connection| {
            let tx = begin(connection)?;
            let session = load_session(&tx, session_id)?;
            if session.deleted {
                return Err(deleted());
            }
            apply(&tx, &session, change, &now)?;
            let result = build_snapshot(&tx, load_session(&tx, &session.id)?)?;
            tx.commit().map_err(write_failed)?;
            Ok(result)
        })
    }

    /// Regista um pedido pendente com o contexto exato que vai ser enviado.
    ///
    /// Devolve `None` quando não há nada a pedir: estado sem pedido, sessão
    /// eliminada ou já existe um pedido pendente nesta geração (clique duplo).
    pub fn begin_article_request(
        &self,
        session_id: &str,
        config_json: &str,
    ) -> Result<Option<ArticleTicket>> {
        let now = utc_now();
        self.with_connection(|connection| {
            let tx = begin(connection)?;
            let session = load_session(&tx, session_id)?;
            if session.deleted {
                return Ok(None);
            }
            let turns = project_branch(&tx, &session.branch)?;
            let kind = match session.state {
                ArticleState::AwaitingTutor if turns.iter().all(|t| !t.sendable) => {
                    ArticleRequestKind::Opening
                }
                ArticleState::AwaitingTutor => ArticleRequestKind::Reply,
                ArticleState::Finishing => ArticleRequestKind::Feedback,
                _ => return Ok(None),
            };
            let pending: bool = tx
                .query_row(
                    "SELECT count(*) > 0 FROM article_requests
                     WHERE session_id = ?1 AND state = 'pending' AND generation = ?2",
                    params![session.id, session.generation as i64],
                    |row| row.get(0),
                )
                .map_err(read_failed)?;
            if pending {
                return Ok(None);
            }
            let attempt: i64 = tx
                .query_row(
                    "SELECT count(*) + 1 FROM article_requests
                     WHERE session_id = ?1 AND operation = ?2",
                    params![session.id, kind.as_str()],
                    |row| row.get(0),
                )
                .map_err(read_failed)?;
            let context = context_of(&session, &turns);
            let turn_id = turns
                .iter()
                .rev()
                .find(|t| t.speaker == ArticleSpeaker::User && t.sendable)
                .map(|t| t.id.clone())
                .filter(|_| kind == ArticleRequestKind::Reply);
            let request_id = Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO article_requests
                   (id, session_id, operation, generation, branch_id, turn_id, attempt,
                    source_version_ids_json, config_json, context_json, state, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'pending', ?11)",
                params![
                    request_id,
                    session.id,
                    kind.as_str(),
                    session.generation as i64,
                    session.branch,
                    turn_id,
                    attempt,
                    serde_json::to_string(&context.source_version_ids).unwrap_or_default(),
                    config_json,
                    serde_json::to_string(&context).map_err(|_| invalid("Contexto inválido."))?,
                    now
                ],
            )
            .map_err(write_failed)?;
            tx.commit().map_err(write_failed)?;
            Ok(Some(ArticleTicket {
                request_id,
                kind,
                context,
                attempt: attempt as u32,
            }))
        })
    }

    /// Fecha um pedido: aplica o resultado só se a sessão ainda o aceita.
    ///
    /// Revogação, término, eliminação ou mudança de estado tornam o pedido
    /// `stale`; o consumo conhecido fica guardado em qualquer caso. Devolve o
    /// estado atual da sessão, ou `None` se a conversa já foi limpa ou eliminada.
    pub fn finish_article_request(
        &self,
        request_id: &str,
        outcome: ArticleOutcome,
        usage_json: Option<&str>,
    ) -> Result<Option<ArticleSnapshot>> {
        let now = utc_now();
        self.with_connection(|connection| {
            let tx = begin(connection)?;
            let row: Option<(String, String, i64, String, String)> = tx
                .query_row(
                    "SELECT session_id, operation, generation, branch_id, state
                     FROM article_requests WHERE id = ?1",
                    [request_id],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                        ))
                    },
                )
                .optional()
                .map_err(read_failed)?;
            let Some((session_id, operation, generation, branch, state)) = row else {
                return Ok(None);
            };
            let session = load_session(&tx, &session_id)?;
            let expected_state = if operation == "feedback" {
                ArticleState::Finishing
            } else {
                ArticleState::AwaitingTutor
            };
            let current = state == "pending"
                && !session.deleted
                && session.generation as i64 == generation
                && session.branch == branch
                && session.state == expected_state;
            let (request_state, error_json) = if !current {
                ("stale", None)
            } else {
                match &outcome {
                    ArticleOutcome::Reply { comment, question } => {
                        let text = match comment {
                            Some(comment) => format!("{comment}\n\n{question}"),
                            None => question.clone(),
                        };
                        apply(&tx, &session, ArticleMutation::AddTutorTurn { text }, &now)?;
                        ("completed", None)
                    }
                    ArticleOutcome::Report(report) => {
                        tx.execute(
                            "INSERT INTO article_reports
                               (id, session_id, branch_id, request_id, version_ids_json,
                                report_json, created_at)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                            params![
                                report.id,
                                session.id,
                                session.branch,
                                request_id,
                                serde_json::to_string(&report.version_ids).unwrap_or_default(),
                                serde_json::to_string(&**report)
                                    .map_err(|_| invalid("Relatório inválido."))?,
                                now
                            ],
                        )
                        .map_err(write_failed)?;
                        apply(&tx, &session, ArticleMutation::Complete, &now)?;
                        ("completed", None)
                    }
                    ArticleOutcome::Failed(error) => {
                        apply(
                            &tx,
                            &session,
                            ArticleMutation::Fail {
                                error: error.clone(),
                            },
                            &now,
                        )?;
                        ("failed", serde_json::to_string(error).ok())
                    }
                }
            };
            if state == "pending" {
                tx.execute(
                    "UPDATE article_requests
                     SET state = ?2, usage_json = ?3, error_json = ?4, finished_at = ?5
                     WHERE id = ?1",
                    params![request_id, request_state, usage_json, error_json, now],
                )
                .map_err(write_failed)?;
            }
            let result = if session.deleted {
                None
            } else {
                Some(build_snapshot(&tx, load_session(&tx, &session_id)?)?)
            };
            tx.commit().map_err(write_failed)?;
            Ok(result)
        })
    }
}

/// Consumo agregado dos pedidos já terminados; qualquer valor desconhecido
/// deixa o campo em `None` em vez de contar como zero.
fn usage_view(connection: &Connection, session_id: &str) -> Result<Option<ArticleUsageView>> {
    let rows = collect(
        connection,
        "SELECT usage_json FROM article_requests
         WHERE session_id = ?1 AND state IN ('completed', 'failed', 'stale', 'interrupted')",
        session_id,
        |row| row.get::<_, Option<String>>(0),
    )?;
    if rows.is_empty() {
        return Ok(None);
    }
    let (mut input, mut output, mut cost) = (Some(0u64), Some(0u64), Some(0f64));
    for raw in rows {
        let usage: Option<serde_json::Value> =
            raw.and_then(|json| serde_json::from_str(&json).ok());
        let field = |name: &str| usage.as_ref().and_then(|u| u[name].as_u64());
        input = input.zip(field("input")).map(|(a, b)| a + b);
        output = output.zip(field("output")).map(|(a, b)| a + b);
        let money = usage
            .as_ref()
            .and_then(|u| u["reported"].as_f64().or_else(|| u["estimated"].as_f64()));
        cost = cost.zip(money).map(|(a, b)| a + b);
    }
    Ok(Some(ArticleUsageView {
        input_tokens: input,
        output_tokens: output,
        estimated_cost: cost,
    }))
}

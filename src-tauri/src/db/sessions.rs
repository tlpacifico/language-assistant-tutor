//! Sessões guiadas: criação atómica, sorteio sem repetição e consulta.
//!
//! A criação corre numa única transação `BEGIN IMMEDIATE`: valida o pedido,
//! relê as perguntas elegíveis, sorteia com `ORDER BY random() LIMIT ?`, guarda
//! a sessão, as cópias completas de cada pergunta (texto, categoria,
//! competências e critérios STAR comuns mais os específicos) e o recibo do
//! pedido. Qualquer falha reverte tudo: nunca fica uma sessão sem perguntas nem
//! um recibo sem sessão. A contagem real é conferida antes do commit.
//!
//! Existe no máximo uma sessão em curso. O índice parcial
//! `one_active_session` é a autoridade entre ligações concorrentes e a sua
//! violação é traduzida em `ACTIVE_SESSION_EXISTS`, nunca numa mensagem genérica
//! de escrita nem numa segunda sessão.
//!
//! Repetir um pedido com o mesmo `requestId` e o mesmo conteúdo devolve a
//! sessão original, mesmo já concluída, sem escrever nada; o mesmo `requestId`
//! com outro conteúdo devolve `STALE_SOURCE`. O hash é SHA-256 sobre a
//! serialização canónica dos campos do pedido, sem o `requestId`.
//!
//! As leituras (`active_session`, `list_sessions`, `load_session`) devolvem
//! DTOs e nunca escrevem. As gravações e as confirmações são preenchidas pelas
//! tarefas seguintes; nesta fase vêm vazias e a consulta não as inventa.

use rusqlite::{params, Connection, ErrorCode, OptionalExtension, Row, TransactionBehavior};
use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::questions::{json_array, json_list, parse_category, utc_now, STAR_CRITERIA};
use super::Database;
use crate::domain::{error_code, AppError};
use crate::interview::{
    validate_count, Category, Id, QuestionDetail, Result, SessionDetail, SessionMode,
    SessionQuestion, SessionStatus, SessionSummary, StartSession, Version,
};

/// Colunas do resumo, com o número de respostas confirmadas da sessão.
///
/// `confirmed_count` conta as confirmações das perguntas da própria sessão; a
/// subconsulta corre por sessão e mantém a leitura independente das gravações.
const SUMMARY_SELECT: &str = "
SELECT s.id, s.category, s.question_count, s.position, s.status, s.mode,
       s.version, s.created_at, s.completed_at,
       (SELECT count(*)
          FROM confirmations c
          JOIN session_questions sq ON sq.id = c.session_question_id
         WHERE sq.session_id = s.id) AS confirmed_count
FROM sessions s";

impl Database {
    /// Cria uma sessão com uma seleção fixa de perguntas, numa transação.
    ///
    /// A quantidade tem de ficar entre uma e o total elegível: não há redução
    /// silenciosa nem perguntas repetidas. O mesmo `requestId` com o mesmo
    /// conteúdo devolve a sessão original; com outro conteúdo devolve
    /// `STALE_SOURCE`. Se já existir uma sessão em curso, nada é criado e o
    /// erro é `ACTIVE_SESSION_EXISTS`.
    pub fn start_session(&self, request: StartSession) -> Result<SessionDetail> {
        let request_id = request.request_id.trim().to_owned();
        if request_id.is_empty() {
            return Err(invalid_request());
        }
        let payload_hash = start_payload_hash(request.category, request.count)?;
        let now = utc_now();

        self.with_connection(|connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(write_failed)?;

            // O recibo é verificado antes da validação: uma repetição devolve o
            // resultado já guardado e nunca executa a mutação outra vez.
            if let Some(receipt) = find_receipt(&transaction, &request_id)? {
                return replay_start(&transaction, &receipt, &payload_hash);
            }

            let eligible = count_eligible(&transaction, request.category)?;
            validate_count(request.count, eligible)?;

            let drawn = draw_questions(&transaction, request.category, request.count)?;
            if drawn.len() != request.count as usize {
                return Err(insufficient_questions(eligible));
            }

            let session_id = Uuid::new_v4().to_string();
            insert_session(
                &transaction,
                &session_id,
                request.category,
                request.count,
                &now,
            )?;
            for (index, source) in drawn.iter().enumerate() {
                let position = u32::try_from(index + 1)
                    .map_err(|_| read_corrupt("posição da pergunta fora do intervalo"))?;
                insert_session_question(&transaction, &session_id, source, position)?;
            }

            verify_question_count(&transaction, &session_id, request.count)?;
            insert_receipt(&transaction, &request_id, &payload_hash, &session_id)?;

            let detail = load_detail(&transaction, &session_id)?
                .ok_or_else(|| read_corrupt("a sessão recém-criada não foi encontrada"))?;
            transaction.commit().map_err(write_failed)?;
            Ok(detail)
        })
    }

    /// Devolve a única sessão em curso, se existir, sem escrever.
    pub fn active_session(&self) -> Result<Option<SessionDetail>> {
        self.with_connection(|connection| {
            let session_id: Option<Id> = connection
                .query_row(
                    "SELECT id FROM sessions WHERE status = 'in_progress'",
                    [],
                    |row| row.get(0),
                )
                .optional()
                .map_err(read_failed)?;
            match session_id {
                Some(session_id) => load_detail(connection, &session_id),
                None => Ok(None),
            }
        })
    }

    /// Lista as sessões para o histórico, da mais recente para a mais antiga.
    ///
    /// Cada resumo traz a categoria, a quantidade, a posição, o estado e o
    /// número de respostas confirmadas. Não altera nada.
    pub fn list_sessions(&self) -> Result<Vec<SessionSummary>> {
        self.with_connection(|connection| {
            query_summaries(
                connection,
                &format!("{SUMMARY_SELECT} ORDER BY s.created_at DESC, s.rowid DESC"),
                &[],
            )
        })
    }

    /// Carrega uma sessão completa com as perguntas copiadas, pela ordem fixa.
    ///
    /// Uma sessão desconhecida devolve `STALE_SOURCE` sem escrever nada.
    pub fn load_session(&self, session_id: &str) -> Result<SessionDetail> {
        self.with_connection(|connection| {
            load_detail(connection, session_id)?.ok_or_else(session_not_found)
        })
    }
}

/// Recibo de um pedido já aplicado.
struct Receipt {
    kind: String,
    payload_hash: String,
    session_id: Option<Id>,
}

/// Pergunta do banco escolhida para a sessão, antes de ser copiada.
struct DrawnQuestion {
    id: Id,
    version: Version,
    text: String,
    category: Category,
    competencies: Vec<String>,
    criteria: Vec<String>,
}

/// Repete um início já guardado: mesma configuração devolve a sessão original,
/// configuração diferente é uma origem desatualizada.
///
/// A transação não escreve nada, pelo que termina sem commit.
fn replay_start(
    connection: &Connection,
    receipt: &Receipt,
    payload_hash: &str,
) -> Result<SessionDetail> {
    if receipt.kind != "start_session" || receipt.payload_hash != payload_hash {
        return Err(stale_request());
    }
    let session_id = receipt
        .session_id
        .as_deref()
        .ok_or_else(|| read_corrupt("o recibo do início não indica a sessão criada"))?;
    load_detail(connection, session_id)?
        .ok_or_else(|| read_corrupt("a sessão indicada pelo recibo do início não existe na base"))
}

fn find_receipt(connection: &Connection, request_id: &str) -> Result<Option<Receipt>> {
    connection
        .query_row(
            "SELECT kind, payload_hash, session_id
             FROM command_receipts
             WHERE request_id = ?1",
            [request_id],
            |row| {
                Ok(Receipt {
                    kind: row.get("kind")?,
                    payload_hash: row.get("payload_hash")?,
                    session_id: row.get("session_id")?,
                })
            },
        )
        .optional()
        .map_err(read_failed)
}

fn count_eligible(connection: &Connection, category: Option<Category>) -> Result<usize> {
    let count: i64 = connection
        .query_row(
            "SELECT count(*) FROM questions WHERE (?1 IS NULL OR category = ?1)",
            params![category.map(Category::as_str)],
            |row| row.get(0),
        )
        .map_err(read_failed)?;
    Ok(usize::try_from(count).unwrap_or(usize::MAX))
}

/// Sorteia as perguntas elegíveis com `ORDER BY random() LIMIT ?`.
///
/// O sorteio acontece dentro da transação de criação, depois de a quantidade
/// ter sido validada. A ordem devolvida é a que fica guardada nas posições.
fn draw_questions(
    connection: &Connection,
    category: Option<Category>,
    count: u32,
) -> Result<Vec<DrawnQuestion>> {
    let mut statement = connection
        .prepare(
            "SELECT id, version, text, category, competencies_json, criteria_json
             FROM questions
             WHERE (?1 IS NULL OR category = ?1)
             ORDER BY random()
             LIMIT ?2",
        )
        .map_err(read_failed)?;
    let mut rows = statement
        .query(params![category.map(Category::as_str), i64::from(count)])
        .map_err(read_failed)?;

    let mut drawn = Vec::new();
    while let Some(row) = rows.next().map_err(read_failed)? {
        drawn.push(drawn_question(row)?);
    }
    Ok(drawn)
}

fn drawn_question(row: &Row<'_>) -> Result<DrawnQuestion> {
    let category_name: String = row.get("category").map_err(read_failed)?;
    let category = parse_category(&category_name)
        .ok_or_else(|| read_corrupt(&format!("categoria desconhecida «{category_name}»")))?;
    let version: i64 = row.get("version").map_err(read_failed)?;
    let version = Version::try_from(version)
        .map_err(|_| read_corrupt(&format!("versão inválida «{version}»")))?;

    Ok(DrawnQuestion {
        id: row.get("id").map_err(read_failed)?,
        version,
        text: row.get("text").map_err(read_failed)?,
        category,
        competencies: json_list(
            &row.get::<_, String>("competencies_json")
                .map_err(read_failed)?,
            "competências",
        )?,
        criteria: json_list(
            &row.get::<_, String>("criteria_json").map_err(read_failed)?,
            "critérios",
        )?,
    })
}

fn insert_session(
    connection: &Connection,
    session_id: &str,
    category: Option<Category>,
    count: u32,
    now: &str,
) -> Result<()> {
    connection
        .execute(
            "INSERT INTO sessions
               (id, category, question_count, position, status, mode, version,
                created_at, completed_at)
             VALUES (?1, ?2, ?3, 1, 'in_progress', 'local_v2', 1, ?4, NULL)",
            params![
                session_id,
                category.map(Category::as_str),
                i64::from(count),
                now
            ],
        )
        .map_err(|error| {
            if is_active_session_conflict(&error) {
                active_session_exists()
            } else {
                write_failed(error)
            }
        })?;
    Ok(())
}

/// Copia a pergunta para a sessão: texto, categoria, competências e critérios
/// efetivos (STAR comuns seguidos dos específicos da pergunta).
fn insert_session_question(
    connection: &Connection,
    session_id: &str,
    source: &DrawnQuestion,
    position: u32,
) -> Result<()> {
    let mut criteria = Vec::with_capacity(STAR_CRITERIA.len() + source.criteria.len());
    criteria.extend(
        STAR_CRITERIA
            .iter()
            .map(|criterion| (*criterion).to_owned()),
    );
    criteria.extend(source.criteria.iter().cloned());

    let source_version =
        i64::try_from(source.version).map_err(|_| read_corrupt("versão de origem inválida"))?;

    connection
        .execute(
            "INSERT INTO session_questions
               (id, session_id, source_question_id, source_version, position, text,
                category, competencies_json, criteria_json, selected_recording_id,
                selection_version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, 0)",
            params![
                Uuid::new_v4().to_string(),
                session_id,
                source.id,
                source_version,
                i64::from(position),
                source.text,
                source.category.as_str(),
                json_array(&source.competencies)?,
                json_array(&criteria)?,
            ],
        )
        .map_err(write_failed)?;
    Ok(())
}

/// Confere a contagem real antes do commit: nem menos perguntas do que o
/// pedido, nem a mesma origem repetida.
fn verify_question_count(connection: &Connection, session_id: &str, expected: u32) -> Result<()> {
    let (stored, distinct): (i64, i64) = connection
        .query_row(
            "SELECT count(*), count(DISTINCT source_question_id)
             FROM session_questions
             WHERE session_id = ?1",
            [session_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(read_failed)?;

    let expected = i64::from(expected);
    if stored != expected || distinct != expected {
        return Err(AppError::new(
            error_code::STORAGE_WRITE_FAILED,
            format!(
                "A sessão ficou com {stored} de {expected} perguntas previstas; \
                 nenhuma alteração foi guardada. Repita a criação da entrevista."
            ),
            true,
        ));
    }
    Ok(())
}

fn insert_receipt(
    connection: &Connection,
    request_id: &str,
    payload_hash: &str,
    session_id: &str,
) -> Result<()> {
    connection
        .execute(
            "INSERT INTO command_receipts
               (request_id, kind, payload_hash, session_id, entity_id, result_version)
             VALUES (?1, 'start_session', ?2, ?3, ?3, 1)",
            params![request_id, payload_hash, session_id],
        )
        .map_err(write_failed)?;
    Ok(())
}

fn load_detail(connection: &Connection, session_id: &str) -> Result<Option<SessionDetail>> {
    let Some(summary) = find_summary(connection, session_id)? else {
        return Ok(None);
    };

    let mut statement = connection
        .prepare(
            "SELECT id, session_id, source_question_id, source_version, position, text,
                    category, competencies_json, criteria_json, selected_recording_id,
                    selection_version
             FROM session_questions
             WHERE session_id = ?1
             ORDER BY position",
        )
        .map_err(read_failed)?;
    let mut rows = statement.query([session_id]).map_err(read_failed)?;

    let mut questions = Vec::new();
    while let Some(row) = rows.next().map_err(read_failed)? {
        let question = session_question(row)?;
        // As gravações são preenchidas pela tarefa 4; a confirmação pertence à
        // tarefa 5 e a consulta não inventa resultados.
        let recordings = super::answers::load_recordings(connection, &question.id)?;
        questions.push(QuestionDetail {
            question,
            recordings,
            confirmation: None,
        });
    }

    Ok(Some(SessionDetail { summary, questions }))
}

fn find_summary(connection: &Connection, session_id: &str) -> Result<Option<SessionSummary>> {
    let parameters: [&dyn rusqlite::ToSql; 1] = [&session_id];
    let mut summaries = query_summaries(
        connection,
        &format!("{SUMMARY_SELECT} WHERE s.id = ?1"),
        &parameters,
    )?;
    Ok(summaries.pop())
}

fn query_summaries(
    connection: &Connection,
    sql: &str,
    parameters: &[&dyn rusqlite::ToSql],
) -> Result<Vec<SessionSummary>> {
    let mut statement = connection.prepare(sql).map_err(read_failed)?;
    let mut rows = statement.query(parameters).map_err(read_failed)?;
    let mut summaries = Vec::new();
    while let Some(row) = rows.next().map_err(read_failed)? {
        summaries.push(summary(row)?);
    }
    Ok(summaries)
}

fn summary(row: &Row<'_>) -> Result<SessionSummary> {
    let category: Option<String> = row.get("category").map_err(read_failed)?;
    let category = match category {
        Some(name) => Some(
            parse_category(&name)
                .ok_or_else(|| read_corrupt(&format!("categoria desconhecida «{name}»")))?,
        ),
        None => None,
    };

    let status: String = row.get("status").map_err(read_failed)?;
    let status = match status.as_str() {
        "in_progress" => SessionStatus::InProgress,
        "completed" => SessionStatus::Completed,
        other => {
            return Err(read_corrupt(&format!(
                "estado de sessão desconhecido «{other}»"
            )))
        }
    };

    let mode: String = row.get("mode").map_err(read_failed)?;
    let mode = match mode.as_str() {
        "local_v2" => SessionMode::LocalV2,
        other => {
            return Err(read_corrupt(&format!(
                "modo de sessão desconhecido «{other}»"
            )))
        }
    };

    Ok(SessionSummary {
        id: row.get("id").map_err(read_failed)?,
        category,
        count: count_from_row(row, "question_count")?,
        confirmed_count: count_from_row(row, "confirmed_count")?,
        position: count_from_row(row, "position")?,
        status,
        mode,
        version: version_from_row(row, "version")?,
        created_at: row.get("created_at").map_err(read_failed)?,
        completed_at: row.get("completed_at").map_err(read_failed)?,
    })
}

fn session_question(row: &Row<'_>) -> Result<SessionQuestion> {
    let category_name: String = row.get("category").map_err(read_failed)?;
    let category = parse_category(&category_name)
        .ok_or_else(|| read_corrupt(&format!("categoria desconhecida «{category_name}»")))?;

    Ok(SessionQuestion {
        id: row.get("id").map_err(read_failed)?,
        session_id: row.get("session_id").map_err(read_failed)?,
        source_question_id: row.get("source_question_id").map_err(read_failed)?,
        source_version: version_from_row(row, "source_version")?,
        position: count_from_row(row, "position")?,
        text: row.get("text").map_err(read_failed)?,
        category,
        competencies: json_list(
            &row.get::<_, String>("competencies_json")
                .map_err(read_failed)?,
            "competências",
        )?,
        criteria: json_list(
            &row.get::<_, String>("criteria_json").map_err(read_failed)?,
            "critérios",
        )?,
        selected_recording_id: row.get("selected_recording_id").map_err(read_failed)?,
        selection_version: version_from_row(row, "selection_version")?,
    })
}

fn count_from_row(row: &Row<'_>, column: &str) -> Result<u32> {
    let value: i64 = row.get(column).map_err(read_failed)?;
    u32::try_from(value).map_err(|_| read_corrupt(&format!("«{column}» inválido «{value}»")))
}

fn version_from_row(row: &Row<'_>, column: &str) -> Result<Version> {
    let value: i64 = row.get(column).map_err(read_failed)?;
    Version::try_from(value).map_err(|_| read_corrupt(&format!("«{column}» inválido «{value}»")))
}

/// Campos do pedido de início sem o `requestId`, pela ordem fixa do tipo.
///
/// A serialização é canónica (ordem de campos do tipo, categorias snake_case)
/// para o hash não depender de mapas nem da ordem de chaves.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StartPayload<'a> {
    category: Option<&'a str>,
    count: u32,
}

fn start_payload_hash(category: Option<Category>, count: u32) -> Result<String> {
    let payload = StartPayload {
        category: category.map(Category::as_str),
        count,
    };
    let canonical = serde_json::to_vec(&payload).map_err(|error| {
        AppError::new(
            error_code::INVALID_INPUT,
            format!("Não foi possível preparar o pedido de início da entrevista: {error}"),
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

/// A única restrição de unicidade que este `INSERT` pode violar é o índice
/// parcial da sessão em curso: o identificador é um UUID novo.
fn is_active_session_conflict(error: &rusqlite::Error) -> bool {
    match error {
        rusqlite::Error::SqliteFailure(failure, message) => {
            failure.code == ErrorCode::ConstraintViolation
                && message
                    .as_deref()
                    .is_none_or(|message| message.contains("one_active_session"))
        }
        _ => false,
    }
}

fn insufficient_questions(eligible: usize) -> AppError {
    AppError::new(
        error_code::INSUFFICIENT_QUESTIONS,
        format!(
            "Só existem {eligible} perguntas elegíveis. Escolha uma quantidade entre 1 e {eligible}."
        ),
        false,
    )
}

fn active_session_exists() -> AppError {
    AppError::new(
        error_code::ACTIVE_SESSION_EXISTS,
        "Já existe uma entrevista em curso. Retome-a ou termine-a antes de iniciar outra.",
        false,
    )
}

fn stale_request() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "Este pedido já foi usado com outra configuração. Abra uma nova preparação \
         para iniciar outra entrevista.",
        false,
    )
}

fn session_not_found() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "A sessão indicada não existe. Atualize o histórico antes de a abrir.",
        false,
    )
}

fn invalid_request() -> AppError {
    AppError::new(
        error_code::INVALID_INPUT,
        "O pedido de início não tem identificador. Repita o início a partir da preparação.",
        false,
    )
}

fn read_failed(error: rusqlite::Error) -> AppError {
    AppError::new(
        error_code::DATABASE_UNAVAILABLE,
        format!("Não foi possível ler as sessões guardadas: {error}"),
        false,
    )
}

fn write_failed(error: rusqlite::Error) -> AppError {
    AppError::new(
        error_code::STORAGE_WRITE_FAILED,
        format!("Não foi possível guardar a sessão; nenhuma alteração foi aplicada: {error}"),
        true,
    )
}

fn read_corrupt(detail: &str) -> AppError {
    AppError::new(
        error_code::DATABASE_UNAVAILABLE,
        format!("Não foi possível ler as sessões guardadas: {detail}."),
        false,
    )
}

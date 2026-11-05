//! DTOs serializados (camelCase, enums em snake_case), limites e validação.

use serde::{Deserialize, Serialize};

use crate::domain::{error_code, AppError};

pub type Result<T> = std::result::Result<T, AppError>;

/// Aviso de envio automático por turno. Muda quando o texto do aviso muda.
pub const ARTICLE_NOTICE_VERSION: &str = "article-auto-v1";
/// Texto do aviso: artigo, envio por turno, destino e áudio local explícitos.
pub const ARTICLE_NOTICE_TEXT: &str = "Ao autorizar, o texto do artigo e a transcrição de cada resposta são enviados automaticamente ao destino configurado quando termina a gravação. O áudio permanece neste computador.";

pub const MAX_ARTICLE_CHARS: usize = 12_000;
pub const MAX_TITLE_CHARS: usize = 200;
pub const MAX_ANSWER_CHARS: usize = 8_000;
pub const MAX_TURNS_PER_BRANCH: usize = 20;
pub const SUGGEST_FINISH_AFTER: usize = 5;
pub const MAX_CONTEXT_BYTES: usize = 48_000;
pub const NEXT_ANSWER_RESERVE_BYTES: usize = 16_000;
/// Comentário (500) + pergunta (300) do tutor, no pior caso UTF-8 (4 bytes).
pub const MAX_TUTOR_REPLY_BYTES: usize = (MAX_COMMENT_CHARS + MAX_QUESTION_CHARS) * 4;
/// Teto de uma resposta em bytes: resposta e réplica do tutor cabem sempre na
/// reserva, por isso o feedback final nunca excede `MAX_CONTEXT_BYTES`.
/// Teto do artigo em bytes: deixa espaço, na parte do contexto anterior à
/// reserva, para o título (200 caracteres) e a pergunta de abertura do tutor
/// (300 caracteres), no pior caso UTF-8.
pub const MAX_ARTICLE_BYTES: usize =
    MAX_CONTEXT_BYTES - NEXT_ANSWER_RESERVE_BYTES - (MAX_TITLE_CHARS + MAX_QUESTION_CHARS) * 4;
pub const MAX_ANSWER_BYTES: usize = NEXT_ANSWER_RESERVE_BYTES - MAX_TUTOR_REPLY_BYTES;
pub const MAX_REQUEST_BYTES: usize = 160_000;
pub const MAX_COMMENT_CHARS: usize = 500;
pub const MAX_QUESTION_CHARS: usize = 300;
pub const MAX_IMPROVEMENTS: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArticleCommand {
    pub request_id: String,
    pub session_id: String,
    pub expected_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArticleCreate {
    pub request_id: String,
    pub title: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArticleConsent {
    pub command: ArticleCommand,
    pub destination: String,
    pub notice_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArticleCorrection {
    pub command: ArticleCommand,
    pub turn_id: String,
    pub text: String,
}

/// Estados da sessão. A reprodução é um estado independente (`ArticleAudioView`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArticleState {
    Draft,
    AwaitingTutor,
    Ready,
    Recording,
    Transcribing,
    NeedsReview,
    RecoverableError,
    Finishing,
    Completed,
    Paused,
}

/// Origem de uma versão do texto de um turno do utilizador.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArticleVersionSource {
    AutomaticSent,
    ManualReview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArticleSpeaker {
    Tutor,
    User,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArticleAuthorization {
    pub destination: String,
    pub notice_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArticleError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArticleUsageView {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub estimated_cost: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArticleSessionView {
    pub id: String,
    pub title: Option<String>,
    pub article_text: String,
    pub version: u64,
    pub state: ArticleState,
    pub active_branch_id: String,
    pub authorization: Option<ArticleAuthorization>,
    pub answers: u32,
    pub suggest_finish: bool,
    pub limit_reached: bool,
    pub error: Option<ArticleError>,
    pub usage: Option<ArticleUsageView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArticleVersionView {
    pub id: String,
    pub source: Option<ArticleVersionSource>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArticleTurnView {
    pub id: String,
    pub branch_id: String,
    pub index: u32,
    pub speaker: ArticleSpeaker,
    /// Versão efetiva na projeção do ramo ativo.
    pub version: ArticleVersionView,
    pub audio_available: bool,
    pub from_previous_branch: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DimensionStatus {
    Clear,
    Partial,
    InsufficientEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArticleEvidence {
    pub turn_id: String,
    pub quote: String,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArticleDimension {
    pub status: DimensionStatus,
    pub summary: String,
    pub evidence: Vec<ArticleEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArticleReport {
    pub id: String,
    pub branch_id: String,
    pub version_ids: Vec<String>,
    pub coherence: ArticleDimension,
    pub cohesion: ArticleDimension,
    pub development: ArticleDimension,
    pub interaction: ArticleDimension,
    pub improvements: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArticlePlayback {
    Idle,
    Playing,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArticleAudioView {
    pub playback: ArticlePlayback,
    pub turn_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArticleSnapshot {
    pub session: ArticleSessionView,
    pub turns: Vec<ArticleTurnView>,
    pub report: Option<ArticleReport>,
    pub audio: ArticleAudioView,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextRole {
    Tutor,
    User,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextTurn {
    pub role: ContextRole,
    pub text: String,
}

/// Contexto textual exato de um pedido; fixa o ramo e as versões usadas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArticleContext {
    pub article: String,
    pub title: Option<String>,
    pub turns: Vec<ContextTurn>,
    pub branch_id: String,
    /// Versão efetiva de cada fala; paralelo a `turns`.
    pub source_version_ids: Vec<String>,
    /// Identificador do turno de cada fala; paralelo a `turns`.
    pub turn_ids: Vec<String>,
}

impl ArticleContext {
    /// Bytes UTF-8 do artigo, do título e das falas.
    pub fn byte_len(&self) -> usize {
        self.article.len()
            + self.title.as_deref().map_or(0, str::len)
            + self.turns.iter().map(|t| t.text.len()).sum::<usize>()
    }

    /// Há margem para uma resposta seguinte sem exceder o teto do contexto.
    pub fn has_room_for_next_answer(&self) -> bool {
        self.byte_len() <= MAX_CONTEXT_BYTES - NEXT_ANSWER_RESERVE_BYTES
    }
}

fn invalid(message: &str) -> AppError {
    AppError::new(error_code::INVALID_INPUT, message, false)
}

/// Valida artigo e título antes de criar a sessão; nunca corta o texto.
pub fn validate_article(input: &ArticleCreate) -> Result<()> {
    let text = input.text.trim();
    if text.is_empty() {
        return Err(invalid("Cole o texto do artigo."));
    }
    if text.chars().count() > MAX_ARTICLE_CHARS {
        return Err(invalid(
            "O artigo excede 12 000 caracteres. Reduza o texto; a aplicação não o corta.",
        ));
    }
    if text.len() > MAX_ARTICLE_BYTES {
        return Err(invalid(
            "O artigo é demasiado extenso para a conversa. Reduza o texto; a aplicação não o corta.",
        ));
    }
    if let Some(title) = &input.title {
        if title.trim().chars().count() > MAX_TITLE_CHARS {
            return Err(invalid("O título excede 200 caracteres."));
        }
    }
    Ok(())
}

/// Valida o texto de uma resposta antes do envio; o excesso exige revisão.
pub fn validate_answer_text(text: &str) -> Result<()> {
    let text = text.trim();
    if text.is_empty() {
        return Err(invalid("A resposta está vazia."));
    }
    if text.chars().count() > MAX_ANSWER_CHARS || text.len() > MAX_ANSWER_BYTES {
        return Err(invalid(
            "A resposta é demasiado longa (máximo 8 000 caracteres e 12 800 bytes). Reveja o texto antes de o enviar.",
        ));
    }
    Ok(())
}

//! Tipos de domínio novos da fase 4: práticas, reformulação, histórias e comparação.
//!
//! Convenções partilhadas com a fase 2: JSON em camelCase, enums em snake_case,
//! identificadores UUID em texto, datas UTC em RFC3339 e versões em `u64`. Os
//! tipos espelham a secção 2.1 do plano de implementação da fase 4. Nesta tarefa
//! só são consumidos os tipos necessários à migração e ao resolvedor de
//! propriedade; os restantes ficam prontos para as tarefas 2 a 7.

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::ai::{NormalizedUsage, SessionUsage};
use crate::interview::{AnswerSlot, Category, Confirmation, Id, RecordingView, Version};

/// Origem de uma resposta confirmada.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseOrigin {
    Original {
        #[serde(rename = "sessionId")]
        session_id: Id,
        #[serde(rename = "sessionQuestionId")]
        session_question_id: Id,
    },
    Practice {
        #[serde(rename = "practiceId")]
        practice_id: Id,
    },
}

/// Dono de uma única resposta, discriminado por entrevista ou prática.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CaptureOwner {
    Interview {
        #[serde(rename = "sessionId")]
        session_id: Id,
        #[serde(rename = "sessionQuestionId")]
        session_question_id: Id,
        #[serde(rename = "answerId")]
        answer_id: Id,
    },
    Practice {
        #[serde(rename = "practiceId")]
        practice_id: Id,
        #[serde(rename = "answerId")]
        answer_id: Id,
    },
}

/// Estado de uma prática individual.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PracticeStatus {
    #[default]
    Preparing,
    Answering,
    Confirmed,
    Abandoned,
}

impl PracticeStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            PracticeStatus::Preparing => "preparing",
            PracticeStatus::Answering => "answering",
            PracticeStatus::Confirmed => "confirmed",
            PracticeStatus::Abandoned => "abandoned",
        }
    }
}

/// Nível de reformulação pedido; é um objetivo de linguagem, não diagnóstico CEFR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RewriteLevel {
    B1,
    B2,
    C1,
}

/// Pedido para criar uma prática a partir de uma pergunta confirmada.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePractice {
    pub request_id: Id,
    pub session_id: Id,
    pub session_question_id: Id,
    pub previous_practice_id: Option<Id>,
}

/// Comando sobre uma prática existente, com a versão esperada.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PracticeCommand {
    pub request_id: Id,
    pub practice_id: Id,
    pub expected_version: Version,
}

/// Pergunta fixa de uma sessão, com a cópia imutável do conteúdo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionView {
    pub text: String,
    pub category: Category,
    pub competencies: Vec<String>,
    pub criteria: Vec<String>,
}

/// Conteúdo editável de uma história factual.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoryInput {
    pub title: String,
    pub situation: String,
    pub task: String,
    pub actions: String,
    pub outcome: String,
    pub competencies: Vec<String>,
}

/// Fotografia da história escolhida para apoio à captura.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorySnapshot {
    pub story_id: Id,
    pub story_version: Version,
    pub content: StoryInput,
}

/// Estado de bloqueio da preparação de uma resposta.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LockState {
    #[default]
    Editable,
    PendingStart,
    Locked,
    StartUncertain,
}

/// Tipo de apoio à preparação.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SupportKind {
    #[default]
    None,
    Story,
    HistoricalNone,
}

/// Preparação observável de um alvo de resposta.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparationView {
    pub answer_id: Id,
    pub version: Version,
    pub lock_state: LockState,
    pub selected_story_id: Option<Id>,
    pub selected_story_version: Option<Version>,
    pub snapshot: Option<StorySnapshot>,
    pub support: SupportKind,
    pub locked_capture_id: Option<Id>,
}

/// Detalhe de uma prática individual.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PracticeDetail {
    pub id: Id,
    pub source_session_id: Id,
    pub session_question_id: Id,
    pub ordinal: u32,
    pub previous_practice_id: Option<Id>,
    pub status: PracticeStatus,
    pub version: Version,
    pub created_at: String,
    pub confirmed_at: Option<String>,
    pub abandoned_at: Option<String>,
    pub question: QuestionView,
    pub answer: AnswerSlot,
    pub recordings: Vec<RecordingView>,
    pub confirmation: Option<Confirmation>,
    pub preparation: PreparationView,
}

/// Alvo de resposta de uma prática.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PracticeAnswerTarget {
    pub owner: CaptureOwner,
    pub recording_id: Id,
    pub selection_version: Version,
}

/// Fotografia do estado de prática observável pela interface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExerciseSnapshot {
    pub revision: u64,
    pub active_practice: Option<PracticeDetail>,
    pub active_owner: Option<CaptureOwner>,
    pub pending_preparation: Option<Id>,
}

impl ExerciseSnapshot {
    /// Estado neutro: nenhuma prática aberta.
    pub fn neutral() -> Self {
        Self {
            revision: 0,
            active_practice: None,
            active_owner: None,
            pending_preparation: None,
        }
    }
}

impl Default for ExerciseSnapshot {
    fn default() -> Self {
        Self::neutral()
    }
}

/// Pedido de reformulação de uma resposta confirmada.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RewriteCommand {
    pub request_id: Id,
    pub origin: ResponseOrigin,
    pub level: RewriteLevel,
}

/// Pedido de avaliação de uma prática confirmada.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluatePractice {
    pub request_id: Id,
    pub practice_id: Id,
    pub expected_version: Version,
}

/// Estado de um relatório ou reformulação pedidos à IA.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportState {
    Pending,
    Running,
    Failed,
    Ready,
}

/// Estado de uma tentativa de rede.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkAttemptState {
    Running,
    Completed,
    Failed,
    Interrupted,
    Discarded,
}

/// Versões de rubrica, prompt e esquema associadas a um resultado.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportVersions {
    pub rubric_version: String,
    pub prompt_version: String,
    pub schema_version: String,
}

/// Tentativa de rede observável, sem dados de histórias nem credenciais.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkAttemptView {
    pub network_attempt_id: Id,
    pub request_id: Id,
    pub purpose: String,
    pub origin: ResponseOrigin,
    pub state: NetworkAttemptState,
    pub generation: u64,
    pub usage: NormalizedUsage,
    pub created_at: String,
    pub finished_at: Option<String>,
}

/// Relatório de avaliação de uma prática.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PracticeReportView {
    pub practice_id: Id,
    pub state: ReportState,
    pub report: Option<Json>,
    pub rubric_version: Option<String>,
    pub prompt_version: Option<String>,
    pub schema_version: Option<String>,
    pub usage: SessionUsage,
    pub network_attempts: Vec<NetworkAttemptView>,
}

/// Pedido de comparação local entre duas origens confirmadas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComparisonRequest {
    pub left: ResponseOrigin,
    pub right: ResponseOrigin,
}

/// Campo STAR em falta numa história.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissingField {
    Situation,
    Task,
    Actions,
    Outcome,
}

/// História factual guardada.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoryView {
    pub id: Id,
    pub version: Version,
    pub title: String,
    pub situation: String,
    pub task: String,
    pub actions: String,
    pub outcome: String,
    pub competencies: Vec<String>,
    pub missing_fields: Vec<MissingField>,
    pub created_at: String,
    pub updated_at: String,
}

/// Preferência local de nível de reformulação.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RewritePreference {
    pub version: Version,
    pub level: RewriteLevel,
    pub updated_at: String,
}

/// Resposta confirmada de uma origem, com as gravações e a confirmação.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComparisonAnswer {
    pub role: String,
    pub recordings: Vec<RecordingView>,
    pub confirmation: Option<Confirmation>,
}

/// Estado do relatório numa comparação.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComparisonReportState {
    Pending,
    Running,
    Failed,
    Ready,
    NotAvailable,
}

/// Relatório com versões, apresentado numa comparação.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComparisonReport {
    pub content: Json,
    pub versions: ReportVersions,
}

/// Entrada de uma comparação: origem, datas, respostas e relatório.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComparisonEntry {
    pub origin: ResponseOrigin,
    pub created_at: String,
    pub confirmed_at: Option<String>,
    pub question: QuestionView,
    pub answers: Vec<ComparisonAnswer>,
    pub report: Option<ComparisonReport>,
    pub report_state: ComparisonReportState,
    /// Sugestões válidas da origem; a forma exata é fixada em §2.4 (tarefa 4).
    pub rewrites: Vec<Json>,
    pub preparation: PreparationView,
}

/// Compatibilidade de rubrica entre duas comparações.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RubricCompatibility {
    Same,
    Different,
    Unknown,
}

/// Comparação local entre duas origens.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComparisonView {
    pub left: ComparisonEntry,
    pub right: ComparisonEntry,
    pub rubric_compatibility: RubricCompatibility,
}

/// Consumo por finalidade.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PurposeUsage {
    pub purpose: String,
    pub usage: SessionUsage,
    pub network_attempt_count: u64,
}

/// Consumo agregado de um conjunto de tentativas.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageGroup {
    pub totals: SessionUsage,
    pub network_attempt_count: u64,
    pub by_purpose: Vec<PurposeUsage>,
}

/// Consumo de treino separado por entrevista original e prática posterior.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainingUsageView {
    pub session_id: Id,
    pub original_interview: UsageGroup,
    pub later_practice: UsageGroup,
    pub combined: UsageGroup,
}

/// Pergunta, competências e critérios fixos de uma origem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactParent {
    pub source_session_id: Id,
    pub answer_id: Id,
}

/// Resposta resolvida por [`crate::db::Database::resolve_capture_owner`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedAnswer {
    pub owner: CaptureOwner,
    pub slot: AnswerSlot,
    pub source_session_id: Id,
    pub session_question_id: Id,
    pub question: QuestionView,
    pub confirmation: Option<Confirmation>,
    pub owner_version: Version,
    pub writable: bool,
    pub artifact_parent: ArtifactParent,
}

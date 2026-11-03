//! Tipos serializados da fase 2 e validação pura.
//!
//! Convenções da secção 3 do plano: JSON em camelCase, enums em snake_case,
//! identificadores UUID em texto, datas UTC em RFC3339 e durações em
//! milissegundos. Os DTOs espelham `src/lib/interview-contracts.ts` e
//! `contracts/interview-v2.example.json` fixa a forma exata.

use serde::{Deserialize, Serialize};

use crate::domain::{
    error_code, AppError, Availability, CaptureMeta, JobStage, OperationKind, PlaybackTarget,
    Transcript,
};

/// Identificador opaco de uma entidade da fase 2 (UUID em texto).
pub type Id = String;

/// Versão de concorrência ou de conteúdo. Zero significa «ainda não guardado».
pub type Version = u64;

/// Resultado das operações do núcleo, com código de erro estável.
pub type Result<T> = std::result::Result<T, AppError>;

/// As seis categorias comportamentais da spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    ConflictCollaboration,
    LeadershipInitiative,
    FailureLearning,
    ProblemSolving,
    PrioritiesDeadlines,
    ChangeAdaptation,
}

impl Category {
    /// Todas as categorias por ordem de apresentação.
    pub const ALL: [Category; 6] = [
        Category::ConflictCollaboration,
        Category::LeadershipInitiative,
        Category::FailureLearning,
        Category::ProblemSolving,
        Category::PrioritiesDeadlines,
        Category::ChangeAdaptation,
    ];

    /// Nome estável guardado na base de dados e enviado no JSON.
    pub const fn as_str(self) -> &'static str {
        match self {
            Category::ConflictCollaboration => "conflict_collaboration",
            Category::LeadershipInitiative => "leadership_initiative",
            Category::FailureLearning => "failure_learning",
            Category::ProblemSolving => "problem_solving",
            Category::PrioritiesDeadlines => "priorities_deadlines",
            Category::ChangeAdaptation => "change_adaptation",
        }
    }

    /// Etiqueta em português de Portugal usada na interface e nas mensagens.
    pub const fn label_pt(self) -> &'static str {
        match self {
            Category::ConflictCollaboration => "Conflitos e colaboração",
            Category::LeadershipInitiative => "Liderança e iniciativa",
            Category::FailureLearning => "Falhas e aprendizagens",
            Category::ProblemSolving => "Resolução de problemas",
            Category::PrioritiesDeadlines => "Prioridades e prazos",
            Category::ChangeAdaptation => "Adaptação a mudanças",
        }
    }
}

/// Campos editáveis de uma pergunta do banco.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionInput {
    pub text: String,
    pub category: Category,
    pub competencies: Vec<String>,
    pub criteria: Vec<String>,
}

/// Pergunta guardada, com identidade e versão de conteúdo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Question {
    pub id: Id,
    pub text: String,
    pub category: Category,
    pub competencies: Vec<String>,
    pub criteria: Vec<String>,
    pub version: Version,
    pub created_at: String,
    pub updated_at: String,
}

/// Pedido de início de sessão. `category` nulo significa «todas as categorias».
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartSession {
    #[serde(default)]
    pub mode: SessionMode,
    pub request_id: Id,
    pub category: Option<Category>,
    pub count: u32,
}

/// Estado de uma sessão.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    InProgress,
    Completed,
}

impl SessionStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            SessionStatus::InProgress => "in_progress",
            SessionStatus::Completed => "completed",
        }
    }
}

/// Modo de realização da sessão. As sessões da fase 1 não pertencem a este modo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum SessionMode {
    #[default]
    LocalV2,
    AiV3,
}

impl SessionMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            SessionMode::LocalV2 => "local_v2",
            SessionMode::AiV3 => "ai_v3",
        }
    }
}

/// Resumo observável de uma sessão.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: Id,
    pub category: Option<Category>,
    pub count: u32,
    pub confirmed_count: u32,
    /// Posição corrente, baseada em 1.
    pub position: u32,
    pub status: SessionStatus,
    pub mode: SessionMode,
    pub version: Version,
    pub created_at: String,
    pub completed_at: Option<String>,
}

/// Pergunta de uma sessão, com a cópia imutável do conteúdo à data de início.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionQuestion {
    pub id: Id,
    pub session_id: Id,
    pub source_question_id: Id,
    pub source_version: Version,
    /// Posição na sessão, baseada em 1.
    pub position: u32,
    pub text: String,
    pub category: Category,
    pub competencies: Vec<String>,
    /// Critérios comuns e específicos efetivos, como foram copiados.
    pub criteria: Vec<String>,
    pub selected_recording_id: Option<Id>,
    pub selection_version: Version,
}

/// Alvo exato de uma resposta. Nunca aceita caminhos de ficheiro do frontend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerTarget {
    #[serde(default)]
    pub answer_id: Option<Id>,
    pub session_id: Id,
    pub session_question_id: Id,
    pub recording_id: Id,
    pub selection_version: Version,
}

/// Origem de uma revisão.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewOrigin {
    Automatic,
    ManualExplicit,
}

impl ReviewOrigin {
    /// Nome estável guardado na base de dados.
    pub const fn as_str(self) -> &'static str {
        match self {
            ReviewOrigin::Automatic => "automatic",
            ReviewOrigin::ManualExplicit => "manual_explicit",
        }
    }
}

/// Revisão guardada (ou em edição) de uma gravação.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewDraft {
    pub recording_id: Id,
    pub source_transcript_id: Option<Id>,
    pub origin: ReviewOrigin,
    pub text: String,
    /// Zero representa uma revisão ainda não guardada.
    pub version: Version,
    /// Nulo enquanto a revisão não foi guardada.
    pub saved_at: Option<String>,
}

/// Pedido para guardar uma revisão, sem confirmar nem avançar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveReview {
    pub request_id: Id,
    pub target: AnswerTarget,
    pub expected_review_version: Version,
    pub source_transcript_id: Option<Id>,
    pub origin: ReviewOrigin,
    pub text: String,
}

/// Pedido para confirmar a resposta: guarda o texto e confirma num só commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmAnswer {
    pub request_id: Id,
    pub target: AnswerTarget,
    pub expected_review_version: Version,
    pub source_transcript_id: Option<Id>,
    pub origin: ReviewOrigin,
    pub text: String,
    pub expected_session_version: Version,
}

/// Texto confirmado e as suas referências, todos imutáveis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Confirmation {
    pub answer_id: Id,
    pub id: Id,
    pub session_question_id: Id,
    pub recording_id: Id,
    pub review_version: Version,
    pub source_transcript_id: Option<Id>,
    pub origin: ReviewOrigin,
    pub text: String,
    pub confirmed_at: String,
}

/// Pedido explícito para avançar a sessão.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdvanceSession {
    pub request_id: Id,
    pub session_id: Id,
    pub session_question_id: Id,
    pub expected_session_version: Version,
}

/// Áudio que o núcleo sabe resolver para reprodução.
///
/// O chamador indica apenas identidades: os caminhos são resolvidos no núcleo e
/// nunca chegam da interface. A variante `Legacy` lê um treino da fase 1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AudioSelection {
    FollowUp {
        #[serde(rename = "sessionId")]
        session_id: Id,
        #[serde(rename = "sessionQuestionId")]
        session_question_id: Id,
        #[serde(rename = "answerId")]
        answer_id: Id,
    },
    Question {
        #[serde(rename = "sessionId")]
        session_id: Id,
        #[serde(rename = "sessionQuestionId")]
        session_question_id: Id,
    },
    Recording {
        #[serde(rename = "recordingId")]
        recording_id: Id,
    },
    /// Voz da sugestão de reformulação (texto fixo `spokenText`).
    Rewrite {
        #[serde(rename = "rewriteId")]
        rewrite_id: Id,
    },
    /// Voz da pergunta fixa de uma prática.
    PracticeQuestion {
        #[serde(rename = "practiceId")]
        practice_id: Id,
    },
    Legacy {
        #[serde(rename = "attemptId")]
        attempt_id: Id,
    },
}

/// Estado de uma gravação no ciclo de reserva, captura e publicação.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingState {
    Reserved,
    Capturing,
    PublishPending,
    Ready,
    Interrupted,
}

impl RecordingState {
    pub const fn as_str(self) -> &'static str {
        match self {
            RecordingState::Reserved => "reserved",
            RecordingState::Capturing => "capturing",
            RecordingState::PublishPending => "publish_pending",
            RecordingState::Ready => "ready",
            RecordingState::Interrupted => "interrupted",
        }
    }
}

/// Gravação exposta à interface, com os resultados que lhe pertencem.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingView {
    pub answer_id: Id,
    pub id: Id,
    pub session_question_id: Id,
    pub ordinal: u32,
    pub state: RecordingState,
    pub capture: Option<CaptureMeta>,
    pub original_file: Option<String>,
    pub normalized_file: Option<String>,
    pub transcripts: Vec<Transcript>,
    pub review: Option<ReviewDraft>,
    pub warnings: Vec<AppError>,
}

/// Pergunta de uma sessão com as gravações e a confirmação, se existir.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionDetail {
    pub follow_up: Option<Box<QuestionDetail>>,
    pub follow_up_stage: Option<String>,
    pub initial_answer_id: Id,
    pub question: SessionQuestion,
    pub recordings: Vec<RecordingView>,
    pub confirmation: Option<Confirmation>,
}

/// Sessão completa para consulta e retoma.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDetail {
    pub summary: SessionSummary,
    pub questions: Vec<QuestionDetail>,
}

/// Identidade do trabalho a que um resultado pertence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationTarget {
    #[serde(default)]
    pub answer_id: Option<Id>,
    pub session_id: Id,
    pub session_question_id: Id,
    pub recording_id: Option<Id>,
    pub selection_version: Version,
    pub operation_id: Id,
    /// Zero identifica a ausência de uma encarnação do auxiliar.
    pub worker_generation: u64,
}

/// Estado da base de dados, visível mesmo quando a abertura falha.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DatabaseState {
    Ready,
    Error,
    Opening,
}

impl DatabaseState {
    pub const fn as_str(self) -> &'static str {
        match self {
            DatabaseState::Ready => "ready",
            DatabaseState::Error => "error",
            DatabaseState::Opening => "opening",
        }
    }
}

/// Trabalho de modelo em curso, sempre associado ao alvo que o originou.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelJobView {
    pub id: Id,
    pub kind: OperationKind,
    pub stage: JobStage,
    pub target: OperationTarget,
}

/// Fotografia do estado da entrevista observável pela interface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterviewSnapshot {
    pub current_answer: Option<AnswerSlot>,
    pub ai_job: Option<crate::ai::AiJobView>,
    pub pending_ai_publication: Option<Id>,
    /// Revisão monotónica em memória; não substitui as versões persistidas.
    pub revision: u64,
    pub database: DatabaseState,
    pub active_session: Option<SessionSummary>,
    pub current: Option<QuestionDetail>,
    pub availability: Availability,
    pub recording_id: Option<Id>,
    /// Captura finalizada cuja publicação falhou: a reserva continua por guardar.
    pub pending_recording_id: Option<Id>,
    pub finalizing: bool,
    pub model_job: Option<ModelJobView>,
    pub playback: Option<PlaybackTarget>,
    pub warnings: Vec<AppError>,
    pub last_error: Option<AppError>,
    /// Estado de prática; neutro enquanto nenhuma prática está aberta.
    pub practice: crate::practice::ExerciseSnapshot,
}

/// Resultado da recuperação no arranque.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryReport {
    pub published: Vec<Id>,
    pub warnings: Vec<AppError>,
    pub interrupted_operations: Vec<Id>,
}

/// Valida e normaliza uma pergunta antes de a guardar.
///
/// Ignora os espaços exteriores e exige texto, pelo menos uma competência e um
/// critério não vazios; não altera o conteúdo interior.
pub fn validate_question(input: QuestionInput) -> Result<QuestionInput> {
    let text = input.text.trim().to_owned();
    if text.is_empty() {
        return Err(invalid_input("Escreva o texto da pergunta em inglês."));
    }

    let competencies = normalize_list(input.competencies, "Indique pelo menos uma competência.")?;
    let criteria = normalize_list(input.criteria, "Indique pelo menos um critério.")?;

    Ok(QuestionInput {
        text,
        category: input.category,
        competencies,
        criteria,
    })
}

/// Valida o texto de uma revisão ou confirmação.
///
/// Só recusa texto vazio ou apenas com espaços; `trim()` não é aplicado ao
/// texto guardado, porque a confirmação fixa exatamente o que o utilizador viu.
pub fn validate_review_text(text: &str) -> Result<()> {
    if text.trim().is_empty() {
        return Err(invalid_input(
            "O texto da resposta está vazio. Escreva a resposta antes de a guardar ou confirmar.",
        ));
    }
    Ok(())
}

/// Valida a quantidade pedida contra o total elegível.
///
/// A quantidade tem de ficar entre uma e o total elegível. Não reduz o total
/// nem repete perguntas: zero ou mais do que as elegíveis é recusado para o
/// utilizador ajustar a quantidade, e a mensagem indica as disponíveis.
pub fn validate_count(count: u32, eligible: usize) -> Result<()> {
    if eligible == 0 {
        return Err(AppError::new(
            error_code::INSUFFICIENT_QUESTIONS,
            "Não existem perguntas elegíveis para esta escolha. Adicione perguntas \
             ao banco antes de iniciar a entrevista.",
            false,
        ));
    }
    if count == 0 || count as usize > eligible {
        return Err(AppError::new(
            error_code::INSUFFICIENT_QUESTIONS,
            format!(
                "Só existem {eligible} perguntas elegíveis. Escolha uma quantidade entre 1 e {eligible}."
            ),
            false,
        ));
    }
    Ok(())
}

fn normalize_list(values: Vec<String>, empty_message: &str) -> Result<Vec<String>> {
    if values.is_empty() {
        return Err(invalid_input(empty_message));
    }
    values
        .into_iter()
        .map(|value| {
            let trimmed = value.trim().to_owned();
            if trimmed.is_empty() {
                Err(invalid_input(empty_message))
            } else {
                Ok(trimmed)
            }
        })
        .collect()
}

fn invalid_input(message: &str) -> AppError {
    AppError::new(error_code::INVALID_INPUT, message, false)
}

impl QuestionDetail {
    pub fn active(&self) -> &QuestionDetail {
        if self.confirmation.is_some() {
            self.follow_up.as_deref().unwrap_or(self)
        } else {
            self
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerSlot {
    pub id: Id,
    pub session_question_id: Id,
    pub role: String,
    pub text: String,
    pub selected_recording_id: Option<Id>,
    pub selection_version: Version,
    /// Prática a que pertence a tentativa (`attempt`); nulo para inicial/seguimento.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub practice_id: Option<Id>,
}

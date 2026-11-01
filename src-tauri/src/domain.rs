//! Contratos de domínio da fase 1.
//!
//! Este módulo não depende de hardware, de ficheiros ou do processo auxiliar.
//! Os tipos serializados espelham `src/lib/contracts.ts`: JSON em camelCase,
//! enums em snake_case, identificadores UUID em texto, datas UTC em RFC3339 e
//! durações inteiras em milissegundos.

use serde::{Deserialize, Serialize};

/// Pergunta fixa da fase 1.
pub const QUESTION: &str = "Tell me about a time you solved a difficult problem at work.";

/// Mensagem de um componente cuja verificação ainda não terminou.
pub const CHECKING_AVAILABILITY: &str = "A verificar disponibilidade";

/// Códigos de erro estáveis. As mensagens associadas são em português de Portugal.
pub mod error_code {
    pub const BUSY: &str = "BUSY";
    pub const DEVICE_UNAVAILABLE: &str = "DEVICE_UNAVAILABLE";
    pub const DEVICE_FORMAT_UNSUPPORTED: &str = "DEVICE_FORMAT_UNSUPPORTED";
    pub const CAPTURE_INTERRUPTED: &str = "CAPTURE_INTERRUPTED";
    pub const STORAGE_WRITE_FAILED: &str = "STORAGE_WRITE_FAILED";
    pub const ARTIFACT_MISSING: &str = "ARTIFACT_MISSING";
    pub const INVALID_AUDIO: &str = "INVALID_AUDIO";
    pub const MODEL_MISSING: &str = "MODEL_MISSING";
    pub const WORKER_UNAVAILABLE: &str = "WORKER_UNAVAILABLE";
    pub const WORKER_PROTOCOL: &str = "WORKER_PROTOCOL";
    pub const WORKER_TIMEOUT: &str = "WORKER_TIMEOUT";
    pub const STALE_SOURCE: &str = "STALE_SOURCE";
}

/// Erro com código estável, mensagem para o utilizador e indicação de repetição.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

impl AppError {
    pub fn new(code: &str, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code: code.to_owned(),
            message: message.into(),
            retryable,
        }
    }

    /// Operação recusada por ocupação: pode ser repetida depois de terminar.
    pub fn busy(message: &str) -> Self {
        Self::new(error_code::BUSY, message, true)
    }
}

/// Estado de um componente de disponibilidade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentStatus {
    Ready,
    Missing,
    Loading,
    Error,
}

/// Disponibilidade de um componente, com a razão legível quando não está pronto.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentState {
    pub status: ComponentStatus,
    pub message: Option<String>,
}

impl ComponentState {
    /// Estado antes de qualquer verificação real.
    pub fn loading() -> Self {
        Self {
            status: ComponentStatus::Loading,
            message: Some(CHECKING_AVAILABILITY.to_owned()),
        }
    }

    pub fn ready() -> Self {
        Self {
            status: ComponentStatus::Ready,
            message: None,
        }
    }
}

/// Disponibilidade dos quatro componentes observados pela interface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Availability {
    pub microphone: ComponentState,
    pub playback: ComponentState,
    pub transcription: ComponentState,
    pub synthesis: ComponentState,
}

impl Availability {
    /// Todos os componentes por verificar.
    pub fn loading() -> Self {
        Self {
            microphone: ComponentState::loading(),
            playback: ComponentState::loading(),
            transcription: ComponentState::loading(),
            synthesis: ComponentState::loading(),
        }
    }
}

/// Metadados da captura original, antes de qualquer conversão.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureMeta {
    pub sample_rate: u32,
    pub channels: u16,
    pub duration_ms: u64,
    pub bytes: u64,
    pub incomplete: bool,
}

/// Estado de uma transcrição. Ausência de fala é um resultado, não um erro.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptStatus {
    Completed,
    NoSpeech,
}

/// Segmento com os tempos e o texto exatamente como o motor os devolveu.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// Transcrição automática original, imutável depois de gravada.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transcript {
    pub id: String,
    pub status: TranscriptStatus,
    pub text: String,
    pub segments: Vec<Segment>,
    pub model: String,
    pub language: String,
    pub created_at: String,
    pub load_ms: u64,
    pub processing_ms: u64,
}

/// Revisão guardada de uma transcrição. Distinta da confirmação para avaliação.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    pub source_transcript_id: String,
    pub text: String,
    pub saved_at: String,
}

/// Tipo de operação delegada ao processo auxiliar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    Transcribe,
    Synthesize,
}

/// Estado de uma operação registada na resposta.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    Running,
    Completed,
    Failed,
    Interrupted,
    NoSpeech,
}

/// Última operação aplicada a uma resposta.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationRecord {
    pub id: String,
    pub kind: OperationKind,
    pub state: OperationState,
    pub error: Option<AppError>,
}

/// Resposta ativa. Os caminhos de ficheiro são referências relativas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attempt {
    pub id: String,
    pub question: String,
    pub created_at: String,
    pub capture: CaptureMeta,
    pub original_file: String,
    pub normalized_file: Option<String>,
    pub transcripts: Vec<Transcript>,
    pub review: Option<Review>,
    pub last_operation: Option<OperationRecord>,
}

/// Fase de um trabalho do modelo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStage {
    Loading,
    Processing,
}

/// Trabalho de modelo em curso, visível na interface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelJob {
    pub id: String,
    pub attempt_id: Option<String>,
    pub kind: OperationKind,
    pub stage: JobStage,
}

/// Alvo de reprodução; só resolve ficheiros já pertencentes à aplicação.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackTarget {
    Question,
    Response,
}

/// Fotografia do estado observável pela interface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub revision: u64,
    pub question: String,
    pub availability: Availability,
    pub attempt: Option<Attempt>,
    pub recording_id: Option<String>,
    pub model_job: Option<ModelJob>,
    pub playback: Option<PlaybackTarget>,
    pub last_error: Option<AppError>,
}

/// Resultado de recuperar a última resposta, com avisos por componentes em falta.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResult {
    pub attempt: Option<Attempt>,
    pub warnings: Vec<AppError>,
}

/// Intenção aplicada pela coordenação serial.
///
/// Espelha os comandos da secção 3 do plano; a tarefa 6 liga cada variante ao
/// serviço correspondente.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    RefreshAvailability,
    StartRecording,
    StopRecording,
    RetrySaveRecording,
    TranscribeResponse {
        attempt_id: String,
    },
    PlayAudio {
        target: PlaybackTarget,
    },
    StopAudio,
    SaveReview {
        attempt_id: String,
        source_id: String,
        text: String,
    },
    RestartWorker,
    PrepareClose,
}

/// Estado da entrevista. A geração do auxiliar é interna e não serializa.
#[derive(Debug, Clone, PartialEq)]
pub struct AppState {
    pub revision: u64,
    pub question: String,
    pub availability: Availability,
    pub attempt: Option<Attempt>,
    pub recording_id: Option<String>,
    pub model_job: Option<ModelJob>,
    pub playback: Option<PlaybackTarget>,
    pub last_error: Option<AppError>,
    worker_generation: u64,
    active_request: Option<String>,
}

impl AppState {
    /// Estado inicial: nada ocupado e nenhum componente verificado.
    pub fn initial() -> Self {
        Self {
            revision: 0,
            question: QUESTION.to_owned(),
            availability: Availability::loading(),
            attempt: None,
            recording_id: None,
            model_job: None,
            playback: None,
            last_error: None,
            worker_generation: 0,
            active_request: None,
        }
    }

    /// Verifica ocupação antes de gravar.
    ///
    /// A captura não depende do processo auxiliar nem dos modelos; só uma
    /// gravação, um trabalho de modelo ou a reprodução impedem começar.
    pub fn can_start_recording(&self) -> Result<(), AppError> {
        if self.recording_id.is_some() {
            return Err(AppError::busy("Já está uma gravação em curso."));
        }
        if self.model_job.is_some() {
            return Err(AppError::busy(
                "Há uma transcrição ou síntese em curso. Aguarde que termine.",
            ));
        }
        if self.playback.is_some() {
            return Err(AppError::busy("A reprodução de áudio está ativa."));
        }
        Ok(())
    }

    /// Um resultado só se aplica ao pedido pendente e à geração atual do auxiliar.
    pub fn accepts_result(&self, request_id: &str, generation: u64) -> bool {
        generation == self.worker_generation && self.active_request.as_deref() == Some(request_id)
    }

    /// Ocupa o estado com uma nova gravação, se estiver livre.
    pub fn begin_recording(&mut self, recording_id: impl Into<String>) -> Result<(), AppError> {
        self.can_start_recording()?;
        self.recording_id = Some(recording_id.into());
        Ok(())
    }

    /// Regista o trabalho de modelo pendente e o pedido que o identifica.
    pub fn begin_model_job(
        &mut self,
        job: ModelJob,
        request_id: impl Into<String>,
    ) -> Result<(), AppError> {
        self.can_start_recording()?;
        self.active_request = Some(request_id.into());
        self.model_job = Some(job);
        Ok(())
    }

    /// Nova geração do auxiliar: resultados anteriores deixam de ser aceites.
    pub fn set_worker_generation(&mut self, generation: u64) {
        self.worker_generation = generation;
    }

    /// Liberta o trabalho de modelo e o pedido pendente.
    ///
    /// Chamado quando a operação termina; um resultado tardio deixa de ser
    /// aceite para não aplicar duas vezes a mesma resposta.
    pub fn finish_model_job(&mut self) {
        self.model_job = None;
        self.active_request = None;
    }

    /// Fotografia do estado observável pela interface.
    pub fn snapshot(&self) -> AppSnapshot {
        AppSnapshot {
            revision: self.revision,
            question: self.question.clone(),
            availability: self.availability.clone(),
            attempt: self.attempt.clone(),
            recording_id: self.recording_id.clone(),
            model_job: self.model_job.clone(),
            playback: self.playback,
            last_error: self.last_error.clone(),
        }
    }
}

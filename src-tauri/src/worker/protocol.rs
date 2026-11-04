//! Protocolo v1 do auxiliar Python.
//!
//! Uma linha JSON por mensagem. Os pedidos usam `protocolVersion`, `id`, `op` e
//! `params`; as respostas têm um `event` (`result` ou `progress`) e exatamente
//! uma resposta final por pedido válido. Os nomes são camelCase e os valores
//! enumerados snake_case, como no resto dos contratos da fase 1.
//!
//! Este módulo só codifica e valida mensagens; a supervisão do processo está em
//! [`super`]. O formato coincide com `audio-worker/src/audio_worker/protocol.py`
//! e com `contracts/worker-v1.examples.json`.

use std::path::PathBuf;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::{error_code, AppError, ComponentState, JobStage, Segment, TranscriptStatus};

/// Versão do protocolo falada pela aplicação.
pub const PROTOCOL_VERSION: u32 = 1;

pub const HEALTH: &str = "health";
pub const TRANSCRIBE: &str = "transcribe";
pub const SYNTHESIZE: &str = "synthesize";
pub const PRELOAD: &str = "preload";
pub const SHUTDOWN: &str = "shutdown";

/// Comprimento máximo do identificador de um pedido.
pub const MAX_ID_LENGTH: usize = 128;

/// Operações aceites pelo protocolo v1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorkerOp {
    Health,
    Transcribe,
    Synthesize,
    Preload,
    Shutdown,
}

impl WorkerOp {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Health => HEALTH,
            Self::Transcribe => TRANSCRIBE,
            Self::Synthesize => SYNTHESIZE,
            Self::Preload => PRELOAD,
            Self::Shutdown => SHUTDOWN,
        }
    }
}

impl std::fmt::Display for WorkerOp {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Parâmetros de uma transcrição. Os valores fixos da fase 1 são CPU, int8 e inglês.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscribeParams {
    pub audio_path: PathBuf,
    pub model_path: PathBuf,
    pub device: String,
    pub compute_type: String,
    pub language: String,
}

impl TranscribeParams {
    pub fn cpu(audio_path: impl Into<PathBuf>, model_path: impl Into<PathBuf>) -> Self {
        Self {
            audio_path: audio_path.into(),
            model_path: model_path.into(),
            device: "cpu".to_owned(),
            compute_type: "int8".to_owned(),
            language: "en".to_owned(),
        }
    }
}

/// Parâmetros de uma síntese. A voz e a velocidade são as da fase 1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SynthesizeParams {
    pub text: String,
    pub output_path: PathBuf,
    pub voice: String,
    pub speed: f32,
}

impl SynthesizeParams {
    pub fn new(text: impl Into<String>, output_path: impl Into<PathBuf>) -> Self {
        Self {
            text: text.into(),
            output_path: output_path.into(),
            voice: "af_heart".to_owned(),
            speed: 1.0,
        }
    }
}

/// Disponibilidade devolvida por `health`, sem carregar modelos.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthResult {
    pub transcription: ComponentState,
    pub synthesis: ComponentState,
}

/// Resultado de uma transcrição, com o conteúdo exatamente como o motor o devolveu.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionResult {
    pub status: TranscriptStatus,
    pub text: String,
    pub segments: Vec<Segment>,
    pub model: String,
    pub language: String,
    pub load_ms: u64,
    pub processing_ms: u64,
}

/// Resultado de uma síntese, com o WAV PCM16 escrito pelo auxiliar.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SynthesisResult {
    pub audio_path: PathBuf,
    pub sample_rate: u32,
    pub duration_ms: u64,
    pub load_ms: u64,
    pub processing_ms: u64,
}

/// Confirmação de encerramento do auxiliar.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ShutdownResult {
    pub stopped: bool,
}

/// Resultado de `preload`: os estados de cada componente depois de carregar o
/// que estava disponível. Não falha por modelos em falta — devolve o motivo.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreloadResult {
    pub transcription: ComponentState,
    pub synthesis: ComponentState,
}

/// Resultado final já validado de uma operação.
#[derive(Debug, Clone, PartialEq)]
pub enum WorkerResult {
    Health(HealthResult),
    Transcription(TranscriptionResult),
    Synthesis(SynthesisResult),
    Preload(PreloadResult),
    Shutdown(ShutdownResult),
}

/// Mensagem recebida do auxiliar, antes de ser associada ao pedido pendente.
#[derive(Debug, Clone, PartialEq)]
pub enum WorkerResponse {
    Progress {
        id: String,
        stage: JobStage,
    },
    Result {
        id: String,
        ok: bool,
        result: Option<Value>,
        error: Option<AppError>,
    },
}

impl WorkerResponse {
    pub fn id(&self) -> &str {
        match self {
            Self::Progress { id, .. } | Self::Result { id, .. } => id,
        }
    }
}

/// Erro de protocolo estável, com mensagem em português de Portugal.
pub fn protocol_error(message: impl Into<String>) -> AppError {
    AppError::new(error_code::WORKER_PROTOCOL, message, false)
}

/// Valida o identificador antes de o enviar ao auxiliar.
pub fn validate_id(id: &str) -> Result<(), AppError> {
    if id.is_empty() {
        return Err(protocol_error(
            "O identificador do pedido não pode estar vazio.",
        ));
    }
    if id.len() > MAX_ID_LENGTH {
        return Err(protocol_error(format!(
            "O identificador do pedido excede {MAX_ID_LENGTH} caracteres."
        )));
    }
    if id.chars().any(char::is_control) {
        return Err(protocol_error(
            "O identificador do pedido contém caracteres de controlo.",
        ));
    }
    Ok(())
}

/// Converte parâmetros tipados no objeto JSON aceite pelo protocolo.
pub fn params_value<T: Serialize>(params: &T) -> Result<Value, AppError> {
    serde_json::to_value(params)
        .map_err(|_| protocol_error("Não foi possível codificar os parâmetros do pedido."))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RequestBody<'a> {
    protocol_version: u32,
    id: &'a str,
    op: &'a str,
    params: &'a Value,
}

/// Codifica um pedido numa linha JSON compacta, sem quebra de linha final.
pub fn encode_request(id: &str, op: WorkerOp, params: &Value) -> Result<String, AppError> {
    validate_id(id)?;
    if !params.is_object() {
        return Err(protocol_error(
            "Os parâmetros do pedido têm de ser um objeto JSON.",
        ));
    }
    serde_json::to_string(&RequestBody {
        protocol_version: PROTOCOL_VERSION,
        id,
        op: op.as_str(),
        params,
    })
    .map_err(|_| protocol_error("Não foi possível codificar o pedido para o auxiliar."))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawResponse {
    protocol_version: u32,
    id: String,
    event: String,
    #[serde(default)]
    ok: Option<bool>,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<AppError>,
    #[serde(default)]
    stage: Option<String>,
}

/// Valida uma linha de stdout e devolve a mensagem do protocolo v1.
pub fn decode_response(line: &str) -> Result<WorkerResponse, AppError> {
    let payload = line.trim();
    if payload.is_empty() {
        return Err(protocol_error(
            "O auxiliar escreveu uma linha vazia em vez de uma mensagem do protocolo v1.",
        ));
    }
    let raw: RawResponse = serde_json::from_str(payload).map_err(|_| {
        protocol_error("O auxiliar escreveu uma mensagem que não é JSON válido do protocolo v1.")
    })?;
    if raw.protocol_version != PROTOCOL_VERSION {
        return Err(protocol_error(format!(
            "O auxiliar respondeu com a versão de protocolo {}, mas a aplicação fala a versão {PROTOCOL_VERSION}.",
            raw.protocol_version
        )));
    }
    match raw.event.as_str() {
        "progress" => {
            let stage = match raw.stage.as_deref() {
                Some("loading") => JobStage::Loading,
                Some("processing") => JobStage::Processing,
                Some(other) => {
                    return Err(protocol_error(format!(
                        "O auxiliar indicou uma etapa de progresso desconhecida: «{other}»."
                    )))
                }
                None => {
                    return Err(protocol_error(
                        "O auxiliar enviou um progresso sem indicar a etapa.",
                    ))
                }
            };
            Ok(WorkerResponse::Progress { id: raw.id, stage })
        }
        "result" => {
            let ok = raw.ok.ok_or_else(|| {
                protocol_error("O auxiliar enviou um resultado sem indicar sucesso ou falha.")
            })?;
            if ok {
                let result = raw.result.ok_or_else(|| {
                    protocol_error("O auxiliar indicou sucesso sem devolver o resultado.")
                })?;
                Ok(WorkerResponse::Result {
                    id: raw.id,
                    ok: true,
                    result: Some(result),
                    error: None,
                })
            } else {
                let error = raw.error.ok_or_else(|| {
                    protocol_error("O auxiliar indicou uma falha sem a descrever.")
                })?;
                Ok(WorkerResponse::Result {
                    id: raw.id,
                    ok: false,
                    result: None,
                    error: Some(error),
                })
            }
        }
        other => Err(protocol_error(format!(
            "O auxiliar enviou um evento desconhecido: «{other}»."
        ))),
    }
}

/// Valida e tipa o resultado de sucesso de uma operação.
pub fn parse_result(op: WorkerOp, value: Value) -> Result<WorkerResult, AppError> {
    match op {
        WorkerOp::Health => typed(value).map(WorkerResult::Health),
        WorkerOp::Transcribe => typed(value).map(WorkerResult::Transcription),
        WorkerOp::Synthesize => typed(value).map(WorkerResult::Synthesis),
        WorkerOp::Preload => typed(value).map(WorkerResult::Preload),
        WorkerOp::Shutdown => typed(value).map(WorkerResult::Shutdown),
    }
}

fn typed<T: DeserializeOwned>(value: Value) -> Result<T, AppError> {
    serde_json::from_value(value)
        .map_err(|_| protocol_error("O resultado do auxiliar não respeita o protocolo v1."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ComponentStatus, TranscriptStatus};
    use serde_json::json;

    /// Exemplos partilhados e verificados nas três linguagens.
    fn examples() -> Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("contracts")
            .join("worker-v1.examples.json");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("ler «{}»: {error}", path.display()));
        serde_json::from_str(&text).expect("os exemplos partilhados são JSON válido")
    }

    fn encoded(value: &str) -> Value {
        serde_json::from_str(value).expect("o pedido codificado é JSON válido")
    }

    #[test]
    fn requests_match_the_shared_examples() {
        let examples = examples();

        let health = &examples["requests"]["health"];
        let line = encode_request(
            health["id"].as_str().expect("id do exemplo"),
            WorkerOp::Health,
            &json!({}),
        )
        .expect("codificar health");
        assert_eq!(encoded(&line), *health);

        let transcribe = &examples["requests"]["transcribe"];
        let params: TranscribeParams =
            serde_json::from_value(transcribe["params"].clone()).expect("parâmetros do exemplo");
        let params = params_value(&params).expect("codificar parâmetros");
        assert_eq!(params, transcribe["params"]);
        let line = encode_request(
            transcribe["id"].as_str().expect("id do exemplo"),
            WorkerOp::Transcribe,
            &params,
        )
        .expect("codificar transcribe");
        assert_eq!(encoded(&line), *transcribe);

        let synthesize = &examples["requests"]["synthesize"];
        let params: SynthesizeParams =
            serde_json::from_value(synthesize["params"].clone()).expect("parâmetros do exemplo");
        let params = params_value(&params).expect("codificar parâmetros");
        assert_eq!(params, synthesize["params"]);
        let line = encode_request(
            synthesize["id"].as_str().expect("id do exemplo"),
            WorkerOp::Synthesize,
            &params,
        )
        .expect("codificar synthesize");
        assert_eq!(encoded(&line), *synthesize);

        let shutdown = &examples["requests"]["shutdown"];
        let line = encode_request(
            shutdown["id"].as_str().expect("id do exemplo"),
            WorkerOp::Shutdown,
            &json!({}),
        )
        .expect("codificar shutdown");
        assert_eq!(encoded(&line), *shutdown);

        let preload = &examples["requests"]["preload"];
        let line = encode_request(
            preload["id"].as_str().expect("id do exemplo"),
            WorkerOp::Preload,
            &json!({}),
        )
        .expect("codificar preload");
        assert_eq!(encoded(&line), *preload);
    }

    #[test]
    fn decodes_the_preload_example() {
        let examples = examples();
        let response = decode_response(&examples["responses"]["preloadResult"].to_string())
            .expect("descodificar preload");
        let WorkerResponse::Result { ok, result, .. } = response else {
            panic!("preload devolve um resultado");
        };
        assert!(ok);
        let parsed = parse_result(WorkerOp::Preload, result.expect("resultado presente"))
            .expect("tipar preload");
        let WorkerResult::Preload(preload) = parsed else {
            panic!("preload produz PreloadResult");
        };
        assert_eq!(preload.transcription.status, ComponentStatus::Ready);
        assert_eq!(preload.synthesis.status, ComponentStatus::Ready);
    }

    #[test]
    fn progress_matches_the_shared_examples() {
        let examples = examples();
        let loading = &examples["responses"]["progressLoading"];
        assert_eq!(
            decode_response(&loading.to_string()).expect("descodificar progresso"),
            WorkerResponse::Progress {
                id: "req-transcribe-1".to_owned(),
                stage: JobStage::Loading,
            }
        );
        let processing = &examples["responses"]["progressProcessing"];
        assert_eq!(
            decode_response(&processing.to_string()).expect("descodificar progresso"),
            WorkerResponse::Progress {
                id: "req-transcribe-1".to_owned(),
                stage: JobStage::Processing,
            }
        );
    }

    #[test]
    fn decodes_the_health_example() {
        let examples = examples();
        let response = decode_response(&examples["responses"]["healthResult"].to_string())
            .expect("descodificar health");
        let WorkerResponse::Result { id, ok, result, .. } = response else {
            panic!("health devolve um resultado");
        };
        assert_eq!(id, "req-health-1");
        assert!(ok);
        let parsed = parse_result(WorkerOp::Health, result.expect("resultado presente"))
            .expect("tipar health");
        let WorkerResult::Health(health) = parsed else {
            panic!("health produz HealthResult");
        };
        assert_eq!(health.transcription.status, ComponentStatus::Ready);
        assert_eq!(health.transcription.message, None);
        assert_eq!(health.synthesis.status, ComponentStatus::Missing);
        assert!(health
            .synthesis
            .message
            .expect("mensagem da síntese")
            .contains("af_heart"));
    }

    #[test]
    fn decodes_the_transcription_examples() {
        let examples = examples();
        let response = decode_response(&examples["responses"]["transcribeResult"].to_string())
            .expect("descodificar transcrição");
        let WorkerResponse::Result { ok, result, .. } = response else {
            panic!("transcrição devolve um resultado");
        };
        assert!(ok);
        let parsed = parse_result(WorkerOp::Transcribe, result.expect("resultado presente"))
            .expect("tipar transcrição");
        let WorkerResult::Transcription(transcription) = parsed else {
            panic!("transcribe produz TranscriptionResult");
        };
        assert_eq!(transcription.status, TranscriptStatus::Completed);
        assert_eq!(transcription.model, "small");
        assert_eq!(transcription.language, "en");
        assert_eq!(transcription.segments.len(), 2);
        assert_eq!(transcription.load_ms, 4200);

        let response = decode_response(&examples["responses"]["transcribeNoSpeech"].to_string())
            .expect("descodificar ausência de fala");
        let WorkerResponse::Result { result, .. } = response else {
            panic!("transcrição devolve um resultado");
        };
        let parsed = parse_result(WorkerOp::Transcribe, result.expect("resultado presente"))
            .expect("tipar ausência de fala");
        let WorkerResult::Transcription(transcription) = parsed else {
            panic!("transcribe produz TranscriptionResult");
        };
        assert_eq!(transcription.status, TranscriptStatus::NoSpeech);
        assert!(transcription.segments.is_empty());
    }

    #[test]
    fn decodes_the_synthesis_and_shutdown_examples() {
        let examples = examples();
        let response = decode_response(&examples["responses"]["synthesizeResult"].to_string())
            .expect("descodificar síntese");
        let WorkerResponse::Result { result, .. } = response else {
            panic!("síntese devolve um resultado");
        };
        let parsed = parse_result(WorkerOp::Synthesize, result.expect("resultado presente"))
            .expect("tipar síntese");
        let WorkerResult::Synthesis(synthesis) = parsed else {
            panic!("synthesize produz SynthesisResult");
        };
        assert_eq!(synthesis.sample_rate, 24_000);
        assert_eq!(synthesis.duration_ms, 3120);

        let response = decode_response(&examples["responses"]["shutdownResult"].to_string())
            .expect("descodificar encerramento");
        let WorkerResponse::Result { result, .. } = response else {
            panic!("encerramento devolve um resultado");
        };
        let parsed = parse_result(WorkerOp::Shutdown, result.expect("resultado presente"))
            .expect("tipar encerramento");
        assert_eq!(
            parsed,
            WorkerResult::Shutdown(ShutdownResult { stopped: true })
        );
    }

    #[test]
    fn decodes_worker_errors() {
        let examples = examples();
        let model_missing = decode_response(&examples["responses"]["modelMissing"].to_string())
            .expect("descodificar modelo em falta");
        let WorkerResponse::Result { ok, error, .. } = model_missing else {
            panic!("o erro do auxiliar chega como resultado");
        };
        assert!(!ok);
        let error = error.expect("erro presente");
        assert_eq!(error.code, error_code::MODEL_MISSING);
        assert!(!error.retryable);

        let protocol_failure = decode_response(&examples["responses"]["protocolError"].to_string())
            .expect("descodificar erro de protocolo");
        let WorkerResponse::Result { ok, error, id, .. } = protocol_failure else {
            panic!("o erro do auxiliar chega como resultado");
        };
        assert!(!ok);
        assert_eq!(
            error.expect("erro presente").code,
            error_code::WORKER_PROTOCOL
        );
        assert!(
            id.is_empty(),
            "o auxiliar real responde sem id à linha inválida"
        );
    }

    #[test]
    fn rejects_invalid_lines() {
        for line in [
            "",
            "   ",
            "isto não é um pedido",
            "{\"protocolVersion\":2,\"id\":\"req-9\",\"event\":\"result\",\"ok\":true,\"result\":{}}",
            "{\"protocolVersion\":1,\"id\":\"req-9\",\"event\":\"result\",\"ok\":true}",
            "{\"protocolVersion\":1,\"id\":\"req-9\",\"event\":\"progress\",\"stage\":\"thinking\"}",
            "{\"protocolVersion\":1,\"id\":\"req-9\",\"event\":\"finished\"}",
        ] {
            let error = decode_response(line).expect_err("a linha tem de ser recusada");
            assert_eq!(error.code, error_code::WORKER_PROTOCOL, "linha: {line}");
        }
    }

    #[test]
    fn rejects_invalid_requests_before_sending() {
        assert_eq!(
            encode_request("", WorkerOp::Health, &json!({}))
                .expect_err("id vazio")
                .code,
            error_code::WORKER_PROTOCOL
        );
        assert_eq!(
            encode_request("req-1", WorkerOp::Health, &json!([]))
                .expect_err("parâmetros que não são objeto")
                .code,
            error_code::WORKER_PROTOCOL
        );
        assert_eq!(
            encode_request(&"x".repeat(MAX_ID_LENGTH + 1), WorkerOp::Health, &json!({}))
                .expect_err("id demasiado longo")
                .code,
            error_code::WORKER_PROTOCOL
        );
    }

    #[test]
    fn transcription_deadline_grows_with_the_audio() {
        let deadlines = super::super::Deadlines::default();
        assert_eq!(
            deadlines.transcription(std::time::Duration::from_secs(3)),
            std::time::Duration::from_secs(120)
        );
        assert_eq!(
            deadlines.transcription(std::time::Duration::from_secs(60)),
            std::time::Duration::from_secs(300)
        );
    }
}

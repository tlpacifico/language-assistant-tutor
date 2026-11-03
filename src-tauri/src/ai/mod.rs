mod credentials;
mod transport;
mod usage;
mod validation;
use crate::domain::AppError;
pub use credentials::*;
use serde::{Deserialize, Serialize};
pub use transport::*;
pub use usage::*;
pub use validation::*;
pub type Result<T, E = AppError> = std::result::Result<T, E>;
pub fn failure(message: &str) -> AppError {
    let code = if message.contains("Credencial")
        || message.contains("credencial")
        || message.contains("chave")
    {
        "AI_CREDENTIAL"
    } else if message.contains("saldo") || message.contains("Acesso") {
        "AI_ACCESS"
    } else if message.contains("Limite do fornecedor") {
        "AI_RATE_LIMIT"
    } else if message.contains("Tempo limite") {
        "AI_TIMEOUT"
    } else if message.contains("rede") || message.contains("Serviço indisponível") {
        "AI_NETWORK"
    } else if message.contains("guardar") || message.contains("ler os dados") {
        "AI_STORAGE"
    } else if message.contains("Autorize") || message.contains("aviso") {
        "AI_CONSENT"
    } else if message.contains("limite") || message.contains("excede") {
        "AI_LIMIT"
    } else {
        "AI_INVALID_RESULT"
    };
    AppError::new(code, message, true)
}
pub const SCHEMA: &str = "ai-v3-1";
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Tariff {
    pub model: String,
    pub currency: String,
    pub input_per_million: f64,
    pub output_per_million: f64,
    pub cached_input_per_million: Option<f64>,
    pub observed_at: String,
    pub source_url: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AiSettingsInput {
    pub base_url: String,
    pub model: String,
    pub tariff: Option<Tariff>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Consent {
    pub destination: String,
    pub notice_version: String,
    pub accepted_at: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AiSettingsView {
    pub version: u64,
    pub base_url: String,
    pub model: String,
    pub tariff: Option<Tariff>,
    pub credential_configured: bool,
    pub consent: Option<Consent>,
}
impl Default for AiSettingsView {
    fn default() -> Self {
        Self {
            version: 1,
            base_url: "https://opencode.ai/inference/openai/v1".into(),
            model: "deepseek-v4.1-flash".into(),
            tariff: Some(Tariff {
                model: "deepseek-v4.1-flash".into(),
                currency: "USD".into(),
                input_per_million: 0.30,
                output_per_million: 1.20,
                cached_input_per_million: Some(0.006),
                observed_at: "2026-11-02".into(),
                source_url: "https://opencode.ai/v2/docs/console/models/".into(),
            }),
            credential_configured: false,
            consent: None,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveAiSettings {
    pub request_id: String,
    pub expected_version: u64,
    pub input: AiSettingsInput,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptAiConsent {
    pub request_id: String,
    pub expected_settings_version: u64,
    pub destination: String,
    pub notice_version: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiQuestionCommand {
    pub request_id: String,
    pub session_id: String,
    pub session_question_id: String,
    pub expected_session_version: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WaiveFollowUp {
    #[serde(flatten)]
    pub command: AiQuestionCommand,
    pub reason: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSessionCommand {
    pub request_id: String,
    pub session_id: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiJobView {
    pub attempt_id: String,
    pub purpose: String,
    pub session_id: String,
    pub session_question_id: String,
    pub stage: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSource {
    pub confirmation_id: String,
    pub role: String,
    pub text: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSources {
    pub question: String,
    pub criteria: Vec<String>,
    pub answers: Vec<AiSource>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackView {
    pub session_id: String,
    pub state: String,
    pub reports: Vec<serde_json::Value>,
    pub priorities: Vec<serde_json::Value>,
    pub usage: SessionUsage,
    pub attempts: Vec<serde_json::Value>,
}
pub fn validate_settings(mut input: AiSettingsInput) -> Result<AiSettingsInput> {
    input.base_url = input.base_url.trim().trim_end_matches('/').into();
    input.model = input.model.trim().into();
    let url = reqwest::Url::parse(&input.base_url).map_err(|_| failure("URL base inválida."))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path().ends_with("/chat/completions")
        || input.model.is_empty()
        || input.model.ends_with("-free")
    {
        return Err(failure("Use uma base HTTPS sem credenciais, parâmetros ou /chat/completions, e um modelo pago."));
    }
    input.base_url = url.as_str().trim_end_matches('/').into();
    if let Some(t) = &input.tariff {
        if t.currency != "USD"
            || t.model != input.model
            || t.observed_at.trim().is_empty()
            || t.source_url.trim().is_empty()
            || [
                Some(t.input_per_million),
                Some(t.output_per_million),
                t.cached_input_per_million,
            ]
            .into_iter()
            .flatten()
            .any(|n| !n.is_finite() || n < 0.)
        {
            return Err(failure("Tarifário inválido para este modelo."));
        }
    }
    Ok(input)
}

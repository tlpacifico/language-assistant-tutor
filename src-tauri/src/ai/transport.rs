use super::*;
use serde_json::{json, Value};
use std::{future::Future, pin::Pin, time::Duration};
pub struct TransportRequest {
    pub settings: AiSettingsView,
    pub key: String,
    pub task: AiTask,
    pub sources: AiSources,
    /// Identidade persistida da conversa; não regenerar por tentativa HTTP.
    pub conversation_id: String,
}
#[derive(Clone, Debug)]
pub struct RemoteCompletion {
    pub model: String,
    pub content: String,
    pub usage: NormalizedUsage,
    pub invalid: bool,
}
pub trait AiTransport: Send + Sync {
    fn complete(
        &self,
        request: TransportRequest,
    ) -> Pin<Box<dyn Future<Output = Result<RemoteCompletion>> + Send>>;
}
pub struct HttpTransport;
pub fn build_body(settings: &AiSettingsView, task: &AiTask, sources: &AiSources) -> Result<Value> {
    let (messages, max_tokens) = match task {
        AiTask::FollowUp => (build_messages("follow_up", sources)?, 800),
        AiTask::OriginalFeedback => (build_messages("feedback", sources)?, 4000),
        AiTask::PracticeFeedback { .. } => (build_messages("practice_feedback", sources)?, 4000),
        AiTask::Rewrite { level, .. } => (build_rewrite_messages(sources, *level)?, 4000),
    };
    let body =
        json!({"model":settings.model,"messages":messages,"max_tokens":max_tokens,"stream":false});
    if serde_json::to_vec(&body)
        .map_err(|_| failure("Pedido inválido."))?
        .len()
        > 160000
    {
        return Err(failure(
            "O texto excede o limite de envio; foi preservado sem truncar.",
        ));
    }
    Ok(body)
}
impl AiTransport for HttpTransport {
    fn complete(
        &self,
        request: TransportRequest,
    ) -> Pin<Box<dyn Future<Output = Result<RemoteCompletion>> + Send>> {
        complete_with_timeout(request, Duration::from_secs(90))
    }
}
fn complete_with_timeout(
    request: TransportRequest,
    timeout: Duration,
) -> Pin<Box<dyn Future<Output = Result<RemoteCompletion>> + Send>> {
    Box::pin(async move {
        let body = build_body(&request.settings, &request.task, &request.sources)?;
        send_chat(
            &request.settings,
            &request.key,
            &request.conversation_id,
            &body,
            timeout,
        )
        .await
    })
}

/// Envio HTTP comum a todos os modos: autenticação, identidade da conversa,
/// limites, timeout e política de redirects/retries num único sítio.
pub(crate) async fn send_chat(
    settings: &AiSettingsView,
    key: &str,
    conversation_id: &str,
    body: &Value,
    timeout: Duration,
) -> Result<RemoteCompletion> {
    {
        let client = reqwest::Client::builder()
            .user_agent(concat!(
                "LanguageAssistantTutor/",
                env!("CARGO_PKG_VERSION")
            ))
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(timeout)
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| failure("HTTP indisponível."))?;
        let mut response = client
            .post(format!("{}/chat/completions", settings.base_url))
            .bearer_auth(key)
            .header(
                "x-opencode-session",
                format!("language-tutor-{conversation_id}"),
            )
            .json(body)
            .send()
            .await
            .map_err(|e| {
                failure(if e.is_timeout() {
                    "Tempo limite da API; repetir pode ter novo custo."
                } else {
                    "Falha de rede; o custo pode ser desconhecido."
                })
            })?;
        if !response.status().is_success() {
            match response.status().as_u16() {
                402 => return Err(AppError::new(
                    "AI_BILLING",
                    "Crédito indisponível no fornecedor (HTTP 402). Verifique o saldo e o estado da faturação.",
                    true,
                )),
                403 => return Err(AppError::new(
                    "AI_ACCESS",
                    "Acesso recusado pelo fornecedor (HTTP 403). Verifique as permissões da chave e do modelo.",
                    true,
                )),
                _ => {}
            }
            return Err(failure(match response.status().as_u16() {
                401 => "Credencial inválida (HTTP 401); o fornecedor recusou a chave. Substitua-a na Configuração IA.",
                429 => "Limite do fornecedor; tente mais tarde.",
                500..=599 => "Serviço indisponível; repetir pode ter novo custo.",
                _ => "Resposta HTTP recusada.",
            }));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| failure("Resposta interrompida; custo desconhecido."))?
        {
            if bytes.len() + chunk.len() > 1048576 {
                return Err(failure("Resposta HTTP excede 1 MiB."));
            }
            bytes.extend_from_slice(&chunk);
        }
        let envelope: Value =
            serde_json::from_slice(&bytes).map_err(|_| failure("Envelope JSON inválido."))?;
        let model = envelope["model"].as_str().unwrap_or("").to_owned();
        let mut usage_value = envelope["usage"].clone();
        if usage_value.is_object()
            && usage_value["opencode"].is_null()
            && envelope["opencode"].is_object()
        {
            usage_value["opencode"] = envelope["opencode"].clone();
        }
        let usage = normalize_usage(&usage_value, settings.tariff.as_ref(), &model);
        let content = envelope["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .into();
        Ok(RemoteCompletion {
            model,
            content,
            usage,
            invalid: envelope["choices"][0]["finish_reason"] != "stop",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{Arc, Mutex},
    };
    fn server(
        status: &str,
        body: String,
        extra: &str,
    ) -> (String, Arc<Mutex<String>>, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let captured = Arc::new(Mutex::new(String::new()));
        let data = captured.clone();
        let status = status.to_owned();
        let extra = extra.to_owned();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut part = [0u8; 4096];
            loop {
                let n = stream.read(&mut part).unwrap();
                if n == 0 {
                    break;
                }
                bytes.extend_from_slice(&part[..n]);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]);
                    let len = headers
                        .lines()
                        .find_map(|l| {
                            l.to_lowercase()
                                .strip_prefix("content-length: ")
                                .and_then(|s| s.parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + len {
                        break;
                    }
                }
            }
            *data.lock().unwrap() = String::from_utf8(bytes).unwrap();
            let response = format!("HTTP/1.1 {status}\r\n{extra}Connection: close\r\n\r\n{body}");
            let _ = stream.write_all(response.as_bytes());
        });
        (url, captured, handle)
    }
    fn request(url: String) -> TransportRequest {
        let settings = AiSettingsView {
            base_url: url,
            ..Default::default()
        };
        TransportRequest {
            settings,
            conversation_id: "test-session".into(),
            key: "TEST-SECRET".into(),
            task: AiTask::FollowUp,
            sources: AiSources {
                question: "Question".into(),
                criteria: vec!["STAR".into()],
                answers: vec![AiSource {
                    confirmation_id: "c".into(),
                    role: "initial".into(),
                    text: "Confirmed answer".into(),
                }],
            },
        }
    }
    #[tokio::test]
    async fn http_payload_contains_only_confirmed_text_and_usage_survives_invalid_finish() {
        let body=json!({"model":"deepseek-v4.1-flash","choices":[{"finish_reason":"length","message":{"content":"{}"}}],"usage":{"prompt_tokens":10,"completion_tokens":2}}).to_string();
        let (url, captured, thread) = server("200 OK", body, "");
        let result = HttpTransport.complete(request(url)).await.unwrap();
        thread.join().unwrap();
        assert!(result.invalid);
        assert_eq!(result.usage.input, Some(10));
        let raw = captured.lock().unwrap();
        assert!(raw
            .to_lowercase()
            .contains("authorization: bearer test-secret"));
        let body: Value = serde_json::from_str(raw.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        let keys = body.as_object().unwrap();
        assert_eq!(keys.len(), 4);
        assert!(body["messages"][1]["content"]
            .as_str()
            .unwrap()
            .contains("Confirmed answer"));
        assert!(!raw.contains("audioPath"));
    }
    #[tokio::test]
    async fn go_request_has_own_identity_stable_conversation_and_preserves_included_zero_cost() {
        for conversation in ["session-one", "session-one", "practice-two"] {
            let body = json!({"model":"deepseek-v4.1-flash","choices":[{"message":{"content":"{}"},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":2,"cost":0,"opencode":{"billing_source":"go","allowance":0.01}}}).to_string();
            let (url, captured, thread) = server("200 OK", body, "");
            let mut input = request(format!("{url}/inference/go/openai/v1"));
            input.conversation_id = conversation.into();
            let response = HttpTransport.complete(input).await.unwrap();
            thread.join().unwrap();
            let raw = captured.lock().unwrap().to_ascii_lowercase();
            assert!(raw.starts_with("post /inference/go/openai/v1/chat/completions "));
            assert!(raw.contains(&format!(
                "x-opencode-session: language-tutor-{conversation}\r\n"
            )));
            assert!(raw.contains(&format!(
                "user-agent: languageassistanttutor/{}\r\n",
                env!("CARGO_PKG_VERSION")
            )));
            assert_eq!(response.usage.reported, Some(0.0));
            assert_eq!(response.usage.billing_source.as_deref(), Some("go"));
        }
    }
    #[tokio::test]
    async fn billing_and_permission_failures_are_distinct_without_provider_content() {
        for (status, code, hint) in [
            ("402 Payment Required", "AI_BILLING", "faturação"),
            ("403 Forbidden", "AI_ACCESS", "permissões"),
        ] {
            let (url, _, thread) =
                server(status, "TEST-SECRET provider sensitive content".into(), "");
            let error = HttpTransport.complete(request(url)).await.unwrap_err();
            thread.join().unwrap();
            assert_eq!(error.code, code);
            assert!(error.message.contains(hint));
            assert!(error.message.contains(&status[..3]));
            assert!(!error.message.contains("TEST-SECRET"));
            assert!(!error.message.contains("provider sensitive"));
        }
    }
    #[tokio::test]
    async fn http_errors_are_safe_and_redirect_is_not_followed() {
        for status in [
            "401 Unauthorized",
            "403 Forbidden",
            "429 Too Many Requests",
            "500 Server Error",
            "302 Found",
        ] {
            let (url, _, thread) = server(
                status,
                "TEST-SECRET provider sensitive content".into(),
                "Location: http://127.0.0.1:1/secret\r\n",
            );
            let e = HttpTransport.complete(request(url)).await.unwrap_err();
            thread.join().unwrap();
            assert!(!e.message.contains("TEST-SECRET"));
            assert!(!e.message.contains("provider sensitive"));
        }
    }
    #[tokio::test]
    async fn http_rejects_oversize_without_content_length_and_malformed_json() {
        for body in ["x".repeat(1048577), "bad-json".into()] {
            let (url, _, thread) = server("200 OK", body, "");
            assert!(HttpTransport.complete(request(url)).await.is_err());
            thread.join().unwrap();
        }
    }
    #[tokio::test]
    async fn http_timeout_is_bounded() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        let t = std::thread::spawn(move || {
            let (_s, _) = l.accept().unwrap();
            std::thread::sleep(Duration::from_millis(100));
        });
        let e = complete_with_timeout(request(url), Duration::from_millis(20))
            .await
            .unwrap_err();
        assert!(e.message.contains("limite"));
        t.join().unwrap();
    }
}

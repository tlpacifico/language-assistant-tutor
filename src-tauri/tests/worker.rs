//! Testes do supervisor do auxiliar Python.
//!
//! Nenhum teste usa modelos, voz ou microfone: o processo é o
//! [`worker_stub.py`](fixtures/worker_stub.py), um auxiliar falso que fala o
//! protocolo v1 e falha de forma controlada. O intérprete vem de
//! `LANGUAGE_TUTOR_PYTHON` ou, em alternativa, do launcher `py -3.11`; se
//! nenhum existir o teste falha com uma mensagem explícita (nunca é ignorado).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::sync::mpsc::UnboundedReceiver;

use language_assistant_tutor::domain::{
    error_code, AppError, AppState, ComponentStatus, JobStage, ModelJob, OperationKind,
    TranscriptStatus,
};
use language_assistant_tutor::worker::protocol::{SynthesizeParams, TranscribeParams};
use language_assistant_tutor::worker::{
    Deadlines, WorkerClient, WorkerConfig, WorkerEvent, WorkerMessage, WorkerPort, WorkerRequest,
    WorkerResult, WORKER_MODULE,
};

// ---------------------------------------------------------------------------
// Utilitários
// ---------------------------------------------------------------------------

/// Caminho do stub copiado do repositório.
const STUB_SOURCE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/worker_stub.py");

/// Diretório temporário com o stub, o manifesto/cenário e o diretório de trabalho.
struct Stub {
    dir: TempDir,
}

impl Stub {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("criar diretório temporário do teste");
        assert!(
            Path::new(STUB_SOURCE).is_file(),
            "o stub do auxiliar não existe em {STUB_SOURCE}"
        );
        Self { dir }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn log_path(&self) -> PathBuf {
        self.path("worker.log")
    }

    /// Configuração de produção com prazos curtos e o cenário indicado.
    fn config_with(&self, mut scenario: Value, deadlines: Deadlines) -> WorkerConfig {
        let work = self.path("work");
        fs::create_dir_all(&work).expect("criar diretório de trabalho do auxiliar");
        fs::copy(STUB_SOURCE, work.join("audio_worker.py")).expect("copiar o stub como módulo");

        if let Value::Object(ref mut map) = scenario {
            map.entry("log")
                .or_insert_with(|| json!(self.log_path().display().to_string()));
        }
        let manifest = self.path("models.json");
        let body = json!({"schemaVersion": 1, "stub": scenario});
        fs::write(
            &manifest,
            serde_json::to_vec(&body).expect("codificar o cenário do stub"),
        )
        .expect("escrever o cenário do stub");

        WorkerConfig {
            python_executable: python_executable(),
            models_manifest: manifest,
            module: WORKER_MODULE.to_owned(),
            working_directory: work,
            deadlines,
        }
    }

    /// Operações registadas pelo stub, na ordem em que as recebeu.
    fn operations(&self) -> Vec<String> {
        match fs::read_to_string(self.log_path()) {
            Ok(text) => text
                .lines()
                .filter_map(|line| line.split_whitespace().next().map(str::to_owned))
                .collect(),
            Err(_) => Vec::new(),
        }
    }
}

/// Prazos curtos para a suite; os valores de produção estão em [`Deadlines`].
fn test_deadlines() -> Deadlines {
    Deadlines {
        handshake: Duration::from_secs(5),
        model_load: Duration::from_secs(5),
        synthesis: Duration::from_secs(2),
        shutdown: Duration::from_secs(2),
        ..Deadlines::default()
    }
}

/// Intérprete Python 3.11: `LANGUAGE_TUTOR_PYTHON` ou `py -3.11`.
fn python_executable() -> PathBuf {
    static PYTHON: OnceLock<PathBuf> = OnceLock::new();
    PYTHON
        .get_or_init(|| {
            if let Some(configured) = std::env::var_os("LANGUAGE_TUTOR_PYTHON") {
                let path = PathBuf::from(configured);
                assert!(
                    path.is_file(),
                    "LANGUAGE_TUTOR_PYTHON aponta para um ficheiro inexistente: {}",
                    path.display()
                );
                return path;
            }
            let output = Command::new("py")
                .args(["-3.11", "-c", "import sys; print(sys.executable)"])
                .output();
            match output {
                Ok(output) if output.status.success() => {
                    let text = String::from_utf8_lossy(&output.stdout);
                    let path = PathBuf::from(text.trim());
                    assert!(
                        path.is_file(),
                        "`py -3.11` devolveu um caminho inválido: {}",
                        path.display()
                    );
                    path
                }
                _ => panic!(
                    "Python 3.11 não encontrado: defina LANGUAGE_TUTOR_PYTHON ou instale o \
                     launcher `py` com Python 3.11. Os testes do supervisor não podem ser \
                     ignorados silenciosamente."
                ),
            }
        })
        .clone()
}

// ---------------------------------------------------------------------------
// Espera por eventos
// ---------------------------------------------------------------------------

async fn next_event(events: &mut UnboundedReceiver<WorkerEvent>, limit: Duration) -> WorkerEvent {
    match tokio::time::timeout(limit, events.recv()).await {
        Ok(Some(event)) => event,
        Ok(None) => panic!("o canal de eventos do auxiliar fechou inesperadamente"),
        Err(_) => panic!("nenhum evento do auxiliar em {limit:?}"),
    }
}

async fn wait_for(
    events: &mut UnboundedReceiver<WorkerEvent>,
    limit: Duration,
    description: &str,
    mut matches: impl FnMut(&WorkerEvent) -> bool,
) -> WorkerEvent {
    let started = Instant::now();
    while started.elapsed() < limit {
        let remaining = limit - started.elapsed();
        let event = next_event(events, remaining).await;
        if matches(&event) {
            return event;
        }
    }
    panic!("{description} não chegou dentro de {limit:?}");
}

async fn wait_health(events: &mut UnboundedReceiver<WorkerEvent>, limit: Duration) -> WorkerEvent {
    wait_for(events, limit, "o resultado do handshake", |event| {
        matches!(
            event.message,
            WorkerMessage::Completed {
                result: WorkerResult::Health(_),
                ..
            }
        )
    })
    .await
}

async fn wait_progress(
    events: &mut UnboundedReceiver<WorkerEvent>,
    limit: Duration,
    request_id: &str,
    stage: JobStage,
) -> WorkerEvent {
    let request_id = request_id.to_owned();
    wait_for(events, limit, "o progresso do pedido", move |event| {
        matches!(
            &event.message,
            WorkerMessage::Progress { request_id: seen, stage: seen_stage }
                if *seen == request_id && *seen_stage == stage
        )
    })
    .await
}

async fn wait_completed(
    events: &mut UnboundedReceiver<WorkerEvent>,
    limit: Duration,
    request_id: &str,
) -> (u64, WorkerResult) {
    let request_id = request_id.to_owned();
    let event = wait_for(events, limit, "o resultado do pedido", move |event| {
        matches!(
            &event.message,
            WorkerMessage::Completed { request_id: seen, .. } if *seen == request_id
        )
    })
    .await;
    match event.message {
        WorkerMessage::Completed { result, .. } => (event.generation, result),
        _ => unreachable!("o evento foi filtrado por wait_for"),
    }
}

async fn wait_failed(
    events: &mut UnboundedReceiver<WorkerEvent>,
    limit: Duration,
) -> (u64, String, AppError) {
    let event = wait_for(events, limit, "uma falha do auxiliar", |event| {
        matches!(event.message, WorkerMessage::Failed { .. })
    })
    .await;
    match event.message {
        WorkerMessage::Failed { request_id, error } => (event.generation, request_id, error),
        _ => unreachable!("o evento foi filtrado por wait_for"),
    }
}

/// Espera que o processo do auxiliar deixe de estar em execução.
async fn wait_until_stopped(client: &WorkerClient) {
    let started = Instant::now();
    while client.is_running() {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "o processo do auxiliar continuou em execução"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

// ---------------------------------------------------------------------------
// Testes
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn worker_crash_releases_job() {
    let stub = Stub::new();
    let config = stub.config_with(
        json!({"crashOnce": true, "stateFile": stub.path("state.json").display().to_string()}),
        test_deadlines(),
    );
    let client = WorkerClient::start(config).expect("o auxiliar deve arrancar");
    let mut events = client.take_events().expect("canal de eventos disponível");

    let health = wait_health(&mut events, Duration::from_secs(10)).await;
    assert_eq!(health.generation, client.generation());
    assert_eq!(
        stub.operations(),
        vec!["start".to_owned(), "health".to_owned()]
    );

    let request = WorkerRequest::transcribe(
        "req-crash",
        TranscribeParams::cpu(stub.path("response.wav"), stub.path("whisper")),
        Duration::from_secs(2),
    )
    .expect("codificar a transcrição");
    client.submit(request).expect("submeter a transcrição");
    assert!(client.is_busy(), "o pedido fica pendente");
    assert!(client.is_running(), "o processo ainda está vivo");

    let (generation, request_id, error) = wait_failed(&mut events, Duration::from_secs(20)).await;
    assert_eq!(request_id, "req-crash");
    assert_eq!(
        error.code,
        error_code::WORKER_UNAVAILABLE,
        "{}",
        error.message
    );
    assert!(
        !client.is_busy(),
        "o trabalho é libertado quando o processo morre"
    );
    assert!(
        !client.is_running(),
        "a instância que falhou fica encerrada"
    );

    client
        .restart()
        .expect("deve ser possível arrancar uma nova instância");
    let restarted_generation = client.generation();
    assert!(restarted_generation > generation);
    assert!(client.is_running());

    let next = WorkerRequest::transcribe(
        "req-after-crash",
        TranscribeParams::cpu(stub.path("response.wav"), stub.path("whisper")),
        Duration::from_secs(2),
    )
    .expect("codificar a nova transcrição");
    client
        .submit(next)
        .expect("a nova instância aceita trabalho");
    let (_, result) = wait_completed(&mut events, Duration::from_secs(20), "req-after-crash").await;
    match result {
        WorkerResult::Transcription(transcription) => {
            assert_eq!(transcription.status, TranscriptStatus::Completed);
            assert!(transcription.text.contains("release checklist"));
        }
        other => panic!("resultado inesperado: {other:?}"),
    }

    client.shutdown().expect("encerrar o auxiliar");
    assert!(!client.is_running(), "o encerramento termina o processo");
    assert!(
        stub.operations().iter().any(|op| op == "shutdown"),
        "o encerramento usa a operação do protocolo"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn timeout_rejects_old_generation() {
    let stub = Stub::new();
    let deadlines = Deadlines {
        synthesis: Duration::from_millis(400),
        ..test_deadlines()
    };
    let output = stub.path("synthesis.wav");
    let config = stub.config_with(json!({"resultDelay": 30.0}), deadlines);
    let client = WorkerClient::start(config).expect("o auxiliar deve arrancar");
    let mut events = client.take_events().expect("canal de eventos disponível");
    let health = wait_health(&mut events, Duration::from_secs(10)).await;
    let previous_generation = health.generation;
    assert_eq!(previous_generation, 1, "a primeira geração é 1, nunca 0");

    let old_id = "req-timeout".to_owned();
    let params = SynthesizeParams::new(
        "Tell me about a time you solved a difficult problem.",
        &output,
    );
    let request = WorkerRequest::synthesize(&old_id, params).expect("codificar a síntese");
    client.submit(request).expect("submeter a síntese");

    let job = ModelJob {
        id: old_id.clone(),
        attempt_id: Some("attempt-1".to_owned()),
        kind: OperationKind::Synthesize,
        stage: JobStage::Loading,
    };
    // Um resultado que alegue a geração 0 nunca é aceite (estado por sincronizar).
    let mut fresh = AppState::initial();
    fresh
        .begin_model_job(job.clone(), old_id.clone())
        .expect("registar o trabalho na instância inicial");
    assert!(!fresh.accepts_result(&old_id, 0));

    let mut state = AppState::initial();
    state.set_worker_generation(previous_generation);
    state
        .begin_model_job(job, old_id.clone())
        .expect("registar o trabalho");
    assert!(state.accepts_result(&old_id, previous_generation));

    wait_progress(
        &mut events,
        Duration::from_secs(5),
        &old_id,
        JobStage::Processing,
    )
    .await;
    assert!(
        output.is_file(),
        "o auxiliar criou o ficheiro provisório da síntese"
    );

    let (generation, request_id, timeout) = wait_failed(&mut events, Duration::from_secs(10)).await;
    assert_eq!(request_id, old_id);
    assert_eq!(generation, previous_generation);
    assert_eq!(timeout.code, "WORKER_TIMEOUT", "{}", timeout.message);
    assert!(
        !output.exists(),
        "o ficheiro provisório do pedido expirado é invalidado"
    );
    assert!(!client.is_running(), "o processo bloqueado é encerrado");
    assert!(!client.is_busy(), "o pedido deixou de estar pendente");

    client.restart().expect("reiniciar depois do timeout");
    let restarted_generation = client.generation();
    assert!(restarted_generation > previous_generation);
    state.set_worker_generation(restarted_generation);
    assert!(!state.accepts_result(&old_id, previous_generation));

    // Um identificador terminado nunca é reutilizado (nem depois de reiniciar).
    let reused = WorkerRequest::transcribe(
        old_id.clone(),
        TranscribeParams::cpu(stub.path("response.wav"), stub.path("whisper")),
        Duration::from_secs(1),
    )
    .expect("codificar o pedido repetido");
    let error = client
        .submit(reused)
        .expect_err("o identificador de um pedido terminado não pode ser reutilizado");
    assert_eq!(error.code, error_code::WORKER_PROTOCOL);

    client.shutdown().expect("encerrar o auxiliar");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stderr_does_not_block() {
    let stub = Stub::new();
    let config = stub.config_with(json!({"stderrLines": 20000}), test_deadlines());
    let started = Instant::now();
    let client =
        WorkerClient::start(config).expect("o auxiliar arranca mesmo com stderr abundante");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "o stderr do auxiliar é drenado continuamente"
    );
    let mut events = client.take_events().expect("canal de eventos disponível");
    wait_health(&mut events, Duration::from_secs(10)).await;

    let request = WorkerRequest::transcribe(
        "req-stderr",
        TranscribeParams::cpu(stub.path("response.wav"), stub.path("whisper")),
        Duration::from_secs(2),
    )
    .expect("codificar a transcrição");
    client
        .submit(request)
        .expect("submeter com stderr abundante");
    let (_, result) = wait_completed(&mut events, Duration::from_secs(20), "req-stderr").await;
    match result {
        WorkerResult::Transcription(transcription) => {
            assert_eq!(transcription.status, TranscriptStatus::Completed);
        }
        other => panic!("resultado inesperado: {other:?}"),
    }

    client.shutdown().expect("encerrar o auxiliar");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn non_json_stdout_is_protocol_error() {
    let stub = Stub::new();
    let config = stub.config_with(
        json!({"nonJsonOnce": true, "stateFile": stub.path("state.json").display().to_string()}),
        test_deadlines(),
    );
    let client = WorkerClient::start(config).expect("o auxiliar deve arrancar");
    let mut events = client.take_events().expect("canal de eventos disponível");
    let health = wait_health(&mut events, Duration::from_secs(10)).await;
    let previous_generation = health.generation;

    let request = WorkerRequest::transcribe(
        "req-garbage",
        TranscribeParams::cpu(stub.path("response.wav"), stub.path("whisper")),
        Duration::from_secs(2),
    )
    .expect("codificar a transcrição");
    client.submit(request).expect("submeter a transcrição");

    let (generation, request_id, error) = wait_failed(&mut events, Duration::from_secs(20)).await;
    assert_eq!(request_id, "req-garbage");
    assert_eq!(error.code, error_code::WORKER_PROTOCOL, "{}", error.message);
    assert_eq!(generation, previous_generation);
    wait_until_stopped(&client).await;

    client
        .restart()
        .expect("um novo arranque é possível depois de stdout inválido");
    assert!(client.generation() > previous_generation);
    assert!(client.is_running());

    let request = WorkerRequest::transcribe(
        "req-after-garbage",
        TranscribeParams::cpu(stub.path("response.wav"), stub.path("whisper")),
        Duration::from_secs(2),
    )
    .expect("codificar a transcrição");
    client
        .submit(request)
        .expect("a nova instância aceita trabalho");
    wait_completed(&mut events, Duration::from_secs(20), "req-after-garbage").await;

    client.shutdown().expect("encerrar o auxiliar");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn handshake_does_not_load_models() {
    let stub = Stub::new();
    let marker = stub.path("models.loaded");
    let config = stub.config_with(
        json!({
            "loadDelay": 30.0,
            "loadMarker": marker.display().to_string(),
        }),
        test_deadlines(),
    );
    let started = Instant::now();
    let client = WorkerClient::start(config).expect("o auxiliar deve arrancar");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "o handshake não espera pelo carregamento dos modelos"
    );
    assert!(!marker.exists(), "o handshake não carrega modelos");

    let mut events = client.take_events().expect("canal de eventos disponível");
    let health = wait_health(&mut events, Duration::from_secs(5)).await;
    match health.message {
        WorkerMessage::Completed {
            result: WorkerResult::Health(health),
            ..
        } => {
            assert_eq!(health.transcription.status, ComponentStatus::Ready);
            assert_eq!(health.synthesis.status, ComponentStatus::Missing);
        }
        other => panic!("resultado inesperado: {other:?}"),
    }
    assert_eq!(
        stub.operations(),
        vec!["start".to_owned(), "health".to_owned()],
        "o arranque só envia health; nenhuma operação carrega modelos"
    );
    assert!(!marker.exists(), "nenhum modelo foi carregado");

    client.shutdown().expect("encerrar o auxiliar");
    assert!(stub.operations().iter().any(|op| op == "shutdown"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn extra_result_is_protocol_violation() {
    let stub = Stub::new();
    let config = stub.config_with(json!({"extraResultOnce": true, "stateFile": stub.path("state.json").display().to_string()}), test_deadlines());
    let client = WorkerClient::start(config).expect("o auxiliar deve arrancar");
    let mut events = client.take_events().expect("canal de eventos disponível");
    let health = wait_health(&mut events, Duration::from_secs(10)).await;
    let previous_generation = health.generation;

    let request = WorkerRequest::transcribe(
        "req-extra",
        TranscribeParams::cpu(stub.path("response.wav"), stub.path("whisper")),
        Duration::from_secs(2),
    )
    .expect("codificar a transcrição");
    client.submit(request).expect("submeter a transcrição");
    wait_completed(&mut events, Duration::from_secs(20), "req-extra").await;

    // A segunda resposta final não pertence a nenhum pedido: a instância é
    // descartada para nunca aplicar o mesmo resultado duas vezes.
    wait_until_stopped(&client).await;
    client
        .restart()
        .expect("um novo arranque é possível depois de uma resposta extra");
    assert!(client.generation() > previous_generation);

    let request = WorkerRequest::transcribe(
        "req-after-extra",
        TranscribeParams::cpu(stub.path("response.wav"), stub.path("whisper")),
        Duration::from_secs(2),
    )
    .expect("codificar a transcrição");
    client
        .submit(request)
        .expect("a nova instância aceita trabalho");
    wait_completed(&mut events, Duration::from_secs(20), "req-after-extra").await;

    client.shutdown().expect("encerrar o auxiliar");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_kills_unresponsive_worker() {
    let stub = Stub::new();
    let config = stub.config_with(json!({"ignoreShutdown": true}), test_deadlines());
    let client = WorkerClient::start(config).expect("o auxiliar deve arrancar");
    let mut events = client.take_events().expect("canal de eventos disponível");
    wait_health(&mut events, Duration::from_secs(10)).await;

    let started = Instant::now();
    client
        .shutdown()
        .expect("o encerramento termina o processo mesmo sem saída espontânea");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "o encerramento espera no máximo o prazo configurado"
    );
    assert!(!client.is_running(), "o processo é terminado à força");
}

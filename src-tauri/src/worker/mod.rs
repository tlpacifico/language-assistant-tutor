//! Supervisão do auxiliar Python de transcrição e síntese.
//!
//! O auxiliar é um processo separado (`python -u -m audio_worker`) que fala o
//! protocolo v1 por linhas JSON em stdin/stdout. Este módulo garante que:
//!
//! - os argumentos são passados separadamente, sem shell, com stdin/stdout
//!   ligados por pipe e stderr drenado continuamente (um filho falador nunca
//!   bloqueia);
//! - cada etapa tem um prazo próprio e um progresso repetido não o renova;
//! - uma geração identifica uma encarnação do processo e começa em 1, pelo que
//!   um resultado antigo (incluindo após reinício) nunca é aceite;
//! - EOF, stdout inválido, uma resposta final a mais ou um timeout falham o
//!   pedido pendente, encerram o processo e permitem um novo arranque;
//! - no timeout, o processo é terminado e os ficheiros provisórios do pedido
//!   são invalidados; um identificador terminado nunca é reutilizado;
//! - o encerramento envia `shutdown`, espera até ao prazo e termina à força.

pub mod protocol;

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::mpsc::{self as subscriptions, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;

use crate::domain::{error_code, AppError, JobStage};
use protocol::{
    decode_response, encode_request, params_value, parse_result, validate_id, WorkerOp,
    WorkerResponse,
};

pub use protocol::{
    HealthResult, ShutdownResult, SynthesisResult, SynthesizeParams, TranscribeParams,
    TranscriptionResult, WorkerResult,
};

/// Nome fixo do módulo do auxiliar instalado no ambiente virtual.
pub const WORKER_MODULE: &str = "audio_worker";

/// Nome do ficheiro pessoal de configuração do auxiliar.
pub const CONFIG_FILE: &str = "audio.json";

/// Variável que substitui o caminho do ficheiro de configuração.
pub const CONFIG_ENV: &str = "LANGUAGE_TUTOR_CONFIG";

/// Espera máxima por uma resposta do supervisor aos comandos da aplicação.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

/// Espera pela saída espontânea do processo depois de responder ao encerramento.
const EXIT_GRACE: Duration = Duration::from_secs(2);

/// Bytes de stderr conservados para diagnosticar uma saída inesperada.
const STDERR_TAIL: usize = 4 * 1024;

/// Caracteres do fim do stderr usados na mensagem de erro.
const STDERR_SUMMARY: usize = 400;

/// `CREATE_NO_WINDOW`: o auxiliar não abre uma consola visível em Windows.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Prazos das etapas do auxiliar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deadlines {
    /// Handshake `health` depois de arrancar ou reiniciar.
    pub handshake: Duration,
    /// Carregamento de cada modelo (transcrição ou síntese).
    pub model_load: Duration,
    /// Base do prazo de processamento de uma transcrição.
    pub transcription_base: Duration,
    /// Multiplicador do prazo de transcrição pela duração do áudio.
    pub transcription_factor: u32,
    /// Processamento de uma síntese depois de carregar o modelo.
    pub synthesis: Duration,
    /// Espera pela resposta ao `shutdown` antes de terminar à força.
    pub shutdown: Duration,
}

impl Default for Deadlines {
    fn default() -> Self {
        Self {
            handshake: Duration::from_secs(10),
            model_load: Duration::from_secs(180),
            transcription_base: Duration::from_secs(120),
            transcription_factor: 5,
            synthesis: Duration::from_secs(120),
            shutdown: Duration::from_secs(3),
        }
    }
}

impl Deadlines {
    /// Prazo de processamento de uma transcrição: `max(120 s, 5 × duração)`.
    pub fn transcription(&self, audio: Duration) -> Duration {
        self.transcription_base
            .max(audio.saturating_mul(self.transcription_factor))
    }
}

/// Configuração do auxiliar, obtida do ficheiro pessoal `audio.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerConfig {
    /// Caminho absoluto do intérprete Python da `.venv`.
    pub python_executable: PathBuf,
    /// Caminho absoluto do manifesto de modelos preparado offline.
    pub models_manifest: PathBuf,
    /// Módulo fixo executado com `-m`.
    pub module: String,
    /// Diretório de trabalho do processo (a pasta de dados da aplicação).
    pub working_directory: PathBuf,
    /// Prazos por etapa.
    pub deadlines: Deadlines,
}

impl WorkerConfig {
    /// Lê `audio.json` de `config_dir`, ou do caminho indicado por
    /// `LANGUAGE_TUTOR_CONFIG`, e usa `data_dir` como diretório de trabalho.
    pub fn load(config_dir: &Path, data_dir: &Path) -> Result<Self, AppError> {
        let path = match std::env::var_os(CONFIG_ENV) {
            Some(configured) => PathBuf::from(configured),
            None => config_dir.join(CONFIG_FILE),
        };
        Self::from_file(&path, data_dir)
    }

    /// Lê e valida um ficheiro de configuração concreto.
    pub fn from_file(path: &Path, data_dir: &Path) -> Result<Self, AppError> {
        let text = fs::read_to_string(path).map_err(|error| {
            configuration(format!(
                "Não foi possível ler a configuração do auxiliar em «{}»: {error}. \
                 Crie o ficheiro com schemaVersion, pythonExecutable e modelsManifest.",
                path.display()
            ))
        })?;
        let file: ConfigFile = serde_json::from_str(&text).map_err(|_| {
            configuration(format!(
                "A configuração do auxiliar em «{}» não é JSON válido.",
                path.display()
            ))
        })?;
        if file.schema_version != Some(1) {
            return Err(configuration(format!(
                "A configuração do auxiliar em «{}» tem de indicar schemaVersion 1.",
                path.display()
            )));
        }
        let python_executable =
            existing_absolute("pythonExecutable", file.python_executable.as_deref())?;
        let models_manifest = existing_absolute("modelsManifest", file.models_manifest.as_deref())?;
        if !data_dir.is_absolute() {
            return Err(configuration(format!(
                "O diretório de dados do auxiliar tem de ser absoluto: «{}».",
                data_dir.display()
            )));
        }
        Ok(Self {
            python_executable,
            models_manifest,
            module: WORKER_MODULE.to_owned(),
            working_directory: data_dir.to_path_buf(),
            deadlines: Deadlines::default(),
        })
    }

    /// Confirma os caminhos antes de arrancar o processo.
    pub fn validate(&self) -> Result<(), AppError> {
        if !self.python_executable.is_absolute() {
            return Err(configuration(format!(
                "O executável Python do auxiliar tem de ser um caminho absoluto: «{}».",
                self.python_executable.display()
            )));
        }
        if !self.python_executable.is_file() {
            return Err(configuration(format!(
                "O executável Python do auxiliar não existe: «{}».",
                self.python_executable.display()
            )));
        }
        if !self.models_manifest.is_absolute() {
            return Err(configuration(format!(
                "O manifesto de modelos tem de ser um caminho absoluto: «{}».",
                self.models_manifest.display()
            )));
        }
        if !self.models_manifest.is_file() {
            return Err(configuration(format!(
                "O manifesto de modelos não existe: «{}».",
                self.models_manifest.display()
            )));
        }
        if self.module.trim().is_empty() {
            return Err(configuration("O módulo do auxiliar não está definido."));
        }
        if !self.working_directory.is_absolute() {
            return Err(configuration(format!(
                "O diretório de trabalho do auxiliar tem de ser absoluto: «{}».",
                self.working_directory.display()
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfigFile {
    schema_version: Option<u32>,
    python_executable: Option<String>,
    models_manifest: Option<String>,
}

/// Confirma que a chave indica um caminho absoluto para um ficheiro existente.
fn existing_absolute(key: &str, value: Option<&str>) -> Result<PathBuf, AppError> {
    let Some(value) = value.filter(|value| !value.trim().is_empty()) else {
        return Err(configuration(format!(
            "A configuração do auxiliar não indica «{key}»."
        )));
    };
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err(configuration(format!(
            "«{key}» tem de ser um caminho absoluto: «{value}»."
        )));
    }
    if !path.is_file() {
        return Err(configuration(format!(
            "«{key}» aponta para um ficheiro inexistente: «{value}»."
        )));
    }
    Ok(path)
}

/// Pedido endereçado ao auxiliar.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkerRequest {
    pub id: String,
    pub op: WorkerOp,
    pub params: Value,
    /// Duração do áudio; define o prazo de processamento da transcrição.
    pub audio_duration: Option<Duration>,
    /// Ficheiro criado pelo pedido (saída da síntese), removido se falhar.
    pub artifact: Option<PathBuf>,
}

impl WorkerRequest {
    pub fn new(id: impl Into<String>, op: WorkerOp, params: Value) -> Self {
        Self {
            id: id.into(),
            op,
            params,
            audio_duration: None,
            artifact: None,
        }
    }

    /// Pedido `health`, usado no handshake.
    pub fn health(id: impl Into<String>) -> Self {
        Self::new(id, WorkerOp::Health, json!({}))
    }

    pub fn transcribe(
        id: impl Into<String>,
        params: TranscribeParams,
        audio_duration: Duration,
    ) -> Result<Self, AppError> {
        let mut request = Self::new(id, WorkerOp::Transcribe, params_value(&params)?);
        request.audio_duration = Some(audio_duration);
        Ok(request)
    }

    pub fn synthesize(id: impl Into<String>, params: SynthesizeParams) -> Result<Self, AppError> {
        let mut request = Self::new(id, WorkerOp::Synthesize, params_value(&params)?);
        request.artifact = Some(params.output_path.clone());
        Ok(request)
    }
}

/// Mensagem entregue ao serviço sobre o auxiliar.
#[derive(Debug, Clone, PartialEq)]
pub enum WorkerMessage {
    /// O auxiliar entrou numa etapa do pedido.
    Progress { request_id: String, stage: JobStage },
    /// Resultado final aplicável ao pedido.
    Completed {
        request_id: String,
        result: WorkerResult,
    },
    /// O pedido falhou; `error` explica porquê.
    Failed { request_id: String, error: AppError },
}

/// Evento do auxiliar com a geração que o produziu.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkerEvent {
    pub generation: u64,
    pub message: WorkerMessage,
}

/// Operações usadas pelo serviço, substituíveis nos testes.
pub trait WorkerPort: Send + Sync {
    fn submit(&self, request: WorkerRequest) -> Result<(), AppError>;
    fn restart(&self) -> Result<(), AppError>;
}

/// Comandos enviados ao supervisor.
enum Control {
    Submit {
        request: WorkerRequest,
        reply: SyncSender<Result<(), AppError>>,
    },
    Restart {
        reply: SyncSender<Result<u64, AppError>>,
    },
    Shutdown {
        reply: SyncSender<Result<(), AppError>>,
    },
    Detach,
}

/// Entrada do supervisor: comandos da aplicação e saída do processo.
enum Input {
    Command(Control),
    Line { epoch: u64, line: String },
    Closed { epoch: u64, detail: Option<String> },
}

/// Cliente do auxiliar: arranca o processo e entrega eventos por canal.
pub struct WorkerClient {
    inputs: UnboundedSender<Input>,
    events: Mutex<Option<UnboundedReceiver<WorkerEvent>>>,
    generation: Arc<AtomicU64>,
    running: Arc<AtomicBool>,
    busy: Arc<AtomicBool>,
    deadlines: Deadlines,
}

impl WorkerClient {
    /// Arranca o processo e conclui o handshake `health`.
    ///
    /// Tem de ser chamado dentro de um runtime Tokio multi-thread: o handshake
    /// é aguardado de forma síncrona, até ao prazo configurado.
    pub fn start(config: WorkerConfig) -> Result<Self, AppError> {
        config.validate()?;
        let runtime = tokio::runtime::Handle::try_current().map_err(|_| {
            unavailable("O auxiliar tem de ser iniciado a partir de um runtime assíncrono.")
        })?;

        let (inputs, input_receiver) = subscriptions::unbounded_channel();
        let (events, event_receiver) = subscriptions::unbounded_channel();
        let generation = Arc::new(AtomicU64::new(0));
        let running = Arc::new(AtomicBool::new(false));
        let busy = Arc::new(AtomicBool::new(false));
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);

        let supervisor = Supervisor {
            config: config.clone(),
            events,
            inputs: inputs.clone(),
            generation: Arc::clone(&generation),
            running: Arc::clone(&running),
            busy: Arc::clone(&busy),
            stderr: Arc::new(Mutex::new(Vec::new())),
            process: None,
            pending: None,
            handshake: None,
            used: HashSet::new(),
            internal_ids: 0,
            stopping: false,
        };
        runtime.spawn(supervisor.run(input_receiver, ready_sender));

        let wait = config.deadlines.handshake + COMMAND_TIMEOUT;
        match ready_receiver.recv_timeout(wait) {
            Ok(Ok(())) => Ok(Self {
                inputs,
                events: Mutex::new(Some(event_receiver)),
                generation,
                running,
                busy,
                deadlines: config.deadlines,
            }),
            Ok(Err(error)) => Err(error),
            Err(_) => {
                // Sem cliente, o supervisor tem de encerrar o processo.
                let _ = inputs.send(Input::Command(Control::Detach));
                Err(timeout_error(
                    "O auxiliar não ficou pronto dentro do prazo.",
                ))
            }
        }
    }

    /// Canal de eventos. Só pode ser tomado uma vez, pelo serviço.
    pub fn take_events(&self) -> Option<UnboundedReceiver<WorkerEvent>> {
        match self.events.lock() {
            Ok(mut slot) => slot.take(),
            Err(_) => None,
        }
    }

    /// Geração atual do auxiliar. Começa em 1 e nunca diminui.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    /// Indica se existe um processo do auxiliar vivo.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Indica se existe um pedido pendente.
    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::SeqCst)
    }

    fn request<T>(
        &self,
        build: impl FnOnce(SyncSender<Result<T, AppError>>) -> Input,
        wait: Duration,
    ) -> Result<T, AppError> {
        let (reply, response) = mpsc::sync_channel(1);
        self.inputs
            .send(build(reply))
            .map_err(|_| unavailable("O auxiliar não está em execução."))?;
        match response.recv_timeout(wait) {
            Ok(result) => result,
            Err(_) => Err(timeout_error("O auxiliar não respondeu a tempo.")),
        }
    }

    /// Encerra o processo: envia `shutdown`, espera até 3 s e termina à força.
    pub fn shutdown(&self) -> Result<(), AppError> {
        self.request(
            |reply| Input::Command(Control::Shutdown { reply }),
            self.deadlines.shutdown + EXIT_GRACE + COMMAND_TIMEOUT,
        )
    }
}

impl WorkerPort for WorkerClient {
    fn submit(&self, request: WorkerRequest) -> Result<(), AppError> {
        self.request(
            |reply| Input::Command(Control::Submit { request, reply }),
            COMMAND_TIMEOUT,
        )
    }

    fn restart(&self) -> Result<(), AppError> {
        self.request(
            |reply| Input::Command(Control::Restart { reply }),
            self.deadlines.handshake + EXIT_GRACE + COMMAND_TIMEOUT,
        )
        .map(|_generation| ())
    }
}

impl Drop for WorkerClient {
    fn drop(&mut self) {
        // Sem o cliente, o supervisor encerra o processo (`kill_on_drop` cobre
        // o caso de o runtime terminar antes de o comando ser tratado).
        let _ = self.inputs.send(Input::Command(Control::Detach));
    }
}

/// Quem é notificado quando o handshake termina.
enum Notify {
    Start(SyncSender<Result<(), AppError>>),
    Restart(SyncSender<Result<u64, AppError>>),
}

/// Processo do auxiliar e as tarefas que drenam os seus pipes.
struct Process {
    epoch: u64,
    child: Child,
    stdin: ChildStdin,
    stdout_reader: JoinHandle<()>,
    stderr_reader: JoinHandle<()>,
}

/// Pedido à espera de resposta final.
struct Pending {
    id: String,
    op: WorkerOp,
    generation: u64,
    stage: JobStage,
    deadline: Instant,
    audio_duration: Option<Duration>,
    artifact: Option<PathBuf>,
    reply: Option<SyncSender<Result<(), AppError>>>,
}

/// Handshake `health` em curso.
struct Handshake {
    id: String,
    generation: u64,
    deadline: Instant,
    notify: Notify,
}

/// Supervisor do processo, dono único do filho e do estado do protocolo.
struct Supervisor {
    config: WorkerConfig,
    events: UnboundedSender<WorkerEvent>,
    inputs: UnboundedSender<Input>,
    generation: Arc<AtomicU64>,
    running: Arc<AtomicBool>,
    busy: Arc<AtomicBool>,
    stderr: Arc<Mutex<Vec<u8>>>,
    process: Option<Process>,
    pending: Option<Pending>,
    handshake: Option<Handshake>,
    used: HashSet<String>,
    internal_ids: u64,
    stopping: bool,
}

impl Supervisor {
    /// Laço principal: uma única entrada (comandos e linhas do processo) e o prazo.
    async fn run(
        mut self,
        mut inputs: UnboundedReceiver<Input>,
        ready: SyncSender<Result<(), AppError>>,
    ) {
        if !self.launch(Notify::Start(ready)).await {
            return;
        }
        while !self.stopping {
            let deadline = self.next_deadline();
            tokio::select! {
                biased;
                input = inputs.recv() => match input {
                    Some(Input::Command(command)) => {
                        if self.handle_command(command).await {
                            break;
                        }
                    }
                    Some(Input::Line { epoch, line }) => self.handle_line(epoch, &line).await,
                    Some(Input::Closed { epoch, detail }) => self.handle_closed(epoch, detail).await,
                    None => break,
                },
                _ = sleep_until(deadline) => self.handle_deadline().await,
            }
        }
        self.terminate().await;
    }

    // -- arranque -----------------------------------------------------------

    /// Arranca o processo e envia o `health` do handshake.
    async fn launch(&mut self, notify: Notify) -> bool {
        let generation = match self.spawn_process() {
            Ok(generation) => generation,
            Err(error) => {
                let _ = self.notify(notify, Err(error));
                return false;
            }
        };
        let id = self.internal_id("health");
        let line = match encode_request(&id, WorkerOp::Health, &json!({})) {
            Ok(line) => line,
            Err(error) => {
                let _ = self.notify(notify, Err(error));
                self.terminate().await;
                return false;
            }
        };
        if let Err(error) = self.write_line(&line).await {
            let _ = self.notify(notify, Err(error));
            self.terminate().await;
            return false;
        }
        self.handshake = Some(Handshake {
            id,
            generation,
            deadline: Instant::now() + self.config.deadlines.handshake,
            notify,
        });
        true
    }

    /// Cria o processo com argumentos separados e pipes drenados.
    fn spawn_process(&mut self) -> Result<u64, AppError> {
        fs::create_dir_all(&self.config.working_directory).map_err(|error| {
            unavailable(format!(
                "Não foi possível preparar o diretório do auxiliar «{}»: {error}.",
                self.config.working_directory.display()
            ))
        })?;

        let mut command = Command::new(&self.config.python_executable);
        command
            .arg("-u")
            .arg("-m")
            .arg(&self.config.module)
            .arg("--models-manifest")
            .arg(&self.config.models_manifest)
            .current_dir(&self.config.working_directory)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.as_std_mut().creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = command.spawn().map_err(|error| {
            unavailable(format!(
                "Não foi possível iniciar o auxiliar Python «{}»: {error}.",
                self.config.python_executable.display()
            ))
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| unavailable("O auxiliar não abriu a entrada padrão."))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| unavailable("O auxiliar não abriu a saída padrão."))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| unavailable("O auxiliar não abriu a saída de erro."))?;

        if let Ok(mut tail) = self.stderr.lock() {
            tail.clear();
        }
        let generation = self.bump_generation();
        let stdout_reader = spawn_stdout_reader(stdout, self.inputs.clone(), generation);
        let stderr_reader = spawn_stderr_reader(stderr, Arc::clone(&self.stderr));
        self.process = Some(Process {
            epoch: generation,
            child,
            stdin,
            stdout_reader,
            stderr_reader,
        });
        self.running.store(true, Ordering::SeqCst);
        Ok(generation)
    }

    fn internal_id(&mut self, kind: &str) -> String {
        self.internal_ids += 1;
        format!("worker-{kind}-{}", self.internal_ids)
    }

    fn bump_generation(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    fn notify(&self, notify: Notify, result: Result<u64, AppError>) -> bool {
        match notify {
            Notify::Start(sender) => sender.send(result.map(|_| ())).is_ok(),
            Notify::Restart(sender) => sender.send(result).is_ok(),
        }
    }

    // -- entrada ------------------------------------------------------------

    fn next_deadline(&self) -> Option<Instant> {
        let handshake = self.handshake.as_ref().map(|handshake| handshake.deadline);
        let pending = self.pending.as_ref().map(|pending| pending.deadline);
        match (handshake, pending) {
            (Some(first), Some(second)) => Some(first.min(second)),
            (Some(only), None) | (None, Some(only)) => Some(only),
            (None, None) => None,
        }
    }

    async fn handle_command(&mut self, command: Control) -> bool {
        match command {
            Control::Submit { request, reply } => {
                self.handle_submit(request, reply).await;
                false
            }
            Control::Restart { reply } => {
                self.handle_restart(reply).await;
                false
            }
            Control::Shutdown { reply } => {
                self.handle_shutdown(reply).await;
                false
            }
            Control::Detach => true,
        }
    }

    async fn handle_submit(
        &mut self,
        request: WorkerRequest,
        reply: SyncSender<Result<(), AppError>>,
    ) {
        if self.handshake.is_some() {
            let _ = reply.send(Err(AppError::busy("O auxiliar está a arrancar.")));
            return;
        }
        if self.pending.is_some() {
            let _ = reply.send(Err(AppError::busy(
                "O auxiliar já está a processar um pedido.",
            )));
            return;
        }
        let Some(epoch) = self.process.as_ref().map(|process| process.epoch) else {
            let _ = reply.send(Err(unavailable(
                "O auxiliar não está em execução. Reinicie-o antes de repetir o pedido.",
            )));
            return;
        };
        if let Err(error) = validate_id(&request.id) {
            let _ = reply.send(Err(error));
            return;
        }
        if self.used.contains(&request.id) {
            let _ = reply.send(Err(protocol::protocol_error(
                "O identificador do pedido já foi utilizado por um pedido terminado.",
            )));
            return;
        }
        let line = match encode_request(&request.id, request.op, &request.params) {
            Ok(line) => line,
            Err(error) => {
                let _ = reply.send(Err(error));
                return;
            }
        };
        if let Some(artifact) = &request.artifact {
            // Um artefacto antigo nunca pode ser confundido com o resultado novo.
            let _ = fs::remove_file(artifact);
        }
        if let Err(error) = self.write_line(&line).await {
            let _ = reply.send(Err(error));
            self.invalidate_process().await;
            return;
        }

        let window = match request.op {
            WorkerOp::Health => self.config.deadlines.handshake,
            WorkerOp::Shutdown => self.config.deadlines.shutdown,
            WorkerOp::Transcribe | WorkerOp::Synthesize => self.config.deadlines.model_load,
        };
        self.used.insert(request.id.clone());
        self.busy.store(true, Ordering::SeqCst);
        self.pending = Some(Pending {
            id: request.id,
            op: request.op,
            generation: epoch,
            stage: JobStage::Loading,
            deadline: Instant::now() + window,
            audio_duration: request.audio_duration,
            artifact: request.artifact,
            reply: None,
        });
        let _ = reply.send(Ok(()));
    }

    async fn handle_restart(&mut self, reply: SyncSender<Result<u64, AppError>>) {
        if self.handshake.is_some() {
            let _ = reply.send(Err(AppError::busy("O auxiliar está a arrancar.")));
            return;
        }
        if self.pending.is_some() {
            let _ = reply.send(Err(AppError::busy(
                "Há um pedido em curso; aguarde que termine antes de reiniciar o auxiliar.",
            )));
            return;
        }
        self.terminate().await;
        let _ = self.launch(Notify::Restart(reply)).await;
    }

    async fn handle_shutdown(&mut self, reply: SyncSender<Result<(), AppError>>) {
        if self.handshake.is_some() {
            let _ = reply.send(Err(AppError::busy(
                "O auxiliar está a arrancar; aguarde antes de encerrar.",
            )));
            return;
        }
        if self.process.is_none() {
            let _ = reply.send(Ok(()));
            self.stopping = true;
            return;
        }
        if let Some(pending) = self.pending.take() {
            self.busy.store(false, Ordering::SeqCst);
            self.remove_artifact(&pending.artifact);
            if let Some(reply) = pending.reply {
                let _ = reply.send(Ok(()));
            }
            if pending.op != WorkerOp::Shutdown {
                self.emit(
                    pending.generation,
                    WorkerMessage::Failed {
                        request_id: pending.id,
                        error: unavailable("O auxiliar foi encerrado antes de terminar o pedido."),
                    },
                );
            }
        }
        let Some(epoch) = self.process.as_ref().map(|process| process.epoch) else {
            let _ = reply.send(Ok(()));
            self.stopping = true;
            return;
        };
        let id = self.internal_id("shutdown");
        let line = match encode_request(&id, WorkerOp::Shutdown, &json!({})) {
            Ok(line) => line,
            Err(error) => {
                let _ = reply.send(Err(error));
                self.terminate().await;
                self.stopping = true;
                return;
            }
        };
        if self.write_line(&line).await.is_err() {
            self.terminate().await;
            let _ = reply.send(Ok(()));
            self.stopping = true;
            return;
        }
        self.used.insert(id.clone());
        self.pending = Some(Pending {
            id,
            op: WorkerOp::Shutdown,
            generation: epoch,
            stage: JobStage::Processing,
            deadline: Instant::now() + self.config.deadlines.shutdown,
            audio_duration: None,
            artifact: None,
            reply: Some(reply),
        });
    }

    // -- protocolo ----------------------------------------------------------

    async fn handle_line(&mut self, epoch: u64, line: &str) {
        if self.process.as_ref().map(|process| process.epoch) != Some(epoch) {
            return; // saída de uma instância anterior
        }
        if self.handshake.is_some() {
            self.handle_handshake_line(line).await;
            return;
        }
        if self.pending.is_none() {
            // Uma mensagem sem pedido pendente quebra o protocolo: a instância
            // não pode continuar a ser usada (evita aplicar resultados a mais).
            self.invalidate_process().await;
            return;
        }
        let (pending_id, generation) = match self.pending.as_ref() {
            Some(pending) => (pending.id.clone(), pending.generation),
            None => return,
        };
        let response = match decode_response(line) {
            Ok(response) => response,
            Err(error) => {
                self.fail_pending(error).await;
                return;
            }
        };
        if response.id() != pending_id {
            self.fail_pending(protocol::protocol_error(
                "O auxiliar respondeu a um pedido diferente do pendente.",
            ))
            .await;
            return;
        }
        match response {
            WorkerResponse::Progress { stage, .. } => {
                self.handle_progress(generation, stage).await;
            }
            WorkerResponse::Result {
                ok, result, error, ..
            } => {
                self.handle_result(ok, result, error).await;
            }
        }
    }

    async fn handle_handshake_line(&mut self, line: &str) {
        let Some(handshake) = self.handshake.as_ref() else {
            return;
        };
        let expected = handshake.id.clone();
        let response = match decode_response(line) {
            Ok(response) => response,
            Err(error) => {
                self.fail_handshake(error).await;
                return;
            }
        };
        if response.id() != expected {
            self.fail_handshake(protocol::protocol_error(
                "O auxiliar respondeu ao handshake com outro identificador.",
            ))
            .await;
            return;
        }
        match response {
            // O handshake não produz progresso; uma etapa inesperada não altera
            // o prazo.
            WorkerResponse::Progress { .. } => {}
            WorkerResponse::Result {
                ok, result, error, ..
            } => {
                if !ok {
                    let error = error.unwrap_or_else(|| {
                        protocol::protocol_error(
                            "O auxiliar recusou o handshake sem indicar o erro.",
                        )
                    });
                    self.fail_handshake(error).await;
                    return;
                }
                let Some(value) = result else {
                    self.fail_handshake(protocol::protocol_error(
                        "O auxiliar concluiu o handshake sem resultado.",
                    ))
                    .await;
                    return;
                };
                match parse_result(WorkerOp::Health, value) {
                    Ok(WorkerResult::Health(health)) => self.complete_handshake(health),
                    Ok(_) => {
                        self.fail_handshake(protocol::protocol_error(
                            "O handshake devolveu um resultado inesperado.",
                        ))
                        .await;
                    }
                    Err(error) => self.fail_handshake(error).await,
                }
            }
        }
    }

    fn complete_handshake(&mut self, health: HealthResult) {
        let Some(handshake) = self.handshake.take() else {
            return;
        };
        self.used.insert(handshake.id.clone());
        self.emit(
            handshake.generation,
            WorkerMessage::Completed {
                request_id: handshake.id,
                result: WorkerResult::Health(health),
            },
        );
        let _ = self.notify(handshake.notify, Ok(handshake.generation));
        self.running.store(true, Ordering::SeqCst);
    }

    async fn fail_handshake(&mut self, error: AppError) {
        let Some(handshake) = self.handshake.take() else {
            return;
        };
        self.emit(
            handshake.generation,
            WorkerMessage::Failed {
                request_id: handshake.id,
                error: error.clone(),
            },
        );
        let _ = self.notify(handshake.notify, Err(error));
        self.invalidate_process().await;
    }

    async fn handle_progress(&mut self, generation: u64, stage: JobStage) {
        let deadlines = self.config.deadlines;
        let mut regression = false;
        let request_id;
        {
            let Some(pending) = self.pending.as_mut() else {
                return;
            };
            if pending.op == WorkerOp::Shutdown {
                // O encerramento é interno; o serviço não o vê como trabalho.
                return;
            }
            request_id = pending.id.clone();
            match (pending.stage, stage) {
                (JobStage::Loading, JobStage::Processing) => {
                    pending.stage = JobStage::Processing;
                    let window = match pending.op {
                        WorkerOp::Transcribe => {
                            deadlines.transcription(pending.audio_duration.unwrap_or_default())
                        }
                        WorkerOp::Synthesize => deadlines.synthesis,
                        WorkerOp::Health | WorkerOp::Shutdown => deadlines.model_load,
                    };
                    pending.deadline = Instant::now() + window;
                }
                (JobStage::Processing, JobStage::Loading) => regression = true,
                // Repetir a etapa não renova indefinidamente o prazo.
                _ => {}
            }
        }
        if regression {
            self.fail_pending(protocol::protocol_error(
                "O auxiliar recuou do processamento para o carregamento.",
            ))
            .await;
            return;
        }
        self.emit(generation, WorkerMessage::Progress { request_id, stage });
    }

    async fn handle_result(&mut self, ok: bool, result: Option<Value>, error: Option<AppError>) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        self.busy.store(false, Ordering::SeqCst);
        let Pending {
            id,
            op,
            generation,
            artifact,
            reply,
            ..
        } = pending;

        if op == WorkerOp::Shutdown {
            // O encerramento é interno: o resultado não é um evento do serviço
            // e o processo é terminado mesmo que o auxiliar recuse o pedido.
            self.graceful_exit().await;
            if let Some(reply) = reply {
                let _ = reply.send(Ok(()));
            }
            self.stopping = true;
            return;
        }

        if !ok {
            let error = error.unwrap_or_else(|| {
                protocol::protocol_error("O auxiliar recusou o pedido sem indicar o erro.")
            });
            crate::diagnostics::record(crate::diagnostics::Event::new(
                "worker",
                op.as_str(),
                &error.code,
            ));
            self.remove_artifact(&artifact);
            self.emit(
                generation,
                WorkerMessage::Failed {
                    request_id: id,
                    error,
                },
            );
            return;
        }

        let Some(value) = result else {
            self.remove_artifact(&artifact);
            self.emit(
                generation,
                WorkerMessage::Failed {
                    request_id: id,
                    error: protocol::protocol_error(
                        "O auxiliar indicou sucesso sem devolver o resultado.",
                    ),
                },
            );
            self.invalidate_process().await;
            return;
        };

        match parse_result(op, value) {
            Ok(result) => self.emit(
                generation,
                WorkerMessage::Completed {
                    request_id: id,
                    result,
                },
            ),
            Err(error) => {
                self.remove_artifact(&artifact);
                self.emit(
                    generation,
                    WorkerMessage::Failed {
                        request_id: id,
                        error,
                    },
                );
                self.invalidate_process().await;
            }
        }
    }

    async fn handle_deadline(&mut self) {
        let now = Instant::now();
        if let Some(handshake) = self.handshake.as_ref() {
            if handshake.deadline <= now {
                self.fail_handshake(timeout_error(
                    "O auxiliar não respondeu ao handshake dentro do prazo.",
                ))
                .await;
                return;
            }
        }
        let Some(pending) = self.pending.as_ref() else {
            return;
        };
        if pending.deadline > now {
            return;
        }
        let message = match pending.stage {
            JobStage::Loading => {
                "O auxiliar não terminou o carregamento do modelo dentro do prazo."
            }
            JobStage::Processing => "O auxiliar não devolveu o resultado dentro do prazo.",
        };
        self.fail_pending(timeout_error(message)).await;
    }

    async fn handle_closed(&mut self, epoch: u64, detail: Option<String>) {
        if self.process.as_ref().map(|process| process.epoch) != Some(epoch) {
            return;
        }
        let error = self.unexpected_exit(detail);
        if self.handshake.is_some() {
            self.fail_handshake(error).await;
        } else if self.pending.is_some() {
            self.fail_pending(error).await;
        } else {
            self.invalidate_process().await;
        }
    }

    // -- falhas e encerramento ---------------------------------------------

    /// Falha o pedido pendente e descarta a instância do auxiliar.
    async fn fail_pending(&mut self, error: AppError) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        self.busy.store(false, Ordering::SeqCst);
        self.remove_artifact(&pending.artifact);
        if pending.op == WorkerOp::Shutdown {
            // O encerramento conseguiu o que queria: o processo vai terminar.
            self.invalidate_process().await;
            if let Some(reply) = pending.reply {
                let _ = reply.send(Ok(()));
            }
            self.stopping = true;
            return;
        }
        if let Some(reply) = pending.reply {
            let _ = reply.send(Err(error.clone()));
        }
        self.emit(
            pending.generation,
            WorkerMessage::Failed {
                request_id: pending.id,
                error,
            },
        );
        self.invalidate_process().await;
    }

    /// Termina o processo e queima a geração: resultados antigos deixam de valer.
    async fn invalidate_process(&mut self) {
        self.bump_generation();
        self.running.store(false, Ordering::SeqCst);
        self.terminate().await;
    }

    /// Termina o processo imediatamente e espera pela saída.
    async fn terminate(&mut self) {
        if let Some(mut process) = self.process.take() {
            let _ = process.child.start_kill();
            let _ = tokio::time::timeout(EXIT_GRACE, process.child.wait()).await;
            process.stdout_reader.abort();
            process.stderr_reader.abort();
        }
        self.running.store(false, Ordering::SeqCst);
    }

    /// Deixa o processo sair sozinho depois do `shutdown`, forçando se preciso.
    async fn graceful_exit(&mut self) {
        if let Some(mut process) = self.process.take() {
            if tokio::time::timeout(EXIT_GRACE, process.child.wait())
                .await
                .is_err()
            {
                let _ = process.child.start_kill();
                let _ = tokio::time::timeout(EXIT_GRACE, process.child.wait()).await;
            }
            process.stdout_reader.abort();
            process.stderr_reader.abort();
        }
        self.running.store(false, Ordering::SeqCst);
    }

    fn remove_artifact(&self, artifact: &Option<PathBuf>) {
        if let Some(path) = artifact {
            let _ = fs::remove_file(path);
        }
    }

    fn emit(&self, generation: u64, message: WorkerMessage) {
        let _ = self.events.send(WorkerEvent {
            generation,
            message,
        });
    }

    /// Mensagem de saída inesperada, com o fim do stderr para diagnóstico.
    fn unexpected_exit(&self, detail: Option<String>) -> AppError {
        let mut message = "O auxiliar terminou inesperadamente.".to_owned();
        if let Some(detail) = detail {
            message.push_str(&format!(" Falha ao ler a saída: {detail}."));
        }
        if let Some(tail) = self.stderr_summary() {
            message.push_str(&format!(" Última saída de erro: «{tail}»"));
        }
        unavailable(message)
    }

    fn stderr_summary(&self) -> Option<String> {
        let buffer = self.stderr.lock().ok()?;
        if buffer.is_empty() {
            return None;
        }
        let start = buffer.len().saturating_sub(STDERR_SUMMARY);
        let text = String::from_utf8_lossy(&buffer[start..]).replace(['\r', '\n'], " ");
        let text = text.trim();
        if text.is_empty() {
            None
        } else {
            Some(text.chars().take(STDERR_SUMMARY).collect())
        }
    }

    async fn write_line(&mut self, line: &str) -> Result<(), AppError> {
        let process = self
            .process
            .as_mut()
            .ok_or_else(|| unavailable("O auxiliar não está em execução."))?;
        process
            .stdin
            .write_all(line.as_bytes())
            .await
            .map_err(write_failed)?;
        process.stdin.write_all(b"\n").await.map_err(write_failed)?;
        process.stdin.flush().await.map_err(write_failed)
    }
}

/// O prazo de um pedido nunca é estendido por progressos repetidos.
async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(at) => tokio::time::sleep_until(tokio::time::Instant::from_std(at)).await,
        None => std::future::pending::<()>().await,
    }
}

/// Drena stdout linha a linha para o supervisor, sem bloquear o processo.
fn spawn_stdout_reader(
    stdout: ChildStdout,
    inputs: UnboundedSender<Input>,
    epoch: u64,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => {
                    let _ = inputs.send(Input::Closed {
                        epoch,
                        detail: None,
                    });
                    break;
                }
                Ok(_) => {
                    let text = line.trim_end_matches(['\r', '\n']).to_owned();
                    if inputs.send(Input::Line { epoch, line: text }).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    let _ = inputs.send(Input::Closed {
                        epoch,
                        detail: Some(error.to_string()),
                    });
                    break;
                }
            }
        }
    })
}

/// Drena stderr continuamente e conserva apenas o fim para diagnóstico.
fn spawn_stderr_reader(stderr: ChildStderr, tail: Arc<Mutex<Vec<u8>>>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => break,
                Ok(_) => {
                    if let Some(event) = crate::diagnostics::worker_event(&line) {
                        crate::diagnostics::record(event);
                    }
                    if let Ok(mut buffer) = tail.lock() {
                        buffer.extend_from_slice(line.as_bytes());
                        let excess = buffer.len().saturating_sub(STDERR_TAIL);
                        if excess > 0 {
                            buffer.drain(..excess);
                        }
                    }
                }
                Err(_) => break,
            }
        }
    })
}

fn write_failed(error: std::io::Error) -> AppError {
    unavailable(format!(
        "Não foi possível enviar o pedido ao auxiliar: {error}."
    ))
}

/// O auxiliar não está disponível ou terminou; pode ser repetido.
fn unavailable(message: impl Into<String>) -> AppError {
    AppError::new(error_code::WORKER_UNAVAILABLE, message, true)
}

/// Problema de configuração do auxiliar, que exige corrigir os ficheiros.
fn configuration(message: impl Into<String>) -> AppError {
    AppError::new(error_code::WORKER_UNAVAILABLE, message, false)
}

/// Prazo excedido; repetir depois de reiniciar o auxiliar.
fn timeout_error(message: impl Into<String>) -> AppError {
    AppError::new(error_code::WORKER_TIMEOUT, message, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Escreve um `audio.json` válido e devolve o caminho.
    fn write_config(dir: &Path, python: &Path, manifest: &Path) -> PathBuf {
        let path = dir.join(CONFIG_FILE);
        let body = json!({
            "schemaVersion": 1,
            "pythonExecutable": python.display().to_string(),
            "modelsManifest": manifest.display().to_string(),
        });
        fs::write(
            &path,
            serde_json::to_vec(&body).expect("codificar configuração"),
        )
        .expect("escrever configuração");
        path
    }

    fn temporary_paths() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().expect("criar diretório temporário");
        let manifest = dir.path().join("models.json");
        fs::write(&manifest, b"{}").expect("escrever manifesto de teste");
        (
            dir,
            std::env::current_exe().expect("executável do teste"),
            manifest,
        )
    }

    #[test]
    fn config_loads_absolute_existing_paths() {
        let (dir, python, manifest) = temporary_paths();
        let path = write_config(dir.path(), &python, &manifest);

        let config = WorkerConfig::from_file(&path, dir.path()).expect("carregar configuração");

        assert_eq!(config.module, WORKER_MODULE);
        assert_eq!(config.module, "audio_worker");
        assert_eq!(config.python_executable, python);
        assert_eq!(config.models_manifest, manifest);
        assert_eq!(config.working_directory, dir.path());
        assert_eq!(config.deadlines, Deadlines::default());
        config.validate().expect("a configuração é válida");
    }

    #[test]
    fn config_rejects_relative_and_missing_paths() {
        let (dir, python, manifest) = temporary_paths();

        let relative = dir.path().join("relative.json");
        fs::write(
            &relative,
            serde_json::to_vec(&json!({
                "schemaVersion": 1,
                "pythonExecutable": "python.exe",
                "modelsManifest": manifest.display().to_string(),
            }))
            .expect("codificar"),
        )
        .expect("escrever configuração relativa");
        let error = WorkerConfig::from_file(&relative, dir.path()).expect_err("caminho relativo");
        assert_eq!(error.code, error_code::WORKER_UNAVAILABLE);
        assert!(!error.retryable);

        let missing = dir.path().join("missing.json");
        fs::write(
            &missing,
            serde_json::to_vec(&json!({
                "schemaVersion": 1,
                "pythonExecutable": dir.path().join("nao-existe.exe").display().to_string(),
                "modelsManifest": manifest.display().to_string(),
            }))
            .expect("codificar"),
        )
        .expect("escrever configuração com ficheiro em falta");
        let error = WorkerConfig::from_file(&missing, dir.path()).expect_err("ficheiro em falta");
        assert_eq!(error.code, error_code::WORKER_UNAVAILABLE);

        let manifest_missing = dir.path().join("manifest-missing.json");
        fs::write(
            &manifest_missing,
            serde_json::to_vec(&json!({
                "schemaVersion": 1,
                "pythonExecutable": python.display().to_string(),
                "modelsManifest": dir.path().join("nao-existe.json").display().to_string(),
            }))
            .expect("codificar"),
        )
        .expect("escrever configuração com manifesto em falta");
        let error =
            WorkerConfig::from_file(&manifest_missing, dir.path()).expect_err("manifesto em falta");
        assert_eq!(error.code, error_code::WORKER_UNAVAILABLE);

        let relative_data = WorkerConfig::from_file(
            &write_config(dir.path(), &python, &manifest),
            Path::new("dados"),
        )
        .expect_err("diretório de dados relativo");
        assert_eq!(relative_data.code, error_code::WORKER_UNAVAILABLE);
    }

    #[test]
    fn config_rejects_invalid_documents() {
        let (dir, python, manifest) = temporary_paths();

        let wrong_schema = dir.path().join("schema.json");
        fs::write(
            &wrong_schema,
            serde_json::to_vec(&json!({
                "schemaVersion": 2,
                "pythonExecutable": python.display().to_string(),
                "modelsManifest": manifest.display().to_string(),
            }))
            .expect("codificar"),
        )
        .expect("escrever configuração");
        assert_eq!(
            WorkerConfig::from_file(&wrong_schema, dir.path())
                .expect_err("schemaVersion diferente")
                .code,
            error_code::WORKER_UNAVAILABLE
        );

        let invalid = dir.path().join("invalid.json");
        fs::write(&invalid, "isto não é json").expect("escrever configuração inválida");
        assert_eq!(
            WorkerConfig::from_file(&invalid, dir.path())
                .expect_err("JSON inválido")
                .code,
            error_code::WORKER_UNAVAILABLE
        );

        let incomplete = dir.path().join("incomplete.json");
        fs::write(
            &incomplete,
            serde_json::to_vec(&json!({"schemaVersion": 1})).expect("codificar"),
        )
        .expect("escrever configuração incompleta");
        assert_eq!(
            WorkerConfig::from_file(&incomplete, dir.path())
                .expect_err("chaves em falta")
                .code,
            error_code::WORKER_UNAVAILABLE
        );

        assert_eq!(
            WorkerConfig::from_file(&dir.path().join("nao-existe.json"), dir.path())
                .expect_err("ficheiro inexistente")
                .code,
            error_code::WORKER_UNAVAILABLE
        );
    }

    #[test]
    fn validate_rejects_a_removed_executable() {
        let (dir, python, manifest) = temporary_paths();
        let path = write_config(dir.path(), &python, &manifest);
        let config = WorkerConfig::from_file(&path, dir.path()).expect("carregar configuração");

        let removed = WorkerConfig {
            python_executable: dir.path().join("python-removido.exe"),
            ..config
        };
        let error = removed.validate().expect_err("executável em falta");
        assert_eq!(error.code, error_code::WORKER_UNAVAILABLE);
        assert!(!error.retryable);
    }
}

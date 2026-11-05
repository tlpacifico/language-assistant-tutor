//! Porta de áudio da fase 1.
//!
//! Uma única thread de áudio possui os dispositivos de entrada e saída
//! (`capture::Capture`, `playback::Playback`) e recebe comandos por canal. Os
//! streams da CPAL e do Rodio nunca são guardados fora dessa thread nem
//! partilhados por um `Mutex`: os comandos e as respostas atravessam canais e
//! os callbacks de captura entregam amostras por um canal limitado, sem
//! escrever em disco nem aguardar locks.
//!
//! O original é escrito em PCM16 à taxa e ao número de canais efetivos da
//! entrada, em modo streaming (ver [`capture`]). O derivado para transcrição é
//! produzido por [`normalize_wav`].

pub mod capture;
pub mod normalize;
pub mod playback;

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::domain::{error_code, AppError, CaptureMeta, ComponentState, ComponentStatus};

pub use capture::{CaptureFailure, CaptureSpec};
pub use normalize::normalize_wav;

/// Nome do áudio original dentro da pasta da reserva.
///
/// Coincide com o nome usado pela persistência ao publicar a resposta; a
/// captura escreve sempre num ficheiro provisório e só o renomeia depois de o
/// cabeçalho WAV estar fechado.
pub const ORIGINAL_FILE: &str = "original.wav";

/// Espera máxima por uma resposta da thread de áudio.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

/// Intervalo de verificação de falhas assíncronas durante captura ou reprodução.
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Reserva de uma gravação: identificador e pasta próprios.
///
/// A pasta é criada vazia pela persistência e recebe `original.wav` durante a
/// captura; cada gravação nova obtém sempre uma pasta diferente.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordingTarget {
    pub id: String,
    pub directory: PathBuf,
}

/// Áudio finalizado de uma gravação, pronto a publicar.
///
/// `original` é o ficheiro que a captura fechou e `meta` descreve o que foi
/// efetivamente escrito. Publicar valida o ficheiro antes de o tornar visível.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedAudio {
    pub id: String,
    pub original: PathBuf,
    pub meta: CaptureMeta,
}

/// Disponibilidade dos dois componentes de áudio.
#[derive(Debug, Clone, PartialEq)]
pub struct AvailabilityAudio {
    pub microphone: ComponentState,
    pub playback: ComponentState,
}

/// Evento assíncrono da porta de áudio.
#[derive(Debug, Clone, PartialEq)]
pub enum AudioEvent {
    /// A captura parou sem ação do utilizador.
    ///
    /// `capture` contém o áudio parcial recuperável (marcado `incomplete`)
    /// quando existe alguma amostra escrita; `None` significa que não há áudio
    /// recuperável e a resposta tem de ser repetida.
    CaptureInterrupted {
        /// Gravação que originou a interrupção.
        ///
        /// O serviço só pode aplicar o evento à captura que o produziu: sem
        /// esta identidade, uma interrupção atrasada de uma gravação já
        /// reaberta invalidaria a gravação nova e retiraria o botão de
        /// terminar.
        recording_id: Option<String>,
        capture: Option<CapturedAudio>,
        error: AppError,
    },
    /// A reprodução chegou ao fim do ficheiro.
    PlaybackFinished,
}

/// Captura interrompida, já fechada e pronta a entregar ao serviço.
#[derive(Debug, Clone, PartialEq)]
pub struct CaptureInterruption {
    pub capture: Option<CapturedAudio>,
    pub error: AppError,
}

/// Destinatário dos eventos da porta de áudio.
pub type EventSink = Arc<dyn Fn(AudioEvent) + Send + Sync>;

/// Porta de áudio usada pelo serviço.
///
/// Captura e reprodução são mutuamente exclusivas: começar a gravar pára a
/// reprodução e `play` é recusado com `BUSY` enquanto o microfone está aberto.
pub trait AudioPort: Send + Sync {
    fn availability(&self) -> AvailabilityAudio;
    fn start_capture(&self, target: RecordingTarget) -> Result<(), AppError>;
    fn stop_capture(&self) -> Result<CapturedAudio, AppError>;
    fn play(&self, path: PathBuf) -> Result<(), AppError>;
    /// Acrescenta um WAV à fila do mesmo player, sem substituir o que já toca.
    /// Por omissão equivale a `play` (portas sem fila).
    fn append(&self, path: PathBuf) -> Result<(), AppError> {
        self.play(path)
    }
    fn stop_playback(&self) -> Result<(), AppError>;
    /// Reabre a porta depois de um comando ter excedido o prazo.
    ///
    /// Depois de um `AUDIO_TIMEOUT` a porta recusa novos comandos: a thread
    /// antiga pode ainda estar no dispositivo e o estado do serviço deixou de
    /// ser comparável. Reabrir cria uma thread e uns dispositivos novos.
    fn reopen(&self) -> Result<(), AppError>;
}

/// Operações de dispositivo executadas apenas pela thread de áudio.
///
/// Existe para que a coordenação da porta (parar a reprodução ao começar a
/// gravar, publicar interrupções, terminar a reprodução) seja verificável sem
/// hardware, através de um adaptador controlado.
pub trait AudioBackend {
    fn availability(&mut self) -> AvailabilityAudio;
    fn start_capture(&mut self, target: RecordingTarget) -> Result<(), AppError>;
    /// Interrupção já detetada; a captura ativa é fechada por esta chamada.
    fn capture_interruption(&mut self) -> Option<CaptureInterruption>;
    fn stop_capture(&mut self) -> Result<CapturedAudio, AppError>;
    fn play(&mut self, path: PathBuf) -> Result<(), AppError>;
    /// Acrescenta à fila; por omissão equivale a `play`.
    fn append(&mut self, path: PathBuf) -> Result<(), AppError> {
        self.play(path)
    }
    fn stop_playback(&mut self) -> Result<(), AppError>;
    fn playback_finished(&mut self) -> bool;
}

/// Comandos aceites pela thread de áudio.
enum Command {
    Availability(Sender<AvailabilityAudio>),
    StartCapture {
        target: RecordingTarget,
        reply: Sender<Result<(), AppError>>,
    },
    StopCapture(Sender<Result<CapturedAudio, AppError>>),
    Play {
        path: PathBuf,
        /// `true` acrescenta à fila em vez de substituir a reprodução.
        queue: bool,
        reply: Sender<Result<(), AppError>>,
    },
    StopPlayback(Sender<Result<(), AppError>>),
    Shutdown,
}

/// Construtor do adaptador: a porta guarda-o para poder reabrir a thread.
type BackendFactory = Arc<dyn Fn() -> Box<dyn AudioBackend> + Send + Sync>;

/// Estado da thread de áudio corrente.
struct AudioThread {
    commands: Sender<Command>,
    handle: Option<JoinHandle<()>>,
    /// Um comando excedeu o prazo: só `reopen` volta a aceitar comandos.
    failed: bool,
}

/// Porta de áudio nativa: uma thread proprietária dos dispositivos.
pub struct NativeAudio {
    inner: Mutex<AudioThread>,
    factory: BackendFactory,
    events: EventSink,
    timeout: Duration,
}

impl NativeAudio {
    /// Cria a porta com os dispositivos reais (abertos apenas quando usados).
    pub fn new(events: impl Fn(AudioEvent) + Send + Sync + 'static) -> Result<Self, AppError> {
        Self::with_backend(|| Box::new(NativeDevices::new()), events)
    }

    /// Cria a porta sobre um adaptador controlado.
    ///
    /// O adaptador é construído dentro da thread de áudio, que passa a ser a
    /// sua única proprietária. O construtor é repetido por `reopen`, pelo que
    /// tem de poder ser chamado mais de uma vez.
    pub fn with_backend<F>(
        factory: F,
        events: impl Fn(AudioEvent) + Send + Sync + 'static,
    ) -> Result<Self, AppError>
    where
        F: Fn() -> Box<dyn AudioBackend> + Send + Sync + 'static,
    {
        Self::with_timeout(factory, events, COMMAND_TIMEOUT)
    }

    /// Variante com prazo explícito, usada pelos testes da porta.
    pub fn with_timeout<F>(
        factory: F,
        events: impl Fn(AudioEvent) + Send + Sync + 'static,
        timeout: Duration,
    ) -> Result<Self, AppError>
    where
        F: Fn() -> Box<dyn AudioBackend> + Send + Sync + 'static,
    {
        let factory: BackendFactory = Arc::new(factory);
        let events: EventSink = Arc::new(events);
        let inner = Mutex::new(spawn_audio_thread(&factory, &events)?);
        Ok(Self {
            inner,
            factory,
            events,
            timeout,
        })
    }

    fn lock_inner(&self) -> std::sync::MutexGuard<'_, AudioThread> {
        self.inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// Envia um comando e espera a resposta, sem bloquear indefinidamente.
    ///
    /// Um prazo excedido marca a porta como falhada: a thread pode concluir a
    /// operação mais tarde, e nesse caso o microfone ou o ficheiro deixariam de
    /// corresponder ao estado do serviço. Em vez de continuar às cegas, todos
    /// os comandos passam a devolver `AUDIO_TIMEOUT` até [`AudioPort::reopen`].
    fn request<T>(&self, build: impl FnOnce(Sender<T>) -> Command) -> Result<T, AppError> {
        let mut inner = self.lock_inner();
        if inner.failed {
            return Err(audio_timeout(
                "A operação de áudio anterior não terminou dentro do prazo. Use «Reiniciar áudio local» antes de continuar.",
            ));
        }
        let (reply, response) = mpsc::channel();
        if inner.commands.send(build(reply)).is_err() {
            inner.failed = true;
            return Err(device_unavailable(
                "A thread de áudio terminou inesperadamente.",
            ));
        }
        match response.recv_timeout(self.timeout) {
            Ok(value) => Ok(value),
            Err(_) => {
                inner.failed = true;
                Err(audio_timeout(
                    "A thread de áudio não respondeu a tempo e a operação ficou em estado desconhecido. Use «Reiniciar áudio local» para reabrir o áudio.",
                ))
            }
        }
    }
}

/// Arranca uma thread de áudio com um adaptador novo.
fn spawn_audio_thread(
    factory: &BackendFactory,
    events: &EventSink,
) -> Result<AudioThread, AppError> {
    let (commands, receiver) = mpsc::channel();
    let factory = Arc::clone(factory);
    let events = Arc::clone(events);
    let handle = thread::Builder::new()
        .name("audio".to_owned())
        .spawn(move || {
            let backend = factory();
            audio_loop(backend, events, receiver);
        })
        .map_err(|error| {
            AppError::new(
                error_code::DEVICE_UNAVAILABLE,
                format!("Não foi possível iniciar a thread de áudio: {error}."),
                true,
            )
        })?;
    Ok(AudioThread {
        commands,
        handle: Some(handle),
        failed: false,
    })
}

impl AudioPort for NativeAudio {
    fn availability(&self) -> AvailabilityAudio {
        self.request(Command::Availability)
            .unwrap_or_else(|error| AvailabilityAudio {
                microphone: ComponentState {
                    status: ComponentStatus::Error,
                    message: Some(error.message.clone()),
                },
                playback: ComponentState {
                    status: ComponentStatus::Error,
                    message: Some(error.message),
                },
            })
    }

    fn start_capture(&self, target: RecordingTarget) -> Result<(), AppError> {
        self.request(|reply| Command::StartCapture { target, reply })?
    }

    fn stop_capture(&self) -> Result<CapturedAudio, AppError> {
        self.request(Command::StopCapture)?
    }

    fn play(&self, path: PathBuf) -> Result<(), AppError> {
        self.request(|reply| Command::Play {
            path,
            queue: false,
            reply,
        })?
    }

    fn append(&self, path: PathBuf) -> Result<(), AppError> {
        self.request(|reply| Command::Play {
            path,
            queue: true,
            reply,
        })?
    }

    fn stop_playback(&self) -> Result<(), AppError> {
        self.request(Command::StopPlayback)?
    }

    fn reopen(&self) -> Result<(), AppError> {
        // A thread antiga pode estar presa no dispositivo. A thread nova usa
        // dispositivos novos; a antiga termina quando o comando pendente
        // acabar, porque o canal de comandos é fechado ao substituir o
        // remetente. Não se faz `join` para não esperar por esse comando.
        let replacement = spawn_audio_thread(&self.factory, &self.events)?;
        let previous = {
            let mut inner = self.lock_inner();
            std::mem::replace(&mut *inner, replacement)
        };
        drop(previous);
        Ok(())
    }
}

impl Drop for NativeAudio {
    /// Fecha a thread de áudio, entregando uma captura ativa como interrompida.
    ///
    /// O `join` é curto: a thread só tem de fechar o WAV provisório que já
    /// estava a ser escrito.
    fn drop(&mut self) {
        let mut inner = self.lock_inner();
        let _ = inner.commands.send(Command::Shutdown);
        if let Some(handle) = inner.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Laço da thread de áudio.
///
/// `interruption_error` guarda a falha que já foi publicada como
/// `CaptureInterrupted`: se o utilizador ainda pedir para terminar a gravação,
/// recebe o mesmo erro em vez de um resultado de sucesso.
fn audio_loop(mut backend: Box<dyn AudioBackend>, events: EventSink, commands: Receiver<Command>) {
    let mut capturing = false;
    let mut capturing_id: Option<String> = None;
    let mut playing = false;
    let mut interruption_error: Option<AppError> = None;

    loop {
        let command = if capturing || playing {
            match commands.recv_timeout(POLL_INTERVAL) {
                Ok(command) => Some(command),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match commands.recv() {
                Ok(command) => Some(command),
                Err(_) => break,
            }
        };

        if let Some(command) = command {
            match command {
                Command::Availability(reply) => {
                    let _ = reply.send(backend.availability());
                }
                Command::StartCapture { target, reply } => {
                    // Começar a gravar pára qualquer reprodução em curso; a
                    // entrada tem prioridade mesmo que a paragem falhe.
                    if backend.stop_playback().is_ok() {
                        playing = false;
                    }
                    let recording_id = target.id.clone();
                    match backend.start_capture(target) {
                        Ok(()) => {
                            capturing = true;
                            capturing_id = Some(recording_id);
                            interruption_error = None;
                            let _ = reply.send(Ok(()));
                        }
                        Err(error) => {
                            let _ = reply.send(Err(error));
                        }
                    }
                }
                Command::StopCapture(reply) => {
                    if !capturing {
                        let error = interruption_error.take().unwrap_or_else(not_capturing);
                        let _ = reply.send(Err(error));
                    } else {
                        capturing = false;
                        let recording_id = capturing_id.take();
                        let result = backend.stop_capture();
                        let failed = result.is_err();
                        let _ = reply.send(result);
                        if failed {
                            report_interruption(&mut backend, &events, recording_id);
                        }
                    }
                }
                Command::Play { path, queue, reply } => {
                    // O microfone e a saída não funcionam ao mesmo tempo: a
                    // captura tem prioridade e a reprodução é recusada até ela
                    // terminar (só esta camada conhece o estado da captura).
                    if capturing {
                        let _ = reply.send(Err(AppError::busy(
                            "Não é possível reproduzir áudio enquanto o microfone está a gravar.",
                        )));
                    } else {
                        let started = if queue {
                            backend.append(path)
                        } else {
                            backend.play(path)
                        };
                        match started {
                            Ok(()) => {
                                playing = true;
                                let _ = reply.send(Ok(()));
                            }
                            Err(error) => {
                                let _ = reply.send(Err(error));
                            }
                        }
                    }
                }
                Command::StopPlayback(reply) => {
                    let result = backend.stop_playback();
                    if result.is_ok() {
                        playing = false;
                    }
                    let _ = reply.send(result);
                }
                Command::Shutdown => {
                    if capturing {
                        report_interruption(&mut backend, &events, capturing_id.take());
                    }
                    let _ = backend.stop_playback();
                    break;
                }
            }
        }

        // Falhas e fim de reprodução chegam sem comando: verificar em cada volta.
        if capturing {
            if let Some(interruption) = backend.capture_interruption() {
                capturing = false;
                interruption_error = Some(interruption.error.clone());
                events(AudioEvent::CaptureInterrupted {
                    recording_id: capturing_id.take(),
                    capture: interruption.capture,
                    error: interruption.error,
                });
            }
        }
        if playing && backend.playback_finished() {
            playing = false;
            events(AudioEvent::PlaybackFinished);
        }
    }
}

/// Fecha a captura ativa e publica a interrupção com o áudio recuperável.
fn report_interruption(
    backend: &mut Box<dyn AudioBackend>,
    events: &EventSink,
    recording_id: Option<String>,
) {
    let interruption = backend
        .capture_interruption()
        .unwrap_or_else(|| CaptureInterruption {
            capture: None,
            error: capture_interrupted("A gravação foi interrompida e não há áudio recuperável."),
        });
    events(AudioEvent::CaptureInterrupted {
        recording_id,
        capture: interruption.capture,
        error: interruption.error,
    });
}

/// Dispositivos reais: captura CPAL e reprodução Rodio.
struct NativeDevices {
    capture: capture::Capture,
    playback: playback::Playback,
    /// Interrupção já fechada, à espera de ser entregue ao serviço.
    pending: Option<CaptureInterruption>,
}

impl NativeDevices {
    fn new() -> Self {
        Self {
            capture: capture::Capture::new(),
            playback: playback::Playback::new(),
            pending: None,
        }
    }
}

impl AudioBackend for NativeDevices {
    fn availability(&mut self) -> AvailabilityAudio {
        AvailabilityAudio {
            microphone: self.capture.availability(),
            playback: self.playback.availability(),
        }
    }

    fn start_capture(&mut self, target: RecordingTarget) -> Result<(), AppError> {
        self.pending = None;
        self.capture.start(target)
    }

    fn capture_interruption(&mut self) -> Option<CaptureInterruption> {
        if let Some(pending) = self.pending.take() {
            return Some(pending);
        }
        let interruption = self.capture.interruption()?;
        Some(interruption)
    }

    fn stop_capture(&mut self) -> Result<CapturedAudio, AppError> {
        let result = self.capture.stop();
        if let Err(error) = &result {
            if let Some(capture) = self.capture.take_recovered() {
                self.pending = Some(CaptureInterruption {
                    capture: Some(capture),
                    error: error.clone(),
                });
            }
        }
        result
    }

    fn play(&mut self, path: PathBuf) -> Result<(), AppError> {
        self.playback.play(&path)
    }

    fn append(&mut self, path: PathBuf) -> Result<(), AppError> {
        self.playback.append(&path)
    }

    fn stop_playback(&mut self) -> Result<(), AppError> {
        self.playback.stop()
    }

    fn playback_finished(&mut self) -> bool {
        self.playback.finished()
    }
}

/// Falha ao abrir ou usar um dispositivo de áudio.
fn device_unavailable(message: impl Into<String>) -> AppError {
    AppError::new(error_code::DEVICE_UNAVAILABLE, message, true)
}

/// Comando de áudio que excedeu o prazo; a porta tem de ser reaberta.
fn audio_timeout(message: impl Into<String>) -> AppError {
    AppError::new(error_code::AUDIO_TIMEOUT, message, true)
}

/// Captura interrompida por falha de escrita, capacidade ou dispositivo.
pub(crate) fn capture_interrupted(message: impl Into<String>) -> AppError {
    AppError::new(error_code::CAPTURE_INTERRUPTED, message, true)
}

/// Não existe gravação em curso para terminar.
fn not_capturing() -> AppError {
    AppError::new(
        error_code::CAPTURE_INTERRUPTED,
        "Não há nenhuma gravação em curso para terminar.",
        false,
    )
}

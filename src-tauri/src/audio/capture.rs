//! Captura de entrada (CPAL) e escrita do original em PCM16.
//!
//! O original é escrito em modo streaming: o callback do dispositivo converte
//! as amostras para PCM16 e entrega blocos por um canal limitado; uma thread
//! escritora consome esses blocos e escreve no ficheiro provisório com Hound.
//! O callback nunca escreve em disco e nunca aguarda um lock: um canal cheio é
//! reportado como interrupção explícita (não há descarte silencioso), tal como
//! um erro de escrita ou do dispositivo. Em qualquer interrupção o cabeçalho
//! WAV é fechado e o áudio parcial recuperável é publicado com
//! `incomplete = true`.
//!
//! Formatos aceites na entrada: I16, U16 e F32, convertidos para PCM16 sem
//! alterar a taxa nem os canais efetivos. Qualquer outro formato devolve
//! `DEVICE_FORMAT_UNSUPPORTED` em vez de provocar pânico.

use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat as CpalFormat;
use hound::{SampleFormat as WavFormat, WavSpec, WavWriter};

use crate::audio::{
    capture_interrupted, device_unavailable, CaptureInterruption, CapturedAudio, RecordingTarget,
    ORIGINAL_FILE,
};
use crate::domain::{error_code, AppError, CaptureMeta, ComponentState, ComponentStatus};

/// Ficheiro provisório; só passa a `original.wav` depois de fechado.
pub const PARTIAL_FILE: &str = "original.partial.wav";

/// Blocos em espera entre o callback e o escritor.
const DEFAULT_CHANNEL_BLOCKS: usize = 64;

/// Nome do escritor, útil nos relatórios de erro do sistema.
const WRITER_THREAD: &str = "audio-capture";

const INTERRUPT_NONE: u8 = 0;
const INTERRUPT_CHANNEL_FULL: u8 = 1;
const INTERRUPT_WRITER: u8 = 2;
const INTERRUPT_DEVICE: u8 = 3;

/// Mensagem quando o consumidor não acompanha a captura.
const CHANNEL_FULL_MESSAGE: &str =
    "O áudio deixou de ser escrito a tempo e a captura foi interrompida; o que já estava gravado foi conservado.";
const WRITER_MESSAGE: &str =
    "Não foi possível escrever o áudio da gravação e a captura foi interrompida; o que já estava gravado foi conservado.";
const DEVICE_MESSAGE: &str =
    "O dispositivo de entrada falhou e a captura foi interrompida; o que já estava gravado foi conservado.";

/// Taxa e canais efetivos da entrada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureSpec {
    pub sample_rate: u32,
    pub channels: u16,
}

impl CaptureSpec {
    /// Especificação dos canais de um WAV PCM16.
    fn wav_spec(self) -> WavSpec {
        WavSpec {
            channels: self.channels,
            sample_rate: self.sample_rate,
            bits_per_sample: 16,
            sample_format: WavFormat::Int,
        }
    }
}

/// Origem de uma interrupção de captura.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    /// O canal limitado encheu: o escritor não acompanhou o callback.
    ChannelFull,
    /// Falha ao escrever o ficheiro.
    Writer,
    /// Falha reportada pelo dispositivo de entrada.
    Device,
}

/// Falha explícita que interrompe a captura.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureFailure {
    kind: FailureKind,
    detail: String,
}

impl CaptureFailure {
    pub fn channel_full() -> Self {
        Self {
            kind: FailureKind::ChannelFull,
            detail: CHANNEL_FULL_MESSAGE.to_owned(),
        }
    }

    pub fn writer(detail: impl Into<String>) -> Self {
        Self {
            kind: FailureKind::Writer,
            detail: detail.into(),
        }
    }

    pub fn device(detail: impl Into<String>) -> Self {
        Self {
            kind: FailureKind::Device,
            detail: detail.into(),
        }
    }

    pub fn kind(&self) -> FailureKind {
        self.kind
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// Erro estável para a interface: a captura foi interrompida.
    pub fn error(&self) -> AppError {
        capture_interrupted(self.detail.clone())
    }
}

fn kind_code(kind: FailureKind) -> u8 {
    match kind {
        FailureKind::ChannelFull => INTERRUPT_CHANNEL_FULL,
        FailureKind::Writer => INTERRUPT_WRITER,
        FailureKind::Device => INTERRUPT_DEVICE,
    }
}

fn kind_from_code(code: u8) -> FailureKind {
    match code {
        INTERRUPT_CHANNEL_FULL => FailureKind::ChannelFull,
        INTERRUPT_WRITER => FailureKind::Writer,
        _ => FailureKind::Device,
    }
}

fn default_detail(kind: FailureKind) -> &'static str {
    match kind {
        FailureKind::ChannelFull => CHANNEL_FULL_MESSAGE,
        FailureKind::Writer => WRITER_MESSAGE,
        FailureKind::Device => DEVICE_MESSAGE,
    }
}

/// Estado de falha partilhado entre callback, escritor e thread de áudio.
///
/// A origem da falha vive num atómico para que o callback de tempo real nunca
/// bloqueie; o detalhe só é escrito pelas threads que podem esperar.
struct CaptureMonitor {
    kind: AtomicU8,
    detail: Mutex<Option<String>>,
}

impl CaptureMonitor {
    fn new() -> Self {
        Self {
            kind: AtomicU8::new(INTERRUPT_NONE),
            detail: Mutex::new(None),
        }
    }

    /// Registra a primeira falha; as seguintes não substituem a original.
    fn record(&self, kind: FailureKind, detail: Option<String>) {
        let won = self
            .kind
            .compare_exchange(
                INTERRUPT_NONE,
                kind_code(kind),
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_ok();
        if won {
            if let Some(detail) = detail {
                let mut slot = self
                    .detail
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                *slot = Some(detail);
            }
        }
    }

    /// Marca a falha sem tocar em locks (usado pelo callback de tempo real).
    fn flag(&self, kind: FailureKind) {
        let _ = self.kind.compare_exchange(
            INTERRUPT_NONE,
            kind_code(kind),
            Ordering::SeqCst,
            Ordering::SeqCst,
        );
    }

    fn failed(&self) -> bool {
        self.kind.load(Ordering::SeqCst) != INTERRUPT_NONE
    }

    fn failure(&self) -> Option<CaptureFailure> {
        let code = self.kind.load(Ordering::SeqCst);
        if code == INTERRUPT_NONE {
            return None;
        }
        let kind = kind_from_code(code);
        let detail = self
            .detail
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
            .unwrap_or_else(|| default_detail(kind).to_owned());
        Some(CaptureFailure { kind, detail })
    }
}

/// Bloco entregue pelo callback ao escritor.
enum Block {
    Samples(Vec<i16>),
    End,
}

/// Lado produtor do canal limitado entre o callback e o escritor.
///
/// Público para que a contrapressão (canal cheio interrompe a captura, sem
/// descartar amostras em silêncio) seja verificável sem um microfone; é o
/// mesmo canal usado por [`PendingCapture`].
pub struct CaptureChannel {
    sender: SyncSender<Block>,
    monitor: Arc<CaptureMonitor>,
}

impl CaptureChannel {
    /// Entrega um bloco de amostras intercaladas, sem bloquear.
    ///
    /// Um bloco deve conter frames inteiros (`samples.len()` múltiplo dos
    /// canais). Um canal cheio ou desligado devolve `CAPTURE_INTERRUPTED` e
    /// fica registado como falha; nenhuma amostra é descartada em silêncio.
    pub fn push(&self, samples: Vec<i16>) -> Result<(), AppError> {
        match self.sender.try_send(Block::Samples(samples)) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => {
                self.monitor.flag(FailureKind::ChannelFull);
                Err(capture_interrupted(CHANNEL_FULL_MESSAGE))
            }
            Err(TrySendError::Disconnected(_)) => {
                self.monitor.flag(FailureKind::Writer);
                Err(capture_interrupted(WRITER_MESSAGE))
            }
        }
    }

    /// Registra uma falha detetada fora do canal (dispositivo, escritor).
    pub fn fail(&self, failure: CaptureFailure) {
        self.monitor.record(failure.kind, Some(failure.detail));
    }

    /// Primeira falha registada, se existir.
    pub fn interruption(&self) -> Option<CaptureFailure> {
        self.monitor.failure()
    }

    /// O callback deixa de tentar entregar amostras depois de uma falha.
    fn has_failed(&self) -> bool {
        self.monitor.failed()
    }
}

impl Clone for CaptureChannel {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            monitor: Arc::clone(&self.monitor),
        }
    }
}

/// Recetor dos blocos, retido por quem controla o consumidor.
///
/// Existe para que um teste possa manter o canal ligado e cheio sem ler, tal
/// como acontece quando o escritor está atrasado.
pub struct CaptureBlocks(#[allow(dead_code)] Receiver<Block>);

/// Cria o par produtor/recetor usado pela captura, com a capacidade em blocos.
pub fn channel(capacity: usize) -> (CaptureChannel, CaptureBlocks) {
    let (sender, receiver) = mpsc::sync_channel(capacity);
    (
        CaptureChannel {
            sender,
            monitor: Arc::new(CaptureMonitor::new()),
        },
        CaptureBlocks(receiver),
    )
}

/// Resultado de fechar uma captura.
///
/// `capture` existe quando há pelo menos uma amostra escrita; `error` existe
/// quando a captura foi interrompida. Os dois juntos significam áudio parcial
/// recuperável, marcado `incomplete` nos metadados.
#[derive(Debug, Clone, PartialEq)]
pub struct CaptureOutcome {
    pub capture: Option<CapturedAudio>,
    pub error: Option<AppError>,
}

/// Captura em curso: canal, escritor e ficheiro provisório.
pub struct PendingCapture {
    id: String,
    directory: PathBuf,
    spec: CaptureSpec,
    partial: PathBuf,
    channel: CaptureChannel,
    sender: Option<SyncSender<Block>>,
    written: Arc<AtomicU64>,
    writer: Option<JoinHandle<WriterOutcome>>,
}

impl PendingCapture {
    /// Abre o ficheiro provisório e começa a escrever em streaming.
    ///
    /// Falha de imediato (sem deixar ficheiros) quando a pasta não existe ou o
    /// ficheiro não pode ser criado.
    pub fn begin(target: RecordingTarget, spec: CaptureSpec) -> Result<Self, AppError> {
        if spec.channels == 0 || spec.sample_rate == 0 {
            return Err(AppError::new(
                error_code::INVALID_AUDIO,
                "A entrada de áudio indicou um formato inválido.",
                false,
            ));
        }
        let partial = target.directory.join(PARTIAL_FILE);
        let file = File::create(&partial).map_err(|error| {
            capture_interrupted(format!(
                "Não foi possível criar o ficheiro da gravação em {}: {error}.",
                target.directory.display()
            ))
        })?;
        let writer = WavWriter::new(BufWriter::new(file), spec.wav_spec()).map_err(|error| {
            let _ = fs::remove_file(&partial);
            capture_interrupted(format!(
                "Não foi possível preparar o ficheiro WAV da gravação: {error}."
            ))
        })?;

        let (sender, receiver) = mpsc::sync_channel(DEFAULT_CHANNEL_BLOCKS);
        let monitor = Arc::new(CaptureMonitor::new());
        let written = Arc::new(AtomicU64::new(0));
        let writer_monitor = Arc::clone(&monitor);
        let writer_written = Arc::clone(&written);
        let channels = spec.channels;
        let handle = thread::Builder::new()
            .name(WRITER_THREAD.to_owned())
            .spawn(move || write_blocks(writer, receiver, writer_monitor, writer_written, channels))
            .map_err(|error| {
                remove_partial(&partial);
                capture_interrupted(format!(
                    "Não foi possível iniciar a escrita da gravação: {error}."
                ))
            })?;

        Ok(Self {
            id: target.id,
            directory: target.directory,
            spec,
            partial,
            channel: CaptureChannel {
                sender: sender.clone(),
                monitor,
            },
            sender: Some(sender),
            written,
            writer: Some(handle),
        })
    }

    /// Canal usado pelo callback do dispositivo.
    pub fn channel(&self) -> &CaptureChannel {
        &self.channel
    }

    /// Frames já escritos e confirmados no ficheiro.
    pub fn written_frames(&self) -> u64 {
        self.written.load(Ordering::SeqCst)
    }

    /// Fecha o WAV, renomeia o provisório e devolve o que é recuperável.
    ///
    /// Só publica `original.wav` quando o cabeçalho foi fechado e o ficheiro
    /// reaberto confirma as amostras escritas: um WAV com tamanhos provisórios
    /// não é reproduzível e nunca é apresentado como recuperável. Sem amostras
    /// não há ficheiro nenhum: o provisório é removido e o resultado diz que
    /// não existe áudio recuperável.
    pub fn finish(mut self) -> CaptureOutcome {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(Block::End);
        }
        let outcome = match self.writer.take() {
            Some(handle) => handle.join().unwrap_or_else(|_| WriterOutcome {
                frames: 0,
                finalized: false,
                error: Some(capture_interrupted(
                    "A escrita do áudio terminou de forma inesperada e não há áudio recuperável; grave novamente.",
                )),
            }),
            None => WriterOutcome {
                frames: 0,
                finalized: false,
                error: None,
            },
        };
        // Uma falha detetada fora do escritor (canal cheio, dispositivo) também
        // interrompe a captura, mesmo que a escrita em si tenha terminado bem.
        let error = outcome
            .error
            .or_else(|| self.channel.interruption().map(|failure| failure.error()));

        // Sem cabeçalho fechado o ficheiro não é reproduzível: não se publica.
        if !outcome.finalized {
            remove_partial(&self.partial);
            return CaptureOutcome {
                capture: None,
                error: Some(error.unwrap_or_else(|| {
                    capture_interrupted(
                        "Não foi possível fechar o cabeçalho do ficheiro WAV e não há áudio recuperável; grave novamente.",
                    )
                })),
            };
        }

        let frames = outcome.frames;
        if frames == 0 {
            remove_partial(&self.partial);
            return CaptureOutcome {
                capture: None,
                error: Some(error.unwrap_or_else(|| {
                    capture_interrupted(
                        "A gravação ficou sem amostras e não há áudio para recuperar; grave novamente.",
                    )
                })),
            };
        }

        // Reabrir confirma que o cabeçalho publicado descreve as amostras que o
        // escritor contou; sem essa confirmação o áudio não é recuperável.
        if let Err(verify_error) = verify_closed_wav(&self.partial, self.spec, frames) {
            remove_partial(&self.partial);
            return CaptureOutcome {
                capture: None,
                error: Some(verify_error),
            };
        }

        let original = self.directory.join(ORIGINAL_FILE);
        if let Err(rename_error) = fs::rename(&self.partial, &original) {
            remove_partial(&self.partial);
            return CaptureOutcome {
                capture: None,
                error: Some(capture_interrupted(format!(
                    "Não foi possível publicar a gravação em {}: {rename_error}.",
                    self.directory.display()
                ))),
            };
        }

        let bytes = fs::metadata(&original).map(|meta| meta.len()).unwrap_or(0);
        let meta = CaptureMeta {
            sample_rate: self.spec.sample_rate,
            channels: self.spec.channels,
            duration_ms: duration_ms(frames, self.spec.sample_rate),
            bytes,
            incomplete: error.is_some(),
        };
        CaptureOutcome {
            capture: Some(CapturedAudio {
                id: self.id,
                original,
                meta,
            }),
            error,
        }
    }
}

/// Resultado do escritor: frames confirmados, cabeçalho fechado e eventual falha.
struct WriterOutcome {
    frames: u64,
    finalized: bool,
    error: Option<AppError>,
}

/// Corpo da thread escritora: consome blocos e escreve PCM16 em streaming.
fn write_blocks(
    mut writer: WavWriter<BufWriter<File>>,
    receiver: Receiver<Block>,
    monitor: Arc<CaptureMonitor>,
    written: Arc<AtomicU64>,
    channels: u16,
) -> WriterOutcome {
    let mut samples = 0u64;
    let mut error = None;
    let mut finalized = false;

    while let Ok(block) = receiver.recv() {
        let Block::Samples(block) = block else {
            break;
        };
        let mut block_samples = 0u64;
        let mut failure = None;
        for sample in &block {
            match writer.write_sample(*sample) {
                Ok(()) => block_samples += 1,
                Err(write_error) => {
                    failure = Some(write_error);
                    break;
                }
            }
        }
        samples += block_samples;
        written.store(samples / u64::from(channels.max(1)), Ordering::SeqCst);
        if let Some(write_error) = failure {
            let message = format!(
                "Não foi possível escrever o áudio da gravação: {write_error}. O que já estava gravado foi conservado."
            );
            monitor.record(FailureKind::Writer, Some(message.clone()));
            error = Some(capture_interrupted(message));
            break;
        }
    }

    match writer.finalize() {
        Ok(()) => finalized = true,
        Err(finalize_error) => {
            let message = format!(
                "Não foi possível fechar o ficheiro WAV da gravação: {finalize_error}. Não há áudio recuperável; grave novamente."
            );
            monitor.record(FailureKind::Writer, Some(message.clone()));
            if error.is_none() {
                error = Some(capture_interrupted(message));
            }
        }
    }

    WriterOutcome {
        frames: samples / u64::from(channels.max(1)),
        finalized,
        error,
    }
}

/// Confirma que o provisório fechado é um WAV PCM16 decodificável com o número
/// de frames contado pelo escritor.
fn verify_closed_wav(path: &Path, spec: CaptureSpec, frames: u64) -> Result<(), AppError> {
    let reader = hound::WavReader::open(path).map_err(|error| {
        capture_interrupted(format!(
            "O ficheiro da gravação ficou ilegível ({error}) e não há áudio recuperável; grave novamente."
        ))
    })?;
    let read = reader.spec();
    let read_frames = u64::from(reader.duration());
    if read.channels != spec.channels
        || read.sample_rate != spec.sample_rate
        || read.bits_per_sample != 16
        || read_frames != frames
    {
        return Err(capture_interrupted(format!(
            "O ficheiro da gravação ficou incompleto ({read_frames} frames de {frames} esperados) e não há áudio recuperável; grave novamente."
        )));
    }
    Ok(())
}

/// Duração em milissegundos, arredondada ao milissegundo.
fn duration_ms(frames: u64, sample_rate: u32) -> u64 {
    let rate = u64::from(sample_rate.max(1));
    (frames * 1_000 + rate / 2) / rate
}

/// Remove um ficheiro provisório sem propagar o erro.
fn remove_partial(path: &Path) {
    let _ = fs::remove_file(path);
}

/// Captura ativa: o stream pertence à thread de áudio.
struct ActiveCapture {
    stream: cpal::Stream,
    pending: PendingCapture,
}

impl ActiveCapture {
    /// Pára o stream e fecha a captura, conservando o recuperável.
    fn finish(self) -> CaptureOutcome {
        let _ = self.stream.pause();
        drop(self.stream);
        self.pending.finish()
    }
}

/// Dispositivos de entrada, usados apenas pela thread de áudio.
pub struct Capture {
    active: Option<ActiveCapture>,
    recovered: Option<CapturedAudio>,
}

impl Default for Capture {
    fn default() -> Self {
        Self::new()
    }
}

impl Capture {
    pub fn new() -> Self {
        Self {
            active: None,
            recovered: None,
        }
    }

    /// Verifica se existe microfone e se o formato predefinido é aceite.
    pub fn availability(&self) -> ComponentState {
        let host = cpal::default_host();
        let Some(device) = host.default_input_device() else {
            return missing("Não foi encontrado nenhum microfone.");
        };
        match device.default_input_config() {
            Ok(supported) => match supported_format(supported.sample_format()) {
                Ok(_) => ComponentState::ready(),
                Err(error) => ComponentState {
                    status: ComponentStatus::Error,
                    message: Some(error.message),
                },
            },
            Err(error) => ComponentState {
                status: ComponentStatus::Error,
                message: Some(format!(
                    "Não foi possível usar o microfone predefinido: {error}."
                )),
            },
        }
    }

    /// Abre o stream de entrada e começa a escrever o original em PCM16.
    pub fn start(&mut self, target: RecordingTarget) -> Result<(), AppError> {
        self.recovered = None;
        if self.active.is_some() {
            return Err(AppError::busy("Já está uma gravação em curso."));
        }

        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| device_unavailable("Não foi encontrado nenhum microfone."))?;
        let supported = device.default_input_config().map_err(|error| {
            device_unavailable(format!(
                "Não foi possível obter a configuração do microfone: {error}."
            ))
        })?;
        let format = supported_format(supported.sample_format())?;
        let config = supported.config();
        let spec = CaptureSpec {
            sample_rate: config.sample_rate,
            channels: config.channels,
        };
        if spec.channels == 0 || spec.sample_rate == 0 {
            return Err(AppError::new(
                error_code::DEVICE_FORMAT_UNSUPPORTED,
                "O microfone indicou um formato inválido.",
                false,
            ));
        }

        let pending = PendingCapture::begin(target, spec)?;
        let channel = pending.channel().clone();
        let monitor = Arc::clone(&channel.monitor);
        let stream = match build_stream(&device, &config, format, channel, monitor) {
            Ok(stream) => stream,
            Err(error) => {
                let _ = pending.finish();
                return Err(device_unavailable(format!(
                    "Não foi possível abrir o microfone: {error}."
                )));
            }
        };
        if let Err(error) = stream.play() {
            let _ = pending.finish();
            return Err(device_unavailable(format!(
                "Não foi possível iniciar a captura do microfone: {error}."
            )));
        }

        self.active = Some(ActiveCapture { stream, pending });
        Ok(())
    }

    /// Interrupção detetada, com a captura já fechada.
    pub fn interruption(&mut self) -> Option<CaptureInterruption> {
        let active = self.active.as_ref()?;
        let failure = active.pending.channel().interruption()?;
        let active = self.active.take().expect("captura ativa");
        let outcome = active.finish();
        Some(CaptureInterruption {
            capture: outcome.capture,
            error: failure.error(),
        })
    }

    /// Fecha a captura em curso; num erro conserva o parcial recuperável.
    pub fn stop(&mut self) -> Result<CapturedAudio, AppError> {
        let Some(active) = self.active.take() else {
            return Err(AppError::new(
                error_code::CAPTURE_INTERRUPTED,
                "Não há nenhuma gravação em curso para terminar.",
                false,
            ));
        };
        let outcome = active.finish();
        match (outcome.error, outcome.capture) {
            (None, Some(capture)) => Ok(capture),
            (Some(error), capture) => {
                self.recovered = capture;
                Err(error)
            }
            (None, None) => Err(capture_interrupted(
                "A gravação ficou sem amostras e não há áudio para recuperar; grave novamente.",
            )),
        }
    }

    /// Áudio parcial de uma captura que terminou com erro.
    pub fn take_recovered(&mut self) -> Option<CapturedAudio> {
        self.recovered.take()
    }
}

/// Formatos de entrada aceites, com conversão para PCM16.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptureFormat {
    I16,
    U16,
    F32,
}

/// Aceita I16, U16 e F32; os restantes formatos são recusados sem pânico.
fn supported_format(format: CpalFormat) -> Result<CaptureFormat, AppError> {
    match format {
        CpalFormat::I16 => Ok(CaptureFormat::I16),
        CpalFormat::U16 => Ok(CaptureFormat::U16),
        CpalFormat::F32 => Ok(CaptureFormat::F32),
        other => Err(AppError::new(
            error_code::DEVICE_FORMAT_UNSUPPORTED,
            format!(
                "O microfone usa um formato de amostras não suportado ({other:?}); escolha outra entrada ou outro formato no sistema."
            ),
            false,
        )),
    }
}

/// Constrói o stream de entrada para o formato efetivo do dispositivo.
fn build_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    format: CaptureFormat,
    channel: CaptureChannel,
    monitor: Arc<CaptureMonitor>,
) -> Result<cpal::Stream, cpal::Error> {
    match format {
        CaptureFormat::I16 => build_typed::<i16>(device, config, channel, monitor),
        CaptureFormat::U16 => build_typed::<u16>(device, config, channel, monitor),
        CaptureFormat::F32 => build_typed::<f32>(device, config, channel, monitor),
    }
}

/// Constrói o stream para um tipo de amostra concreto.
fn build_typed<T: CaptureSample>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channel: CaptureChannel,
    monitor: Arc<CaptureMonitor>,
) -> Result<cpal::Stream, cpal::Error> {
    let channels = usize::from(config.channels.max(1));
    let callback_channel = channel.clone();
    let error_monitor = Arc::clone(&monitor);
    device.build_input_stream::<T, _, _>(
        *config,
        move |data: &[T], _info: &cpal::InputCallbackInfo| {
            if callback_channel.has_failed() {
                return;
            }
            let whole = data.len() - data.len() % channels;
            if whole == 0 {
                return;
            }
            let mut block = Vec::with_capacity(whole);
            block.extend(data[..whole].iter().map(|sample| sample.to_pcm16()));
            // Um canal cheio é reportado como interrupção; as amostras não são
            // descartadas em silêncio porque a falha fica registada e a thread
            // de áudio fecha a captura.
            let _ = callback_channel.push(block);
        },
        move |error| {
            error_monitor.record(
                FailureKind::Device,
                Some(format!(
                    "O dispositivo de entrada falhou: {error}. O que já estava gravado foi conservado."
                )),
            );
        },
        None,
    )
}

/// Amostra de entrada convertível para PCM16.
trait CaptureSample: cpal::SizedSample {
    fn to_pcm16(self) -> i16;
}

impl CaptureSample for i16 {
    fn to_pcm16(self) -> i16 {
        self
    }
}

impl CaptureSample for u16 {
    /// O formato U16 da CPAL é binário deslocado, com origem em 32768.
    fn to_pcm16(self) -> i16 {
        (self ^ 0x8000) as i16
    }
}

impl CaptureSample for f32 {
    fn to_pcm16(self) -> i16 {
        // `clamp` propaga NaN; a conversão saturante de Rust torna-o silêncio.
        let clamped = self.clamp(-1.0, 1.0);
        (clamped * f32::from(i16::MAX)).round() as i16
    }
}

/// Estado de um componente em falta.
fn missing(message: impl Into<String>) -> ComponentState {
    ComponentState {
        status: ComponentStatus::Missing,
        message: Some(message.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_unsigned_and_float_samples() {
        assert_eq!(0u16.to_pcm16(), i16::MIN);
        assert_eq!(32768u16.to_pcm16(), 0);
        assert_eq!(u16::MAX.to_pcm16(), i16::MAX);

        assert_eq!(0.0f32.to_pcm16(), 0);
        assert_eq!(1.0f32.to_pcm16(), i16::MAX);
        assert_eq!((-1.0f32).to_pcm16(), -i16::MAX);
        assert_eq!(2.0f32.to_pcm16(), i16::MAX);
        assert_eq!(f32::NAN.to_pcm16(), 0);
    }

    #[test]
    fn rejects_other_formats() {
        let error = supported_format(CpalFormat::I32).expect_err("I32 não é aceite");
        assert_eq!(error.code, error_code::DEVICE_FORMAT_UNSUPPORTED);
        assert_eq!(supported_format(CpalFormat::I16), Ok(CaptureFormat::I16));
        assert_eq!(supported_format(CpalFormat::U16), Ok(CaptureFormat::U16));
        assert_eq!(supported_format(CpalFormat::F32), Ok(CaptureFormat::F32));
    }
}

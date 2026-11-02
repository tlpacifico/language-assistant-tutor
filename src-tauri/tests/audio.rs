//! Testes da porta de áudio: captura, reprodução e normalização.
//!
//! Nenhum teste abre um microfone ou uma saída de áudio real. A captura é
//! exercitada pelo escritor WAV em streaming e pelo canal limitado do
//! callback; a coordenação da porta usa o adaptador controlado [`FakeAudio`];
//! os WAV de entrada são sintéticos, escritos com Hound.

use std::f32::consts::TAU;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use sha2::{Digest, Sha256};

use language_assistant_tutor::audio::capture::{
    channel, CaptureFailure, CaptureSpec, FailureKind, PendingCapture, PARTIAL_FILE,
};
use language_assistant_tutor::audio::normalize::normalize_wav;
use language_assistant_tutor::audio::{
    AudioBackend, AudioEvent, AudioPort, AvailabilityAudio, CaptureInterruption, CapturedAudio,
    NativeAudio, RecordingTarget,
};
use language_assistant_tutor::domain::{AppError, CaptureMeta, ComponentState, ComponentStatus};

// ---------------------------------------------------------------------------
// Utilitários
// ---------------------------------------------------------------------------

fn int_spec(sample_rate: u32, channels: u16) -> WavSpec {
    WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    }
}

fn write_wav(path: &Path, spec: WavSpec, samples: &[i16]) {
    let mut writer = WavWriter::create(path, spec).expect("criar WAV de teste");
    for sample in samples {
        writer.write_sample(*sample).expect("escrever amostra");
    }
    writer.finalize().expect("fechar WAV de teste");
}

fn read_wav(path: &Path) -> (WavSpec, Vec<i16>) {
    let mut reader = WavReader::open(path).expect("abrir WAV");
    let spec = reader.spec();
    let samples = reader
        .samples::<i16>()
        .collect::<Result<Vec<_>, _>>()
        .expect("ler amostras PCM16");
    (spec, samples)
}

fn sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn file_sha256(path: &Path) -> String {
    sha256(&fs::read(path).expect("ler ficheiro para hash"))
}

fn quantize(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16
}

/// Tom senoidal intercalado em `channels` canais iguais.
fn tone(sample_rate: u32, frequency: f32, seconds: f32, amplitude: f32) -> Vec<i16> {
    let frames = (sample_rate as f32 * seconds).round() as u32;
    (0..frames)
        .map(|frame| {
            let phase = TAU * frequency * frame as f32 / sample_rate as f32;
            quantize(amplitude * phase.sin())
        })
        .collect()
}

fn rms(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples
        .iter()
        .map(|sample| {
            let value = *sample as f64;
            value * value
        })
        .sum();
    (sum / samples.len() as f64).sqrt() as f32
}

fn peak(samples: &[i16]) -> i16 {
    samples.iter().map(|sample| sample.abs()).max().unwrap_or(0)
}

fn target(root: &Path, name: &str) -> RecordingTarget {
    let directory = root.join(name);
    fs::create_dir_all(&directory).expect("criar pasta da gravação");
    RecordingTarget {
        id: name.to_owned(),
        directory,
    }
}

fn spec_48k_mono() -> CaptureSpec {
    CaptureSpec {
        sample_rate: 48_000,
        channels: 1,
    }
}

/// Espera que o escritor confirme as amostras, sem dormir um tempo fixo.
fn wait_for_frames(pending: &PendingCapture, frames: u64) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while pending.written_frames() < frames {
        assert!(
            Instant::now() < deadline,
            "o escritor não confirmou {frames} frames (tem {})",
            pending.written_frames()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

// ---------------------------------------------------------------------------
// Adaptador controlado
// ---------------------------------------------------------------------------

/// Adaptador de áudio controlado pelos testes: registra a ordem das operações
/// e permite falhar a captura sem abrir dispositivos reais.
#[derive(Clone, Default)]
struct FakeAudio {
    steps: Arc<Mutex<Vec<String>>>,
    capturing: Option<RecordingTarget>,
    playing: Option<PathBuf>,
    /// Atraso artificial em `start_capture`, para exercitar o prazo.
    start_delay: Duration,
    /// Falha que a próxima verificação entrega como interrupção da captura.
    interruption: Arc<Mutex<Option<AppError>>>,
}

impl FakeAudio {
    fn record(&self, step: &str) {
        self.steps
            .lock()
            .expect("registar passo")
            .push(step.to_owned());
    }

    /// Faz a próxima verificação entregar uma interrupção sem áudio recuperável.
    fn interrupt_next(&self, error: AppError) {
        *self.interruption.lock().expect("falha programada") = Some(error);
    }

    fn steps(&self) -> Vec<String> {
        self.steps.lock().expect("ler passos").clone()
    }
}

impl AudioBackend for FakeAudio {
    fn availability(&mut self) -> AvailabilityAudio {
        self.record("availability");
        AvailabilityAudio {
            microphone: ComponentState::ready(),
            playback: ComponentState::ready(),
        }
    }

    fn start_capture(&mut self, target: RecordingTarget) -> Result<(), AppError> {
        self.record("start_capture");
        if !self.start_delay.is_zero() {
            std::thread::sleep(self.start_delay);
        }
        self.capturing = Some(target);
        Ok(())
    }

    fn capture_interruption(&mut self) -> Option<CaptureInterruption> {
        let error = self.interruption.lock().expect("falha programada").take()?;
        Some(CaptureInterruption {
            capture: None,
            error,
        })
    }

    fn stop_capture(&mut self) -> Result<CapturedAudio, AppError> {
        self.record("stop_capture");
        let target = self.capturing.take().expect("captura em curso");
        Ok(CapturedAudio {
            id: target.id,
            original: target.directory.join("original.wav"),
            meta: CaptureMeta {
                sample_rate: 48_000,
                channels: 1,
                duration_ms: 1_000,
                bytes: 96_000,
                incomplete: false,
            },
        })
    }

    fn play(&mut self, path: PathBuf) -> Result<(), AppError> {
        self.record("play");
        self.playing = Some(path);
        Ok(())
    }

    fn stop_playback(&mut self) -> Result<(), AppError> {
        self.record("stop_playback");
        self.playing = None;
        Ok(())
    }

    fn playback_finished(&mut self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Testes exigidos
// ---------------------------------------------------------------------------

#[test]
fn normalizes_stereo_48k_without_mutating_source() {
    let root = tempfile::tempdir().expect("pasta temporária");
    let source = root.path().join("original.wav");
    let destination = root.path().join("normalized.wav");

    let frames = 48_000u32;
    let mut stereo = Vec::with_capacity(frames as usize * 2);
    for frame in 0..frames {
        let time = frame as f32 / 48_000.0;
        stereo.push(quantize(0.5 * (TAU * 440.0 * time).sin()));
        stereo.push(quantize(0.25 * (TAU * 660.0 * time).sin()));
    }
    write_wav(&source, int_spec(48_000, 2), &stereo);

    let source_before = fs::read(&source).expect("ler original");
    let meta = normalize_wav(&source, &destination).expect("normalizar");
    let source_after = fs::read(&source).expect("reler original");

    let (output_spec, output) = read_wav(&destination);

    assert_eq!(
        (
            output_spec.sample_rate,
            output_spec.channels,
            output_spec.bits_per_sample
        ),
        (16000, 1, 16)
    );
    assert!(
        output.len().abs_diff(16000) <= 16,
        "frames {}",
        output.len()
    );
    assert_eq!(sha256(&source_before), sha256(&source_after));
    assert_eq!(source_before, source_after);

    // Média dos canais e não um deles: RMS de (L + R) / 2.
    // L = 0.5 sin, R = 0.25 sin em frequências diferentes.
    let expected_rms = ((0.25f32.powi(2) + 0.125f32.powi(2)) / 2.0).sqrt() * 32_767.0;
    assert!(
        (rms(&output) - expected_rms).abs() < 600.0,
        "RMS {} não corresponde à média dos canais ({expected_rms})",
        rms(&output)
    );

    assert_eq!(meta.sample_rate, 16_000);
    assert_eq!(meta.channels, 1);
    assert!(!meta.incomplete);
    assert_eq!(meta.bytes, fs::metadata(&destination).unwrap().len());
}

#[test]
fn empty_audio_is_invalid() {
    let root = tempfile::tempdir().expect("pasta temporária");
    let source = root.path().join("empty.wav");
    let destination = root.path().join("normalized.wav");
    write_wav(&source, int_spec(48_000, 1), &[]);

    let error = normalize_wav(&source, &destination).expect_err("áudio vazio é inválido");
    assert_eq!(error.code, "INVALID_AUDIO");
    assert!(!destination.exists(), "não pode publicar um derivado vazio");

    // Uma captura que termina sem amostras não produz áudio recuperável.
    let folder = target(root.path(), "vazia");
    let pending = PendingCapture::begin(folder.clone(), spec_48k_mono()).expect("iniciar captura");
    let outcome = pending.finish();
    assert!(outcome.capture.is_none());
    assert_eq!(
        outcome.error.expect("falha explícita").code,
        "CAPTURE_INTERRUPTED"
    );
    assert!(!folder.directory.join("original.wav").exists());
    let leftovers: Vec<_> = fs::read_dir(&folder.directory)
        .expect("ler pasta")
        .collect::<Result<Vec<_>, _>>()
        .expect("entradas")
        .into_iter()
        .map(|entry| entry.file_name())
        .collect();
    assert!(leftovers.is_empty(), "ficaram ficheiros: {leftovers:?}");
}

#[test]
fn capture_failure_preserves_previous() {
    let root = tempfile::tempdir().expect("pasta temporária");

    // Gravação anterior, completa.
    let first = target(root.path(), "primeira");
    let pending = PendingCapture::begin(first.clone(), spec_48k_mono()).expect("iniciar gravação");
    pending
        .channel()
        .push(vec![100i16; 480])
        .expect("entregar bloco");
    wait_for_frames(&pending, 480);
    let previous = pending
        .finish()
        .capture
        .expect("gravação anterior recuperável");
    assert!(!previous.meta.incomplete);
    let previous_hash = file_sha256(&previous.original);
    let previous_bytes = fs::read(&previous.original).expect("ler gravação anterior");

    // Gravação nova que falha a meio, com amostras já escritas.
    let second = target(root.path(), "segunda");
    let pending = PendingCapture::begin(second.clone(), spec_48k_mono()).expect("iniciar gravação");
    pending
        .channel()
        .push(vec![200i16; 480])
        .expect("entregar bloco");
    wait_for_frames(&pending, 480);
    pending
        .channel()
        .fail(CaptureFailure::device("O microfone foi desligado."));
    let outcome = pending.finish();

    let partial = outcome.capture.expect("captura parcial recuperável");
    let error = outcome.error.expect("falha explícita");
    assert_eq!(error.code, "CAPTURE_INTERRUPTED");
    assert!(partial.meta.incomplete);
    assert!(partial.meta.bytes > 44);
    let (partial_spec, partial_samples) = read_wav(&partial.original);
    assert_eq!(partial_spec.sample_rate, 48_000);
    assert_eq!(partial_spec.channels, 1);
    assert_eq!(partial_samples, vec![200i16; 480]);
    let published: Vec<_> = fs::read_dir(&second.directory)
        .expect("ler pasta")
        .collect::<Result<Vec<_>, _>>()
        .expect("entradas")
        .into_iter()
        .map(|entry| entry.file_name())
        .collect();
    assert_eq!(published.len(), 1, "ficou um temporário: {published:?}");

    // A gravação anterior mantém-se exatamente igual.
    assert_eq!(file_sha256(&previous.original), previous_hash);
    assert_eq!(fs::read(&previous.original).expect("reler"), previous_bytes);
}

#[test]
fn overflow_is_reported() {
    // O consumidor existe mas não lê: o canal fica cheio como com um escritor
    // atrasado. A amostra não pode ser descartada em silêncio.
    let (sender, _blocks) = channel(1);
    sender.push(vec![1i16; 480]).expect("primeiro bloco cabe");

    let overflow_error = sender.push(vec![2i16; 480]).expect_err("canal cheio");
    assert_eq!(overflow_error.code, "CAPTURE_INTERRUPTED");

    let failure = sender.interruption().expect("falha registada");
    assert_eq!(failure.kind(), FailureKind::ChannelFull);
    assert!(!failure.detail().is_empty());
}

#[test]
fn recording_stops_playback() {
    let root = tempfile::tempdir().expect("pasta temporária");
    let audio_file = root.path().join("resposta.wav");
    write_wav(
        &audio_file,
        int_spec(48_000, 1),
        &tone(48_000, 440.0, 0.1, 0.2),
    );

    let fake = FakeAudio::default();
    let probe = fake.clone();
    let audio = NativeAudio::with_backend(move || Box::new(fake.clone()), |_event: AudioEvent| {})
        .expect("criar porta de áudio");

    audio.play(audio_file).expect("reproduzir");
    let folder = target(root.path(), "gravacao");
    audio
        .start_capture(folder.clone())
        .expect("começar a gravar");

    assert_eq!(
        probe.steps(),
        vec!["play", "stop_playback", "start_capture"],
        "a gravação tem de parar a reprodução antes de abrir a entrada"
    );

    let captured = audio.stop_capture().expect("terminar gravação");
    assert_eq!(captured.id, folder.id);
    assert_eq!(
        probe.steps().last().map(String::as_str),
        Some("stop_capture")
    );
}

#[test]
fn interruption_event_carries_the_recording_id() {
    // Sem a identidade da gravação, uma interrupção atrasada de uma captura já
    // substituída seria aplicada à gravação nova.
    let root = tempfile::tempdir().expect("pasta temporária");
    let fake = FakeAudio::default();
    let factory = fake.clone();
    let events: Arc<Mutex<Vec<AudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    let audio = NativeAudio::with_backend(
        move || Box::new(factory.clone()),
        move |event: AudioEvent| sink.lock().expect("registar evento").push(event),
    )
    .expect("criar porta de áudio");

    let folder = target(root.path(), "gravacao");
    audio
        .start_capture(folder.clone())
        .expect("começar a gravar");
    fake.interrupt_next(AppError::new(
        "CAPTURE_INTERRUPTED",
        "falha de teste na captura",
        true,
    ));

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let received = events.lock().expect("eventos").first().cloned();
        if let Some(AudioEvent::CaptureInterrupted { recording_id, .. }) = received {
            assert_eq!(
                recording_id.as_deref(),
                Some(folder.id.as_str()),
                "o evento tem de identificar a gravação que o originou"
            );
            return;
        }
        assert!(
            Instant::now() < deadline,
            "a interrupção não chegou à porta"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn timed_out_command_fails_the_port_until_reopen() {
    let root = tempfile::tempdir().expect("pasta temporária");
    let fake = FakeAudio {
        start_delay: Duration::from_millis(200),
        ..FakeAudio::default()
    };
    let probe = fake.clone();
    let factory = fake.clone();
    let audio = NativeAudio::with_timeout(
        move || Box::new(factory.clone()),
        |_event: AudioEvent| {},
        Duration::from_millis(40),
    )
    .expect("criar porta de áudio");

    let folder = target(root.path(), "lenta");
    let error = audio
        .start_capture(folder)
        .expect_err("o comando de áudio excede o prazo");
    assert_eq!(
        error.code, "AUDIO_TIMEOUT",
        "código próprio, não DEVICE_UNAVAILABLE"
    );
    assert!(error.retryable);

    // A thread antiga pode concluir a operação depois do prazo; nenhum estado
    // novo é enviado para lá até a porta ser reaberta explicitamente.
    let steps_before = probe.steps();
    let refused = audio
        .stop_playback()
        .expect_err("a porta falhada recusa comandos");
    assert_eq!(refused.code, "AUDIO_TIMEOUT");
    assert_eq!(
        probe.steps(),
        steps_before,
        "um comando recusado não chega à thread antiga"
    );

    // Reabrir usa dispositivos novos e volta a aceitar comandos.
    audio.reopen().expect("reabrir a porta de áudio");
    let availability = audio.availability();
    assert_eq!(availability.microphone.status, ComponentStatus::Ready);
    assert!(
        probe.steps().len() > steps_before.len(),
        "a porta reaberta tem de falar com a thread nova"
    );
}

#[test]
fn playback_is_blocked_while_recording() {
    let root = tempfile::tempdir().expect("pasta temporária");
    let audio_file = root.path().join("resposta.wav");
    write_wav(
        &audio_file,
        int_spec(48_000, 1),
        &tone(48_000, 440.0, 0.1, 0.2),
    );

    let fake = FakeAudio::default();
    let probe = fake.clone();
    let audio = NativeAudio::with_backend(move || Box::new(fake.clone()), |_event: AudioEvent| {})
        .expect("criar porta de áudio");

    audio.play(audio_file.clone()).expect("reproduzir");
    let folder = target(root.path(), "gravacao");
    audio
        .start_capture(folder.clone())
        .expect("começar a gravar");

    let error = audio
        .play(audio_file.clone())
        .expect_err("reproduzir durante a gravação tem de ser recusado");
    assert_eq!(error.code, "BUSY");
    assert!(error.retryable);
    assert_eq!(
        probe.steps(),
        vec!["play", "stop_playback", "start_capture"],
        "a reprodução recusada não pode chegar ao dispositivo nem retomar a saída"
    );

    // A gravação continua válida e a recusa não a interrompe.
    let captured = audio.stop_capture().expect("terminar gravação");
    assert_eq!(captured.id, folder.id);

    // Terminada a captura, a saída volta a estar disponível.
    audio.play(audio_file).expect("reproduzir depois de gravar");
    assert_eq!(probe.steps().last().map(String::as_str), Some("play"));
}

/// Um WAV cujo cabeçalho não pôde ser fechado não é publicado como recuperável.
///
/// O bloqueio de bytes é obrigatório em Windows: a escrita do cabeçalho pelo
/// escritor falha mesmo com a captura a correr, sem depender de um disco cheio.
#[cfg(windows)]
#[test]
fn unfinalized_header_is_not_published() {
    use std::fs::OpenOptions;

    let root = tempfile::tempdir().expect("pasta temporária");
    let folder = target(root.path(), "cabecalho");
    let pending = PendingCapture::begin(folder.clone(), spec_48k_mono()).expect("iniciar captura");
    let partial = folder.directory.join(PARTIAL_FILE);

    // Bloco pequeno: fica no buffer do escritor, pelo que só o fecho do
    // cabeçalho toca no disco bloqueado.
    pending
        .channel()
        .push(vec![50i16; 480])
        .expect("entregar bloco");
    wait_for_frames(&pending, 480);

    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&partial)
        .expect("abrir o provisório para o bloquear");
    lock.lock().expect("bloquear o provisório");

    let outcome = pending.finish();
    drop(lock);

    assert!(
        outcome.capture.is_none(),
        "um WAV sem cabeçalho válido não pode ser apresentado como recuperável"
    );
    let error = outcome.error.expect("falha explícita");
    assert_eq!(error.code, "CAPTURE_INTERRUPTED");
    assert!(
        error.message.contains("fechar o ficheiro WAV"),
        "a falha forçada tem de ser o fecho do cabeçalho: {}",
        error.message
    );
    assert!(!folder.directory.join("original.wav").exists());
    let leftovers: Vec<_> = fs::read_dir(&folder.directory)
        .expect("ler pasta")
        .collect::<Result<Vec<_>, _>>()
        .expect("entradas")
        .into_iter()
        .map(|entry| entry.file_name())
        .collect();
    assert!(leftovers.is_empty(), "ficaram ficheiros: {leftovers:?}");
}

// ---------------------------------------------------------------------------
// Normalização: duração e filtro anti-aliasing
// ---------------------------------------------------------------------------

#[test]
fn normalizes_44k1_duration_within_tolerance() {
    let root = tempfile::tempdir().expect("pasta temporária");
    let source = root.path().join("original.wav");
    let destination = root.path().join("normalized.wav");
    write_wav(
        &source,
        int_spec(44_100, 1),
        &tone(44_100, 1_000.0, 1.0, 0.5),
    );

    normalize_wav(&source, &destination).expect("normalizar");

    let (spec, samples) = read_wav(&destination);
    assert_eq!((spec.sample_rate, spec.channels), (16_000, 1));
    assert!(
        samples.len().abs_diff(16_000) <= 44,
        "1 s a 44,1 kHz devia dar 16000 frames, deu {}",
        samples.len()
    );
}

#[test]
fn normalize_attenuates_above_8khz() {
    let root = tempfile::tempdir().expect("pasta temporária");

    // 12 kHz a 48 kHz cairia em 4 kHz se as amostras fossem simplesmente
    // descartadas; o filtro tem de atenuar.
    let high_source = root.path().join("high.wav");
    let high_target = root.path().join("high-normalized.wav");
    write_wav(
        &high_source,
        int_spec(48_000, 1),
        &tone(48_000, 12_000.0, 0.25, 0.5),
    );
    normalize_wav(&high_source, &high_target).expect("normalizar 12 kHz");
    let (_, high_output) = read_wav(&high_target);

    // Controlo: um tom dentro da banda passa praticamente intacto.
    let low_source = root.path().join("low.wav");
    let low_target = root.path().join("low-normalized.wav");
    write_wav(
        &low_source,
        int_spec(48_000, 1),
        &tone(48_000, 1_000.0, 0.25, 0.5),
    );
    normalize_wav(&low_source, &low_target).expect("normalizar 1 kHz");
    let (_, low_output) = read_wav(&low_target);

    let input_rms = rms(&tone(48_000, 1_000.0, 0.25, 0.5));
    assert!(
        rms(&low_output) > input_rms * 0.9,
        "1 kHz devia passar: RMS {} contra {input_rms}",
        rms(&low_output)
    );
    assert!(
        rms(&high_output) <= input_rms * 0.05,
        "12 kHz devia ser atenuado: RMS {} contra {input_rms} (pico {})",
        rms(&high_output),
        peak(&high_output)
    );
}

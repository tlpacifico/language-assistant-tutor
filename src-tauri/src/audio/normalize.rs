//! Normalização para WAV PCM16 mono a 16 kHz.
//!
//! O original nunca é modificado: o derivado é escrito num ficheiro provisório
//! na pasta de destino e só depois substitui o destino. O sinal é primeiro
//! reduzido a mono pela média dos canais e só então reamostrado com Rubato
//! (FFT), que compensa o atraso do filtro e processa o último bloco
//! incompleto — a resposta não é cortada nem convertida por simples descarte
//! de amostras.
//!
//! Códigos de erro: `INVALID_AUDIO` para um original ilegível, vazio ou com um
//! formato de amostras não suportado; `STORAGE_WRITE_FAILED` quando o derivado
//! não pode ser escrito.

use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use hound::{SampleFormat as WavFormat, WavReader, WavSpec, WavWriter};
use rubato::audioadapter_buffers::owned::InterleavedOwned;
use rubato::{Fft, FixedSync, Resampler};

use crate::domain::{error_code, AppError, CaptureMeta};

/// Taxa do derivado usado na transcrição.
pub const NORMALIZED_SAMPLE_RATE: u32 = 16_000;

/// Bloco de referência do reamostrador FFT.
const RESAMPLER_CHUNK: usize = 1_024;

/// Normaliza `source` para `destination`, sem modificar o original.
///
/// Devolve os metadados do ficheiro escrito: 16 kHz, mono, PCM16.
pub fn normalize_wav(source: &Path, destination: &Path) -> Result<CaptureMeta, AppError> {
    if is_same_path(source, destination) {
        return Err(invalid_audio(
            "Não é possível normalizar uma gravação sobre o próprio original.",
        ));
    }

    let mut reader = WavReader::open(source).map_err(|error| {
        invalid_audio(format!(
            "Não foi possível ler o áudio original em {}: {error}.",
            source.display()
        ))
    })?;
    let spec = reader.spec();
    let mono = read_mono(&mut reader, &spec)?;
    if mono.is_empty() {
        return Err(invalid_audio(
            "A gravação não contém amostras de áudio e não pode ser transcrita.",
        ));
    }

    let samples = if spec.sample_rate == NORMALIZED_SAMPLE_RATE {
        mono
    } else {
        resample(&mono, spec.sample_rate)?
    };
    if samples.is_empty() {
        return Err(invalid_audio("A conversão do áudio não produziu amostras."));
    }

    let bytes = write_normalized(destination, &samples)?;
    let frames = samples.len() as u64;
    let rate = u64::from(NORMALIZED_SAMPLE_RATE);
    Ok(CaptureMeta {
        sample_rate: NORMALIZED_SAMPLE_RATE,
        channels: 1,
        duration_ms: (frames * 1_000 + rate / 2) / rate,
        bytes,
        incomplete: false,
    })
}

/// Lê todas as amostras e reduz a mono pela média dos canais.
fn read_mono(
    reader: &mut WavReader<impl std::io::Read>,
    spec: &WavSpec,
) -> Result<Vec<f32>, AppError> {
    let channels = usize::from(spec.channels);
    if channels == 0 || spec.sample_rate == 0 {
        return Err(invalid_audio(
            "O áudio original tem um formato inválido (canais ou taxa a zero).",
        ));
    }

    let interleaved: Vec<f32> = match (spec.sample_format, spec.bits_per_sample) {
        (WavFormat::Int, 8) => collect_samples(reader, |sample: i8| f32::from(sample) / 128.0)?,
        (WavFormat::Int, 16) => {
            collect_samples(reader, |sample: i16| f32::from(sample) / 32_768.0)?
        }
        (WavFormat::Int, 24) => collect_samples(reader, |sample: i32| sample as f32 / 8_388_608.0)?,
        (WavFormat::Int, 32) => {
            collect_samples(reader, |sample: i32| sample as f32 / 2_147_483_648.0)?
        }
        (WavFormat::Float, 32) => collect_samples(reader, |sample: f32| sample)?,
        _ => {
            return Err(invalid_audio(format!(
                "O áudio original usa um formato de amostras não suportado ({} bits, {:?}).",
                spec.bits_per_sample, spec.sample_format
            )))
        }
    };

    let frames = interleaved.len() / channels;
    let mut mono = Vec::with_capacity(frames);
    for frame in 0..frames {
        let mut sum = 0.0f32;
        for channel in 0..channels {
            sum += interleaved[frame * channels + channel];
        }
        mono.push(sum / channels as f32);
    }
    Ok(mono)
}

/// Lê todas as amostras do formato pedido, traduzindo falhas de leitura.
fn collect_samples<S, F>(
    reader: &mut WavReader<impl std::io::Read>,
    convert: F,
) -> Result<Vec<f32>, AppError>
where
    S: hound::Sample,
    F: Fn(S) -> f32,
{
    let mut samples = Vec::new();
    for sample in reader.samples::<S>() {
        let sample = sample.map_err(|error| {
            invalid_audio(format!(
                "O áudio original está truncado ou ilegível: {error}."
            ))
        })?;
        samples.push(convert(sample));
    }
    Ok(samples)
}

/// Reamostra mono para 16 kHz compensando atraso e último bloco.
fn resample(mono: &[f32], sample_rate: u32) -> Result<Vec<f32>, AppError> {
    let frames = mono.len();
    let input = InterleavedOwned::new_from(mono.to_vec(), 1, frames).map_err(|error| {
        invalid_audio(format!(
            "Não foi possível preparar o áudio para conversão: {error}."
        ))
    })?;
    let mut resampler = Fft::<f32>::new(
        sample_rate as usize,
        NORMALIZED_SAMPLE_RATE as usize,
        RESAMPLER_CHUNK,
        1,
        FixedSync::Both,
    )
    .map_err(|error| {
        invalid_audio(format!(
            "Não foi possível preparar a conversão de {sample_rate} Hz para 16 kHz: {error}."
        ))
    })?;
    let output = resampler
        .process_all(&input, frames, None)
        .map_err(|error| {
            invalid_audio(format!(
                "Não foi possível converter o áudio para 16 kHz: {error}."
            ))
        })?;
    Ok(output.take_data())
}

/// Escreve o derivado num provisório e só depois o publica no destino.
fn write_normalized(destination: &Path, samples: &[f32]) -> Result<u64, AppError> {
    let partial = partial_path(destination)?;
    write_samples(&partial, samples).inspect_err(|_| {
        let _ = fs::remove_file(&partial);
    })?;
    fs::rename(&partial, destination).map_err(|error| {
        let _ = fs::remove_file(&partial);
        storage_failure(format!(
            "Não foi possível publicar o áudio normalizado em {}: {error}.",
            destination.display()
        ))
    })?;
    Ok(fs::metadata(destination)
        .map(|meta| meta.len())
        .unwrap_or(0))
}

/// Escreve PCM16 mono a 16 kHz no ficheiro indicado.
fn write_samples(path: &Path, samples: &[f32]) -> Result<(), AppError> {
    let spec = WavSpec {
        channels: 1,
        sample_rate: NORMALIZED_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: WavFormat::Int,
    };
    let file = File::create(path).map_err(|error| {
        storage_failure(format!(
            "Não foi possível criar o áudio normalizado em {}: {error}.",
            path.display()
        ))
    })?;
    let mut writer = WavWriter::new(BufWriter::new(file), spec).map_err(|error| {
        storage_failure(format!(
            "Não foi possível preparar o ficheiro normalizado: {error}."
        ))
    })?;
    for sample in samples {
        writer.write_sample(quantize(*sample)).map_err(|error| {
            storage_failure(format!(
                "Não foi possível escrever o áudio normalizado: {error}."
            ))
        })?;
    }
    writer.finalize().map_err(|error| {
        storage_failure(format!(
            "Não foi possível fechar o áudio normalizado: {error}."
        ))
    })
}

/// Caminho do provisório ao lado do destino, para a substituição ser atómica.
fn partial_path(destination: &Path) -> Result<PathBuf, AppError> {
    let name = destination.file_name().ok_or_else(|| {
        storage_failure("O destino do áudio normalizado não tem nome de ficheiro.")
    })?;
    let mut partial = name.to_owned();
    partial.push(".partial");
    Ok(destination.with_file_name(partial))
}

/// Duas referências ao mesmo ficheiro, mesmo antes de o destino existir.
fn is_same_path(source: &Path, destination: &Path) -> bool {
    let source = fs::canonicalize(source).unwrap_or_else(|_| source.to_path_buf());
    let destination = fs::canonicalize(destination).unwrap_or_else(|_| destination.to_path_buf());
    source == destination
}

/// Converte uma amostra de [-1, 1] para PCM16, com saturação e silêncio em NaN.
fn quantize(sample: f32) -> i16 {
    let clamped = sample.clamp(-1.0, 1.0);
    (clamped * f32::from(i16::MAX)).round() as i16
}

/// O áudio original não serve para transcrição.
fn invalid_audio(message: impl Into<String>) -> AppError {
    AppError::new(error_code::INVALID_AUDIO, message, false)
}

/// Falha ao escrever o derivado.
fn storage_failure(message: impl Into<String>) -> AppError {
    AppError::new(error_code::STORAGE_WRITE_FAILED, message, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantizes_with_saturation() {
        assert_eq!(quantize(0.0), 0);
        assert_eq!(quantize(1.0), i16::MAX);
        assert_eq!(quantize(-1.0), -i16::MAX);
        assert_eq!(quantize(4.0), i16::MAX);
        assert_eq!(quantize(f32::NAN), 0);
    }
}

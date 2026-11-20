//! Cache local de voz do tutor, por identidade exata de texto, voz, velocidade
//! e motor.
//!
//! Uma entrada é uma pasta `voice-cache/<key>/` com os blocos `chunk-NNNN.wav`
//! e um `manifest.json` escrito por substituição atómica. Só um manifesto com
//! `complete: true`, cuja chave coincida com a pasta e cujos blocos existam e
//! sejam WAV legíveis, é uma entrada reutilizável; uma saída parcial nunca é
//! tratada como uma leitura válida e é removida antes de regenerar.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use atomicwrites::{AllowOverwrite, AtomicFile};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::domain::{error_code, AppError};

/// Identidade do motor de síntese; entra na chave para que uma troca de motor
/// nunca reaproveite áudio gerado por outro.
pub const ENGINE_ID: &str = "kokoro-82m-v1";

/// Nome do manifesto dentro de cada entrada.
pub const MANIFEST_FILE: &str = "manifest.json";

/// Versão do esquema do manifesto.
pub const MANIFEST_VERSION: u32 = 1;

fn invalid(message: impl Into<String>) -> AppError {
    AppError::new(error_code::INVALID_INPUT, message, false)
}

fn storage_failure(message: impl Into<String>) -> AppError {
    AppError::new(error_code::STORAGE_WRITE_FAILED, message, true)
}

fn invalid_audio(message: impl Into<String>) -> AppError {
    AppError::new(error_code::INVALID_AUDIO, message, false)
}

/// Representação hexadecimal minúscula de um resumo SHA-256.
fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Nome do ficheiro de um bloco, como o auxiliar o escreve.
fn chunk_file(index: u32) -> String {
    format!("chunk-{index:04}.wav")
}

/// Duração em milissegundos, arredondada ao milissegundo.
fn duration_ms(frames: u64, sample_rate: u32) -> u64 {
    let rate = u64::from(sample_rate.max(1));
    (frames * 1_000 + rate / 2) / rate
}

/// Chave SHA-256 (64 hex minúsculos) da identidade exata de uma síntese:
/// texto + NUL + voz + NUL + velocidade em décimos + NUL + motor.
pub fn voice_key(text: &str, voice: &str, speed_x10: u8, engine: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    hasher.update([0u8]);
    hasher.update(voice.as_bytes());
    hasher.update([0u8]);
    hasher.update(speed_x10.to_string().as_bytes());
    hasher.update([0u8]);
    hasher.update(engine.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Uma chave válida tem exatamente 64 caracteres hexadecimais minúsculos.
fn validate_key(key: &str) -> Result<(), AppError> {
    if key.len() != 64 || !key.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return Err(invalid("A chave da cache de voz é inválida."));
    }
    Ok(())
}

/// Diretório de uma entrada; a chave válida nunca contém separadores, por isso
/// o caminho fica sempre sob `<session>/voice-cache/`.
fn entry_dir(session_dir: &Path, key: &str) -> Result<PathBuf, AppError> {
    validate_key(key)?;
    Ok(session_dir.join("voice-cache").join(key))
}

/// Um bloco completo de uma entrada, com o caminho resolvido.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedChunk {
    pub index: u32,
    pub duration_ms: u64,
    pub path: PathBuf,
}

/// Entrada de cache reutilizável: manifesto completo e blocos legíveis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedVoice {
    pub key: String,
    pub voice: String,
    pub speed_x10: u8,
    pub engine: String,
    pub text_sha256: String,
    pub chunks: Vec<CachedChunk>,
}

impl CachedVoice {
    /// Duração total em milissegundos, pela soma dos blocos do manifesto.
    pub fn total_duration_ms(&self) -> u64 {
        self.chunks.iter().map(|chunk| chunk.duration_ms).sum()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChunkEntry {
    index: u32,
    duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CacheManifest {
    version: u32,
    key: String,
    voice: String,
    speed_x10: u8,
    engine: String,
    text_sha256: String,
    complete: bool,
    chunks: Vec<ChunkEntry>,
}

/// Reserva (ou limpa) a pasta de uma entrada e devolve-a. Uma entrada anterior
/// incompleta é removida, para que uma repetição comece do início.
pub fn begin_entry(session_dir: &Path, key: &str) -> Result<PathBuf, AppError> {
    let dir = entry_dir(session_dir, key)?;
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|error| {
            storage_failure(format!(
                "Não foi possível limpar a entrada anterior da cache de voz: {error}."
            ))
        })?;
    }
    fs::create_dir_all(&dir).map_err(|error| {
        storage_failure(format!(
            "Não foi possível preparar a pasta da cache de voz: {error}."
        ))
    })?;
    Ok(dir)
}

/// Duração real de um bloco, lida do WAV; um bloco em falta ou ilegível é um
/// erro, para que a entrada nunca seja apresentada como completa.
fn chunk_duration(path: &Path, index: u32) -> Result<u64, AppError> {
    if !path.is_file() {
        return Err(storage_failure(format!(
            "O bloco {index} de voz está em falta."
        )));
    }
    let mut reader = hound::WavReader::open(path).map_err(|error| {
        invalid_audio(format!(
            "O bloco {index} de voz não é um WAV legível: {error}."
        ))
    })?;
    let spec = reader.spec();
    if spec.channels == 0 || spec.sample_rate == 0 {
        return Err(invalid_audio(format!(
            "O bloco {index} de voz tem um formato inválido (canais ou taxa a zero)."
        )));
    }
    let frames = reader.duration();
    if frames == 0 {
        return Err(invalid_audio(format!(
            "O bloco {index} de voz não contém amostras."
        )));
    }
    // Seeking beyond EOF succeeds on a file. Read the last declared frame to
    // distinguish a complete payload from a truncated file with a valid header.
    let last_frame = reader.seek(frames - 1).is_ok()
        && match spec.sample_format {
            hound::SampleFormat::Int => reader.samples::<i32>().all(|sample| sample.is_ok()),
            hound::SampleFormat::Float => reader.samples::<f32>().all(|sample| sample.is_ok()),
        };
    if !last_frame {
        return Err(invalid_audio(format!(
            "O bloco {index} de voz está incompleto."
        )));
    }
    Ok(duration_ms(u64::from(frames), spec.sample_rate))
}

/// Conclui uma entrada: mede a duração real de cada bloco, exige que todos
/// existam e sejam legíveis, e escreve o manifesto completo por substituição
/// atómica. Qualquer bloco inválido devolve erro sem deixar um manifesto
/// completo. A duração reportada em `chunks` é ignorada; vale a medida do WAV.
pub fn finish_entry(
    dir: &Path,
    text: &str,
    voice: &str,
    speed_x10: u8,
    engine: &str,
    chunks: &[(u32, u64)],
) -> Result<(), AppError> {
    let key = dir
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid("A pasta da cache de voz não identifica uma entrada."))?;
    validate_key(key)?;
    if chunks.is_empty() {
        return Err(invalid("A síntese não produziu blocos de voz."));
    }
    let mut entries = Vec::with_capacity(chunks.len());
    for (expected_index, &(index, _reported_ms)) in chunks.iter().enumerate() {
        if index as usize != expected_index {
            return Err(invalid("Os blocos de voz não estão em sequência."));
        }
        let path = dir.join(chunk_file(index));
        let duration_ms = chunk_duration(&path, index)?;
        entries.push(ChunkEntry { index, duration_ms });
    }
    let manifest = CacheManifest {
        version: MANIFEST_VERSION,
        key: key.to_owned(),
        voice: voice.to_owned(),
        speed_x10,
        engine: engine.to_owned(),
        text_sha256: sha256_hex(text.as_bytes()),
        complete: true,
        chunks: entries,
    };
    write_manifest(dir, &manifest)
}

/// Lê uma entrada completa, ou `None` se não for reutilizável. Nunca apaga nada.
pub fn read_cache(session_dir: &Path, key: &str) -> Option<CachedVoice> {
    let dir = entry_dir(session_dir, key).ok()?;
    let manifest = read_manifest(&dir)?;
    if manifest.version != MANIFEST_VERSION
        || manifest.key != key
        || !manifest.complete
        || manifest.chunks.is_empty()
    {
        return None;
    }
    let mut chunks = Vec::with_capacity(manifest.chunks.len());
    for (expected_index, entry) in manifest.chunks.iter().enumerate() {
        let path = dir.join(chunk_file(entry.index));
        if entry.index as usize != expected_index
            || chunk_duration(&path, entry.index).ok()? != entry.duration_ms
        {
            return None;
        }
        chunks.push(CachedChunk {
            index: entry.index,
            duration_ms: entry.duration_ms,
            path,
        });
    }
    Some(CachedVoice {
        key: key.to_owned(),
        voice: manifest.voice,
        speed_x10: manifest.speed_x10,
        engine: manifest.engine,
        text_sha256: manifest.text_sha256,
        chunks,
    })
}

/// Remove uma entrada por chave; uma entrada ausente é um sucesso.
pub fn clear_entry(session_dir: &Path, key: &str) -> Result<(), AppError> {
    let dir = entry_dir(session_dir, key)?;
    match fs::remove_dir_all(&dir) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(storage_failure(format!(
            "Não foi possível remover a entrada da cache de voz: {error}."
        ))),
    }
}

/// Remove as subpastas de `voice-cache/` sem um manifesto completo válido.
/// Nunca apaga ficheiros nem nada fora dessa pasta.
pub fn remove_incomplete(session_dir: &Path) -> Result<(), AppError> {
    let root = session_dir.join("voice-cache");
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(storage_failure(format!(
                "Não foi possível ler a cache de voz: {error}."
            )))
        }
    };
    for entry in entries {
        let entry = entry.map_err(|error| {
            storage_failure(format!("Não foi possível ler a cache de voz: {error}."))
        })?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if read_cache(session_dir, name).is_none() {
            let _ = fs::remove_dir_all(&path);
        }
    }
    Ok(())
}

fn read_manifest(dir: &Path) -> Option<CacheManifest> {
    let raw = fs::read_to_string(dir.join(MANIFEST_FILE)).ok()?;
    serde_json::from_str(&raw).ok()
}

fn write_manifest(dir: &Path, manifest: &CacheManifest) -> Result<(), AppError> {
    let path = dir.join(MANIFEST_FILE);
    let bytes = serde_json::to_vec_pretty(manifest).map_err(|error| {
        storage_failure(format!(
            "Não foi possível preparar o manifesto da cache de voz: {error}."
        ))
    })?;
    AtomicFile::new(&path, AllowOverwrite)
        .write(|file| {
            file.write_all(&bytes)?;
            file.flush()
        })
        .map_err(|error| {
            storage_failure(format!(
                "Não foi possível guardar o manifesto da cache de voz: {error}."
            ))
        })
}

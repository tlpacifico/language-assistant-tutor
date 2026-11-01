//! Artefactos locais da fase 1: JSON atómico, publicação e recuperação.
//!
//! Sem SQLite, a raiz recebida por [`AttemptStore::new`] guarda tudo o que a
//! aplicação precisa de relembrar:
//!
//! ```text
//! phase1/
//!   last-attempt.json                      # {schemaVersion: 1, attemptId}
//!   attempts/<attempt-id>/
//!     original.wav
//!     normalized.wav                       # derivado da tarefa 3
//!     attempt.json                         # metadados; não duplica textos
//!     transcripts/<transcript-id>.json     # transcrição original imutável
//!     review.json                          # revisão guardada
//! ```
//!
//! Regras aplicadas aqui:
//!
//! - `attempt.json` é um manifesto ([`AttemptManifest`]) com os campos da
//!   resposta exceto `transcripts` e `review`; esses textos vivem apenas nos
//!   ficheiros próprios e são relidos para reconstruir a resposta completa.
//! - Todo o JSON é substituído por `atomicwrites`, que escreve um temporário na
//!   mesma pasta e o move para o destino. Nunca se remove o destino antes de o
//!   substituir, também em Windows.
//! - Os metadados são escritos antes de `last-attempt.json`; a resposta nova só
//!   fica visível depois de o apontador ser atualizado. Se essa atualização
//!   falhar, o apontador anterior mantém-se e a gravação nova continua na pasta
//!   para repetir a publicação.
//! - `restore_last` não escreve nem apaga nada: recupera o que é válido, avisa
//!   por cada artefacto em falta e recalcula as operações interrompidas.
//!
//! Os códigos de erro são os estáveis de [`crate::domain::error_code`]:
//! `STORAGE_WRITE_FAILED` cobre qualquer falha de leitura, escrita ou
//! substituição do próprio armazenamento (é o único código de armazenamento
//! disponível e o mais próximo do que a interface mostra); `ARTIFACT_MISSING`
//! identifica um áudio, transcrição ou revisão em falta ou ilegível, que não
//! impede recuperar o resto; `INVALID_AUDIO` recusa um áudio vazio.

use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Component, Path, PathBuf};

use atomicwrites::{AllowOverwrite, AtomicFile};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::audio::{CapturedAudio, RecordingTarget};
use crate::domain::{
    error_code, AppError, Attempt, CaptureMeta, OperationRecord, OperationState, RestoreResult,
    Review, Transcript, TranscriptStatus, QUESTION,
};

/// Versão do manifesto e do apontador desta fase.
pub const SCHEMA_VERSION: u32 = 1;

/// Nome do áudio original dentro da pasta da resposta.
pub const ORIGINAL_FILE: &str = "original.wav";

/// Nome do derivado normalizado (WAV PCM16 mono 16 kHz) dentro da pasta.
pub const NORMALIZED_FILE: &str = "normalized.wav";

const ATTEMPTS_DIR: &str = "attempts";
const LAST_ATTEMPT_FILE: &str = "last-attempt.json";
const MANIFEST_FILE: &str = "attempt.json";
const TRANSCRIPTS_DIR: &str = "transcripts";
const REVIEW_FILE: &str = "review.json";

/// Quantas pastas se tentam antes de desistir de um identificador repetido.
const ALLOCATE_ATTEMPTS: usize = 8;

/// Manifesto guardado em `attempt.json`.
///
/// Tem os campos de [`Attempt`] exceto `transcripts` e `review`. As referências
/// de ficheiro são relativas à pasta da resposta, para a pasta poder ser movida
/// sem invalidar os metadados.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AttemptManifest {
    schema_version: u32,
    id: String,
    question: String,
    created_at: String,
    capture: CaptureMeta,
    original_file: String,
    normalized_file: Option<String>,
    last_operation: Option<OperationRecord>,
}

impl AttemptManifest {
    fn into_attempt(self, transcripts: Vec<Transcript>, review: Option<Review>) -> Attempt {
        let last_operation = heal_operation(&self.last_operation, &transcripts);
        Attempt {
            id: self.id,
            question: self.question,
            created_at: self.created_at,
            capture: self.capture,
            original_file: self.original_file,
            normalized_file: self.normalized_file,
            transcripts,
            review,
            last_operation,
        }
    }
}

/// Apontador para a resposta ativa.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LastAttempt {
    schema_version: u32,
    attempt_id: String,
}

/// Resposta reconstruída, com os avisos encontrados na leitura.
struct LoadedAttempt {
    attempt: Attempt,
    warnings: Vec<AppError>,
}

/// Armazenamento das respostas locais.
///
/// O tipo só guarda a raiz: pode ser partilhado entre comandos Tauri e não
/// mantém nenhum descritor aberto.
#[derive(Debug, Clone)]
pub struct AttemptStore {
    root: PathBuf,
}

impl AttemptStore {
    /// Cria o armazenamento sobre uma raiz (a aplicação usa `app_data_dir()/phase1`).
    ///
    /// Nada é escrito antes da primeira gravação; a raiz é criada quando é
    /// precisa.
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Reserva uma pasta e um identificador para uma gravação nova.
    ///
    /// `fs::create_dir` (e não `create_dir_all`) garante que nenhum
    /// identificador reaproveita uma pasta existente.
    pub fn allocate(&self) -> Result<RecordingTarget, AppError> {
        let attempts = self.root.join(ATTEMPTS_DIR);
        fs::create_dir_all(&attempts).map_err(|error| {
            storage_failure(format!(
                "Não foi possível preparar a pasta das respostas: {error}."
            ))
        })?;

        for _ in 0..ALLOCATE_ATTEMPTS {
            let id = uuid::Uuid::new_v4().to_string();
            let directory = attempts.join(&id);
            match fs::create_dir(&directory) {
                Ok(()) => return Ok(RecordingTarget { id, directory }),
                Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(storage_failure(format!(
                        "Não foi possível reservar a pasta da gravação: {error}."
                    )))
                }
            }
        }

        Err(storage_failure(
            "Não foi possível reservar uma pasta única para a gravação.",
        ))
    }

    /// Publica uma gravação finalizada e torna-a na resposta ativa.
    ///
    /// Escreve `attempt.json` primeiro e só depois `last-attempt.json`. Uma
    /// falha na segunda escrita devolve erro e deixa a resposta anterior ativa,
    /// com os ficheiros da nova gravação conservados para repetir a publicação.
    pub fn publish(&self, capture: CapturedAudio) -> Result<Attempt, AppError> {
        let CapturedAudio { id, original, meta } = capture;
        let directory = self.attempt_directory(&id)?;
        fs::create_dir_all(&directory).map_err(|error| {
            storage_failure(format!(
                "Não foi possível criar a pasta da resposta: {error}."
            ))
        })?;
        store_original(&original, &directory.join(ORIGINAL_FILE))?;

        // Repetir a publicação do mesmo identificador conserva o que já existe.
        let previous = self.read_manifest(&directory).ok();
        let created_at = previous
            .as_ref()
            .map(|manifest| manifest.created_at.clone())
            .unwrap_or_else(utc_now);
        let manifest = AttemptManifest {
            schema_version: SCHEMA_VERSION,
            id,
            question: QUESTION.to_owned(),
            created_at,
            capture: meta,
            original_file: ORIGINAL_FILE.to_owned(),
            normalized_file: previous.as_ref().and_then(|m| m.normalized_file.clone()),
            last_operation: previous.and_then(|m| m.last_operation),
        };

        write_json(&directory.join(MANIFEST_FILE), &manifest)?;
        write_json(
            &self.root.join(LAST_ATTEMPT_FILE),
            &LastAttempt {
                schema_version: SCHEMA_VERSION,
                attempt_id: manifest.id.clone(),
            },
        )?;

        Ok(self.load(&manifest)?.attempt)
    }

    /// Recupera a última resposta publicada.
    ///
    /// Não escreve nem apaga ficheiros. Falta de áudio ou de revisão gera um
    /// aviso e o resto dos dados é devolvido; um apontador ou manifesto
    /// ilegível devolve erro recuperável, sem eliminar nada e sem impedir uma
    /// gravação nova.
    pub fn restore_last(&self) -> Result<RestoreResult, AppError> {
        let pointer = match read_text(&self.root.join(LAST_ATTEMPT_FILE))? {
            Some(raw) => raw,
            None => {
                return Ok(RestoreResult {
                    attempt: None,
                    warnings: Vec::new(),
                })
            }
        };
        let pointer: LastAttempt = parse_json(&pointer, LAST_ATTEMPT_FILE)?;
        if pointer.schema_version != SCHEMA_VERSION {
            return Err(storage_failure(format!(
                "O apontador da última resposta usa uma versão não suportada ({}).",
                pointer.schema_version
            )));
        }

        let manifest = self.read_manifest(&self.attempt_directory(&pointer.attempt_id)?)?;
        let loaded = self.load(&manifest)?;
        Ok(RestoreResult {
            attempt: Some(loaded.attempt),
            warnings: loaded.warnings,
        })
    }

    /// Guarda uma transcrição original e devolve a resposta atualizada.
    ///
    /// Um resultado repetido para o mesmo identificador não substitui o texto
    /// já gravado. Se o manifesto ainda marcar a operação como `running`, o
    /// estado passa a `completed` ou `no_speech` conforme o resultado.
    pub fn append_transcript(
        &self,
        attempt_id: &str,
        transcript: Transcript,
    ) -> Result<Attempt, AppError> {
        let directory = self.attempt_directory(attempt_id)?;
        let mut manifest = self.read_manifest(&directory)?;
        if !is_safe_id(&transcript.id) {
            return Err(invalid_identifier());
        }

        let transcripts = directory.join(TRANSCRIPTS_DIR);
        fs::create_dir_all(&transcripts).map_err(|error| {
            storage_failure(format!(
                "Não foi possível preparar a pasta das transcrições: {error}."
            ))
        })?;
        let path = transcripts.join(format!("{}.json", transcript.id));
        if !path.exists() {
            write_json(&path, &transcript)?;
        }

        if let Some(operation) = &manifest.last_operation {
            let applies =
                operation.state == OperationState::Running && operation.id == transcript.id;
            if applies {
                manifest.last_operation = Some(OperationRecord {
                    state: match transcript.status {
                        TranscriptStatus::Completed => OperationState::Completed,
                        TranscriptStatus::NoSpeech => OperationState::NoSpeech,
                    },
                    ..operation.clone()
                });
                write_json(&directory.join(MANIFEST_FILE), &manifest)?;
            }
        }

        Ok(self.load(&manifest)?.attempt)
    }

    /// Guarda a revisão de uma transcrição, substituindo a anterior.
    ///
    /// A revisão é uma versão distinta da transcrição original e da resposta
    /// confirmada; se a escrita falhar, a revisão anterior mantém-se intacta.
    pub fn save_review(
        &self,
        attempt_id: &str,
        source_id: &str,
        text: &str,
    ) -> Result<Review, AppError> {
        let directory = self.attempt_directory(attempt_id)?;
        let manifest = self.read_manifest(&directory)?;
        let loaded = self.load(&manifest)?;
        if !loaded
            .attempt
            .transcripts
            .iter()
            .any(|transcript| transcript.id == source_id)
        {
            return Err(AppError::new(
                error_code::STALE_SOURCE,
                "A revisão refere uma transcrição que não pertence a esta resposta.",
                false,
            ));
        }

        let review = Review {
            source_transcript_id: source_id.to_owned(),
            text: text.to_owned(),
            saved_at: utc_now(),
        };
        write_json(&directory.join(REVIEW_FILE), &review)?;
        Ok(review)
    }

    /// Regista a última operação aplicada a uma resposta.
    pub fn set_operation(
        &self,
        attempt_id: &str,
        operation: OperationRecord,
    ) -> Result<(), AppError> {
        let directory = self.attempt_directory(attempt_id)?;
        let mut manifest = self.read_manifest(&directory)?;
        manifest.last_operation = Some(operation);
        write_json(&directory.join(MANIFEST_FILE), &manifest)
    }

    /// Pasta de uma resposta, recusando identificadores que não sejam simples.
    fn attempt_directory(&self, attempt_id: &str) -> Result<PathBuf, AppError> {
        if !is_safe_id(attempt_id) {
            return Err(invalid_identifier());
        }
        Ok(self.root.join(ATTEMPTS_DIR).join(attempt_id))
    }

    /// Lê o manifesto de uma pasta.
    fn read_manifest(&self, directory: &Path) -> Result<AttemptManifest, AppError> {
        let path = directory.join(MANIFEST_FILE);
        let raw = read_text(&path)?.ok_or_else(|| {
            artifact_missing(format!(
                "Os metadados da resposta não foram encontrados em {}; os restantes dados foram conservados.",
                path.display()
            ))
        })?;
        let manifest: AttemptManifest = parse_json(&raw, MANIFEST_FILE)?;
        if manifest.schema_version != SCHEMA_VERSION {
            return Err(storage_failure(format!(
                "Os metadados da resposta usam uma versão não suportada ({}).",
                manifest.schema_version
            )));
        }
        Ok(manifest)
    }

    /// Reconstrói a resposta a partir do manifesto, das transcrições e da revisão.
    fn load(&self, manifest: &AttemptManifest) -> Result<LoadedAttempt, AppError> {
        let directory = self.attempt_directory(&manifest.id)?;
        let mut warnings = Vec::new();

        check_reference(
            &directory,
            &manifest.original_file,
            "O áudio original",
            &mut warnings,
        );
        if let Some(normalized) = manifest.normalized_file.as_deref() {
            check_reference(&directory, normalized, "O áudio normalizado", &mut warnings);
        }

        let transcripts = self.read_transcripts(&directory, &mut warnings)?;
        let review = read_review(&directory, &mut warnings)?;

        let attempt = manifest.clone().into_attempt(transcripts, review);
        Ok(LoadedAttempt { attempt, warnings })
    }

    /// Lê todas as transcrições guardadas, ordenadas por data e identificador.
    fn read_transcripts(
        &self,
        directory: &Path,
        warnings: &mut Vec<AppError>,
    ) -> Result<Vec<Transcript>, AppError> {
        let transcripts_dir = directory.join(TRANSCRIPTS_DIR);
        if !transcripts_dir.is_dir() {
            return Ok(Vec::new());
        }

        let entries = fs::read_dir(&transcripts_dir).map_err(|error| {
            storage_failure(format!(
                "Não foi possível ler a pasta das transcrições: {error}."
            ))
        })?;
        let mut transcripts = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| {
                storage_failure(format!(
                    "Não foi possível ler a pasta das transcrições: {error}."
                ))
            })?;
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
                continue;
            }
            match read_transcript(&path) {
                Ok(transcript) => transcripts.push(transcript),
                Err(warning) => warnings.push(warning),
            }
        }

        transcripts.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(transcripts)
    }
}

/// Copia o áudio finalizado para `original.wav`, validando-o primeiro.
fn store_original(source: &Path, destination: &Path) -> Result<(), AppError> {
    let metadata = fs::metadata(source).map_err(|_| {
        artifact_missing(format!(
            "O áudio gravado não foi encontrado em {}; grave novamente.",
            source.display()
        ))
    })?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(AppError::new(
            error_code::INVALID_AUDIO,
            "A gravação ficou vazia ou ilegível; grave novamente.",
            false,
        ));
    }
    if source != destination {
        fs::copy(source, destination).map_err(|error| {
            storage_failure(format!(
                "Não foi possível guardar o áudio gravado: {error}."
            ))
        })?;
    }
    Ok(())
}

/// Um resultado guardado recompõe a operação que ficou marcada em curso.
///
/// Sem resultado, a operação passa a `interrupted`. A leitura não escreve: o
/// estado é recalculado do mesmo modo em cada recuperação.
fn heal_operation(
    operation: &Option<OperationRecord>,
    transcripts: &[Transcript],
) -> Option<OperationRecord> {
    let operation = operation.clone()?;
    if operation.state != OperationState::Running {
        return Some(operation);
    }

    let state = match transcripts
        .iter()
        .find(|transcript| transcript.id == operation.id)
        .map(|transcript| transcript.status)
    {
        Some(TranscriptStatus::Completed) => OperationState::Completed,
        Some(TranscriptStatus::NoSpeech) => OperationState::NoSpeech,
        None => OperationState::Interrupted,
    };
    Some(OperationRecord { state, ..operation })
}

/// Verifica um ficheiro referenciado pelo manifesto e acumula o aviso.
fn check_reference(directory: &Path, reference: &str, label: &str, warnings: &mut Vec<AppError>) {
    match resolve_reference(directory, reference) {
        Some(path) if path.is_file() => {}
        Some(path) => warnings.push(artifact_missing(format!(
            "{label} não foi encontrado em {}; a resposta mantém os restantes dados e pode ser transcrita a partir do que existir.",
            path.display()
        ))),
        None => warnings.push(artifact_missing(format!(
            "A referência guardada para {label} não é válida."
        ))),
    }
}

/// Resolve uma referência relativa sem permitir sair da pasta da resposta.
fn resolve_reference(directory: &Path, reference: &str) -> Option<PathBuf> {
    let path = Path::new(reference);
    if path.is_absolute() {
        return None;
    }
    let mut resolved = directory.to_path_buf();
    let mut components = 0;
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                resolved.push(part);
                components += 1;
            }
            _ => return None,
        }
    }
    (components > 0).then_some(resolved)
}

/// Lê uma transcrição, devolvendo o aviso em vez de interromper a recuperação.
fn read_transcript(path: &Path) -> Result<Transcript, AppError> {
    let raw = read_text(path)?.ok_or_else(|| {
        artifact_missing(format!(
            "A transcrição {} desapareceu durante a leitura.",
            path.display()
        ))
    })?;
    serde_json::from_str(&raw).map_err(|error| {
        artifact_missing(format!(
            "A transcrição {} está ilegível e foi ignorada: {error}.",
            path.display()
        ))
    })
}

/// Lê a revisão guardada, se existir.
fn read_review(directory: &Path, warnings: &mut Vec<AppError>) -> Result<Option<Review>, AppError> {
    let Some(raw) = read_text(&directory.join(REVIEW_FILE))? else {
        return Ok(None);
    };
    match serde_json::from_str(&raw) {
        Ok(review) => Ok(Some(review)),
        Err(error) => {
            warnings.push(artifact_missing(format!(
                "A revisão guardada está ilegível e foi ignorada: {error}."
            )));
            Ok(None)
        }
    }
}

/// Escreve um JSON pela substituição atómica de `atomicwrites`.
///
/// O temporário é criado na mesma pasta do destino e movido para o lugar; o
/// ficheiro anterior só desaparece depois de a substituição ter êxito.
fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), AppError> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| {
        storage_failure(format!(
            "Não foi possível preparar {}: {error}.",
            label(path)
        ))
    })?;
    AtomicFile::new(path, AllowOverwrite)
        .write(|file| {
            file.write_all(&bytes)?;
            file.flush()
        })
        .map_err(|error| {
            storage_failure(format!(
                "Não foi possível guardar {}: {error}.",
                label(path)
            ))
        })
}

/// Texto de um ficheiro, ou `None` quando ainda não existe.
fn read_text(path: &Path) -> Result<Option<String>, AppError> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(storage_failure(format!(
            "Não foi possível ler {}: {error}.",
            label(path)
        ))),
    }
}

fn parse_json<T: DeserializeOwned>(raw: &str, name: &str) -> Result<T, AppError> {
    serde_json::from_str(raw).map_err(|error| {
        storage_failure(format!(
            "Os dados de {name} estão ilegíveis: {error}. Os ficheiros foram conservados."
        ))
    })
}

/// Falha do próprio armazenamento; a operação pode ser repetida.
fn storage_failure(message: impl Into<String>) -> AppError {
    AppError::new(error_code::STORAGE_WRITE_FAILED, message, true)
}

/// Artefacto em falta ou ilegível; repetir não o traz de volta.
fn artifact_missing(message: impl Into<String>) -> AppError {
    AppError::new(error_code::ARTIFACT_MISSING, message, false)
}

/// Identificador recusado por não ser um nome de pasta simples.
fn invalid_identifier() -> AppError {
    AppError::new(
        error_code::ARTIFACT_MISSING,
        "O identificador indicado não corresponde a uma resposta guardada.",
        false,
    )
}

/// Aceita apenas nomes simples: letras, dígitos, hífen e sublinhado.
fn is_safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '-' || character == '_'
        })
}

/// Nome curto para as mensagens de erro.
fn label(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Instante atual em UTC e RFC3339, com milissegundos.
///
/// A fase 1 não traz uma biblioteca de datas: a conversão civil direta do
/// calendário gregoriano chega para datar respostas e revisões.
fn utc_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let seconds = now.as_secs() as i64;
    let millis = now.subsec_millis();
    let days = seconds.div_euclid(86_400);
    let second_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = second_of_day / 3_600;
    let minute = (second_of_day % 3_600) / 60;
    let second = second_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

/// Data civil a partir do número de dias desde 1970-01-01 (algoritmo de Hinnant).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = (shifted - era * 146_097) as u64;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era as i64 + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_position = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_position + 2) / 5 + 1) as u32;
    let month = if month_position < 10 {
        month_position + 3
    } else {
        month_position - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

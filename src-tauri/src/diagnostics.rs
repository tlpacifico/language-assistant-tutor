//! Diagnóstico sem conteúdo: fila limitada, disco numa thread própria, retenção
//! independente da versão e exportação de eventos anteriormente enfileirados.
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::json;

pub const MAX_BYTES: u64 = 1024 * 1024;
const MAX_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const FILES: [&str; 5] = [
    "events.jsonl",
    "events.1.jsonl",
    "events.2.jsonl",
    "events.3.jsonl",
    "events.4.jsonl",
];
static GLOBAL: OnceLock<Diagnostics> = OnceLock::new();

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildInfo {
    pub version: String,
    pub revision: String,
    pub dirty: bool,
    pub built_at: String,
}

impl BuildInfo {
    pub fn current() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").into(),
            revision: option_env!("TUTOR_REVISION").unwrap_or("unknown").into(),
            dirty: option_env!("TUTOR_DIRTY") == Some("true"),
            built_at: option_env!("TUTOR_BUILT_AT").unwrap_or("unknown").into(),
        }
    }
}

/// Entrada deliberadamente sem campo de mensagem, payload ou stack bruta.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Event {
    pub component: String,
    pub operation: String,
    pub code: String,
    pub duration_ms: Option<u64>,
    pub file: Option<String>,
    pub line: Option<u32>,
}

impl Event {
    pub fn new(component: &str, operation: &str, code: &str) -> Self {
        Self {
            component: component.into(),
            operation: operation.into(),
            code: code.into(),
            duration_ms: None,
            file: None,
            line: None,
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub build: BuildInfo,
    pub writable: bool,
    pub dropped: u64,
    pub directory: PathBuf,
}

enum Message {
    Event(serde_json::Value),
    Flush(mpsc::Sender<bool>),
    Export(
        PathBuf,
        serde_json::Value,
        mpsc::Sender<io::Result<PathBuf>>,
    ),
}

#[derive(Clone)]
pub struct Diagnostics {
    sender: mpsc::SyncSender<Message>,
    writable: Arc<AtomicBool>,
    dropped: Arc<AtomicU64>,
    build: BuildInfo,
    run_id: String,
    root: PathBuf,
}

fn millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn generic_error() -> io::Error {
    io::Error::other("Não foi possível guardar ou exportar o diagnóstico local.")
}

impl Diagnostics {
    pub fn open(root: PathBuf, build: BuildInfo) -> Self {
        let (sender, receiver) = mpsc::sync_channel(256);
        let writable = Arc::new(AtomicBool::new(false));
        let dropped = Arc::new(AtomicU64::new(0));
        let result = Self {
            sender,
            writable: writable.clone(),
            dropped: dropped.clone(),
            build,
            run_id: uuid::Uuid::new_v4().to_string(),
            root: root.clone(),
        };
        let spawned = std::thread::Builder::new()
            .name("diagnostics".into())
            .spawn(move || {
                let mut next_prune = 0;
                writable.store(
                    with_lock(&root, || {
                        prepare(&root)?;
                        next_prune = prune_expired(&root)?;
                        Ok(())
                    })
                    .is_ok(),
                    Ordering::Relaxed,
                );
                while let Ok(message) = receiver.recv() {
                    match message {
                        Message::Event(value) => {
                            let ok = with_lock(&root, || {
                                if millis() >= next_prune {
                                    prepare(&root)?;
                                    next_prune = prune_expired(&root)?;
                                }
                                append(&root, &value)
                            })
                            .is_ok();
                            writable.store(ok, Ordering::Relaxed);
                            if !ok {
                                dropped.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        Message::Flush(reply) => {
                            let _ = reply.send(writable.load(Ordering::Relaxed));
                        }
                        Message::Export(directory, mut metadata, reply) => {
                            metadata["dropped"] = json!(dropped.load(Ordering::Relaxed));
                            metadata["writable"] = json!(writable.load(Ordering::Relaxed));
                            let exported = with_lock(&root, || export(&root, &directory, metadata));
                            let _ = reply.send(exported);
                        }
                    }
                }
            });
        if spawned.is_err() {
            result.dropped.fetch_add(1, Ordering::Relaxed);
        }
        result.record(Event::new("rust", "startup", "OK"));
        result
    }

    pub fn record(&self, event: Event) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let value = json!({
            "schemaVersion": 1, "id": id, "timestampMs": millis(), "runId": self.run_id,
            "build": self.build,
            "component": match event.component.as_str() { "rust" => "rust", "frontend" => "frontend", "worker" => "worker", _ => "unknown" },
            "operation": safe_operation(&event.operation), "code": safe_code(&event.code),
            "durationMs": event.duration_ms.map(|n| n.min(86_400_000)),
            "file": event.file.as_deref().filter(|s| matches!(*s, "__main__.py" | "engines.py" | "protocol.py")),
            "line": event.line,
        });
        if self.sender.try_send(Message::Event(value)).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
        id
    }

    /// Chamado fora da thread da interface; nunca usado para registar eventos.
    pub fn flush(&self) -> io::Result<()> {
        let (tx, rx) = mpsc::channel();
        self.sender
            .try_send(Message::Flush(tx))
            .map_err(|_| generic_error())?;
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(true) => Ok(()),
            _ => Err(generic_error()),
        }
    }

    pub fn export(&self, directory: &Path) -> io::Result<PathBuf> {
        if !directory.is_absolute() {
            return Err(generic_error());
        }
        let (tx, rx) = mpsc::channel();
        self.sender
            .try_send(Message::Export(
                directory.into(),
                json!({ "build": self.build, "runId": self.run_id }),
                tx,
            ))
            .map_err(|_| generic_error())?;
        rx.recv_timeout(Duration::from_secs(15))
            .map_err(|_| generic_error())?
    }

    pub fn status(&self) -> Status {
        Status {
            build: self.build.clone(),
            writable: self.writable.load(Ordering::Relaxed),
            dropped: self.dropped.load(Ordering::Relaxed),
            directory: self.root.clone(),
        }
    }
}

fn with_lock<T>(root: &Path, action: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    fs::create_dir_all(root)?;
    let path = root.join("writer.lock");
    if let Ok(meta) = fs::symlink_metadata(&path) {
        if !meta.is_file() || meta.file_type().is_symlink() {
            return Err(generic_error());
        }
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    file.lock()?;
    action()
}

fn prepare(root: &Path) -> io::Result<()> {
    fs::create_dir_all(root)?;
    for name in FILES {
        let path = root.join(name);
        if let Ok(meta) = fs::symlink_metadata(&path) {
            if !meta.is_file() || meta.file_type().is_symlink() {
                return Err(generic_error());
            }
            if meta.modified()?.elapsed().unwrap_or_default() > MAX_AGE {
                fs::remove_file(path)?;
            }
        }
    }
    Ok(())
}

/// Calcula a próxima expiração: não relê o histórico em cada evento.
/// Exportar volta sempre a verificar, mesmo com ficheiros alterados externamente.
fn prune_expired(root: &Path) -> io::Result<u64> {
    let now = millis();
    let age = MAX_AGE.as_millis() as u64;
    let mut next = now.saturating_add(age);
    for name in FILES {
        let path = root.join(name);
        let file = match fs::File::open(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        let mut kept = Vec::new();
        let mut changed = false;
        for line in io::BufReader::new(file.take(MAX_BYTES)).lines() {
            let line = line?;
            let timestamp = serde_json::from_str::<serde_json::Value>(&line)
                .ok()
                .and_then(|v| v["timestampMs"].as_u64());
            if let Some(timestamp) = timestamp {
                let expiry = timestamp.saturating_add(age);
                if expiry <= now {
                    changed = true;
                    continue;
                }
                next = next.min(expiry);
            }
            kept.extend_from_slice(line.as_bytes());
            kept.push(b'\n');
        }
        if changed {
            atomicwrites::AtomicFile::new(&path, atomicwrites::AllowOverwrite)
                .write(|file| {
                    file.write_all(&kept)?;
                    file.sync_all()
                })
                .map_err(io::Error::other)?;
        }
    }
    Ok(next)
}

fn append(root: &Path, value: &serde_json::Value) -> io::Result<()> {
    prepare(root)?;
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    if fs::metadata(root.join(FILES[0]))
        .map(|m| m.len())
        .unwrap_or(0)
        + bytes.len() as u64
        > MAX_BYTES
    {
        let oldest = root.join(FILES[4]);
        if oldest.exists() {
            fs::remove_file(oldest)?;
        }
        for index in (0..4).rev() {
            let from = root.join(FILES[index]);
            if from.exists() {
                fs::rename(from, root.join(FILES[index + 1]))?;
            }
        }
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join(FILES[0]))?;
    file.write_all(&bytes)?;
    file.flush()
}

fn export(root: &Path, directory: &Path, metadata: serde_json::Value) -> io::Result<PathBuf> {
    prepare(root)?;
    prune_expired(root)?;
    let mut events = Vec::new();
    for name in FILES.into_iter().rev() {
        let file = match fs::File::open(root.join(name)) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(_) => return Err(generic_error()),
        };
        // Nunca exportar linhas arbitrárias colocadas na pasta de registos.
        for line in io::BufReader::new(file.take(MAX_BYTES)).lines() {
            let line = line?;
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) {
                if let Some(event) = sanitized_stored_event(&value) {
                    events.push(event);
                }
            }
        }
    }
    let path = directory.join(format!(
        "tutor-diagnostic-{}-{}.json",
        millis(),
        uuid::Uuid::new_v4()
    ));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
        .map_err(|_| generic_error())?;
    let result = serde_json::to_writer_pretty(
        &mut file,
        &json!({ "schemaVersion": 1, "exportedAtMs": millis(), "diagnosticStatus": metadata, "events": events }),
    )
    .map_err(io::Error::other)
    .and_then(|_| file.sync_all());
    if result.is_err() {
        let _ = fs::remove_file(&path);
    }
    result.map(|_| path).map_err(|_| generic_error())
}

fn sanitized_stored_event(value: &serde_json::Value) -> Option<serde_json::Value> {
    // Só eventos com a estrutura emitida pelo escritor. Campos desconhecidos
    // nunca atravessam a exportação, mesmo num ficheiro editado manualmente.
    let id = uuid::Uuid::parse_str(value["id"].as_str()?).ok()?;
    let run = uuid::Uuid::parse_str(value["runId"].as_str()?).ok()?;
    let build = &value["build"];
    let version = build["version"].as_str()?;
    let revision = build["revision"].as_str()?;
    let built_at = build["builtAt"].as_str()?;
    if !version.bytes().all(|b| b.is_ascii_digit() || b == b'.') || version.len() > 32 {
        return None;
    }
    if revision != "unknown"
        && !(revision.len() <= 40 && revision.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return None;
    }
    if built_at != "unknown"
        && !(built_at.len() <= 20 && built_at.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    Some(json!({ "schemaVersion": 1, "id": id, "runId": run,
        "timestampMs": value["timestampMs"].as_u64()?,
        "build": { "version": version, "revision": revision, "dirty": build["dirty"].as_bool()?, "builtAt": built_at },
        "component": match value["component"].as_str() { Some("rust") => "rust", Some("frontend") => "frontend", Some("worker") => "worker", _ => "unknown" },
        "operation": safe_operation(value["operation"].as_str().unwrap_or("unknown")),
        "code": safe_code(value["code"].as_str().unwrap_or("UNKNOWN")),
        "durationMs": value["durationMs"].as_u64().map(|n| n.min(86_400_000)),
        "file": value["file"].as_str().filter(|s| matches!(*s, "__main__.py" | "engines.py" | "protocol.py")),
        "line": value["line"].as_u64(),
    }))
}

pub fn install(diagnostics: Diagnostics) {
    let _ = GLOBAL.set(diagnostics);
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // A mensagem do panic pode conter conteúdo ou credenciais.
        record(Event::new("rust", "panic", "UNEXPECTED"));
        previous(info);
    }));
}

pub fn record(event: Event) {
    if let Some(diagnostics) = GLOBAL.get() {
        diagnostics.record(event);
    }
}

/// Apenas o canal estruturado próprio; stderr das bibliotecas nunca é persistido.
pub fn worker_event(line: &str) -> Option<Event> {
    let json = line.trim_end().strip_prefix("TUTOR_DIAGNOSTIC ")?;
    if json.len() > 2048 {
        return None;
    }
    let mut event: Event = serde_json::from_str(json).ok()?;
    event.component = "worker".into();
    Some(event)
}

fn safe_code(code: &str) -> &str {
    const CODES: &[&str] = &[
        "OK",
        "UNKNOWN",
        "UNEXPECTED",
        "BUSY",
        "DEVICE_UNAVAILABLE",
        "DEVICE_FORMAT_UNSUPPORTED",
        "AUDIO_TIMEOUT",
        "CAPTURE_INTERRUPTED",
        "STORAGE_WRITE_FAILED",
        "ARTIFACT_MISSING",
        "INVALID_AUDIO",
        "MODEL_MISSING",
        "WORKER_UNAVAILABLE",
        "WORKER_PROTOCOL",
        "WORKER_TIMEOUT",
        "STALE_SOURCE",
        "INVALID_INPUT",
        "INSUFFICIENT_QUESTIONS",
        "ACTIVE_SESSION_EXISTS",
        "ANSWER_CONFIRMED",
        "ANSWER_REQUIRED",
        "DATABASE_UNAVAILABLE",
        "DATABASE_VERSION_UNSUPPORTED",
        "AI_CREDENTIAL",
        "AI_ACCESS",
        "AI_BILLING",
        "AI_RATE_LIMIT",
        "AI_TIMEOUT",
        "AI_NETWORK",
        "AI_STORAGE",
        "AI_CONSENT",
        "AI_LIMIT",
        "AI_INVALID_RESULT",
        "ARTICLE_DELETED",
    ];
    if CODES.contains(&code) {
        code
    } else {
        "UNKNOWN"
    }
}

fn safe_operation(operation: &str) -> &str {
    const OPERATIONS: &[&str] = &[
        "startup",
        "error",
        "panic",
        "render",
        "window_error",
        "unhandled_rejection",
        "health",
        "transcribe",
        "synthesize",
        "shutdown",
        "snapshot_publish",
    ];
    if OPERATIONS.contains(&operation) || COMMANDS.contains(&operation) {
        operation
    } else {
        "unknown"
    }
}

// Lista fechada; nunca persistir nomes/argumentos livres enviados pelo frontend.
const COMMANDS: &[&str] = &[
    "interview_abandon_practice",
    "interview_advance_session",
    "interview_ai_accept_ai_consent",
    "interview_ai_evaluate_question",
    "interview_ai_generate_feedback",
    "interview_ai_load_ai_settings",
    "interview_ai_load_feedback",
    "interview_ai_remove_ai_key",
    "interview_ai_request_follow_up",
    "interview_ai_retry_ai_publication",
    "interview_ai_save_ai_key",
    "interview_ai_save_ai_settings",
    "interview_ai_waive_follow_up",
    "interview_compare_responses",
    "interview_confirm_answer",
    "interview_confirm_practice_answer",
    "interview_create_practice",
    "interview_create_question",
    "interview_create_story",
    "interview_evaluate_practice",
    "interview_list_legacy",
    "interview_list_practices",
    "interview_list_questions",
    "interview_list_rewrites",
    "interview_list_sessions",
    "interview_list_stories",
    "interview_load_legacy",
    "interview_load_practice",
    "interview_load_practice_report",
    "interview_load_preparation",
    "interview_load_rewrite_preference",
    "interview_load_session",
    "interview_load_training_usage",
    "interview_play_audio",
    "interview_preload_models",
    "interview_prepare_close",
    "interview_refresh_availability",
    "interview_request_rewrite",
    "interview_resolve_preparation",
    "interview_restart_worker",
    "interview_retry_database",
    "interview_retry_preparation_publication",
    "interview_retry_save_recording",
    "interview_save_practice_review",
    "interview_save_review",
    "interview_select_story",
    "interview_set_rewrite_level",
    "interview_snapshot",
    "interview_start_practice_recording",
    "interview_start_recording",
    "interview_start_session",
    "interview_stop_audio",
    "interview_stop_recording",
    "interview_transcribe_practice_response",
    "interview_transcribe_response",
    "interview_update_question",
    "interview_update_story",
];

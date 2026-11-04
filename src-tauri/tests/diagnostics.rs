use language_assistant_tutor::diagnostics::{BuildInfo, Diagnostics, Event, MAX_BYTES};
use std::fs;

fn build(version: &str) -> BuildInfo {
    BuildInfo {
        version: version.into(),
        revision: "abc1234".into(),
        dirty: true,
        built_at: "123".into(),
    }
}

#[test]
fn reopening_with_another_version_preserves_history_and_exports_in_queue_order() {
    let dir = tempfile::tempdir().unwrap();
    let logs = dir.path().join("diagnostics");
    let first = Diagnostics::open(logs.clone(), build("0.1.0"));
    first.record(Event::new("rust", "error", "STORAGE_WRITE_FAILED"));
    first.flush().unwrap();
    drop(first);
    let second = Diagnostics::open(logs, build("0.2.0"));
    second.record(Event::new("frontend", "render", "UNEXPECTED"));
    let path = second.export(dir.path()).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let events = value["events"].as_array().unwrap();
    assert!(events.iter().any(|e| e["build"]["version"] == "0.1.0"));
    assert!(events
        .iter()
        .any(|e| e["build"]["version"] == "0.2.0" && e["operation"] == "render"));
    assert_ne!(path, second.export(dir.path()).unwrap());
}

#[test]
fn free_text_is_never_persisted_or_exported() {
    let dir = tempfile::tempdir().unwrap();
    let diagnostics = Diagnostics::open(dir.path().join("logs"), build("0.1.0"));
    let mut event = Event::new("secret-key", "my private answer", "API-SECRET");
    event.file = Some("C:/Users/private-person/secret.py".into());
    diagnostics.record(event);
    diagnostics.record(Event::new("rust", "error", "AI_BILLING"));
    let export = diagnostics.export(dir.path()).unwrap();
    let text = fs::read_to_string(export).unwrap();
    for secret in [
        "secret-key",
        "my private answer",
        "API-SECRET",
        "private-person",
        "secret.py",
    ] {
        assert!(!text.contains(secret), "{secret}");
    }
    assert!(text.contains("UNKNOWN"));
    assert!(text.contains("AI_BILLING"));
}

#[test]
fn rotation_is_bounded_and_does_not_clean_unrelated_files() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("events.jsonl"),
        "x".repeat(MAX_BYTES as usize),
    )
    .unwrap();
    fs::write(dir.path().join("keep.txt"), "untouched").unwrap();
    let diagnostics = Diagnostics::open(dir.path().into(), build("0.1.0"));
    for _ in 0..6 {
        diagnostics.record(Event::new("rust", "error", "UNKNOWN"));
        diagnostics.flush().unwrap();
        fs::write(
            dir.path().join("events.jsonl"),
            "x".repeat(MAX_BYTES as usize),
        )
        .unwrap();
    }
    diagnostics.record(Event::new("rust", "error", "UNKNOWN"));
    diagnostics.flush().unwrap();
    let logs: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .filter(|p| p.file_name().to_string_lossy().ends_with("jsonl"))
        .collect();
    assert_eq!(logs.len(), 5);
    assert_eq!(
        fs::read_to_string(dir.path().join("keep.txt")).unwrap(),
        "untouched"
    );
}

#[test]
fn retention_removes_only_owned_old_files() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["events.1.jsonl", "unrelated.jsonl"] {
        let file = fs::File::create(dir.path().join(name)).unwrap();
        file.set_modified(std::time::UNIX_EPOCH).unwrap();
    }
    let diagnostics = Diagnostics::open(dir.path().into(), build("0.1.0"));
    diagnostics.flush().unwrap();
    assert!(!dir.path().join("events.1.jsonl").exists());
    assert!(dir.path().join("unrelated.jsonl").exists());
}

#[test]
fn storage_failure_is_observable_without_panicking() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("not-a-directory");
    fs::write(&file, "existing").unwrap();
    let diagnostics = Diagnostics::open(file, build("0.1.0"));
    diagnostics.record(Event::new("rust", "error", "UNKNOWN"));
    assert!(diagnostics.flush().is_err());
    assert!(!diagnostics.status().writable);
    assert!(diagnostics.status().dropped > 0);
}

#[test]
fn old_events_expire_even_when_the_file_was_recently_written() {
    let dir = tempfile::tempdir().unwrap();
    let logs = dir.path().join("logs");
    let first = Diagnostics::open(logs.clone(), build("0.1.0"));
    first.flush().unwrap();
    let path = logs.join("events.jsonl");
    let mut event: serde_json::Value =
        serde_json::from_str(fs::read_to_string(&path).unwrap().lines().next().unwrap()).unwrap();
    event["timestampMs"] = serde_json::json!(1);
    fs::write(&path, format!("{event}\n")).unwrap();
    first.record(Event::new("rust", "error", "BUSY"));
    let export = first.export(dir.path()).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&fs::read(export).unwrap()).unwrap();
    assert!(!value["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["timestampMs"] == 1));
    assert!(fs::read_to_string(path).unwrap().lines().all(|line| {
        serde_json::from_str::<serde_json::Value>(line).unwrap()["timestampMs"] != 1
    }));
}

#[test]
fn export_reports_previous_losses_after_storage_recovers() {
    let dir = tempfile::tempdir().unwrap();
    let logs = dir.path().join("logs");
    fs::write(&logs, "unavailable").unwrap();
    let diagnostics = Diagnostics::open(logs.clone(), build("0.1.0"));
    assert!(diagnostics.flush().is_err());
    fs::remove_file(&logs).unwrap();
    diagnostics.record(Event::new("rust", "error", "BUSY"));
    let export = diagnostics.export(dir.path()).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&fs::read(export).unwrap()).unwrap();
    assert!(value["diagnosticStatus"]["dropped"].as_u64().unwrap_or(0) > 0);
    assert_eq!(value["diagnosticStatus"]["writable"], true);
    assert!(!fs::read_to_string(diagnostics.export(dir.path()).unwrap())
        .unwrap()
        .contains(dir.path().to_str().unwrap()));
}

#[test]
fn concurrent_writers_do_not_overflow_or_lose_events_during_rotation() {
    let dir = tempfile::tempdir().unwrap();
    let one = Diagnostics::open(dir.path().join("logs"), build("0.1.0"));
    let two = Diagnostics::open(dir.path().join("logs"), build("0.2.0"));
    one.flush().unwrap();
    two.flush().unwrap();
    let event: serde_json::Value = serde_json::from_str(
        fs::read_to_string(dir.path().join("logs/events.jsonl"))
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    let line = format!("{event}\n");
    fs::write(
        dir.path().join("logs/events.jsonl"),
        line.repeat(MAX_BYTES as usize / line.len()),
    )
    .unwrap();
    for _ in 0..20 {
        one.record(Event::new("rust", "error", "BUSY"));
        two.record(Event::new("rust", "error", "BUSY"));
    }
    one.flush().unwrap();
    two.flush().unwrap();
    assert_eq!(one.status().dropped + two.status().dropped, 0);
    for file in fs::read_dir(dir.path().join("logs")).unwrap().flatten() {
        assert!(file.metadata().unwrap().len() <= MAX_BYTES);
    }
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(one.export(dir.path()).unwrap()).unwrap()).unwrap();
    assert_eq!(
        value["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["code"] == "BUSY")
            .count(),
        40
    );
}

#[test]
fn writer_honours_the_cross_process_file_lock() {
    let dir = tempfile::tempdir().unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.path().join("writer.lock"))
        .unwrap();
    lock.lock().unwrap();
    let diagnostics = Diagnostics::open(dir.path().into(), build("0.1.0"));
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(diagnostics.flush().is_ok());
    });
    let before_unlock = rx.recv_timeout(std::time::Duration::from_millis(500));
    drop(lock);
    assert!(
        before_unlock.is_err(),
        "o escritor ignorou o lock de outra instância"
    );
    assert!(rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap());
}

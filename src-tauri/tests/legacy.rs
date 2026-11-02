//! Consulta dos treinos da fase 1 (tarefa 9, secção 8 da spec).
//!
//! A árvore `phase1/` é conservada e apenas lida: uma listagem não depende de
//! `last-attempt.json` (o apontador não é um índice), uma pasta inválida não
//! esconde as restantes e nenhuma leitura altera um byte. Os caminhos que saem
//! da pasta da resposta são recusados antes de qualquer acesso ao disco.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use language_assistant_tutor::audio::CapturedAudio;
use language_assistant_tutor::domain::{
    error_code, AppError, CaptureMeta, Segment, Transcript, TranscriptStatus,
};
use language_assistant_tutor::legacy::LegacyStore;
use language_assistant_tutor::storage::AttemptStore;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

/// Raiz temporária, árvore `phase1` e armazenamento de origem dos dados.
fn fixture() -> (TempDir, PathBuf, AttemptStore) {
    let temp = tempfile::tempdir().expect("criar raiz temporária");
    let root = temp.path().join("phase1");
    let store = AttemptStore::new(root.clone());
    (temp, root, store)
}

/// Reserva uma gravação e deixa o áudio original, como a fase 1 fazia.
fn record(store: &AttemptStore, payload: &[u8]) -> CapturedAudio {
    let target = store.allocate().expect("reservar gravação");
    let original = target.directory.join("original.wav");
    fs::write(&original, payload).expect("escrever o áudio original");
    CapturedAudio {
        id: target.id,
        original,
        meta: CaptureMeta {
            sample_rate: 48_000,
            channels: 1,
            duration_ms: 1_500,
            bytes: payload.len() as u64,
            incomplete: false,
        },
    }
}

fn transcript(id: &str, created_at: &str, text: &str) -> Transcript {
    Transcript {
        id: id.to_owned(),
        status: TranscriptStatus::Completed,
        text: text.to_owned(),
        segments: vec![Segment {
            start_ms: 0,
            end_ms: 900,
            text: text.to_owned(),
        }],
        model: "small".to_owned(),
        language: "en".to_owned(),
        created_at: created_at.to_owned(),
        load_ms: 320,
        processing_ms: 1_200,
    }
}

fn attempt_path(root: &Path, id: &str) -> PathBuf {
    root.join("attempts").join(id)
}

/// Todos os ficheiros da árvore, com o SHA-256 do conteúdo.
fn hash_tree(root: &Path) -> BTreeMap<String, String> {
    fn collect(root: &Path, directory: &Path, files: &mut BTreeMap<String, String>) {
        let Ok(entries) = fs::read_dir(directory) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect(root, &path, files);
            } else {
                let relative = path
                    .strip_prefix(root)
                    .expect("caminho dentro da árvore")
                    .to_string_lossy()
                    .replace('\\', "/");
                let bytes = fs::read(&path).unwrap_or_else(|error| {
                    panic!("ler {}: {error}", path.display());
                });
                let digest = Sha256::digest(&bytes);
                files.insert(relative, format!("{digest:x}"));
            }
        }
    }

    let mut files = BTreeMap::new();
    collect(root, root, &mut files);
    files
}

/// Identificadores das respostas devolvidas pela listagem.
fn listed_ids(results: &[language_assistant_tutor::domain::RestoreResult]) -> BTreeSet<String> {
    results
        .iter()
        .filter_map(|result| result.attempt.as_ref().map(|attempt| attempt.id.clone()))
        .collect()
}

fn all_warnings(results: &[language_assistant_tutor::domain::RestoreResult]) -> Vec<AppError> {
    results
        .iter()
        .flat_map(|result| result.warnings.iter().cloned())
        .collect()
}

#[test]
fn legacy_list_ignores_last_pointer() -> Result<(), AppError> {
    let (_temp, root, store) = fixture();
    let first = store.publish(record(&store, b"RIFF primeira resposta"))?;
    let second = store.publish(record(&store, b"RIFF segunda resposta"))?;

    // O apontador da última resposta não é um índice: apagá-lo ou corrompê-lo
    // não pode esconder os treinos existentes.
    let pointer = root.join("last-attempt.json");
    fs::remove_file(&pointer).expect("remover o apontador");
    let legacy = LegacyStore::new(root.clone());
    let listed = legacy.list()?;
    assert_eq!(
        listed_ids(&listed),
        BTreeSet::from([first.id.clone(), second.id.clone()]),
        "a listagem enumera as pastas, não o apontador"
    );
    assert!(all_warnings(&listed).is_empty(), "nenhum aviso nos válidos");

    fs::write(&pointer, b"{ isto nao e json").expect("corromper o apontador");
    assert_eq!(
        listed_ids(&legacy.list()?),
        BTreeSet::from([first.id.clone(), second.id.clone()]),
        "um apontador ilegível não interrompe a enumeração"
    );

    // As respostas válidas são lidas por identificador, sem o apontador.
    let loaded = legacy.load(&first.id)?;
    assert_eq!(
        loaded.attempt.as_ref().map(|attempt| attempt.id.as_str()),
        Some(first.id.as_str())
    );
    Ok(())
}

#[test]
fn corrupt_legacy_item_does_not_hide_valid_items() -> Result<(), AppError> {
    let (_temp, root, store) = fixture();
    let valid = store.publish(record(&store, b"RIFF resposta valida"))?;
    store.append_transcript(
        &valid.id,
        transcript(
            "request-1",
            "2026-11-01T10:00:00.000Z",
            "I solved the problem step by step.",
        ),
    )?;
    store.save_review(&valid.id, "request-1", "Revisão guardada do treino.")?;

    let corrupt = store.publish(record(&store, b"RIFF resposta corrompida"))?;
    fs::write(
        attempt_path(&root, &corrupt.id).join("attempt.json"),
        b"{ nao e json",
    )
    .expect("corromper o manifesto");

    // Uma pasta sem manifesto também é um item inválido, não um erro total.
    let empty = root.join("attempts").join("pasta-sem-manifesto");
    fs::create_dir(&empty).expect("criar pasta sem manifesto");

    let legacy = LegacyStore::new(root.clone());
    let listed = legacy.list()?;
    assert!(
        listed_ids(&listed).contains(&valid.id),
        "o treino válido continua listado: {listed:?}"
    );
    assert_eq!(listed.len(), 3, "cada pasta produz um item: {listed:?}");

    let corrupt_entry = listed
        .iter()
        .find(|result| {
            result
                .warnings
                .iter()
                .any(|warning| warning.message.contains(&corrupt.id))
        })
        .expect("o item corrompido aparece com aviso");
    assert!(corrupt_entry.attempt.is_none(), "sem resposta reconstruída");
    assert!(!corrupt_entry.warnings.is_empty());
    assert!(
        corrupt_entry
            .warnings
            .iter()
            .all(|warning| warning.code == error_code::STORAGE_WRITE_FAILED),
        "um manifesto ilegível é reportado sem ser apagado: {:?}",
        corrupt_entry.warnings
    );

    let valid_entry = listed
        .iter()
        .find(|result| {
            result
                .attempt
                .as_ref()
                .is_some_and(|attempt| attempt.id == valid.id)
        })
        .expect("o treino válido continua legível");
    assert!(
        valid_entry.warnings.is_empty(),
        "o válido não herda avisos dos outros: {:?}",
        valid_entry.warnings
    );
    let attempt = valid_entry.attempt.as_ref().expect("resposta válida");
    assert_eq!(attempt.transcripts.len(), 1);
    assert_eq!(
        attempt.review.as_ref().map(|review| review.text.as_str()),
        Some("Revisão guardada do treino.")
    );

    // Abrir diretamente o item corrompido devolve o erro, não dados inventados.
    let error = legacy
        .load(&corrupt.id)
        .expect_err("um manifesto ilegível não é reconstruído");
    assert_eq!(error.code, error_code::STORAGE_WRITE_FAILED);
    Ok(())
}

#[test]
fn legacy_read_changes_no_bytes() -> Result<(), AppError> {
    let (temp, root, store) = fixture();
    let attempt = store.publish(record(&store, b"RIFF resposta original"))?;
    store.append_transcript(
        &attempt.id,
        transcript(
            "request-1",
            "2026-11-01T10:00:00.000Z",
            "Original automatic answer.",
        ),
    )?;
    store.save_review(&attempt.id, "request-1", "Revisão guardada.")?;
    let normalized = attempt_path(&root, &attempt.id).join("normalized.wav");
    fs::write(&normalized, b"RIFF derivado").expect("criar derivado");
    store.set_normalized(&attempt.id, "normalized.wav")?;

    let before = hash_tree(&root);
    let parent_before: BTreeSet<String> = fs::read_dir(temp.path())
        .expect("ler a raiz temporária")
        .map(|entry| {
            entry
                .expect("entrada")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert!(!before.is_empty(), "a árvore tem artefactos para comparar");

    let legacy = LegacyStore::new(root.clone());
    let listed = legacy.list()?;
    assert_eq!(listed.len(), 1);
    let loaded = legacy.load(&attempt.id)?;
    assert!(loaded.attempt.is_some());
    let original = legacy.resolve_original(&attempt.id)?;
    assert_eq!(
        original,
        attempt_path(&root, &attempt.id).join("original.wav")
    );

    // Uma leitura de um identificador inexistente também não cria nada.
    assert!(legacy.load("resposta-inexistente").is_err());
    assert!(legacy.resolve_original("resposta-inexistente").is_err());

    let after = hash_tree(&root);
    assert_eq!(
        before, after,
        "ler não altera um único byte nem cria ficheiros"
    );

    let parent_after: BTreeSet<String> = fs::read_dir(temp.path())
        .expect("reler a raiz temporária")
        .map(|entry| {
            entry
                .expect("entrada")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(
        parent_before, parent_after,
        "nenhuma base de dados ou pasta nova é criada pelos treinos antigos"
    );
    assert!(
        !temp.path().join("tutor.sqlite3").exists(),
        "os treinos da fase 1 não são copiados para SQLite"
    );
    Ok(())
}

#[test]
fn legacy_rejects_path_escape() -> Result<(), AppError> {
    let (_temp, root, store) = fixture();
    let attempt = store.publish(record(&store, b"RIFF resposta"))?;
    let legacy = LegacyStore::new(root.clone());

    for id in [
        "",
        "..",
        "../fora",
        "..\\fora",
        "attempts/../fora",
        "C:\\Windows\\evil",
        "/etc/passwd",
        "a/b",
    ] {
        assert!(
            legacy.load(id).is_err(),
            "identificador aceite na leitura: {id}"
        );
        assert!(
            legacy.resolve_original(id).is_err(),
            "identificador aceite no original: {id}"
        );
    }

    // Uma referência guardada que sai da pasta da resposta é recusada, mesmo
    // que o ficheiro exista ao lado.
    let outside = root.join("attempts").join("evil.wav");
    fs::write(&outside, b"RIFF fora da pasta").expect("criar original fora da pasta");
    let manifest_path = attempt_path(&root, &attempt.id).join("attempt.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).expect("ler o manifesto"))
            .expect("o manifesto é JSON");
    manifest["originalFile"] = serde_json::json!("../evil.wav");
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).expect("serializar o manifesto"),
    )
    .expect("guardar o manifesto");

    let error = legacy
        .resolve_original(&attempt.id)
        .expect_err("uma referência que sai da pasta é recusada");
    assert_eq!(error.code, error_code::ARTIFACT_MISSING);
    assert!(!error.retryable);

    // O treino continua listado; a referência inválida é um aviso, não um erro
    // que esconde o resto.
    let listed = legacy.list()?;
    assert_eq!(listed.len(), 1, "a pasta continua a ser enumerada");
    Ok(())
}

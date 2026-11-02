//! Persistência local da fase 1 e recuperação sem SQLite (secção 3 do plano).
//!
//! Os testes usam uma raiz temporária. O que se fixa aqui: uma falha de escrita
//! conserva a última resposta válida e a revisão anterior, uma transcrição
//! original nunca é substituída, a recuperação avisa por cada artefacto em
//! falta e uma operação que ficou em curso passa a interrompida.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use language_assistant_tutor::audio::CapturedAudio;
use language_assistant_tutor::domain::{
    error_code, AppError, CaptureMeta, OperationKind, OperationRecord, OperationState, Review,
    Segment, Transcript, TranscriptStatus,
};
use language_assistant_tutor::storage::AttemptStore;
use tempfile::TempDir;

/// Raiz temporária, pasta da fase 1 e armazenamento a testar.
fn fixture() -> (TempDir, PathBuf, AttemptStore) {
    let temp = tempfile::tempdir().expect("criar raiz temporária");
    let root = temp.path().join("phase1");
    let store = AttemptStore::new(root.clone());
    (temp, root, store)
}

/// Reserva uma gravação e deixa o áudio original na pasta, como a tarefa 3 fará.
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

fn attempt_path(root: &Path, id: &str) -> PathBuf {
    root.join("attempts").join(id)
}

fn transcript(id: &str, created_at: &str, status: TranscriptStatus, text: &str) -> Transcript {
    Transcript {
        id: id.to_owned(),
        status,
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

fn running(id: &str) -> OperationRecord {
    OperationRecord {
        id: id.to_owned(),
        kind: OperationKind::Transcribe,
        state: OperationState::Running,
        error: None,
    }
}

fn failed(id: &str) -> OperationRecord {
    OperationRecord {
        id: id.to_owned(),
        kind: OperationKind::Transcribe,
        state: OperationState::Failed,
        error: Some(AppError::new(
            error_code::WORKER_TIMEOUT,
            "O auxiliar não respondeu a tempo.",
            true,
        )),
    }
}

/// Impede a substituição de um ficheiro já existente.
///
/// Em Windows o ficheiro fica só de leitura e `MoveFileExW` recusa substituí-lo;
/// nos restantes sistemas a pasta deixa de aceitar o ficheiro temporário. É a
/// falha injetada entre a escrita do temporário e a substituição atómica.
struct BlockedWrites {
    path: PathBuf,
}

impl Drop for BlockedWrites {
    // Em Windows `set_readonly(false)` limpa o atributo FILE_ATTRIBUTE_READONLY;
    // o lint avisa do significado diferente em Unix, onde este ramo não corre.
    #[allow(clippy::permissions_set_readonly_false)]
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            let mut permissions = fs::metadata(&self.path)
                .expect("metadados do ficheiro bloqueado")
                .permissions();
            permissions.set_readonly(false);
            fs::set_permissions(&self.path, permissions).expect("desbloquear escrita");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let parent = self.path.parent().expect("pasta do ficheiro bloqueado");
            fs::set_permissions(parent, fs::Permissions::from_mode(0o755))
                .expect("desbloquear escrita");
        }
    }
}

fn block_writes(path: &Path) -> BlockedWrites {
    #[cfg(windows)]
    {
        let mut permissions = fs::metadata(path)
            .expect("metadados do ficheiro a bloquear")
            .permissions();
        permissions.set_readonly(true);
        fs::set_permissions(path, permissions).expect("bloquear a substituição");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let parent = path.parent().expect("pasta do ficheiro a bloquear");
        fs::set_permissions(parent, fs::Permissions::from_mode(0o555)).expect("bloquear a escrita");
    }
    BlockedWrites {
        path: path.to_path_buf(),
    }
}

fn manifest_json(directory: &Path) -> serde_json::Value {
    let path = directory.join("attempt.json");
    let raw =
        fs::read_to_string(&path).unwrap_or_else(|error| panic!("ler {}: {error}", path.display()));
    serde_json::from_str(&raw).expect("o manifesto deve ser JSON válido")
}

#[test]
fn failed_publish_preserves_last() -> Result<(), AppError> {
    let (_temp, root, store) = fixture();
    let previous = store.publish(record(&store, b"RIFF primeira resposta"))?;
    let previous_id = previous.id.clone();

    let blocked = block_writes(&root.join("last-attempt.json"));
    let next = record(&store, b"RIFF segunda resposta");
    let error = store
        .publish(next.clone())
        .expect_err("publicar com o apontador bloqueado deve falhar");
    assert_eq!(error.code, error_code::STORAGE_WRITE_FAILED);
    assert!(error.retryable, "a publicação pode ser repetida");
    assert!(
        error.message.contains("last-attempt.json"),
        "a falha é na substituição do apontador: {}",
        error.message
    );

    assert_eq!(store.restore_last()?.attempt.unwrap().id, previous_id);

    // A gravação nova ficou no disco, à espera de nova publicação.
    let unpublished = attempt_path(&root, &next.id);
    assert!(unpublished.join("attempt.json").is_file());
    assert!(unpublished.join("original.wav").is_file());

    drop(blocked);
    let published = store.publish(next)?;
    assert_ne!(published.id, previous_id, "cada gravação tem pasta própria");
    assert_eq!(store.restore_last()?.attempt.unwrap().id, published.id);
    Ok(())
}

#[test]
fn failed_review_write_preserves_saved_text() -> Result<(), AppError> {
    let (_temp, root, store) = fixture();
    let attempt = store.publish(record(&store, b"RIFF resposta"))?;
    store.append_transcript(
        &attempt.id,
        transcript(
            "request-1",
            "2026-11-01T10:00:00.000Z",
            TranscriptStatus::Completed,
            "I solved it.",
        ),
    )?;
    let saved = store.save_review(&attempt.id, "request-1", "original review")?;
    assert_eq!(saved.source_transcript_id, "request-1");

    let read_saved_review = || -> Result<Review, AppError> {
        store
            .restore_last()?
            .attempt
            .expect("a última resposta continua publicada")
            .review
            .ok_or_else(|| {
                AppError::new(error_code::ARTIFACT_MISSING, "Sem revisão guardada.", false)
            })
    };

    let blocked = block_writes(&attempt_path(&root, &attempt.id).join("review.json"));
    let error = store
        .save_review(&attempt.id, "request-1", "segunda revisão")
        .expect_err("a substituição bloqueada deve falhar");
    assert_eq!(error.code, error_code::STORAGE_WRITE_FAILED);
    assert!(error.retryable);
    assert!(
        error.message.contains("review.json"),
        "a falha é na substituição da revisão: {}",
        error.message
    );

    assert_eq!(read_saved_review()?.text, "original review");

    // Substituição real de um JSON já existente, depois de desbloquear.
    drop(blocked);
    let replaced = store.save_review(&attempt.id, "request-1", "segunda revisão")?;
    assert_eq!(replaced.text, "segunda revisão");
    assert_eq!(read_saved_review()?.text, "segunda revisão");

    // Uma revisão não pode apontar para uma transcrição de outra resposta.
    let stale = store
        .save_review(&attempt.id, "request-2", "revisão órfã")
        .expect_err("a fonte da revisão tem de existir na resposta");
    assert_eq!(stale.code, error_code::STALE_SOURCE);
    assert_eq!(read_saved_review()?.text, "segunda revisão");
    Ok(())
}

#[test]
fn restore_without_previous_attempt_is_empty() -> Result<(), AppError> {
    let (_temp, _root, store) = fixture();

    let restored = store.restore_last()?;
    assert!(restored.attempt.is_none());
    assert!(restored.warnings.is_empty());
    Ok(())
}

#[test]
fn corrupt_pointer_is_recoverable() -> Result<(), AppError> {
    let (_temp, root, store) = fixture();
    let previous = store.publish(record(&store, b"RIFF resposta"))?;
    let directory = attempt_path(&root, &previous.id);
    let audio_before = fs::read(directory.join("original.wav")).expect("ler o áudio original");

    fs::write(root.join("last-attempt.json"), b"{ isto nao e json").expect("corromper o apontador");

    let error = store
        .restore_last()
        .expect_err("um apontador ilegível devolve erro recuperável");
    assert_eq!(error.code, error_code::STORAGE_WRITE_FAILED);
    assert!(error.retryable);
    // Nada foi eliminado e a aplicação continua a poder gravar.
    assert_eq!(
        fs::read(directory.join("original.wav")).expect("o áudio continua no disco"),
        audio_before
    );
    let published = store.publish(record(&store, b"RIFF resposta nova"))?;
    assert_ne!(published.id, previous.id);
    assert_eq!(store.restore_last()?.attempt.unwrap().id, published.id);
    Ok(())
}

#[test]
fn append_does_not_overwrite_raw() -> Result<(), AppError> {
    let (_temp, _root, store) = fixture();
    let attempt = store.publish(record(&store, b"RIFF resposta"))?;

    let raw = transcript(
        "request-1",
        "2026-11-01T10:00:00.000Z",
        TranscriptStatus::Completed,
        "Tell me about the problem.",
    );
    store.append_transcript(&attempt.id, raw.clone())?;

    // O mesmo pedido repetido não substitui o texto original já gravado.
    store.append_transcript(
        &attempt.id,
        transcript(
            "request-1",
            "2026-11-01T10:00:00.000Z",
            TranscriptStatus::Completed,
            "texto diferente",
        ),
    )?;

    store.append_transcript(
        &attempt.id,
        transcript(
            "request-0",
            "2026-11-01T09:00:00.000Z",
            TranscriptStatus::NoSpeech,
            "",
        ),
    )?;

    let restored = store
        .restore_last()?
        .attempt
        .expect("a resposta continua publicada");
    assert_eq!(restored.transcripts.len(), 2);
    assert_eq!(
        restored.transcripts[0].id, "request-0",
        "ordenadas por data"
    );
    assert_eq!(
        restored.transcripts[1].text, raw.text,
        "a transcrição original mantém-se intacta"
    );

    // Datas iguais desempatam pelo identificador.
    store.append_transcript(
        &attempt.id,
        transcript(
            "aaa",
            "2026-11-01T09:00:00.000Z",
            TranscriptStatus::Completed,
            "outra",
        ),
    )?;
    let ordered: Vec<String> = store
        .restore_last()?
        .attempt
        .expect("a resposta continua publicada")
        .transcripts
        .into_iter()
        .map(|entry| entry.id)
        .collect();
    assert_eq!(ordered, ["aaa", "request-0", "request-1"]);
    Ok(())
}

#[test]
fn restore_reports_missing_original() -> Result<(), AppError> {
    let (_temp, root, store) = fixture();
    let attempt = store.publish(record(&store, b"RIFF resposta"))?;
    store.append_transcript(
        &attempt.id,
        transcript(
            "request-1",
            "2026-11-01T10:00:00.000Z",
            TranscriptStatus::Completed,
            "I solved it.",
        ),
    )?;
    store.save_review(&attempt.id, "request-1", "revisão guardada")?;

    let directory = attempt_path(&root, &attempt.id);
    let audio = directory.join("original.wav");
    let bytes = fs::read(&audio).expect("ler o áudio original");
    fs::remove_file(&audio).expect("remover o áudio original");

    // O manifesto também referencia um derivado que não existe.
    let manifest_path = directory.join("attempt.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).expect("ler o manifesto"))
            .expect("o manifesto é JSON");
    manifest["normalizedFile"] = serde_json::json!("normalized.wav");
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).expect("serializar o manifesto"),
    )
    .expect("guardar o manifesto");

    let restored = store.restore_last()?;
    let attempt_after = restored
        .attempt
        .expect("o texto e a revisão continuam recuperáveis");
    assert_eq!(attempt_after.transcripts.len(), 1);
    assert_eq!(
        attempt_after
            .review
            .as_ref()
            .map(|review| review.text.as_str()),
        Some("revisão guardada")
    );
    assert_eq!(attempt_after.original_file, "original.wav");
    let missing: Vec<&str> = restored
        .warnings
        .iter()
        .filter(|warning| warning.code == error_code::ARTIFACT_MISSING)
        .map(|warning| warning.message.as_str())
        .collect();
    assert_eq!(missing.len(), 2, "um aviso por artefacto: {missing:?}");
    assert!(
        restored.warnings.iter().all(|warning| !warning.retryable),
        "um ficheiro em falta não se resolve a repetir"
    );

    // Devolver o original permite retomar a transcrição sem perder nada.
    fs::write(&audio, &bytes).expect("repor o áudio original");
    let recovered = store.restore_last()?;
    assert_eq!(recovered.warnings.len(), 1, "o derivado continua em falta");

    fs::write(directory.join("normalized.wav"), b"RIFF normalizado").expect("criar o derivado");
    assert!(
        store.restore_last()?.warnings.is_empty(),
        "sem avisos quando os ficheiros existem"
    );
    Ok(())
}

#[test]
fn restore_interrupted_job() -> Result<(), AppError> {
    let (_temp, _root, store) = fixture();
    let attempt = store.publish(record(&store, b"RIFF resposta"))?;
    store.set_operation(&attempt.id, running("request-1"))?;

    let restored = store
        .restore_last()?
        .attempt
        .expect("a resposta continua publicada");
    assert_eq!(
        restored.last_operation.unwrap().state,
        OperationState::Interrupted
    );

    // Um resultado já gravado recompõe a operação que ficou marcada em curso:
    // é a janela em que a transcrição existe mas o manifesto ainda diz running.
    let other = store.publish(record(&store, b"RIFF outra resposta"))?;
    store.append_transcript(
        &other.id,
        transcript(
            "request-2",
            "2026-11-01T11:00:00.000Z",
            TranscriptStatus::NoSpeech,
            "",
        ),
    )?;
    store.set_operation(&other.id, running("request-2"))?;

    let healed = store
        .restore_last()?
        .attempt
        .expect("a resposta continua publicada");
    assert_eq!(
        healed.last_operation.unwrap().state,
        OperationState::NoSpeech
    );
    Ok(())
}

#[test]
fn replaces_existing_json_atomically() -> Result<(), AppError> {
    let (_temp, root, store) = fixture();
    let attempt = store.publish(record(&store, b"RIFF resposta"))?;
    store.append_transcript(
        &attempt.id,
        transcript(
            "request-1",
            "2026-11-01T10:00:00.000Z",
            TranscriptStatus::Completed,
            "texto",
        ),
    )?;
    let directory = attempt_path(&root, &attempt.id);

    store.set_operation(&attempt.id, running("request-1"))?;
    store.set_operation(&attempt.id, failed("request-1"))?;

    let manifest = manifest_json(&directory);
    assert_eq!(manifest["schemaVersion"], 1);
    assert_eq!(manifest["lastOperation"]["state"], "failed");
    assert_eq!(
        manifest["lastOperation"]["error"]["code"],
        error_code::WORKER_TIMEOUT
    );
    assert_eq!(manifest["originalFile"], "original.wav");
    assert!(
        manifest.get("transcripts").is_none(),
        "o manifesto não duplica transcrições"
    );
    assert!(
        manifest.get("review").is_none(),
        "o manifesto não duplica a revisão"
    );

    store.save_review(&attempt.id, "request-1", "primeira versão")?;
    let review_path = directory.join("review.json");
    let before = fs::read(&review_path).expect("ler a revisão guardada");
    store.save_review(&attempt.id, "request-1", "segunda versão")?;
    let after = fs::read(&review_path).expect("ler a revisão substituída");
    assert_ne!(before, after, "o ficheiro existente foi substituído");

    let names: BTreeSet<String> = fs::read_dir(&directory)
        .expect("listar a pasta da resposta")
        .map(|entry| {
            entry
                .expect("entrada da pasta")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    let expected: BTreeSet<String> = ["attempt.json", "original.wav", "review.json", "transcripts"]
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    assert_eq!(names, expected, "sem restos de ficheiros temporários");
    Ok(())
}

#[test]
fn set_normalized_registers_derived_file() -> Result<(), AppError> {
    let (_temp, root, store) = fixture();
    let attempt = store.publish(record(&store, b"RIFF resposta"))?;
    let directory = attempt_path(&root, &attempt.id);
    assert!(attempt.normalized_file.is_none(), "ainda não há derivado");

    fs::write(directory.join("normalized.wav"), b"RIFF normalizado")
        .expect("criar o derivado normalizado");

    let updated = store.set_normalized(&attempt.id, "normalized.wav")?;
    assert_eq!(
        updated.normalized_file.as_deref(),
        Some("normalized.wav"),
        "o derivado fica registado no manifesto"
    );

    let restored = store.restore_last()?;
    let attempt_after = restored.attempt.expect("a resposta continua publicada");
    assert_eq!(
        attempt_after.normalized_file.as_deref(),
        Some("normalized.wav")
    );
    assert!(
        restored.warnings.is_empty(),
        "o derivado existe, logo não há avisos: {:?}",
        restored.warnings
    );
    assert_eq!(
        manifest_json(&directory)["normalizedFile"],
        "normalized.wav"
    );
    Ok(())
}

#[test]
fn set_normalized_rejects_invalid_reference() -> Result<(), AppError> {
    let (_temp, root, store) = fixture();
    let attempt = store.publish(record(&store, b"RIFF resposta"))?;
    let directory = attempt_path(&root, &attempt.id);
    fs::write(root.join("evil.wav"), b"RIFF fora da pasta").expect("criar ficheiro fora");

    let escaping = store
        .set_normalized(&attempt.id, "../evil.wav")
        .expect_err("uma referência que sai da pasta deve ser recusada");
    assert_eq!(escaping.code, error_code::ARTIFACT_MISSING);
    assert!(!escaping.retryable);

    let missing = store
        .set_normalized(&attempt.id, "normalized.wav")
        .expect_err("um derivado inexistente não pode ser registado");
    assert_eq!(missing.code, error_code::ARTIFACT_MISSING);
    assert!(!missing.retryable);

    // Nada foi registado: a resposta continua sem derivado.
    assert!(manifest_json(&directory)["normalizedFile"].is_null());
    let restored = store
        .restore_last()?
        .attempt
        .expect("a resposta continua publicada");
    assert!(restored.normalized_file.is_none());
    Ok(())
}

#[test]
fn failed_write_before_replace_keeps_data() -> Result<(), AppError> {
    let (_temp, root, store) = fixture();
    let attempt = store.publish(record(&store, b"RIFF resposta"))?;
    let directory = attempt_path(&root, &attempt.id);

    // O caminho das transcrições está ocupado por um ficheiro: a escrita falha
    // antes de tocar no manifesto ou nos dados já guardados.
    let occupied = directory.join("transcripts");
    fs::write(&occupied, b"nao e uma pasta").expect("ocupar o caminho das transcrições");

    let error = store
        .append_transcript(
            &attempt.id,
            transcript(
                "request-1",
                "2026-11-01T10:00:00.000Z",
                TranscriptStatus::Completed,
                "texto",
            ),
        )
        .expect_err("a escrita deve falhar antes de substituir o manifesto");
    assert_eq!(error.code, error_code::STORAGE_WRITE_FAILED);
    assert!(error.retryable);

    let restored = store
        .restore_last()?
        .attempt
        .expect("a resposta continua publicada");
    assert!(restored.transcripts.is_empty());
    assert!(restored.last_operation.is_none());

    let missing = store
        .set_operation("resposta-inexistente", running("request-1"))
        .expect_err("uma resposta inexistente não recebe operações");
    assert_eq!(missing.code, error_code::ARTIFACT_MISSING);

    // Libertar o caminho permite retomar a transcrição sem perder a gravação.
    fs::remove_file(&occupied).expect("libertar o caminho das transcrições");
    let updated = store.append_transcript(
        &attempt.id,
        transcript(
            "request-1",
            "2026-11-01T10:00:00.000Z",
            TranscriptStatus::Completed,
            "texto",
        ),
    )?;
    assert_eq!(updated.transcripts.len(), 1);
    Ok(())
}

#[test]
fn allocate_never_reuses_a_directory() -> Result<(), AppError> {
    let (_temp, root, store) = fixture();
    let mut seen = BTreeSet::new();
    for _ in 0..8 {
        let target = store.allocate()?;
        assert!(seen.insert(target.id.clone()), "pasta repetida: {target:?}");
        assert_eq!(target.directory, root.join("attempts").join(&target.id));
        assert!(target.directory.is_dir(), "a pasta é criada ao reservar");
    }
    Ok(())
}

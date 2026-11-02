//! Artefactos de áudio, publicações interrompidas e reconciliação (tarefa 4).
//!
//! Os testes usam diretórios temporários reais, WAV escritos com Hound e só a
//! API pública da base e do armazenamento. Não há microfone nem modelos: o que
//! se verifica é que o ficheiro finalizado sobrevive a uma falha da base, que um
//! provisório nunca é apresentado como resposta, que uma publicação antiga não
//! substitui uma seleção mais recente e que a falta de áudio conserva os textos.

use std::fs;
use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavSpec, WavWriter};
use rusqlite::Connection;
use serde_json::json;

use language_assistant_tutor::artifacts::{reconcile, ArtifactStore};
use language_assistant_tutor::audio::capture::PARTIAL_FILE;
use language_assistant_tutor::audio::{CapturedAudio, RecordingTarget};
use language_assistant_tutor::db::Database;
use language_assistant_tutor::domain::{
    error_code, AppError, Segment, Transcript, TranscriptStatus,
};
use language_assistant_tutor::interview::{
    OperationTarget, RecordingState, RecordingView, StartSession,
};

/// Base temporária com a raiz de artefactos ao lado, como no arranque real.
struct Harness {
    directory: tempfile::TempDir,
    path: PathBuf,
    database: Database,
    artifacts: ArtifactStore,
}

fn harness() -> Harness {
    let directory = tempfile::tempdir().expect("criar o diretório temporário");
    let path = directory.path().join("tutor.sqlite3");
    let database = Database::open(path.clone()).expect("abrir a base de teste");
    let artifacts = ArtifactStore::new(directory.path().join("sessions"));
    Harness {
        directory,
        path,
        database,
        artifacts,
    }
}

impl Harness {
    /// Reabre a base como num reinício, mantendo a mesma raiz de artefactos.
    fn reopen(&self) -> Database {
        Database::open(self.path.clone()).expect("reabrir a base de teste")
    }

    /// Sessão com uma pergunta sorteada; devolve os dois identificadores.
    fn session(&self) -> (String, String) {
        let detail = self
            .database
            .start_session(StartSession {
                request_id: "req-artifacts".to_owned(),
                category: None,
                count: 1,
            })
            .expect("iniciar a sessão de teste");
        (detail.summary.id, detail.questions[0].question.id.clone())
    }

    /// Reserva a linha na base e depois a pasta da gravação.
    fn reserve(&self, session: &str, question: &str, expected: u64) -> (RecordingTarget, String) {
        let view = self
            .database
            .reserve_recording(question, expected)
            .expect("reservar a gravação");
        let target = self
            .artifacts
            .reserve(session, question, &view.id)
            .expect("reservar a pasta da gravação");
        (target, view.id)
    }

    fn detail(&self, session: &str) -> language_assistant_tutor::interview::SessionDetail {
        self.database
            .load_session(session)
            .expect("carregar a sessão de teste")
    }
}

/// WAV PCM16 mono a 48 kHz com 480 amostras (10 ms).
fn write_wav(path: &Path) {
    let spec = WavSpec {
        channels: 1,
        sample_rate: 48_000,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).expect("criar o WAV de teste");
    for sample in 0..480i16 {
        writer.write_sample(sample).expect("escrever uma amostra");
    }
    writer.finalize().expect("fechar o WAV de teste");
}

/// Escreve o original finalizado e devolve o áudio validado.
fn finalized(harness: &Harness, target: &RecordingTarget) -> CapturedAudio {
    write_wav(&target.directory.join("original.wav"));
    harness
        .artifacts
        .inspect(target)
        .expect("inspecionar um original finalizado")
}

fn transcript(id: &str, text: &str, created_at: &str) -> Transcript {
    Transcript {
        id: id.to_owned(),
        status: TranscriptStatus::Completed,
        text: text.to_owned(),
        segments: vec![Segment {
            start_ms: 0,
            end_ms: 1_200,
            text: text.to_owned(),
        }],
        model: "small".to_owned(),
        language: "en".to_owned(),
        created_at: created_at.to_owned(),
        load_ms: 10,
        processing_ms: 20,
    }
}

fn target_of(
    session: &str,
    question: &str,
    recording: &str,
    selection_version: u64,
    operation: &str,
) -> OperationTarget {
    OperationTarget {
        session_id: session.to_owned(),
        session_question_id: question.to_owned(),
        recording_id: Some(recording.to_owned()),
        selection_version,
        operation_id: operation.to_owned(),
        worker_generation: 2,
    }
}

fn test_error(error: rusqlite::Error) -> AppError {
    AppError::new("TEST_SQLITE", error.to_string(), false)
}

/// Executa SQL de teste (falhas injetadas, fixtures de tabelas da tarefa 5).
fn execute(database: &Database, sql: &str) {
    database
        .with_connection(|connection: &mut Connection| {
            connection.execute_batch(sql).map_err(test_error)
        })
        .expect("executar o SQL de teste");
}

fn count(database: &Database, sql: &str) -> i64 {
    database
        .with_connection(|connection| {
            connection
                .query_row(sql, [], |row| row.get::<_, i64>(0))
                .map_err(test_error)
        })
        .expect("contar as linhas de teste")
}

/// Cria uma ligação de diretório: símbolo quando permitido pelo sistema, junção
/// caso contrário (que não exige privilégios). Devolve o motivo da falha.
#[cfg(windows)]
fn create_directory_link(target: &Path, link: &Path) -> Result<(), String> {
    use std::os::windows::fs::symlink_dir;

    if symlink_dir(target, link).is_ok() {
        return Ok(());
    }
    let output = std::process::Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg("/J")
        .arg(link)
        .arg(target)
        .output()
        .map_err(|error| format!("não foi possível executar mklink: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "mklink /J falhou ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(not(windows))]
fn create_directory_link(target: &Path, link: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(target, link).map_err(|error| error.to_string())
}

/// Gravação `ready` de uma pergunta, pela ordem guardada.
fn recordings(detail: &language_assistant_tutor::interview::SessionDetail) -> &[RecordingView] {
    &detail.questions[0].recordings
}

#[test]
fn finalized_audio_survives_database_failure() {
    let harness = harness();
    let (session, question) = harness.session();
    let (target, recording_id) = harness.reserve(&session, &question, 0);
    let capture = finalized(&harness, &target);
    let bytes = fs::metadata(&capture.original)
        .expect("ler o tamanho do original")
        .len();
    assert!(capture.meta.duration_ms > 0);

    // Base bloqueada depois de o WAV estar fechado.
    execute(
        &harness.database,
        "CREATE TRIGGER fail_recording_publication
         BEFORE UPDATE ON recordings
         BEGIN
             SELECT RAISE(ABORT, 'falha injetada na publicação');
         END;",
    );
    let error = harness
        .database
        .publish_recording(capture.clone())
        .expect_err("a publicação deve falhar");
    assert_eq!(error.code, error_code::STORAGE_WRITE_FAILED);
    assert!(error.retryable);
    assert!(capture.original.is_file(), "o WAV finalizado é conservado");
    assert_eq!(
        fs::metadata(&capture.original)
            .expect("reler o tamanho do original")
            .len(),
        bytes,
        "o ficheiro não é truncado nem reescrito"
    );

    let detail = harness.detail(&session);
    let recording = &recordings(&detail)[0];
    assert_eq!(recording.state, RecordingState::Reserved);
    assert!(recording.original_file.is_none());
    assert_eq!(detail.questions[0].question.selection_version, 0);

    execute(
        &harness.database,
        "DROP TRIGGER fail_recording_publication;",
    );

    // Reinício: a reconciliação publica sem nova captura e sem duplicar linhas.
    let reopened = harness.reopen();
    let report = reconcile(&reopened, &harness.artifacts).expect("reconciliar no arranque");
    assert_eq!(report.published, vec![recording_id.clone()]);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);

    let detail = reopened
        .load_session(&session)
        .expect("carregar a sessão reconciliada");
    let stored = recordings(&detail);
    assert_eq!(stored.len(), 1, "uma reserva gera uma única gravação");
    let recording = &stored[0];
    assert_eq!(recording.state, RecordingState::Ready);
    let reference = recording
        .original_file
        .as_deref()
        .expect("a gravação pronta indica o original");
    assert_eq!(
        fs::metadata(
            harness
                .artifacts
                .resolve(reference)
                .expect("resolver o original")
        )
        .expect("ler o original publicado")
        .len(),
        bytes,
        "o original publicado é exatamente o ficheiro que sobreviveu"
    );
    let capture = recording
        .capture
        .as_ref()
        .expect("a gravação pronta tem metadados");
    assert!(
        capture.incomplete,
        "sem o fecho confirmado a publicação é marcada incompleta"
    );
    assert!(capture.duration_ms > 0, "a duração vem do ficheiro real");
    assert!(recording.warnings.is_empty());
    assert_eq!(
        detail.questions[0]
            .question
            .selected_recording_id
            .as_deref(),
        Some(recording_id.as_str())
    );
    assert_eq!(detail.questions[0].question.selection_version, 1);

    // Uma segunda reconciliação não repete a publicação.
    let again = reconcile(&reopened, &harness.artifacts).expect("reconciliar outra vez");
    assert!(again.published.is_empty());
    assert_eq!(count(&reopened, "SELECT count(*) FROM recordings"), 1);
}

#[test]
fn reopen_reconciles_once() {
    let harness = harness();
    let (session, question) = harness.session();

    // Já publicada: não é pendente e não pode ser tocada.
    let (first, first_id) = harness.reserve(&session, &question, 0);
    harness
        .database
        .publish_recording(finalized(&harness, &first))
        .expect("publicar a primeira gravação");

    // Fechada mas não publicada.
    let (second, second_id) = harness.reserve(&session, &question, 1);
    write_wav(&second.directory.join("original.wav"));

    // Só com provisório e sem ficheiro nenhum: capturas interrompidas.
    let (partial, partial_id) = harness.reserve(&session, &question, 1);
    write_wav(&partial.directory.join(PARTIAL_FILE));
    let (empty, empty_id) = harness.reserve(&session, &question, 1);

    execute(
        &harness.database,
        &format!(
            "INSERT INTO operations
               (id, kind, state, session_id, session_question_id, recording_id,
                worker_generation, created_at, finished_at)
             VALUES ('op-stale', 'transcribe', 'running', '{session}', '{question}',
                     '{first_id}', 2, '2026-11-02T10:00:00Z', NULL);"
        ),
    );

    let report = reconcile(&harness.database, &harness.artifacts).expect("reconciliar");
    assert_eq!(report.published, vec![second_id.clone()]);
    assert_eq!(report.interrupted_operations, vec!["op-stale".to_owned()]);
    assert_eq!(
        report.warnings.len(),
        2,
        "cada captura interrompida gera o seu aviso: {:?}",
        report.warnings
    );

    // Segundo arranque: nada a publicar, nada a interromper, nenhuma linha nova.
    let reopened = harness.reopen();
    let store = ArtifactStore::new(harness.directory.path().join("sessions"));
    let second_report = reconcile(&reopened, &store).expect("reconciliar outra vez");
    assert!(second_report.published.is_empty());
    assert!(second_report.interrupted_operations.is_empty());
    assert!(
        second_report.warnings.is_empty(),
        "{:?}",
        second_report.warnings
    );

    let detail = reopened
        .load_session(&session)
        .expect("carregar a sessão reconciliada");
    let stored = recordings(&detail);
    assert_eq!(stored.len(), 4, "quatro reservas, quatro gravações");
    let state = |id: &str| {
        stored
            .iter()
            .find(|recording| recording.id == id)
            .expect("gravação guardada")
            .state
    };
    assert_eq!(state(&first_id), RecordingState::Ready);
    assert_eq!(state(&second_id), RecordingState::Ready);
    assert_eq!(state(&partial_id), RecordingState::Interrupted);
    assert_eq!(state(&empty_id), RecordingState::Interrupted);

    let partial_view = stored
        .iter()
        .find(|recording| recording.id == partial_id)
        .expect("gravação com provisório");
    assert!(partial_view.original_file.is_none());
    assert!(partial_view.capture.is_none(), "sem duração inventada");
    assert!(partial_view.transcripts.is_empty());

    assert!(
        partial.directory.join(PARTIAL_FILE).is_file(),
        "o provisório é conservado, nunca eliminado"
    );
    assert!(second.directory.join("original.wav").is_file());
    assert!(
        empty.directory.is_dir(),
        "a pasta de uma reserva desconhecida não é apagada"
    );
    assert_eq!(
        count(&reopened, "SELECT count(*) FROM operations WHERE state = 'interrupted' AND finished_at IS NOT NULL"),
        1
    );
}

/// No Windows, um ficheiro aberto sem partilha de leitura reproduz o indexador
/// ou o antivírus a segurar o WAV finalizado. O alvo da fase 2 é Windows; noutro
/// sistema o teste não é compilado (não passa em silêncio, simplesmente não
/// existe, porque o bloqueio de partilha também não existe).
#[test]
#[cfg(windows)]
fn locked_original_stays_publishable_for_the_next_start() {
    use std::os::windows::fs::OpenOptionsExt;

    let harness = harness();
    let (session, question) = harness.session();
    let (target, recording_id) = harness.reserve(&session, &question, 0);
    write_wav(&target.directory.join("original.wav"));

    // `share_mode(0)` recusa qualquer outra abertura do ficheiro.
    let locked = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(target.directory.join("original.wav"))
        .expect("bloquear o WAV de teste");

    let report =
        reconcile(&harness.database, &harness.artifacts).expect("reconciliar com bloqueio");
    assert!(
        report.published.is_empty(),
        "um ficheiro bloqueado não é publicado"
    );
    assert!(
        !report.warnings.is_empty(),
        "o bloqueio produz um aviso recuperável"
    );
    assert!(
        report.warnings.iter().all(|warning| warning.retryable),
        "só avisos repetíveis: {:?}",
        report.warnings
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.code == error_code::STORAGE_WRITE_FAILED),
        "o bloqueio é reportado como falha de acesso repetível: {:?}",
        report.warnings
    );

    let detail = harness.detail(&session);
    let recording = &recordings(&detail)[0];
    assert_eq!(
        recording.state,
        RecordingState::Reserved,
        "a reserva continua pendente para o próximo arranque"
    );
    assert!(recording.original_file.is_none());
    assert!(
        target.directory.join("original.wav").is_file(),
        "o ficheiro bloqueado nunca é eliminado"
    );

    // Libertar o bloqueio (o antivírus termina) permite publicar sem nova captura.
    drop(locked);
    let report =
        reconcile(&harness.database, &harness.artifacts).expect("reconciliar sem bloqueio");
    assert_eq!(report.published, vec![recording_id.clone()]);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);

    let detail = harness.detail(&session);
    let stored = recordings(&detail);
    assert_eq!(stored.len(), 1, "uma só gravação, uma só publicação");
    assert_eq!(stored[0].state, RecordingState::Ready);
    assert!(stored[0].original_file.is_some());
    assert_eq!(
        count(&harness.database, "SELECT count(*) FROM recordings"),
        1
    );
}

#[test]
fn part_file_is_not_a_saved_answer() {
    let harness = harness();
    let (session, question) = harness.session();
    let (target, recording_id) = harness.reserve(&session, &question, 0);

    // Um provisório com um WAV válido continua a não ser um original fechado.
    write_wav(&target.directory.join(PARTIAL_FILE));
    let error = harness
        .artifacts
        .inspect(&target)
        .expect_err("um provisório nunca é inspecionado como original");
    assert_eq!(error.code, error_code::ARTIFACT_MISSING);
    assert!(!target.directory.join("original.wav").exists());

    let report = reconcile(&harness.database, &harness.artifacts).expect("reconciliar");
    assert!(report.published.is_empty());
    assert_eq!(report.warnings.len(), 1);

    let detail = harness.detail(&session);
    let recording = &recordings(&detail)[0];
    assert_eq!(recording.id, recording_id);
    assert_eq!(recording.state, RecordingState::Interrupted);
    assert!(recording.original_file.is_none());
    assert!(
        recording.capture.is_none(),
        "uma captura interrompida não inventa duração nem sucesso"
    );
    assert_eq!(recording.ordinal, 1);
    assert_eq!(detail.questions[0].question.selection_version, 0);
    assert!(
        target.directory.join(PARTIAL_FILE).is_file(),
        "o provisório é conservado para diagnóstico"
    );
}

#[test]
fn older_publication_cannot_replace_newer_selection() {
    let harness = harness();
    let (session, question) = harness.session();

    let (older, older_id) = harness.reserve(&session, &question, 0);
    let (newer, newer_id) = harness.reserve(&session, &question, 0);
    let older_capture = finalized(&harness, &older);
    let newer_capture = finalized(&harness, &newer);

    // A gravação mais recente é publicada primeiro e passa a ser a seleção.
    harness
        .database
        .publish_recording(newer_capture.clone())
        .expect("publicar a gravação mais recente");
    let detail = harness.detail(&session);
    assert_eq!(
        detail.questions[0]
            .question
            .selected_recording_id
            .as_deref(),
        Some(newer_id.as_str())
    );
    assert_eq!(detail.questions[0].question.selection_version, 1);

    // A publicação atrasada da gravação mais antiga fica guardada e legível,
    // mas não substitui a seleção mais recente.
    let older_view = harness
        .database
        .publish_recording(older_capture)
        .expect("publicar a gravação mais antiga");
    assert_eq!(older_view.ordinal, 1);
    assert_eq!(older_view.state, RecordingState::Ready);
    let detail = harness.detail(&session);
    assert_eq!(
        detail.questions[0]
            .question
            .selected_recording_id
            .as_deref(),
        Some(newer_id.as_str())
    );
    assert_eq!(detail.questions[0].question.selection_version, 1);
    let stored = recordings(&detail);
    assert_eq!(stored.len(), 2);
    for (recording, expected) in stored.iter().zip([&older_id, &newer_id]) {
        assert_eq!(&recording.id, expected);
        assert_eq!(recording.state, RecordingState::Ready);
        let reference = recording.original_file.as_deref().expect("original ligado");
        assert!(harness
            .artifacts
            .resolve(reference)
            .expect("resolver o original")
            .is_file());
    }

    // Repetir a publicação mais recente não avança a versão outra vez.
    harness
        .database
        .publish_recording(newer_capture)
        .expect("repetir a publicação");
    let detail = harness.detail(&session);
    assert_eq!(detail.questions[0].question.selection_version, 1);
    assert_eq!(
        detail.questions[0]
            .question
            .selected_recording_id
            .as_deref(),
        Some(newer_id.as_str())
    );

    // Uma captura nova e posterior avança a seleção.
    let (latest, latest_id) = harness.reserve(&session, &question, 1);
    let latest_view = harness
        .database
        .publish_recording(finalized(&harness, &latest))
        .expect("publicar a gravação nova");
    assert_eq!(latest_view.ordinal, 3);
    let detail = harness.detail(&session);
    assert_eq!(
        detail.questions[0]
            .question
            .selected_recording_id
            .as_deref(),
        Some(latest_id.as_str())
    );
    assert_eq!(detail.questions[0].question.selection_version, 2);
    assert_eq!(recordings(&detail).len(), 3);

    // Reservar contra uma seleção já ultrapassada é recusado sem escrever.
    let error = harness
        .database
        .reserve_recording(&question, 1)
        .expect_err("a seleção esperada está desatualizada");
    assert_eq!(error.code, error_code::STALE_SOURCE);
    assert_eq!(
        count(&harness.database, "SELECT count(*) FROM recordings"),
        3
    );
}

#[test]
fn missing_audio_keeps_text() {
    let harness = harness();
    let (session, question) = harness.session();
    let (target, recording_id) = harness.reserve(&session, &question, 0);
    harness
        .database
        .publish_recording(finalized(&harness, &target))
        .expect("publicar a gravação");

    harness
        .database
        .append_transcript(
            target_of(&session, &question, &recording_id, 1, "op-1"),
            transcript("tr-1", "I led the migration.", "2026-11-02T10:00:00Z"),
            json!({ "beamSize": 5 }),
        )
        .expect("guardar a transcrição");

    // A tarefa 5 escreve revisões; aqui basta uma linha direta para provar que a
    // falta de áudio não a apaga.
    execute(
        &harness.database,
        &format!(
            "INSERT INTO reviews
               (recording_id, version, source_transcript_id, origin, text, saved_at)
             VALUES ('{recording_id}', 1, 'tr-1', 'automatic',
                     'I led the migration carefully.', '2026-11-02T10:01:00Z');"
        ),
    );

    fs::remove_file(target.directory.join("original.wav")).expect("remover o original de teste");

    let mut detail = harness.detail(&session);
    let recording = &recordings(&detail)[0];
    assert_eq!(
        recording.state,
        RecordingState::Ready,
        "a base conserva o estado guardado e não inventa outro"
    );
    assert_eq!(recording.transcripts.len(), 1);
    assert_eq!(recording.transcripts[0].text, "I led the migration.");
    assert_eq!(
        recording.review.as_ref().expect("revisão guardada").text,
        "I led the migration carefully."
    );
    assert!(
        recording.warnings.is_empty(),
        "sem a raiz de artefactos a base não verifica ficheiros"
    );

    harness.artifacts.annotate(&mut detail);
    let recording = &recordings(&detail)[0];
    assert!(
        recording
            .warnings
            .iter()
            .any(|warning| warning.code == error_code::ARTIFACT_MISSING),
        "o áudio em falta produz um aviso por artefacto: {:?}",
        recording.warnings
    );
    assert_eq!(
        recording.transcripts[0].text, "I led the migration.",
        "os textos continuam legíveis"
    );
    assert_eq!(
        recording.review.as_ref().expect("revisão guardada").text,
        "I led the migration carefully."
    );
}

#[test]
fn new_capture_failure_keeps_previous_selection() {
    let harness = harness();
    let (session, question) = harness.session();
    let (first, first_id) = harness.reserve(&session, &question, 0);
    harness
        .database
        .publish_recording(finalized(&harness, &first))
        .expect("publicar a primeira gravação");

    // A segunda captura é reservada mas nunca produz um original fechado.
    let (failed, failed_id) = harness.reserve(&session, &question, 1);
    let report = reconcile(&harness.database, &harness.artifacts).expect("reconciliar");
    assert!(!report.published.contains(&failed_id));

    let detail = harness.detail(&session);
    let stored = recordings(&detail);
    let failed_view = stored
        .iter()
        .find(|recording| recording.id == failed_id)
        .expect("gravação falhada");
    assert_eq!(failed_view.state, RecordingState::Interrupted);
    assert_eq!(
        detail.questions[0]
            .question
            .selected_recording_id
            .as_deref(),
        Some(first_id.as_str()),
        "a seleção anterior mantém-se"
    );
    assert_eq!(detail.questions[0].question.selection_version, 1);
    let first_view = stored
        .iter()
        .find(|recording| recording.id == first_id)
        .expect("gravação anterior");
    assert_eq!(first_view.state, RecordingState::Ready);
    assert!(harness
        .artifacts
        .resolve(
            first_view
                .original_file
                .as_deref()
                .expect("original ligado")
        )
        .expect("resolver o original")
        .is_file());

    // Reservar contra a versão antiga é recusado; a versão correta continua a
    // permitir uma tentativa nova, depois da falha.
    let error = harness
        .database
        .reserve_recording(&question, 0)
        .expect_err("a versão da seleção está desatualizada");
    assert_eq!(error.code, error_code::STALE_SOURCE);

    let (retry, retry_id) = harness.reserve(&session, &question, 1);
    let retry_view = harness
        .database
        .publish_recording(finalized(&harness, &retry))
        .expect("publicar a tentativa nova");
    assert_eq!(retry_view.ordinal, 3);
    let detail = harness.detail(&session);
    assert_eq!(
        detail.questions[0]
            .question
            .selected_recording_id
            .as_deref(),
        Some(retry_id.as_str())
    );
    assert_eq!(detail.questions[0].question.selection_version, 2);
    let failed_view = recordings(&detail)
        .iter()
        .find(|recording| recording.id == failed_id)
        .expect("gravação falhada");
    assert_eq!(failed_view.state, RecordingState::Interrupted);
    assert!(
        failed.directory.is_dir(),
        "a pasta da captura falhada não é eliminada"
    );
}

#[test]
fn transcript_append_does_not_replace_review() {
    let harness = harness();
    let (session, question) = harness.session();
    let (target, recording_id) = harness.reserve(&session, &question, 0);
    harness
        .database
        .publish_recording(finalized(&harness, &target))
        .expect("publicar a gravação");

    execute(
        &harness.database,
        "INSERT INTO operations (id, kind, state, created_at, finished_at)
         VALUES ('op-1', 'transcribe', 'running', '2026-11-02T09:59:00Z', NULL);
         INSERT INTO operations (id, kind, state, created_at, finished_at)
         VALUES ('op-2', 'transcribe', 'running', '2026-11-02T10:04:00Z', NULL);",
    );

    let first = harness
        .database
        .append_transcript(
            target_of(&session, &question, &recording_id, 1, "op-1"),
            transcript("tr-1", "First automatic text.", "2026-11-02T10:00:00Z"),
            json!({ "beamSize": 5 }),
        )
        .expect("guardar a primeira transcrição");
    assert_eq!(first.transcripts.len(), 1);

    execute(
        &harness.database,
        &format!(
            "INSERT INTO reviews
               (recording_id, version, source_transcript_id, origin, text, saved_at)
             VALUES ('{recording_id}', 1, 'tr-1', 'manual_explicit',
                     'Reviewed text the user saved.', '2026-11-02T10:02:00Z');"
        ),
    );

    let second = harness
        .database
        .append_transcript(
            target_of(&session, &question, &recording_id, 1, "op-2"),
            transcript("tr-2", "Second automatic text.", "2026-11-02T10:05:00Z"),
            json!({ "beamSize": 8 }),
        )
        .expect("guardar a segunda transcrição");
    assert_eq!(second.transcripts.len(), 2);
    assert_eq!(second.transcripts[0].text, "First automatic text.");
    assert_eq!(second.transcripts[1].text, "Second automatic text.");
    assert_eq!(
        second.review.as_ref().expect("revisão guardada").text,
        "Reviewed text the user saved."
    );
    assert_eq!(second.review.as_ref().expect("revisão guardada").version, 1);

    // Repetir o mesmo resultado não duplica transcrições nem reescreve a revisão.
    let replay = harness
        .database
        .append_transcript(
            target_of(&session, &question, &recording_id, 1, "op-2"),
            transcript("tr-2", "Second automatic text.", "2026-11-02T10:05:00Z"),
            json!({ "beamSize": 8 }),
        )
        .expect("repetir a transcrição");
    assert_eq!(replay.transcripts.len(), 2);
    assert_eq!(
        count(&harness.database, "SELECT count(*) FROM transcripts"),
        2
    );
    assert_eq!(count(&harness.database, "SELECT count(*) FROM reviews"), 1);
    assert_eq!(
        count(
            &harness.database,
            "SELECT count(*) FROM reviews WHERE text = 'Reviewed text the user saved.'"
        ),
        1
    );

    // Um alvo com outra versão de seleção não escreve nada.
    let error = harness
        .database
        .append_transcript(
            target_of(&session, &question, &recording_id, 0, "op-3"),
            transcript("tr-3", "Stale text.", "2026-11-02T10:06:00Z"),
            json!({}),
        )
        .expect_err("a seleção do alvo está desatualizada");
    assert_eq!(error.code, error_code::STALE_SOURCE);
    assert_eq!(
        count(&harness.database, "SELECT count(*) FROM transcripts"),
        2
    );

    // Depois de confirmar, o texto é apenas de leitura e nenhum resultado tardio
    // entra na mesma pergunta.
    execute(
        &harness.database,
        &format!(
            "INSERT INTO confirmations
               (id, session_question_id, recording_id, review_version,
                source_transcript_id, origin, text, confirmed_at)
             VALUES ('c-1', '{question}', '{recording_id}', 1, 'tr-1',
                     'manual_explicit', 'Reviewed text the user saved.',
                     '2026-11-02T10:07:00Z');"
        ),
    );
    let error = harness
        .database
        .append_transcript(
            target_of(&session, &question, &recording_id, 1, "op-4"),
            transcript("tr-4", "Late text.", "2026-11-02T10:08:00Z"),
            json!({}),
        )
        .expect_err("a resposta confirmada é apenas de leitura");
    assert_eq!(error.code, error_code::ANSWER_CONFIRMED);
    assert_eq!(
        count(&harness.database, "SELECT count(*) FROM transcripts"),
        2
    );
    assert_eq!(
        count(
            &harness.database,
            "SELECT count(*) FROM operations WHERE id = 'op-4'"
        ),
        0
    );
}

#[test]
fn paths_cannot_escape_artifact_root() {
    let harness = harness();
    let store = &harness.artifacts;

    for hostile in ["..", ".", "", "a/b", "a\\b", "C:\\Windows", "/etc"] {
        let error = store
            .reserve("session", "question", hostile)
            .expect_err("um identificador hostil é recusado");
        assert_eq!(error.code, error_code::INVALID_INPUT, "«{hostile}»");
        let error = store
            .reserve("session", hostile, "recording")
            .expect_err("uma pergunta hostil é recusada");
        assert_eq!(error.code, error_code::INVALID_INPUT, "«{hostile}»");
        let error = store
            .reserve(hostile, "question", "recording")
            .expect_err("uma sessão hostil é recusada");
        assert_eq!(error.code, error_code::INVALID_INPUT, "«{hostile}»");
    }

    let target = store
        .reserve("session", "question", "recording")
        .expect("reservar com identificadores válidos");
    assert!(target.directory.starts_with(harness.directory.path()));

    // Um alvo construído fora da raiz gerida nunca é inspecionado.
    let outside = tempfile::tempdir().expect("criar um diretório exterior");
    write_wav(&outside.path().join("original.wav"));
    let forged = RecordingTarget {
        id: "recording".to_owned(),
        directory: outside.path().to_path_buf(),
    };
    let error = store
        .inspect(&forged)
        .expect_err("um alvo fora da raiz é recusado");
    assert_eq!(error.code, error_code::INVALID_INPUT);

    // A pasta tem de terminar no identificador da própria gravação.
    let mismatched = RecordingTarget {
        id: "other".to_owned(),
        directory: target.directory.clone(),
    };
    let error = store
        .inspect(&mismatched)
        .expect_err("um identificador que não corresponde à pasta é recusado");
    assert_eq!(error.code, error_code::INVALID_INPUT);

    // Referências guardadas também não podem sair da raiz.
    assert_eq!(
        store.resolve("../original.wav").unwrap_err().code,
        error_code::INVALID_INPUT
    );
    assert_eq!(
        store.resolve("C:\\Windows\\original.wav").unwrap_err().code,
        error_code::INVALID_INPUT
    );
    assert_eq!(
        store
            .resolve("session/../../original.wav")
            .unwrap_err()
            .code,
        error_code::INVALID_INPUT
    );

    // Uma ligação (símbolo ou junção) com nomes válidos mas a apontar para fora
    // da raiz é recusada, apesar de os componentes lexicais estarem corretos.
    let link = harness
        .directory
        .path()
        .join("sessions")
        .join("linked-session")
        .join("linked-question")
        .join("linked-recording");
    fs::create_dir_all(link.parent().expect("pasta pai da ligação"))
        .expect("criar as pastas da ligação");
    match create_directory_link(outside.path(), &link) {
        Ok(()) => {
            let linked = RecordingTarget {
                id: "linked-recording".to_owned(),
                directory: link.clone(),
            };
            assert_eq!(
                store.inspect(&linked).unwrap_err().code,
                error_code::INVALID_INPUT,
                "uma ligação que sai da raiz nunca é inspecionada"
            );
            assert_eq!(
                store
                    .resolve("linked-session/linked-question/linked-recording/original.wav")
                    .unwrap_err()
                    .code,
                error_code::INVALID_INPUT
            );
            assert_eq!(
                store
                    .reserve("linked-session", "linked-question", "linked-recording")
                    .unwrap_err()
                    .code,
                error_code::INVALID_INPUT,
                "uma ligação nunca recebe uma captura"
            );
            assert!(
                outside.path().join("original.wav").is_file(),
                "o alvo da ligação não é lido nem alterado"
            );
            fs::remove_dir(&link).expect("remover a ligação sem seguir o alvo");
            assert!(outside.path().join("original.wav").is_file());
        }
        Err(reason) => {
            eprintln!(
                "SKIP do caso de ligação em paths_cannot_escape_artifact_root: {reason}. \
                 A recusa lexical de `..` e caminhos absolutos continua verificada; \
                 esta máquina não permitiu criar a ligação."
            );
        }
    }

    let (_session, question) = harness.session();
    let view = harness
        .database
        .reserve_recording(&question, 0)
        .expect("reservar a gravação na base");
    let error = harness
        .database
        .set_normalized(&view.id, "../outside/normalized.wav")
        .expect_err("uma referência fora da gravação é recusada");
    assert_eq!(error.code, error_code::INVALID_INPUT);
}

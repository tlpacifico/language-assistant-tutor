//! Regras de domínio da fase 1 (secção 3 do plano técnico).
//!
//! O exemplo partilhado `contracts/app-v1.example.json` é lido aqui para fixar
//! o contrato: JSON em camelCase, enums em snake_case e a pergunta exata.

use std::path::{Path, PathBuf};

use language_assistant_tutor::domain::{
    AppSnapshot, AppState, ComponentStatus, JobStage, ModelJob, OperationKind, OperationState,
    PlaybackTarget, TranscriptStatus, CHECKING_AVAILABILITY, QUESTION,
};

fn transcription_job() -> ModelJob {
    ModelJob {
        id: "job-1".to_owned(),
        attempt_id: Some("attempt-1".to_owned()),
        kind: OperationKind::Transcribe,
        stage: JobStage::Loading,
    }
}

fn example_document() -> serde_json::Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../contracts/app-v1.example.json");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("ler {}: {error}", path.display()));
    serde_json::from_str(&raw).expect("o exemplo partilhado deve ser JSON válido")
}

#[test]
fn recording_gate() {
    // A captura não depende de o processo auxiliar ou os modelos estarem prontos.
    let idle_without_worker = AppState::initial();
    assert!(idle_without_worker.can_start_recording().is_ok());

    let mut recording = AppState::initial();
    recording
        .begin_recording("capture-1")
        .expect("uma gravação deve começar com o estado livre");
    assert_eq!(recording.can_start_recording().unwrap_err().code, "BUSY");

    let mut transcribing = AppState::initial();
    transcribing
        .begin_model_job(transcription_job(), "request-1")
        .expect("uma transcrição deve começar com o estado livre");
    assert_eq!(transcribing.can_start_recording().unwrap_err().code, "BUSY");
}

#[test]
fn stale_generation() {
    let mut waiting = AppState::initial();
    waiting.set_worker_generation(2);
    waiting
        .begin_model_job(transcription_job(), "current-request")
        .expect("o pedido atual deve ficar pendente");

    assert!(!waiting.accepts_result("old-request", 1));
    // Controlo positivo: o pedido pendente e a geração atual são aceites.
    assert!(waiting.accepts_result("current-request", 2));
}

#[test]
fn finished_operation_rejects_late_result() {
    let mut state = AppState::initial();
    state.set_worker_generation(1);
    state
        .begin_model_job(transcription_job(), "request-1")
        .expect("o pedido deve ficar pendente");
    assert!(state.accepts_result("request-1", 1));

    state.finish_model_job();
    assert!(!state.accepts_result("request-1", 1));
}

#[test]
fn initial_snapshot_is_idle_and_unverified() {
    let snapshot = AppState::initial().snapshot();

    assert_eq!(snapshot.revision, 0);
    assert!(snapshot.attempt.is_none());
    assert!(snapshot.recording_id.is_none());
    assert!(snapshot.model_job.is_none());
    assert!(snapshot.playback.is_none());
    assert!(snapshot.last_error.is_none());
    assert_eq!(
        snapshot.question,
        "Tell me about a time you solved a difficult problem at work."
    );
    assert_eq!(snapshot.question, QUESTION);
    assert_eq!(CHECKING_AVAILABILITY, "A verificar disponibilidade");

    let components = [
        &snapshot.availability.microphone,
        &snapshot.availability.playback,
        &snapshot.availability.transcription,
        &snapshot.availability.synthesis,
    ];
    for component in components {
        assert_eq!(component.status, ComponentStatus::Loading);
        assert_eq!(component.message.as_deref(), Some(CHECKING_AVAILABILITY));
    }

    let initial_from_example =
        serde_json::from_value(example_document()["initialSnapshot"].clone())
            .expect("initialSnapshot deve corresponder a AppSnapshot");
    assert_eq!(snapshot, initial_from_example);
}

#[test]
fn shared_example_pins_wire_contract() {
    let document = example_document();

    for key in ["initialSnapshot", "populatedSnapshot"] {
        let snapshot: serde_json::Value = document[key].clone();
        let parsed: AppSnapshot = serde_json::from_value(snapshot.clone())
            .unwrap_or_else(|error| panic!("exemplo {key}: {error}"));
        assert_eq!(
            serde_json::to_value(&parsed).expect("serializar snapshot"),
            snapshot,
            "o exemplo {key} deve refletir o contrato camelCase"
        );
        assert_eq!(
            parsed.question,
            "Tell me about a time you solved a difficult problem at work."
        );
    }

    let populated: AppSnapshot = serde_json::from_value(document["populatedSnapshot"].clone())
        .expect("populatedSnapshot deve corresponder a AppSnapshot");
    let attempt = populated
        .attempt
        .as_ref()
        .expect("o exemplo populado tem uma resposta ativa");
    assert_eq!(attempt.transcripts.len(), 1);
    assert_eq!(attempt.transcripts[0].status, TranscriptStatus::Completed);
    assert_eq!(
        attempt.transcripts[0].model, "small",
        "o modelo fixo da fase 1 é small"
    );
    assert_eq!(attempt.transcripts[0].language, "en");
    assert!(!Path::new(&attempt.original_file).is_absolute());
    let operation = attempt
        .last_operation
        .as_ref()
        .expect("o exemplo populado regista a última operação");
    assert_eq!(operation.kind, OperationKind::Transcribe);
    assert_eq!(operation.state, OperationState::Completed);
    assert_eq!(populated.playback, Some(PlaybackTarget::Response));

    // Valores que o exemplo não cobre ficam fixos aqui.
    assert_eq!(
        serde_json::to_value(TranscriptStatus::NoSpeech).expect("serializar"),
        serde_json::json!("no_speech")
    );
    assert_eq!(
        serde_json::to_value(OperationState::NoSpeech).expect("serializar"),
        serde_json::json!("no_speech")
    );
    assert_eq!(
        serde_json::to_value(OperationState::Interrupted).expect("serializar"),
        serde_json::json!("interrupted")
    );
    assert_eq!(
        serde_json::to_value(OperationKind::Synthesize).expect("serializar"),
        serde_json::json!("synthesize")
    );
    assert_eq!(
        serde_json::to_value(JobStage::Processing).expect("serializar"),
        serde_json::json!("processing")
    );
    assert_eq!(
        serde_json::to_value(ComponentStatus::Missing).expect("serializar"),
        serde_json::json!("missing")
    );
}

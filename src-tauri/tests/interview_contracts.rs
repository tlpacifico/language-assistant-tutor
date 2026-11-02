//! Contratos da fase 2 (secção 3 do plano técnico).
//!
//! O exemplo partilhado `contracts/interview-v2.example.json` é lido com
//! `include_str!` e comparado com a serialização dos tipos Rust: se um campo
//! deixar de ser camelCase, se um enum mudar de nome ou se um tipo ganhar ou
//! perder um campo, este teste falha. Os tipos TypeScript equivalentes vivem em
//! `src/lib/interview-contracts.ts`.

use language_assistant_tutor::domain::error_code;
use language_assistant_tutor::interview::{
    validate_count, validate_question, Category, Confirmation, InterviewSnapshot, ModelJobView,
    OperationTarget, QuestionDetail, QuestionInput, ReviewOrigin, SessionMode, SessionStatus,
    SessionSummary,
};
use serde::{Deserialize, Serialize};

/// Forma do exemplo partilhado. O invólucro existe só no teste; os DTOs
/// comparados são os tipos reais da aplicação.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InterviewContractExample {
    schema_version: u32,
    session: SessionSummary,
    current: QuestionDetail,
    confirmation: Confirmation,
    pending_operation: OperationTarget,
    model_job: ModelJobView,
    snapshot: InterviewSnapshot,
}

fn example() -> InterviewContractExample {
    let raw = include_str!("../../contracts/interview-v2.example.json");
    serde_json::from_str(raw).expect("o exemplo partilhado deve ser um contrato válido")
}

#[test]
fn v2_contract_matches_fixture() {
    let raw = include_str!("../../contracts/interview-v2.example.json");
    let parsed: InterviewContractExample = example();
    let original: serde_json::Value =
        serde_json::from_str(raw).expect("o exemplo partilhado deve ser JSON válido");

    // Igualdade exata: os nomes camelCase e os valores snake_case dos enums não
    // podem divergir entre o exemplo e os tipos Rust.
    assert_eq!(
        serde_json::to_value(&parsed).expect("os DTOs devem serializar"),
        original,
        "a serialização Rust deve coincidir com contracts/interview-v2.example.json"
    );

    assert_eq!(parsed.schema_version, 2);
    assert_eq!(parsed.session.status, SessionStatus::InProgress);
    assert_eq!(parsed.session.mode, SessionMode::LocalV2);
    assert_eq!(parsed.session.count, 3);
    assert_eq!(parsed.session.confirmed_count, 1);
    assert_eq!(parsed.session.position, 2);
    assert!(parsed.session.completed_at.is_none());

    let recording = parsed
        .current
        .recordings
        .first()
        .expect("o exemplo deve ter uma gravação na pergunta corrente");
    assert_eq!(recording.state.as_str(), "ready");
    assert_eq!(recording.transcripts.len(), 1);
    let review = recording
        .review
        .as_ref()
        .expect("o exemplo deve ter uma revisão guardada");
    assert_eq!(review.version, 1);
    assert_eq!(review.origin, ReviewOrigin::Automatic);

    // A pergunta corrente ainda não está confirmada; a confirmação do exemplo
    // pertence a uma pergunta anterior da mesma sessão.
    assert!(parsed.current.confirmation.is_none());
    assert_ne!(
        parsed.confirmation.session_question_id,
        parsed.current.question.id
    );
    assert_ne!(parsed.confirmation.recording_id, recording.id);
    assert_eq!(parsed.confirmation.review_version, 1);

    // A operação pendente aponta para a gravação selecionada da pergunta
    // corrente e conserva a versão de seleção que a autorizou.
    assert_eq!(parsed.pending_operation.session_id, parsed.session.id);
    assert_eq!(
        parsed.pending_operation.session_question_id,
        parsed.current.question.id
    );
    assert_eq!(
        parsed.pending_operation.recording_id.as_deref(),
        Some(recording.id.as_str())
    );
    assert_eq!(
        parsed.pending_operation.selection_version,
        parsed.current.question.selection_version
    );
    assert!(parsed.pending_operation.worker_generation > 0);

    // O trabalho em curso reutiliza o alvo da operação pendente.
    assert_eq!(parsed.model_job.target, parsed.pending_operation);
    assert_eq!(parsed.model_job.id, parsed.pending_operation.operation_id);

    assert_eq!(parsed.snapshot.database.as_str(), "opening");
    assert!(parsed.snapshot.active_session.is_none());
    assert!(parsed.snapshot.current.is_none());
    assert!(parsed.snapshot.warnings.is_empty());
    assert!(parsed.snapshot.last_error.is_none());
}

#[test]
fn question_validation_trims_outer_spaces_and_requires_content() {
    let input = QuestionInput {
        text: "  Tell me about a time you solved a difficult problem at work.  ".to_owned(),
        category: Category::ProblemSolving,
        competencies: vec!["  Resolução de problemas  ".to_owned()],
        criteria: vec!["  Explicar o problema e o desfecho.  ".to_owned()],
    };
    let validated = validate_question(input).expect("uma pergunta completa deve ser aceite");
    assert_eq!(
        validated.text,
        "Tell me about a time you solved a difficult problem at work."
    );
    assert_eq!(validated.competencies, vec!["Resolução de problemas"]);
    assert_eq!(
        validated.criteria,
        vec!["Explicar o problema e o desfecho."]
    );

    // Só os espaços exteriores são ignorados; o interior é preservado.
    let interior = validate_question(QuestionInput {
        text: "Tell  me  about  it.".to_owned(),
        category: Category::ProblemSolving,
        competencies: vec!["Análise".to_owned()],
        criteria: vec!["Explicar.".to_owned()],
    })
    .expect("o interior do texto não é alterado");
    assert_eq!(interior.text, "Tell  me  about  it.");

    let empty_text = validate_question(QuestionInput {
        text: "   ".to_owned(),
        category: Category::ProblemSolving,
        competencies: vec!["Análise".to_owned()],
        criteria: vec!["Explicar.".to_owned()],
    })
    .expect_err("texto vazio não é aceite");
    assert_eq!(empty_text.code, error_code::INVALID_INPUT);

    let empty_competencies = validate_question(QuestionInput {
        text: "Tell me about it.".to_owned(),
        category: Category::ProblemSolving,
        competencies: vec!["   ".to_owned()],
        criteria: vec!["Explicar.".to_owned()],
    })
    .expect_err("uma competência vazia não é aceite");
    assert_eq!(empty_competencies.code, error_code::INVALID_INPUT);

    let missing_criteria = validate_question(QuestionInput {
        text: "Tell me about it.".to_owned(),
        category: Category::ProblemSolving,
        competencies: vec!["Análise".to_owned()],
        criteria: Vec::new(),
    })
    .expect_err("sem critérios a pergunta não é aceite");
    assert_eq!(missing_criteria.code, error_code::INVALID_INPUT);
}

#[test]
fn count_validation_rejects_zero_and_more_than_eligible() {
    assert!(validate_count(3, 20).is_ok());
    assert!(validate_count(20, 20).is_ok());

    let zero = validate_count(0, 20).expect_err("zero perguntas não é aceite");
    assert_eq!(zero.code, error_code::INSUFFICIENT_QUESTIONS);
    assert!(!zero.retryable);
    assert!(
        zero.message.contains("20"),
        "a mensagem deve indicar as perguntas disponíveis: {}",
        zero.message
    );

    let excess = validate_count(4, 3).expect_err("não pode exceder as elegíveis");
    assert_eq!(excess.code, error_code::INSUFFICIENT_QUESTIONS);
    assert!(
        excess.message.contains('3'),
        "a mensagem deve indicar as perguntas disponíveis: {}",
        excess.message
    );

    let empty = validate_count(1, 0).expect_err("sem perguntas elegíveis nada inicia");
    assert_eq!(empty.code, error_code::INSUFFICIENT_QUESTIONS);
    assert!(
        empty.message.contains("Não existem"),
        "a mensagem deve explicar a ausência de perguntas: {}",
        empty.message
    );
}

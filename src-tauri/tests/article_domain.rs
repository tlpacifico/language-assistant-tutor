//! Domínio puro da conversa sobre artigos: limites, contexto, consentimento e estados.

use language_assistant_tutor::article::{
    can_send_automatically, transition, validate_answer_text, validate_article,
    ArticleAuthorization, ArticleContext, ArticleCreate, ArticleEvent, ArticleState, ContextRole,
    ContextTurn, ARTICLE_NOTICE_VERSION, MAX_ANSWER_BYTES, MAX_CONTEXT_BYTES,
    MAX_TUTOR_REPLY_BYTES, NEXT_ANSWER_RESERVE_BYTES,
};

fn create(text: String) -> ArticleCreate {
    ArticleCreate {
        request_id: "r1".into(),
        title: None,
        text,
    }
}

#[test]
fn article_limits_are_unicode_aware() {
    // 12 000 caracteres (2 bytes cada) cabem; 12 001 não.
    assert!(validate_article(&create("é".repeat(12_000))).is_ok());
    assert!(validate_article(&create("é".repeat(12_001))).is_err());
    // Espaços exteriores não contam e vazio é recusado.
    assert!(validate_article(&create(format!("  {}  ", "a".repeat(12_000)))).is_ok());
    assert!(validate_article(&create("   \n ".into())).is_err());
    // Título opcional até 200 caracteres.
    let mut long_title = create("texto".into());
    long_title.title = Some("é".repeat(201));
    assert!(validate_article(&long_title).is_err());
    long_title.title = Some("é".repeat(200));
    assert!(validate_article(&long_title).is_ok());
    // Excesso em bytes (4 bytes por carácter) é recusado sem cortar.
    let err = validate_article(&create("😀".repeat(12_000))).unwrap_err();
    assert_eq!(err.code, "INVALID_INPUT");
}

#[test]
fn article_context_reserves_next_answer() {
    let turn = |bytes: usize| ContextTurn {
        role: ContextRole::User,
        text: "a".repeat(bytes),
    };
    let mut context = ArticleContext {
        article: "a".repeat(10_000),
        title: None,
        turns: vec![turn(10_000)],
        branch_id: "b".into(),
        source_version_ids: vec![],
        turn_ids: vec![],
    };
    assert!(context.has_room_for_next_answer());
    // 48 000 - 16 000 = 32 000 bytes disponíveis antes de reservar a resposta.
    context.turns.push(turn(12_000));
    assert_eq!(context.byte_len(), 32_000);
    assert!(context.has_room_for_next_answer());
    context.turns.push(turn(1));
    assert!(!context.has_room_for_next_answer());
}

#[test]
fn automatic_send_requires_consent() {
    let auth = ArticleAuthorization {
        destination: "https://x/v1".into(),
        notice_version: ARTICLE_NOTICE_VERSION.into(),
    };
    assert_eq!(ARTICLE_NOTICE_VERSION, "article-auto-v1");
    assert!(can_send_automatically(Some(&auth), "https://x/v1").is_ok());
    assert!(can_send_automatically(None, "https://x/v1").is_err());
    let moved = can_send_automatically(Some(&auth), "https://other/v1").unwrap_err();
    assert_eq!(moved.code, "AI_CONSENT");
    let old = ArticleAuthorization {
        notice_version: "article-auto-v0".into(),
        ..auth
    };
    assert!(can_send_automatically(Some(&old), "https://x/v1").is_err());
    // Sem autorização não há conversa externa.
    assert!(transition(ArticleState::Draft, ArticleEvent::TutorReplied).is_err());
    assert_eq!(
        transition(
            ArticleState::Draft,
            ArticleEvent::Authorize {
                resume_to: ArticleState::AwaitingTutor
            }
        )
        .unwrap(),
        ArticleState::AwaitingTutor
    );
}

#[test]
fn recording_only_stops_on_user_action() {
    use ArticleEvent::*;
    use ArticleState::*;
    assert_eq!(transition(Ready, StartRecording).unwrap(), Recording);
    assert_eq!(transition(Recording, UserStop).unwrap(), Transcribing);
    // Nenhum outro acontecimento termina automaticamente a gravação.
    for event in [
        TutorReplied,
        TranscriptReady,
        TranscriptFailed,
        Finish,
        ReportReady,
        Revoke,
    ] {
        assert!(transition(Recording, event).is_err(), "{event:?}");
    }
    assert_eq!(
        transition(Recording, CaptureInterrupted).unwrap(),
        RecoverableError
    );
    // Transcrição pronta envia; vazia ou falhada exige revisão e não envia.
    assert_eq!(
        transition(Transcribing, TranscriptReady).unwrap(),
        AwaitingTutor
    );
    assert_eq!(
        transition(Transcribing, TranscriptFailed).unwrap(),
        NeedsReview
    );
    assert_eq!(transition(NeedsReview, ManualText).unwrap(), AwaitingTutor);
    // Terminar passa por finishing e só então completa.
    assert_eq!(transition(Ready, Finish).unwrap(), Finishing);
    assert_eq!(transition(Finishing, ReportReady).unwrap(), Completed);
    assert!(transition(Completed, StartRecording).is_err());
}

#[test]
fn retry_returns_to_the_step_that_failed() {
    use ArticleEvent::*;
    use ArticleState::*;
    // Pedido de resposta falhado: repete a resposta.
    assert_eq!(
        transition(RecoverableError, Retry(AwaitingTutor)).unwrap(),
        AwaitingTutor
    );
    // Feedback final falhado: repete o relatório, não um turno de diálogo.
    assert_eq!(
        transition(RecoverableError, Retry(Finishing)).unwrap(),
        Finishing
    );
    // Gravação interrompida: o turno não tem texto, por isso volta a gravar.
    assert_eq!(
        transition(RecoverableError, Retry(Recording)).unwrap(),
        Ready
    );
    // Nenhum outro destino é aceite e só se repete a partir do erro.
    assert!(transition(RecoverableError, Retry(Draft)).is_err());
    assert!(transition(RecoverableError, Retry(Completed)).is_err());
    assert!(transition(Ready, Retry(AwaitingTutor)).is_err());
}

#[test]
fn authorize_can_only_resume_to_a_state_that_matches_the_conversation() {
    use ArticleEvent::*;
    use ArticleState::*;
    for target in [AwaitingTutor, Ready, NeedsReview] {
        assert_eq!(
            transition(Draft, Authorize { resume_to: target }).unwrap(),
            target
        );
    }
    assert!(transition(
        Draft,
        Authorize {
            resume_to: Recording
        }
    )
    .is_err());
    assert!(transition(
        Draft,
        Authorize {
            resume_to: Completed
        }
    )
    .is_err());
}

#[test]
fn next_answer_and_tutor_reply_always_fit_in_the_context_window() {
    // Pior caso em UTF-8: 4 bytes por carácter.
    assert!(validate_answer_text(&"𝄞".repeat(MAX_ANSWER_BYTES / 4)).is_ok());
    assert!(validate_answer_text(&"𝄞".repeat(MAX_ANSWER_BYTES / 4 + 1)).is_err());
    // Resposta e comentário+pergunta do tutor cabem na reserva.
    const { assert!(MAX_ANSWER_BYTES + MAX_TUTOR_REPLY_BYTES <= NEXT_ANSWER_RESERVE_BYTES) };
    assert_eq!(MAX_TUTOR_REPLY_BYTES, (500 + 300) * 4);
    // Com margem para uma resposta, o contexto final nunca passa do teto.
    const {
        assert!(
            (MAX_CONTEXT_BYTES - NEXT_ANSWER_RESERVE_BYTES)
                + MAX_ANSWER_BYTES
                + MAX_TUTOR_REPLY_BYTES
                <= MAX_CONTEXT_BYTES
        )
    };
    // 8 000 caracteres de 1 byte continuam a ser aceites.
    assert!(validate_answer_text(&"a".repeat(8_000)).is_ok());
    assert!(validate_answer_text(&"a".repeat(8_001)).is_err());
}

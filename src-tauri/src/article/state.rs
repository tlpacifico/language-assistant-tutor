//! Transições de estado e consentimento, validados no núcleo.

use serde::{Deserialize, Serialize};

use super::types::{ArticleAuthorization, ArticleState, Result, ARTICLE_NOTICE_VERSION};
use crate::domain::{error_code, AppError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArticleEvent {
    /// `resume_to`: passo da conversa a retomar (`AwaitingTutor` sem turnos ou
    /// com resposta por enviar, `Ready` depois do tutor, `NeedsReview`, ou
    /// `Finishing` quando o feedback final falhou antes da revogação).
    Authorize {
        resume_to: ArticleState,
    },
    Revoke,
    TutorReplied,
    RequestFailed,
    StartRecording,
    /// Única forma de terminar a gravação.
    UserStop,
    CaptureInterrupted,
    TranscriptReady,
    TranscriptFailed,
    ManualText,
    /// Repete o passo que falhou; o argumento é o estado de onde veio a falha
    /// (`AwaitingTutor`, `Finishing` ou `Recording`).
    Retry(ArticleState),
    Finish,
    ReportReady,
    Pause,
    Resume(ArticleState),
}

fn refused(state: ArticleState, event: ArticleEvent) -> AppError {
    AppError::new(
        error_code::INVALID_INPUT,
        format!("Ação não permitida no estado atual ({state:?}, {event:?})."),
        false,
    )
}

pub fn transition(state: ArticleState, event: ArticleEvent) -> Result<ArticleState> {
    use ArticleEvent as E;
    use ArticleState as S;
    let next = match (state, event) {
        (
            S::Draft,
            E::Authorize {
                resume_to: target @ (S::AwaitingTutor | S::Ready | S::NeedsReview | S::Finishing),
            },
        ) => target,
        (S::AwaitingTutor, E::TutorReplied) => S::Ready,
        (S::AwaitingTutor, E::RequestFailed) => S::RecoverableError,
        (S::AwaitingTutor | S::Ready | S::NeedsReview | S::RecoverableError, E::Revoke) => S::Draft,
        (S::Ready | S::NeedsReview | S::RecoverableError, E::StartRecording) => S::Recording,
        (S::Recording, E::UserStop) => S::Transcribing,
        (S::Recording, E::CaptureInterrupted) => S::RecoverableError,
        (S::Transcribing, E::TranscriptReady) => S::AwaitingTutor,
        (S::Transcribing, E::TranscriptFailed) => S::NeedsReview,
        (S::NeedsReview, E::ManualText) => S::AwaitingTutor,
        (S::RecoverableError, E::Retry(S::AwaitingTutor)) => S::AwaitingTutor,
        (S::RecoverableError, E::Retry(S::Finishing)) => S::Finishing,
        // Gravação interrompida: o turno não tem texto, volta a gravar.
        (S::RecoverableError, E::Retry(S::Recording)) => S::Ready,
        (S::AwaitingTutor | S::Ready | S::NeedsReview | S::RecoverableError, E::Finish) => {
            S::Finishing
        }
        (S::Finishing, E::ReportReady) => S::Completed,
        (S::Finishing, E::RequestFailed) => S::RecoverableError,
        (S::Draft | S::Ready | S::NeedsReview | S::RecoverableError, E::Pause) => S::Paused,
        (
            S::Paused,
            E::Resume(target @ (S::Draft | S::Ready | S::NeedsReview | S::RecoverableError)),
        ) => target,
        _ => return Err(refused(state, event)),
    };
    Ok(next)
}

/// Só envia automaticamente com autorização desta sessão para o destino e o
/// aviso atuais; destino alterado ou aviso diferente bloqueiam o envio.
pub fn can_send_automatically(
    authorization: Option<&ArticleAuthorization>,
    current_destination: &str,
) -> Result<()> {
    let blocked = |message: &str| AppError::new("AI_CONSENT", message, false);
    let Some(auth) = authorization else {
        return Err(blocked(
            "Autorize o envio automático desta conversa antes de continuar.",
        ));
    };
    if auth.notice_version != ARTICLE_NOTICE_VERSION {
        return Err(blocked(
            "O aviso mudou. Autorize novamente o envio automático.",
        ));
    }
    if auth.destination != current_destination {
        return Err(blocked(
            "O destino mudou. Autorize novamente o envio automático.",
        ));
    }
    Ok(())
}

//! Mutações tipadas da conversa: o frontend nunca fornece SQL nem transições.

use serde::{Deserialize, Serialize};

use super::types::ArticleError;

/// Alteração pedida ao repositório; as transições são validadas no núcleo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArticleMutation {
    /// Autoriza o envio automático desta sessão para o destino indicado.
    Authorize {
        destination: String,
        notice_version: String,
    },
    /// Revoga a autorização e invalida pedidos em curso.
    Revoke,
    Pause,
    Resume,
    /// Fala do tutor já validada (abertura ou resposta).
    AddTutorTurn {
        text: String,
    },
    /// Cria o turno do utilizador e regista o áudio exclusivo da sessão.
    StartRecording {
        turn_id: String,
        audio_path: String,
    },
    /// Única forma de terminar a gravação.
    StopRecording,
    /// Resultado da transcrição automática; vazio ou excessivo exige revisão.
    RecordTranscript {
        turn_id: String,
        text: Option<String>,
    },
    /// Texto introduzido pelo utilizador; versão distinta da automática.
    ManualText {
        turn_id: String,
        text: String,
    },
    Finish,
    /// O relatório (ou a ausência dele, sem respostas) foi aplicado.
    Complete,
    /// Falha recuperável de um pedido ou da gravação; guarda de onde veio.
    Fail {
        error: ArticleError,
    },
    /// Repete o passo que falhou.
    Retry,
    /// Eliminação lógica; os ficheiros são removidos depois.
    Delete,
}

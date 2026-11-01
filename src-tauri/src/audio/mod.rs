//! Porta de áudio da fase 1.
//!
//! Este módulo contém apenas os tipos partilhados com a persistência. A porta
//! `AudioPort`, os adaptadores nativos (`capture`, `playback`) e a conversão
//! (`normalize`) entram na tarefa 3; a captura não guarda estado aqui.

use std::path::PathBuf;

use crate::domain::CaptureMeta;

/// Reserva de uma gravação: identificador e pasta próprios.
///
/// A pasta é criada vazia pela persistência e recebe `original.wav` durante a
/// captura; cada gravação nova obtém sempre uma pasta diferente.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordingTarget {
    pub id: String,
    pub directory: PathBuf,
}

/// Áudio finalizado de uma gravação, pronto a publicar.
///
/// `original` é o ficheiro que a captura fechou e `meta` descreve o que foi
/// efetivamente escrito. Publicar valida o ficheiro antes de o tornar visível.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedAudio {
    pub id: String,
    pub original: PathBuf,
    pub meta: CaptureMeta,
}

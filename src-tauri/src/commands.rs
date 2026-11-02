//! Comandos Tauri da fase 1.
//!
//! Cada comando delega no método homónimo de [`AppService`] e devolve a
//! fotografia atual ou um [`AppError`] estável. Não há lógica de domínio aqui:
//! a correspondência entre a interface e os comandos vive apenas em
//! `src/lib/tauri.ts`.

use std::sync::Arc;

use tauri::State;

use crate::domain::{Action, AppError, AppSnapshot, PlaybackTarget};
use crate::service::AppService;

/// Fotografia atual, sem alterar o estado.
#[tauri::command]
pub async fn get_snapshot(service: State<'_, Arc<AppService>>) -> Result<AppSnapshot, AppError> {
    Ok(service.snapshot())
}

/// Verifica microfone, saída e auxiliar.
#[tauri::command]
pub async fn refresh_availability(
    service: State<'_, Arc<AppService>>,
) -> Result<AppSnapshot, AppError> {
    service.apply(Action::RefreshAvailability).await
}

/// Reserva a pasta e abre o microfone.
#[tauri::command]
pub async fn start_recording(service: State<'_, Arc<AppService>>) -> Result<AppSnapshot, AppError> {
    service.apply(Action::StartRecording).await
}

/// Termina a captura e publica a resposta.
#[tauri::command]
pub async fn stop_recording(service: State<'_, Arc<AppService>>) -> Result<AppSnapshot, AppError> {
    service.apply(Action::StopRecording).await
}

/// Repete a publicação de uma captura já finalizada.
#[tauri::command]
pub async fn retry_save_recording(
    service: State<'_, Arc<AppService>>,
) -> Result<AppSnapshot, AppError> {
    service.apply(Action::RetrySaveRecording).await
}

/// Transcreve a resposta ativa.
#[tauri::command]
pub async fn transcribe_response(
    attempt_id: String,
    service: State<'_, Arc<AppService>>,
) -> Result<AppSnapshot, AppError> {
    service
        .apply(Action::TranscribeResponse { attempt_id })
        .await
}

/// Reproduz a pergunta ou o original da resposta ativa.
#[tauri::command]
pub async fn play_audio(
    target: PlaybackTarget,
    service: State<'_, Arc<AppService>>,
) -> Result<AppSnapshot, AppError> {
    service.apply(Action::PlayAudio { target }).await
}

/// Pára a reprodução em curso.
#[tauri::command]
pub async fn stop_audio(service: State<'_, Arc<AppService>>) -> Result<AppSnapshot, AppError> {
    service.apply(Action::StopAudio).await
}

/// Guarda a revisão de uma transcrição; não confirma a resposta.
#[tauri::command]
pub async fn save_review(
    attempt_id: String,
    source_id: String,
    text: String,
    service: State<'_, Arc<AppService>>,
) -> Result<AppSnapshot, AppError> {
    service
        .apply(Action::SaveReview {
            attempt_id,
            source_id,
            text,
        })
        .await
}

/// Reinicia o auxiliar Python.
#[tauri::command]
pub async fn restart_worker(service: State<'_, Arc<AppService>>) -> Result<AppSnapshot, AppError> {
    service.apply(Action::RestartWorker).await
}

/// Finaliza a captura, interrompe o trabalho pendente e encerra o auxiliar.
#[tauri::command]
pub async fn prepare_close(service: State<'_, Arc<AppService>>) -> Result<AppSnapshot, AppError> {
    service.apply(Action::PrepareClose).await
}

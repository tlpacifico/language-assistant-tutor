//! Comandos Tauri da conversa sobre artigos.
//!
//! Cada comando delega no método homónimo de [`AppService`] e devolve a
//! fotografia persistida ou um [`AppError`] estável. Os comandos que lançam
//! trabalho externo devolvem logo o estado pendente; a conclusão chega pelo
//! evento `article://snapshot`.

use std::sync::Arc;

use tauri::State;

use crate::article::{
    ArticleCommand, ArticleConsent, ArticleCorrection, ArticleCreate, ArticleSnapshot,
    ArticleSummary,
};
use crate::domain::AppError;
use crate::service::AppService;

#[tauri::command]
pub async fn article_create(
    input: ArticleCreate,
    service: State<'_, Arc<AppService>>,
) -> Result<ArticleSnapshot, AppError> {
    service.article_create(input).await
}

#[tauri::command]
pub async fn article_authorize(
    consent: ArticleConsent,
    service: State<'_, Arc<AppService>>,
) -> Result<ArticleSnapshot, AppError> {
    service.article_authorize(consent).await
}

#[tauri::command]
pub async fn article_revoke(
    command: ArticleCommand,
    service: State<'_, Arc<AppService>>,
) -> Result<ArticleSnapshot, AppError> {
    service.article_revoke(command).await
}

#[tauri::command]
pub async fn article_start_recording(
    command: ArticleCommand,
    service: State<'_, Arc<AppService>>,
) -> Result<ArticleSnapshot, AppError> {
    service.article_start_recording(command).await
}

#[tauri::command]
pub async fn article_stop_recording(
    command: ArticleCommand,
    service: State<'_, Arc<AppService>>,
) -> Result<ArticleSnapshot, AppError> {
    service.article_stop_recording(command).await
}

#[tauri::command]
pub async fn article_retry(
    command: ArticleCommand,
    service: State<'_, Arc<AppService>>,
) -> Result<ArticleSnapshot, AppError> {
    service.article_retry(command).await
}

#[tauri::command]
pub async fn article_finish(
    command: ArticleCommand,
    service: State<'_, Arc<AppService>>,
) -> Result<ArticleSnapshot, AppError> {
    service.article_finish(command).await
}

#[tauri::command]
pub async fn article_correct(
    correction: ArticleCorrection,
    service: State<'_, Arc<AppService>>,
) -> Result<ArticleSnapshot, AppError> {
    service.article_correct(correction).await
}

#[tauri::command]
pub async fn article_submit_text(
    correction: ArticleCorrection,
    service: State<'_, Arc<AppService>>,
) -> Result<ArticleSnapshot, AppError> {
    service.article_submit_text(correction).await
}

#[tauri::command]
pub async fn article_play_turn(
    session_id: String,
    turn_id: String,
    service: State<'_, Arc<AppService>>,
) -> Result<(), AppError> {
    service.article_play_turn(&session_id, &turn_id).await
}

#[tauri::command]
pub async fn article_resume(
    command: ArticleCommand,
    service: State<'_, Arc<AppService>>,
) -> Result<ArticleSnapshot, AppError> {
    service.article_resume(command).await
}

/// Fotografia persistida; não inicia nenhum trabalho externo.
#[tauri::command]
pub async fn article_snapshot(
    session_id: String,
    service: State<'_, Arc<AppService>>,
) -> Result<ArticleSnapshot, AppError> {
    service.article_snapshot(&session_id).await
}

#[tauri::command]
pub async fn article_list(
    service: State<'_, Arc<AppService>>,
) -> Result<Vec<ArticleSummary>, AppError> {
    service.article_list().await
}

#[tauri::command]
pub async fn article_delete(
    command: ArticleCommand,
    service: State<'_, Arc<AppService>>,
) -> Result<(), AppError> {
    service.article_delete(command).await
}

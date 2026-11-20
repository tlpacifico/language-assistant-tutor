//! Comandos Tauri da conversa sobre artigos.
//!
//! Cada comando delega no método homónimo de [`AppService`] e devolve a
//! fotografia persistida ou um [`AppError`] estável. Os comandos que lançam
//! trabalho externo devolvem logo o estado pendente; a conclusão chega pelo
//! evento `article://snapshot`.

use std::sync::Arc;

use tauri::State;

use crate::article::{
    ArticleCommand, ArticleConsent, ArticleCorrection, ArticleCreate, ArticlePlayerView,
    ArticlePreferences, ArticleSnapshot, ArticleSummary,
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

/// Reproduz uma fala (tutor) ou a gravação de uma resposta, do início.
#[tauri::command]
pub async fn article_player_play(
    session_id: String,
    turn_id: String,
    service: State<'_, Arc<AppService>>,
) -> Result<ArticlePlayerView, AppError> {
    service.article_player_play(&session_id, &turn_id).await
}

#[tauri::command]
pub async fn article_player_pause(
    session_id: String,
    generation: u64,
    service: State<'_, Arc<AppService>>,
) -> Result<ArticlePlayerView, AppError> {
    service.article_player_pause(&session_id, generation).await
}

#[tauri::command]
pub async fn article_player_resume(
    session_id: String,
    generation: u64,
    service: State<'_, Arc<AppService>>,
) -> Result<ArticlePlayerView, AppError> {
    service.article_player_resume(&session_id, generation).await
}

#[tauri::command]
pub async fn article_player_stop(
    session_id: String,
    generation: u64,
    service: State<'_, Arc<AppService>>,
) -> Result<ArticlePlayerView, AppError> {
    service.article_player_stop(&session_id, generation).await
}

#[tauri::command]
pub async fn article_view_focus(
    session_id: String,
    token: String,
    service: State<'_, Arc<AppService>>,
) -> Result<(), AppError> {
    service.article_view_focus(&session_id, &token).await
}

#[tauri::command]
pub async fn article_view_blur(
    token: String,
    service: State<'_, Arc<AppService>>,
) -> Result<(), AppError> {
    service.article_view_blur(&token).await
}

/// Para a voz do tutor sem perder texto nem estado.
#[tauri::command]
pub async fn article_stop_voice(service: State<'_, Arc<AppService>>) -> Result<(), AppError> {
    service.article_stop_voice().await
}

/// Repete a voz de uma fala do tutor, sem nova chamada ao fornecedor de texto.
#[tauri::command]
pub async fn article_speak_turn(
    session_id: String,
    turn_id: String,
    service: State<'_, Arc<AppService>>,
) -> Result<(), AppError> {
    service.article_speak_turn(&session_id, &turn_id).await
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
pub async fn article_preferences_load(
    service: State<'_, Arc<AppService>>,
) -> Result<ArticlePreferences, AppError> {
    service.article_preferences_load().await
}

#[tauri::command]
pub async fn article_preferences_save(
    preferences: ArticlePreferences,
    service: State<'_, Arc<AppService>>,
) -> Result<ArticlePreferences, AppError> {
    service.article_preferences_save(preferences).await
}

#[tauri::command]
pub async fn article_delete(
    command: ArticleCommand,
    service: State<'_, Arc<AppService>>,
) -> Result<(), AppError> {
    service.article_delete(command).await
}

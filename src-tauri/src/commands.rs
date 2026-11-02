//! Comandos Tauri da entrevista guiada.
//!
//! Cada comando delega no método homónimo de [`AppService`] e devolve a
//! fotografia atual, um DTO de leitura ou um [`AppError`] estável com mensagem
//! em português de Portugal. Não há lógica de domínio aqui: a correspondência
//! entre a interface e os comandos vive apenas em `src/lib/interview-client.ts`.
//!
//! Os comandos que podem bloquear (base de dados, disco, áudio, auxiliar) já
//! correm fora da thread do runtime dentro do serviço; nenhum deles usa `expect`
//! nem encerra a aplicação por uma falha de dados.

use std::sync::Arc;

use tauri::State;

use crate::domain::AppError;
use crate::interview::{
    AdvanceSession, AnswerTarget, AudioSelection, Category, ConfirmAnswer, Id, InterviewSnapshot,
    Question, QuestionInput, SaveReview, SessionDetail, SessionSummary, StartSession, Version,
};
use crate::service::AppService;

/// Fotografia atual, sem alterar o estado.
#[tauri::command]
pub async fn interview_snapshot(
    service: State<'_, Arc<AppService>>,
) -> Result<InterviewSnapshot, AppError> {
    Ok(service.snapshot())
}

/// Lista o banco de perguntas, opcionalmente filtrado por categoria.
#[tauri::command]
pub async fn interview_list_questions(
    category: Option<Category>,
    service: State<'_, Arc<AppService>>,
) -> Result<Vec<Question>, AppError> {
    service.list_questions(category).await
}

/// Cria uma pergunta editável no banco.
#[tauri::command]
pub async fn interview_create_question(
    input: QuestionInput,
    service: State<'_, Arc<AppService>>,
) -> Result<Question, AppError> {
    service.create_question(input).await
}

/// Edita uma pergunta com controlo otimista de versão.
#[tauri::command]
pub async fn interview_update_question(
    id: Id,
    expected_version: Version,
    input: QuestionInput,
    service: State<'_, Arc<AppService>>,
) -> Result<Question, AppError> {
    service.update_question(id, expected_version, input).await
}

/// Inicia a entrevista e apresenta a primeira pergunta guardada.
#[tauri::command]
pub async fn interview_start_session(
    request: StartSession,
    service: State<'_, Arc<AppService>>,
) -> Result<InterviewSnapshot, AppError> {
    service.start_session(request).await
}

/// Lista as sessões para o histórico.
#[tauri::command]
pub async fn interview_list_sessions(
    service: State<'_, Arc<AppService>>,
) -> Result<Vec<SessionSummary>, AppError> {
    service.list_sessions().await
}

/// Abre uma sessão completa para consulta ou retoma.
#[tauri::command]
pub async fn interview_load_session(
    session_id: Id,
    service: State<'_, Arc<AppService>>,
) -> Result<SessionDetail, AppError> {
    service.load_session(session_id).await
}

/// Reserva a gravação e abre o microfone na pergunta atual.
#[tauri::command]
pub async fn interview_start_recording(
    session_question_id: Id,
    expected_selection: Version,
    service: State<'_, Arc<AppService>>,
) -> Result<InterviewSnapshot, AppError> {
    service
        .start_recording(session_question_id, expected_selection)
        .await
}

/// Termina a captura, publica o original e pede a transcrição automática.
#[tauri::command]
pub async fn interview_stop_recording(
    service: State<'_, Arc<AppService>>,
) -> Result<InterviewSnapshot, AppError> {
    service.stop_recording().await
}

/// Repete a publicação de uma gravação já finalizada, sem nova captura.
#[tauri::command]
pub async fn interview_retry_save_recording(
    recording_id: Id,
    service: State<'_, Arc<AppService>>,
) -> Result<InterviewSnapshot, AppError> {
    service.retry_save_recording(recording_id).await
}

/// Transcreve uma resposta publicada e ainda não confirmada.
#[tauri::command]
pub async fn interview_transcribe_response(
    target: AnswerTarget,
    service: State<'_, Arc<AppService>>,
) -> Result<InterviewSnapshot, AppError> {
    service.transcribe_response(target).await
}

/// Guarda uma revisão; não confirma nem avança.
#[tauri::command]
pub async fn interview_save_review(
    request: SaveReview,
    service: State<'_, Arc<AppService>>,
) -> Result<InterviewSnapshot, AppError> {
    service.save_review(request).await
}

/// Confirma exatamente a versão de texto que o utilizador viu.
#[tauri::command]
pub async fn interview_confirm_answer(
    request: ConfirmAnswer,
    service: State<'_, Arc<AppService>>,
) -> Result<InterviewSnapshot, AppError> {
    service.confirm_answer(request).await
}

/// Avança para a pergunta seguinte ou termina a sessão.
#[tauri::command]
pub async fn interview_advance_session(
    request: AdvanceSession,
    service: State<'_, Arc<AppService>>,
) -> Result<InterviewSnapshot, AppError> {
    service.advance_session(request).await
}

/// Reproduz uma pergunta, uma gravação ou um treino da fase 1.
#[tauri::command]
pub async fn interview_play_audio(
    target: AudioSelection,
    service: State<'_, Arc<AppService>>,
) -> Result<InterviewSnapshot, AppError> {
    service.play_audio(target).await
}

/// Pára a reprodução em curso.
#[tauri::command]
pub async fn interview_stop_audio(
    service: State<'_, Arc<AppService>>,
) -> Result<InterviewSnapshot, AppError> {
    service.stop_audio().await
}

/// Verifica microfone, saída e auxiliar.
#[tauri::command]
pub async fn interview_refresh_availability(
    service: State<'_, Arc<AppService>>,
) -> Result<InterviewSnapshot, AppError> {
    service.refresh_availability().await
}

/// Reinicia o auxiliar Python.
#[tauri::command]
pub async fn interview_restart_worker(
    service: State<'_, Arc<AppService>>,
) -> Result<InterviewSnapshot, AppError> {
    service.restart_worker().await
}

/// Finaliza a captura, interrompe o trabalho pendente e encerra o auxiliar.
#[tauri::command]
pub async fn interview_prepare_close(
    service: State<'_, Arc<AppService>>,
) -> Result<InterviewSnapshot, AppError> {
    service.prepare_close().await
}

/// Repete a abertura e a reconciliação da base de dados.
#[tauri::command]
pub async fn interview_retry_database(
    service: State<'_, Arc<AppService>>,
) -> Result<InterviewSnapshot, AppError> {
    service.retry_database().await
}

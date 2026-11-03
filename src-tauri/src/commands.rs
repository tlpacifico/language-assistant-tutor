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

use crate::domain::{AppError, RestoreResult};
use crate::interview::{
    AdvanceSession, AnswerTarget, AudioSelection, Category, ConfirmAnswer, Id, InterviewSnapshot,
    Question, QuestionInput, SaveReview, SessionDetail, SessionSummary, StartSession, Version,
};
use crate::practice::{
    CaptureOwner, ComparisonRequest, ComparisonView, ConfirmPracticeAnswer, CreatePractice,
    CreateStory, EvaluatePractice, PracticeAnswerTarget, PracticeCommand, PracticeDetail,
    PracticeReportView, PreparationDecision, ResponseOrigin, RewriteCommand, RewritePreference,
    RewriteView, SavePracticeReview, SelectStory, SetRewriteLevel, StoryView, TrainingUsageView,
    UpdateStory,
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

/// Lista os treinos da fase 1, em modo de consulta.
#[tauri::command]
pub async fn interview_list_legacy(
    service: State<'_, Arc<AppService>>,
) -> Result<Vec<RestoreResult>, AppError> {
    service.list_legacy().await
}

/// Lê um treino da fase 1 pelo identificador, sem alterar o formato antigo.
#[tauri::command]
pub async fn interview_load_legacy(
    attempt_id: Id,
    service: State<'_, Arc<AppService>>,
) -> Result<RestoreResult, AppError> {
    service.load_legacy(attempt_id).await
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

/// Cria uma prática a partir de uma pergunta confirmada de uma sessão concluída.
#[tauri::command]
pub async fn interview_create_practice(
    request: CreatePractice,
    service: State<'_, Arc<AppService>>,
) -> Result<PracticeDetail, AppError> {
    service.create_practice(request).await
}

/// Lê uma prática completa, sem alterar o estado.
#[tauri::command]
pub async fn interview_load_practice(
    practice_id: Id,
    service: State<'_, Arc<AppService>>,
) -> Result<PracticeDetail, AppError> {
    service.load_practice(practice_id).await
}

/// Lista as práticas de uma pergunta da sessão.
#[tauri::command]
pub async fn interview_list_practices(
    session_question_id: Id,
    service: State<'_, Arc<AppService>>,
) -> Result<Vec<PracticeDetail>, AppError> {
    service.list_practices(session_question_id).await
}

/// Abandona uma prática pendente, conservando revisões e gravações.
#[tauri::command]
pub async fn interview_abandon_practice(
    request: PracticeCommand,
    service: State<'_, Arc<AppService>>,
) -> Result<PracticeDetail, AppError> {
    service.abandon_practice(request).await
}

/// Reserva a gravação da tentativa e abre o microfone.
#[tauri::command]
pub async fn interview_start_practice_recording(
    owner: CaptureOwner,
    expected_selection: Version,
    service: State<'_, Arc<AppService>>,
) -> Result<InterviewSnapshot, AppError> {
    service
        .start_practice_recording(owner, expected_selection)
        .await
}

/// Guarda uma revisão da tentativa de prática, sem confirmar.
#[tauri::command]
pub async fn interview_save_practice_review(
    request: SavePracticeReview,
    service: State<'_, Arc<AppService>>,
) -> Result<InterviewSnapshot, AppError> {
    service.save_practice_review(request).await
}

/// Confirma a tentativa de prática, tornando-a imutável.
#[tauri::command]
pub async fn interview_confirm_practice_answer(
    request: ConfirmPracticeAnswer,
    service: State<'_, Arc<AppService>>,
) -> Result<PracticeDetail, AppError> {
    service.confirm_practice_answer(request).await
}

/// Transcreve a gravação selecionada da tentativa de prática.
#[tauri::command]
pub async fn interview_transcribe_practice_response(
    target: PracticeAnswerTarget,
    service: State<'_, Arc<AppService>>,
) -> Result<InterviewSnapshot, AppError> {
    service.transcribe_practice_response(target).await
}

/// Lista as histórias locais, opcionalmente filtradas por competência.
#[tauri::command]
pub async fn interview_list_stories(
    competency: Option<String>,
    service: State<'_, Arc<AppService>>,
) -> Result<Vec<StoryView>, AppError> {
    service.list_stories(competency).await
}

/// Cria uma história local.
#[tauri::command]
pub async fn interview_create_story(
    request: CreateStory,
    service: State<'_, Arc<AppService>>,
) -> Result<StoryView, AppError> {
    service.create_story(request).await
}

/// Edita uma história local com versão esperada.
#[tauri::command]
pub async fn interview_update_story(
    request: UpdateStory,
    service: State<'_, Arc<AppService>>,
) -> Result<StoryView, AppError> {
    service.update_story(request).await
}

/// Lê a preparação (apoio de história) de uma resposta, só em leitura.
#[tauri::command]
pub async fn interview_load_preparation(
    owner: CaptureOwner,
    service: State<'_, Arc<AppService>>,
) -> Result<crate::practice::PreparationView, AppError> {
    service.load_preparation(owner).await
}

/// Seleciona (ou remove) o apoio de história de uma resposta.
#[tauri::command]
pub async fn interview_select_story(
    request: SelectStory,
    service: State<'_, Arc<AppService>>,
) -> Result<InterviewSnapshot, AppError> {
    service.select_story(request).await
}

/// Resolve explicitamente uma preparação interrompida.
#[tauri::command]
pub async fn interview_resolve_preparation(
    recording_id: Id,
    request_id: Id,
    decision: PreparationDecision,
    service: State<'_, Arc<AppService>>,
) -> Result<InterviewSnapshot, AppError> {
    service
        .resolve_preparation(recording_id, request_id, decision)
        .await
}

/// Repete a publicação local dos metadados da preparação.
#[tauri::command]
pub async fn interview_retry_preparation_publication(
    recording_id: Id,
    service: State<'_, Arc<AppService>>,
) -> Result<InterviewSnapshot, AppError> {
    service.retry_preparation_publication(recording_id).await
}

use crate::ai::*;
#[tauri::command]
pub async fn interview_ai_load_ai_settings(
    service: State<'_, Arc<AppService>>,
) -> std::result::Result<AiSettingsView, AppError> {
    service.load_ai_settings().await
}
#[tauri::command]
pub async fn interview_ai_save_ai_settings(
    request: SaveAiSettings,
    service: State<'_, Arc<AppService>>,
) -> std::result::Result<AiSettingsView, AppError> {
    service.save_ai_settings(request).await
}
#[tauri::command]
pub async fn interview_ai_save_ai_key(
    key: String,
    service: State<'_, Arc<AppService>>,
) -> std::result::Result<AiSettingsView, AppError> {
    service.save_ai_key(key).await
}
#[tauri::command]
pub async fn interview_ai_remove_ai_key(
    service: State<'_, Arc<AppService>>,
) -> std::result::Result<AiSettingsView, AppError> {
    service.remove_ai_key().await
}
#[tauri::command]
pub async fn interview_ai_accept_ai_consent(
    request: AcceptAiConsent,
    service: State<'_, Arc<AppService>>,
) -> std::result::Result<AiSettingsView, AppError> {
    service.accept_ai_consent(request).await
}
#[tauri::command]
pub async fn interview_ai_request_follow_up(
    request: AiQuestionCommand,
    service: State<'_, Arc<AppService>>,
) -> std::result::Result<InterviewSnapshot, AppError> {
    service.request_follow_up(request).await
}
#[tauri::command]
pub async fn interview_ai_waive_follow_up(
    request: WaiveFollowUp,
    service: State<'_, Arc<AppService>>,
) -> std::result::Result<InterviewSnapshot, AppError> {
    service.waive_follow_up(request).await
}
#[tauri::command]
pub async fn interview_ai_generate_feedback(
    request: AiSessionCommand,
    service: State<'_, Arc<AppService>>,
) -> std::result::Result<FeedbackView, AppError> {
    service.generate_feedback(request).await
}
#[tauri::command]
pub async fn interview_ai_load_feedback(
    id: Id,
    service: State<'_, Arc<AppService>>,
) -> std::result::Result<FeedbackView, AppError> {
    service.load_feedback(id).await
}
#[tauri::command]
pub async fn interview_ai_retry_ai_publication(
    id: Id,
    service: State<'_, Arc<AppService>>,
) -> std::result::Result<InterviewSnapshot, AppError> {
    service.retry_ai_publication(id).await
}

/// Pede (ou reutiliza) uma reformulação para uma origem e um nível.
#[tauri::command]
pub async fn interview_request_rewrite(
    request: RewriteCommand,
    service: State<'_, Arc<AppService>>,
) -> std::result::Result<RewriteView, AppError> {
    service.request_rewrite(request).await
}

/// Lista as reformulações de uma origem.
#[tauri::command]
pub async fn interview_list_rewrites(
    origin: ResponseOrigin,
    service: State<'_, Arc<AppService>>,
) -> std::result::Result<Vec<RewriteView>, AppError> {
    service.list_rewrites(origin).await
}

/// Pede (ou reutiliza) a avaliação de uma prática confirmada.
#[tauri::command]
pub async fn interview_evaluate_practice(
    request: EvaluatePractice,
    service: State<'_, Arc<AppService>>,
) -> std::result::Result<PracticeReportView, AppError> {
    service.evaluate_practice(request).await
}

/// Relatório de avaliação de uma prática.
#[tauri::command]
pub async fn interview_load_practice_report(
    practice_id: Id,
    service: State<'_, Arc<AppService>>,
) -> std::result::Result<PracticeReportView, AppError> {
    service.load_practice_report(practice_id).await
}

/// Compara duas respostas confirmadas da mesma pergunta (só leitura local).
#[tauri::command]
pub async fn interview_compare_responses(
    request: ComparisonRequest,
    service: State<'_, Arc<AppService>>,
) -> std::result::Result<ComparisonView, AppError> {
    service.compare_responses(request).await
}

/// Consumo de treino de uma sessão: entrevista original, prática e combinado.
#[tauri::command]
pub async fn interview_load_training_usage(
    session_id: Id,
    service: State<'_, Arc<AppService>>,
) -> std::result::Result<TrainingUsageView, AppError> {
    service.load_training_usage(session_id).await
}

/// Preferência local de nível de reformulação.
#[tauri::command]
pub async fn interview_load_rewrite_preference(
    service: State<'_, Arc<AppService>>,
) -> std::result::Result<RewritePreference, AppError> {
    service.load_rewrite_preference().await
}

/// Altera a preferência local de nível de reformulação.
#[tauri::command]
pub async fn interview_set_rewrite_level(
    request: SetRewriteLevel,
    service: State<'_, Arc<AppService>>,
) -> std::result::Result<RewritePreference, AppError> {
    service.set_rewrite_level(request).await
}

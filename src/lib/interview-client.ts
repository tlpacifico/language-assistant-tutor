import type {
  AiSettingsView,
  SaveAiSettings,
  AcceptAiConsent,
  AiQuestionCommand,
  WaiveFollowUp,
  AiSessionCommand,
  FeedbackView,
} from "./ai-contracts";
/**
 * Adaptador Tauri da entrevista guiada.
 *
 * Este é o único ficheiro que conhece os nomes dos comandos `interview_*` e do
 * evento `interview://snapshot`; a interface usa apenas `InterviewClient`. Cada
 * método devolve a fotografia (`InterviewSnapshot`), um DTO de leitura ou
 * rejeita com o `AppError` estável devolvido pelo serviço (mensagem em
 * português de Portugal).
 */

import { diagnosticInvoke as invoke } from "./diagnostics";
import { listen } from "@tauri-apps/api/event";

import type {
  AdvanceSession,
  AnswerTarget,
  AudioSelection,
  Category,
  ConfirmAnswer,
  Id,
  InterviewSnapshot,
  Question,
  QuestionInput,
  SaveReview,
  SessionDetail,
  SessionSummary,
  StartSession,
  Version,
} from "./interview-contracts";
import type { RestoreResult } from "./contracts";
import type {
  CaptureOwner,
  ComparisonRequest,
  ComparisonView,
  ConfirmPracticeAnswer,
  CreatePractice,
  CreateStory,
  EvaluatePractice,
  PracticeAnswerTarget,
  PracticeCommand,
  PracticeDetail,
  PracticeReportView,
  PreparationDecision,
  PreparationView,
  ResponseOrigin,
  RewriteCommand,
  RewritePreference,
  RewriteView,
  SavePracticeReview,
  SelectStory,
  SetRewriteLevel,
  StoryView,
  TrainingUsageView,
  UpdateStory,
} from "./practice-contracts";

/** Evento com a fotografia da entrevista, publicado depois de cada alteração. */
export const INTERVIEW_SNAPSHOT_EVENT = "interview://snapshot";

/** Remoção de um listener de fotografias. */
export type Unsubscribe = () => void;

/** Acesso único da interface aos comandos Tauri da fase 2. */
export interface InterviewClient {
  loadAiSettings(): Promise<AiSettingsView>;
  saveAiSettings(request: SaveAiSettings): Promise<AiSettingsView>;
  saveAiKey(key: string): Promise<AiSettingsView>;
  removeAiKey(): Promise<AiSettingsView>;
  acceptAiConsent(request: AcceptAiConsent): Promise<AiSettingsView>;
  requestFollowUp(request: AiQuestionCommand): Promise<InterviewSnapshot>;
  waiveFollowUp(request: WaiveFollowUp): Promise<InterviewSnapshot>;
  generateFeedback(request: AiSessionCommand): Promise<FeedbackView>;
  evaluateQuestion(request: AiQuestionCommand): Promise<FeedbackView>;
  loadFeedback(id: string): Promise<FeedbackView>;
  retryAiPublication(id: string): Promise<InterviewSnapshot>;
  /** Fotografia atual, sem alterar o estado. */
  snapshot(): Promise<InterviewSnapshot>;
  /** Lista o banco de perguntas, opcionalmente filtrado por categoria. */
  listQuestions(category?: Category | null): Promise<Question[]>;
  /** Cria uma pergunta editável. */
  createQuestion(input: QuestionInput): Promise<Question>;
  /** Edita uma pergunta com controlo otimista de versão. */
  updateQuestion(
    id: Id,
    expectedVersion: Version,
    input: QuestionInput,
  ): Promise<Question>;
  /** Inicia a entrevista e apresenta a primeira pergunta guardada. */
  startSession(request: StartSession): Promise<InterviewSnapshot>;
  /** Lista as sessões para o histórico. */
  listSessions(): Promise<SessionSummary[]>;
  /** Abre uma sessão completa para consulta ou retoma. */
  loadSession(sessionId: Id): Promise<SessionDetail>;
  /** Lista os treinos da fase 1, apenas de leitura. */
  listLegacy(): Promise<RestoreResult[]>;
  /** Lê um treino da fase 1 pelo identificador, sem alterar o formato antigo. */
  loadLegacy(attemptId: Id): Promise<RestoreResult>;
  /** Reserva a gravação e abre o microfone na pergunta atual. */
  startRecording(
    sessionQuestionId: Id,
    expectedSelection: Version,
  ): Promise<InterviewSnapshot>;
  /** Termina a captura, publica o original e pede a transcrição automática. */
  stopRecording(): Promise<InterviewSnapshot>;
  /** Repete a publicação de uma gravação já finalizada. */
  retrySaveRecording(recordingId: Id): Promise<InterviewSnapshot>;
  /** Transcreve uma resposta publicada e ainda não confirmada. */
  transcribeResponse(target: AnswerTarget): Promise<InterviewSnapshot>;
  /** Guarda uma revisão; não confirma nem avança. */
  saveReview(request: SaveReview): Promise<InterviewSnapshot>;
  /** Confirma exatamente a versão de texto que o utilizador viu. */
  confirmAnswer(request: ConfirmAnswer): Promise<InterviewSnapshot>;
  /** Avança para a pergunta seguinte ou termina a sessão. */
  advanceSession(request: AdvanceSession): Promise<InterviewSnapshot>;
  /** Reproduz uma pergunta, uma gravação ou um treino da fase 1. */
  playAudio(target: AudioSelection): Promise<InterviewSnapshot>;
  /** Pára a reprodução em curso. */
  stopAudio(): Promise<InterviewSnapshot>;
  /** Verifica microfone, saída e auxiliar. */
  refreshAvailability(): Promise<InterviewSnapshot>;
  /** Reinicia o auxiliar Python. */
  restartWorker(): Promise<InterviewSnapshot>;
  /** Finaliza a captura, interrompe o trabalho pendente e encerra o auxiliar. */
  prepareClose(): Promise<InterviewSnapshot>;
  /** Repete a abertura e a reconciliação da base de dados. */
  retryDatabase(): Promise<InterviewSnapshot>;
  // -- Fase 4: histórias, práticas, reformulações, comparação e consumo --
  /** Lista as histórias locais, filtradas por competência (só leitura). */
  listStories(competency?: string | null): Promise<StoryView[]>;
  createStory(request: CreateStory): Promise<StoryView>;
  updateStory(request: UpdateStory): Promise<StoryView>;
  /** Apoio guardado de uma resposta, só em leitura local. */
  loadPreparation(owner: CaptureOwner): Promise<PreparationView>;
  selectStory(request: SelectStory): Promise<InterviewSnapshot>;
  resolvePreparation(
    recordingId: Id,
    requestId: Id,
    decision: PreparationDecision,
  ): Promise<InterviewSnapshot>;
  retryPreparationPublication(recordingId: Id): Promise<InterviewSnapshot>;
  createPractice(request: CreatePractice): Promise<PracticeDetail>;
  loadPractice(practiceId: Id): Promise<PracticeDetail>;
  listPractices(sessionQuestionId: Id): Promise<PracticeDetail[]>;
  abandonPractice(request: PracticeCommand): Promise<PracticeDetail>;
  startPracticeRecording(
    owner: CaptureOwner,
    expectedSelection: Version,
  ): Promise<InterviewSnapshot>;
  savePracticeReview(request: SavePracticeReview): Promise<InterviewSnapshot>;
  confirmPracticeAnswer(
    request: ConfirmPracticeAnswer,
  ): Promise<PracticeDetail>;
  transcribePracticeResponse(
    target: PracticeAnswerTarget,
  ): Promise<InterviewSnapshot>;
  evaluatePractice(request: EvaluatePractice): Promise<PracticeReportView>;
  loadPracticeReport(practiceId: Id): Promise<PracticeReportView>;
  requestRewrite(request: RewriteCommand): Promise<RewriteView>;
  listRewrites(origin: ResponseOrigin): Promise<RewriteView[]>;
  compareResponses(request: ComparisonRequest): Promise<ComparisonView>;
  loadTrainingUsage(sessionId: Id): Promise<TrainingUsageView>;
  loadRewritePreference(): Promise<RewritePreference>;
  setRewriteLevel(request: SetRewriteLevel): Promise<RewritePreference>;
  /** Recebe cada fotografia publicada; devolve a remoção do listener. */
  subscribe(listener: (snapshot: InterviewSnapshot) => void): Unsubscribe;
}

/** Cria o cliente sobre a aplicação Tauri em execução. */
export function createInterviewClient(): InterviewClient {
  return {
    loadAiSettings: () =>
      invoke<AiSettingsView>("interview_ai_load_ai_settings"),
    saveAiSettings: (request) =>
      invoke<AiSettingsView>("interview_ai_save_ai_settings", { request }),
    saveAiKey: (key) =>
      invoke<AiSettingsView>("interview_ai_save_ai_key", { key }),
    removeAiKey: () => invoke<AiSettingsView>("interview_ai_remove_ai_key"),
    acceptAiConsent: (request) =>
      invoke<AiSettingsView>("interview_ai_accept_ai_consent", { request }),
    requestFollowUp: (request) =>
      invoke<InterviewSnapshot>("interview_ai_request_follow_up", { request }),
    waiveFollowUp: (request) =>
      invoke<InterviewSnapshot>("interview_ai_waive_follow_up", { request }),
    generateFeedback: (request) =>
      invoke<FeedbackView>("interview_ai_generate_feedback", { request }),
    evaluateQuestion: (request) =>
      invoke<FeedbackView>("interview_ai_evaluate_question", { request }),
    loadFeedback: (id) =>
      invoke<FeedbackView>("interview_ai_load_feedback", { id }),
    retryAiPublication: (id) =>
      invoke<InterviewSnapshot>("interview_ai_retry_ai_publication", { id }),
    snapshot: () => invoke<InterviewSnapshot>("interview_snapshot"),
    listQuestions: (category) =>
      invoke<Question[]>("interview_list_questions", {
        category: category ?? null,
      }),
    createQuestion: (input) =>
      invoke<Question>("interview_create_question", { input }),
    updateQuestion: (id, expectedVersion, input) =>
      invoke<Question>("interview_update_question", {
        id,
        expectedVersion,
        input,
      }),
    startSession: (request) =>
      invoke<InterviewSnapshot>("interview_start_session", { request }),
    listSessions: () => invoke<SessionSummary[]>("interview_list_sessions"),
    loadSession: (sessionId) =>
      invoke<SessionDetail>("interview_load_session", { sessionId }),
    listLegacy: () => invoke<RestoreResult[]>("interview_list_legacy"),
    loadLegacy: (attemptId) =>
      invoke<RestoreResult>("interview_load_legacy", { attemptId }),
    startRecording: (sessionQuestionId, expectedSelection) =>
      invoke<InterviewSnapshot>("interview_start_recording", {
        sessionQuestionId,
        expectedSelection,
      }),
    stopRecording: () => invoke<InterviewSnapshot>("interview_stop_recording"),
    retrySaveRecording: (recordingId) =>
      invoke<InterviewSnapshot>("interview_retry_save_recording", {
        recordingId,
      }),
    transcribeResponse: (target) =>
      invoke<InterviewSnapshot>("interview_transcribe_response", { target }),
    saveReview: (request) =>
      invoke<InterviewSnapshot>("interview_save_review", { request }),
    confirmAnswer: (request) =>
      invoke<InterviewSnapshot>("interview_confirm_answer", { request }),
    advanceSession: (request) =>
      invoke<InterviewSnapshot>("interview_advance_session", { request }),
    playAudio: (target) =>
      invoke<InterviewSnapshot>("interview_play_audio", { target }),
    stopAudio: () => invoke<InterviewSnapshot>("interview_stop_audio"),
    refreshAvailability: () =>
      invoke<InterviewSnapshot>("interview_refresh_availability"),
    restartWorker: () => invoke<InterviewSnapshot>("interview_restart_worker"),
    prepareClose: () => invoke<InterviewSnapshot>("interview_prepare_close"),
    retryDatabase: () => invoke<InterviewSnapshot>("interview_retry_database"),
    listStories: (competency) =>
      invoke<StoryView[]>("interview_list_stories", {
        competency: competency ?? null,
      }),
    createStory: (request) =>
      invoke<StoryView>("interview_create_story", { request }),
    updateStory: (request) =>
      invoke<StoryView>("interview_update_story", { request }),
    loadPreparation: (owner) =>
      invoke<PreparationView>("interview_load_preparation", { owner }),
    selectStory: (request) =>
      invoke<InterviewSnapshot>("interview_select_story", { request }),
    resolvePreparation: (recordingId, requestId, decision) =>
      invoke<InterviewSnapshot>("interview_resolve_preparation", {
        recordingId,
        requestId,
        decision,
      }),
    retryPreparationPublication: (recordingId) =>
      invoke<InterviewSnapshot>("interview_retry_preparation_publication", {
        recordingId,
      }),
    createPractice: (request) =>
      invoke<PracticeDetail>("interview_create_practice", { request }),
    loadPractice: (practiceId) =>
      invoke<PracticeDetail>("interview_load_practice", { practiceId }),
    listPractices: (sessionQuestionId) =>
      invoke<PracticeDetail[]>("interview_list_practices", {
        sessionQuestionId,
      }),
    abandonPractice: (request) =>
      invoke<PracticeDetail>("interview_abandon_practice", { request }),
    startPracticeRecording: (owner, expectedSelection) =>
      invoke<InterviewSnapshot>("interview_start_practice_recording", {
        owner,
        expectedSelection,
      }),
    savePracticeReview: (request) =>
      invoke<InterviewSnapshot>("interview_save_practice_review", { request }),
    confirmPracticeAnswer: (request) =>
      invoke<PracticeDetail>("interview_confirm_practice_answer", { request }),
    transcribePracticeResponse: (target) =>
      invoke<InterviewSnapshot>("interview_transcribe_practice_response", {
        target,
      }),
    evaluatePractice: (request) =>
      invoke<PracticeReportView>("interview_evaluate_practice", { request }),
    loadPracticeReport: (practiceId) =>
      invoke<PracticeReportView>("interview_load_practice_report", {
        practiceId,
      }),
    requestRewrite: (request) =>
      invoke<RewriteView>("interview_request_rewrite", { request }),
    listRewrites: (origin) =>
      invoke<RewriteView[]>("interview_list_rewrites", { origin }),
    compareResponses: (request) =>
      invoke<ComparisonView>("interview_compare_responses", { request }),
    loadTrainingUsage: (sessionId) =>
      invoke<TrainingUsageView>("interview_load_training_usage", { sessionId }),
    loadRewritePreference: () =>
      invoke<RewritePreference>("interview_load_rewrite_preference"),
    setRewriteLevel: (request) =>
      invoke<RewritePreference>("interview_set_rewrite_level", { request }),
    subscribe(listener) {
      let unlisten: Unsubscribe | null = null;
      let disposed = false;
      void listen<InterviewSnapshot>(INTERVIEW_SNAPSHOT_EVENT, (event) => {
        listener(event.payload);
      }).then((remove) => {
        // A subscrição pode terminar antes de `listen` resolver.
        if (disposed) {
          remove();
        } else {
          unlisten = remove;
        }
      });
      return () => {
        disposed = true;
        if (unlisten) {
          unlisten();
          unlisten = null;
        }
      };
    },
  };
}

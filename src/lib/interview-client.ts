/**
 * Adaptador Tauri da entrevista guiada.
 *
 * Este é o único ficheiro que conhece os nomes dos comandos `interview_*` e do
 * evento `interview://snapshot`; a interface usa apenas `InterviewClient`. Cada
 * método devolve a fotografia (`InterviewSnapshot`), um DTO de leitura ou
 * rejeita com o `AppError` estável devolvido pelo serviço (mensagem em
 * português de Portugal).
 */

import { invoke } from "@tauri-apps/api/core";
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

/** Evento com a fotografia da entrevista, publicado depois de cada alteração. */
export const INTERVIEW_SNAPSHOT_EVENT = "interview://snapshot";

/** Remoção de um listener de fotografias. */
export type Unsubscribe = () => void;

/** Acesso único da interface aos comandos Tauri da fase 2. */
export interface InterviewClient {
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
  /** Recebe cada fotografia publicada; devolve a remoção do listener. */
  subscribe(listener: (snapshot: InterviewSnapshot) => void): Unsubscribe;
}

/** Cria o cliente sobre a aplicação Tauri em execução. */
export function createInterviewClient(): InterviewClient {
  return {
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
    stopRecording: () =>
      invoke<InterviewSnapshot>("interview_stop_recording"),
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
    restartWorker: () =>
      invoke<InterviewSnapshot>("interview_restart_worker"),
    prepareClose: () => invoke<InterviewSnapshot>("interview_prepare_close"),
    retryDatabase: () =>
      invoke<InterviewSnapshot>("interview_retry_database"),
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

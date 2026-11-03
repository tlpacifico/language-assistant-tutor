/**
 * Contratos da fase 2 (entrevistas guiadas).
 *
 * Duplicação intencional de `src-tauri/src/interview/types.rs`: JSON em
 * camelCase, enums em snake_case, identificadores UUID em texto, datas UTC em
 * RFC3339 e durações inteiras em milissegundos. `contracts/interview-v2.example.json`
 * fixa a forma exata dos dois lados; o teste Rust compara-o com a serialização
 * dos DTOs e falha se um nome ou valor divergir.
 */

import type {
  AppError,
  Availability,
  CaptureMeta,
  JobStage,
  OperationKind,
  PlaybackTarget,
  Transcript,
} from "./contracts";

/** Identificador opaco de uma entidade da fase 2 (UUID em texto). */
export type Id = string;

/** Versão de concorrência ou de conteúdo. Zero significa «ainda não guardado». */
export type Version = number;

/** Códigos de erro estáveis da fase 2, reutilizando os da fase 1 quando aplicável. */
export const INTERVIEW_ERROR_CODES = [
  "INVALID_INPUT",
  "INSUFFICIENT_QUESTIONS",
  "ACTIVE_SESSION_EXISTS",
  "ANSWER_CONFIRMED",
  "ANSWER_REQUIRED",
  "DATABASE_UNAVAILABLE",
  "DATABASE_VERSION_UNSUPPORTED",
  "BUSY",
  "STALE_SOURCE",
  "STORAGE_WRITE_FAILED",
] as const;

export type InterviewErrorCode = (typeof INTERVIEW_ERROR_CODES)[number];

/** As seis categorias comportamentais da spec. */
export type Category =
  | "conflict_collaboration"
  | "leadership_initiative"
  | "failure_learning"
  | "problem_solving"
  | "priorities_deadlines"
  | "change_adaptation";

/** Etiquetas em português de Portugal, iguais às de `Category::label_pt`. */
export const CATEGORY_LABELS: Record<Category, string> = {
  conflict_collaboration: "Conflitos e colaboração",
  leadership_initiative: "Liderança e iniciativa",
  failure_learning: "Falhas e aprendizagens",
  problem_solving: "Resolução de problemas",
  priorities_deadlines: "Prioridades e prazos",
  change_adaptation: "Adaptação a mudanças",
};

/** Campos editáveis de uma pergunta do banco. */
export interface QuestionInput {
  text: string;
  category: Category;
  competencies: string[];
  criteria: string[];
}

/** Pergunta guardada, com identidade e versão de conteúdo. */
export interface Question extends QuestionInput {
  id: Id;
  version: Version;
  createdAt: string;
  updatedAt: string;
}

/** Pedido de início de sessão. `category` nulo significa «todas as categorias». */
export interface StartSession {
  mode?: SessionMode;
  requestId: Id;
  category: Category | null;
  count: number;
}

export type SessionStatus = "in_progress" | "completed";

/** Modo de realização da sessão. As sessões da fase 1 não pertencem a este modo. */
export type SessionMode = "local_v2" | "ai_v3";

/** Resumo observável de uma sessão. */
export interface SessionSummary {
  id: Id;
  category: Category | null;
  count: number;
  confirmedCount: number;
  /** Posição corrente, baseada em 1. */
  position: number;
  status: SessionStatus;
  mode: SessionMode;
  version: Version;
  createdAt: string;
  completedAt: string | null;
}

/** Pergunta de uma sessão, com a cópia imutável do conteúdo à data de início. */
export interface SessionQuestion {
  id: Id;
  sessionId: Id;
  sourceQuestionId: Id;
  sourceVersion: Version;
  /** Posição na sessão, baseada em 1. */
  position: number;
  text: string;
  category: Category;
  competencies: string[];
  /** Critérios comuns e específicos efetivos, como foram copiados. */
  criteria: string[];
  selectedRecordingId: Id | null;
  selectionVersion: Version;
}

/** Alvo exato de uma resposta. Nunca aceita caminhos de ficheiro do frontend. */
export interface AnswerTarget {
  answerId?: Id;
  sessionId: Id;
  sessionQuestionId: Id;
  recordingId: Id;
  selectionVersion: Version;
}

/**
 * Áudio que o núcleo sabe resolver para reprodução.
 *
 * O chamador indica apenas identidades: os caminhos são resolvidos no núcleo e
 * nunca chegam da interface. A variante `legacy` lê um treino da fase 1.
 */
export type AudioSelection =
  | { type: "follow_up"; sessionId: Id; sessionQuestionId: Id; answerId: Id }
  | { type: "question"; sessionId: Id; sessionQuestionId: Id }
  | { type: "recording"; recordingId: Id }
  | { type: "legacy"; attemptId: Id };

export type ReviewOrigin = "automatic" | "manual_explicit";

/** Revisão guardada (ou em edição) de uma gravação. */
export interface ReviewDraft {
  recordingId: Id;
  sourceTranscriptId: Id | null;
  origin: ReviewOrigin;
  text: string;
  /** Zero representa uma revisão ainda não guardada. */
  version: Version;
  /** Nulo enquanto a revisão não foi guardada. */
  savedAt: string | null;
}

/** Pedido para guardar uma revisão, sem confirmar nem avançar. */
export interface SaveReview {
  requestId: Id;
  target: AnswerTarget;
  expectedReviewVersion: Version;
  sourceTranscriptId: Id | null;
  origin: ReviewOrigin;
  text: string;
}

/** Pedido para confirmar a resposta: guarda o texto e confirma num só commit. */
export interface ConfirmAnswer extends SaveReview {
  expectedSessionVersion: Version;
}

/** Texto confirmado e as suas referências, todos imutáveis. */
export interface Confirmation {
  id: Id;
  sessionQuestionId: Id;
  recordingId: Id;
  reviewVersion: Version;
  sourceTranscriptId: Id | null;
  origin: ReviewOrigin;
  text: string;
  confirmedAt: string;
}

/** Pedido explícito para avançar a sessão. */
export interface AdvanceSession {
  requestId: Id;
  sessionId: Id;
  sessionQuestionId: Id;
  expectedSessionVersion: Version;
}

/** Estado de uma gravação no ciclo de reserva, captura e publicação. */
export type RecordingState =
  | "reserved"
  | "capturing"
  | "publish_pending"
  | "ready"
  | "interrupted";

/** Gravação exposta à interface, com os resultados que lhe pertencem. */
export interface RecordingView {
  id: Id;
  sessionQuestionId: Id;
  ordinal: number;
  state: RecordingState;
  capture: CaptureMeta | null;
  originalFile: string | null;
  normalizedFile: string | null;
  transcripts: Transcript[];
  review: ReviewDraft | null;
  warnings: AppError[];
}

/** Pergunta de uma sessão com as gravações e a confirmação, se existir. */
export interface QuestionDetail {
  initialAnswerId?: Id;
  followUp?: QuestionDetail | null;
  followUpStage?: string | null;
  question: SessionQuestion;
  recordings: RecordingView[];
  confirmation: Confirmation | null;
}

/** Sessão completa para consulta e retoma. */
export interface SessionDetail {
  summary: SessionSummary;
  questions: QuestionDetail[];
}

/** Identidade do trabalho a que um resultado pertence. */
export interface OperationTarget {
  answerId?: Id | null;
  sessionId: Id;
  sessionQuestionId: Id;
  recordingId: Id | null;
  selectionVersion: Version;
  operationId: Id;
  /** Zero identifica a ausência de uma encarnação do auxiliar. */
  workerGeneration: number;
}

/** Estado da base de dados, visível mesmo quando a abertura falha. */
export type DatabaseState = "ready" | "error" | "opening";

/** Trabalho de modelo em curso, sempre associado ao alvo que o originou. */
export interface ModelJobView {
  id: Id;
  kind: OperationKind;
  stage: JobStage;
  target: OperationTarget;
}

/** Fotografia do estado da entrevista observável pela interface. */
export interface InterviewSnapshot {
  currentAnswer?: {
    id: Id;
    sessionQuestionId: Id;
    role: "initial" | "follow_up";
    text: string;
    selectedRecordingId: Id | null;
    selectionVersion: number;
  } | null;
  aiJob?: import("./ai-contracts").AiJobView | null;
  pendingAiPublication?: Id | null;
  /** Revisão monotónica em memória; não substitui as versões persistidas. */
  revision: number;
  database: DatabaseState;
  activeSession: SessionSummary | null;
  current: QuestionDetail | null;
  availability: Availability;
  recordingId: Id | null;
  /**
   * Captura finalizada cuja publicação falhou. A reserva continua por guardar e
   * `startRecording`/`prepareClose` recusam até `retrySaveRecording` a resolver.
   */
  pendingRecordingId: Id | null;
  finalizing: boolean;
  modelJob: ModelJobView | null;
  playback: PlaybackTarget | null;
  warnings: AppError[];
  lastError: AppError | null;
}

/** Resultado da recuperação no arranque. */
export interface RecoveryReport {
  published: Id[];
  warnings: AppError[];
  interruptedOperations: Id[];
}

/** Alvo corrente; o pai conserva sempre a identidade e o progresso principal. */
export function activeAnswer(
  detail: QuestionDetail | null | undefined,
): QuestionDetail | null {
  return detail?.confirmation && detail.followUp
    ? detail.followUp
    : (detail ?? null);
}

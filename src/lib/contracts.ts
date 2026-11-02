/**
 * Contratos partilhados da fase 1 (secção 3 do plano).
 *
 * Duplicação intencional de `src-tauri/src/domain.rs`: JSON em camelCase,
 * enums em snake_case, identificadores UUID em texto, datas UTC em RFC3339 e
 * durações inteiras em milissegundos. `contracts/app-v1.example.json` fixa a
 * forma exata dos dois lados.
 */

/** Pergunta fixa da fase 1. */
export const INITIAL_QUESTION =
  "Tell me about a time you solved a difficult problem at work.";

/** Mensagem de um componente cuja verificação ainda não terminou. */
export const CHECKING_AVAILABILITY_MESSAGE = "A verificar disponibilidade";

/** Códigos de erro estáveis (mensagem em português de Portugal). */
export const ERROR_CODES = [
  "BUSY",
  "DEVICE_UNAVAILABLE",
  "DEVICE_FORMAT_UNSUPPORTED",
  "AUDIO_TIMEOUT",
  "CAPTURE_INTERRUPTED",
  "STORAGE_WRITE_FAILED",
  "ARTIFACT_MISSING",
  "INVALID_AUDIO",
  "MODEL_MISSING",
  "WORKER_UNAVAILABLE",
  "WORKER_PROTOCOL",
  "WORKER_TIMEOUT",
  "STALE_SOURCE",
] as const;

export type ErrorCode = (typeof ERROR_CODES)[number];

export interface AppError {
  code: string;
  message: string;
  retryable: boolean;
}

export type ComponentStatus = "ready" | "missing" | "loading" | "error";

export interface ComponentState {
  status: ComponentStatus;
  message: string | null;
}

export interface Availability {
  microphone: ComponentState;
  playback: ComponentState;
  transcription: ComponentState;
  synthesis: ComponentState;
}

export interface CaptureMeta {
  sampleRate: number;
  channels: number;
  durationMs: number;
  bytes: number;
  incomplete: boolean;
}

export type TranscriptStatus = "completed" | "no_speech";

export interface Segment {
  startMs: number;
  endMs: number;
  text: string;
}

export interface Transcript {
  id: string;
  status: TranscriptStatus;
  text: string;
  segments: Segment[];
  model: string;
  language: string;
  createdAt: string;
  loadMs: number;
  processingMs: number;
}

export interface Review {
  sourceTranscriptId: string;
  text: string;
  savedAt: string;
}

export type OperationKind = "transcribe" | "synthesize";

export type OperationState =
  | "running"
  | "completed"
  | "failed"
  | "interrupted"
  | "no_speech";

export interface OperationRecord {
  id: string;
  kind: OperationKind;
  state: OperationState;
  error: AppError | null;
}

export interface Attempt {
  id: string;
  question: string;
  createdAt: string;
  capture: CaptureMeta;
  originalFile: string;
  normalizedFile: string | null;
  transcripts: Transcript[];
  review: Review | null;
  lastOperation: OperationRecord | null;
}

export type JobStage = "loading" | "processing";

export interface ModelJob {
  id: string;
  attemptId: string | null;
  kind: OperationKind;
  stage: JobStage;
}

export type PlaybackTarget = "question" | "response";

export interface AppSnapshot {
  revision: number;
  question: string;
  availability: Availability;
  attempt: Attempt | null;
  recordingId: string | null;
  modelJob: ModelJob | null;
  playback: PlaybackTarget | null;
  lastError: AppError | null;
}

export interface RestoreResult {
  attempt: Attempt | null;
  warnings: AppError[];
}

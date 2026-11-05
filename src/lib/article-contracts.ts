/** Contratos da conversa sobre artigos; espelham `src-tauri/src/article/types.rs`. */

export const ARTICLE_NOTICE_VERSION = "article-auto-v1";
export const ARTICLE_NOTICE_TEXT =
  "Ao autorizar, o texto do artigo e a transcrição de cada resposta são enviados automaticamente ao destino configurado quando termina a gravação. O áudio permanece neste computador.";

export const MAX_ARTICLE_CHARS = 12_000;
export const MAX_TITLE_CHARS = 200;
export const SUGGEST_FINISH_AFTER = 5;

export type ArticleState =
  | "draft"
  | "awaiting_tutor"
  | "ready"
  | "recording"
  | "transcribing"
  | "needs_review"
  | "recoverable_error"
  | "finishing"
  | "completed"
  | "paused";

export type ArticleVersionSource = "automatic_sent" | "manual_review";
export type ArticleSpeaker = "tutor" | "user";
export type DimensionStatus = "clear" | "partial" | "insufficient_evidence";
export type ArticlePlayback = "idle" | "playing" | "failed";

export interface ArticleCommand {
  requestId: string;
  sessionId: string;
  expectedVersion: number;
}

export interface ArticleCreate {
  requestId: string;
  title: string | null;
  text: string;
}

export interface ArticleConsent {
  command: ArticleCommand;
  destination: string;
  noticeVersion: string;
}

export interface ArticleCorrection {
  command: ArticleCommand;
  turnId: string;
  text: string;
}

export interface ArticleAuthorization {
  destination: string;
  noticeVersion: string;
}

export interface ArticleError {
  code: string;
  message: string;
  retryable: boolean;
}

export interface ArticleUsageView {
  inputTokens: number | null;
  outputTokens: number | null;
  estimatedCost: number | null;
}

export interface ArticleSessionView {
  id: string;
  title: string | null;
  articleText: string;
  version: number;
  state: ArticleState;
  activeBranchId: string;
  authorization: ArticleAuthorization | null;
  answers: number;
  suggestFinish: boolean;
  limitReached: boolean;
  error: ArticleError | null;
  usage: ArticleUsageView | null;
}

export interface ArticleVersionView {
  id: string;
  source: ArticleVersionSource | null;
  text: string;
}

export interface ArticleTurnView {
  id: string;
  branchId: string;
  index: number;
  speaker: ArticleSpeaker;
  version: ArticleVersionView;
  audioAvailable: boolean;
  fromPreviousBranch: boolean;
}

export interface ArticleEvidence {
  turnId: string;
  quote: string;
  note: string;
}

export interface ArticleDimension {
  status: DimensionStatus;
  summary: string;
  evidence: ArticleEvidence[];
}

export interface ArticleReport {
  id: string;
  branchId: string;
  versionIds: string[];
  coherence: ArticleDimension;
  cohesion: ArticleDimension;
  development: ArticleDimension;
  interaction: ArticleDimension;
  improvements: string[];
}

export interface ArticleAudioView {
  playback: ArticlePlayback;
  turnId: string | null;
}

export interface ArticleSnapshot {
  session: ArticleSessionView;
  turns: ArticleTurnView[];
  report: ArticleReport | null;
  audio: ArticleAudioView;
}

export interface ArticleSummary {
  id: string;
  title: string | null;
  state: ArticleState;
  updatedAt: string;
}

export const MAX_ANSWER_CHARS = 8_000;

export const ARTICLE_STATE_LABELS: Record<ArticleState, string> = {
  draft: "Por autorizar",
  awaiting_tutor: "A aguardar o tutor",
  ready: "Pronta para responder",
  recording: "A gravar",
  transcribing: "A transcrever",
  needs_review: "Resposta por rever",
  recoverable_error: "Precisa de atenção",
  finishing: "A gerar o feedback",
  completed: "Concluída",
  paused: "Em pausa",
};

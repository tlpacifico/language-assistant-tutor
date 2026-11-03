/**
 * Contratos da fase 4 (melhorar e repetir).
 *
 * Duplicação intencional de `src-tauri/src/practice.rs` e de
 * `src-tauri/src/ai/rewrite.rs`: JSON em camelCase, enums em snake_case
 * (os níveis B1, B2 e C1 mantêm o nome da variante), identificadores UUID em
 * texto e datas UTC em RFC3339. Sem regras persistentes nem prompts: a
 * interface só apresenta estes DTOs e envia intenções.
 */

import type { QuestionReport } from "./ai-contracts";
import type {
  Category,
  Confirmation,
  Id,
  RecordingView,
  ReviewOrigin,
  Version,
} from "./interview-contracts";

/** Origem de uma resposta confirmada. */
export type ResponseOrigin =
  | { type: "original"; sessionId: Id; sessionQuestionId: Id }
  | { type: "practice"; practiceId: Id };

/** Dono de uma única resposta, discriminado por entrevista ou prática. */
export type CaptureOwner =
  | {
      type: "interview";
      sessionId: Id;
      sessionQuestionId: Id;
      answerId: Id;
    }
  | { type: "practice"; practiceId: Id; answerId: Id };

export type PracticeStatus =
  | "preparing"
  | "answering"
  | "confirmed"
  | "abandoned";

export type RewriteLevel = "B1" | "B2" | "C1";

export const REWRITE_LEVELS: RewriteLevel[] = ["B1", "B2", "C1"];

/** Resposta de um slot, com o papel `attempt` para tentativas de prática. */
export interface AnswerSlot {
  id: Id;
  sessionQuestionId: Id;
  role: "initial" | "follow_up" | "attempt";
  text: string;
  selectedRecordingId: Id | null;
  selectionVersion: Version;
  practiceId?: Id | null;
}

export interface QuestionView {
  text: string;
  category: Category;
  competencies: string[];
  criteria: string[];
}

export interface CreatePractice {
  requestId: Id;
  sessionId: Id;
  sessionQuestionId: Id;
  previousPracticeId: Id | null;
}

export interface PracticeCommand {
  requestId: Id;
  practiceId: Id;
  expectedVersion: Version;
}

export interface StoryInput {
  title: string;
  situation: string;
  task: string;
  actions: string;
  outcome: string;
  competencies: string[];
}

export type MissingField = "situation" | "task" | "actions" | "outcome";

export interface StoryView extends StoryInput {
  id: Id;
  version: Version;
  missingFields: MissingField[];
  createdAt: string;
  updatedAt: string;
}

export interface CreateStory {
  requestId: Id;
  input: StoryInput;
}

export interface UpdateStory {
  requestId: Id;
  id: Id;
  expectedVersion: Version;
  input: StoryInput;
}

export interface StorySnapshot {
  storyId: Id;
  storyVersion: Version;
  content: StoryInput;
}

export type LockState =
  | "editable"
  | "pending_start"
  | "locked"
  | "start_uncertain";

export type SupportKind = "none" | "story" | "historical_none";

export interface PreparationView {
  answerId: Id;
  version: Version;
  lockState: LockState;
  selectedStoryId: Id | null;
  selectedStoryVersion: Version | null;
  snapshot: StorySnapshot | null;
  support: SupportKind;
  lockedCaptureId: Id | null;
}

export interface SelectStory {
  requestId: Id;
  owner: CaptureOwner;
  expectedPreparationVersion: Version;
  storyId: Id | null;
  expectedStoryVersion: Version | null;
}

export type PreparationDecision = "keep" | "did_not_start";

export interface PracticeDetail {
  id: Id;
  sourceSessionId: Id;
  sessionQuestionId: Id;
  ordinal: number;
  previousPracticeId: Id | null;
  status: PracticeStatus;
  version: Version;
  createdAt: string;
  confirmedAt: string | null;
  abandonedAt: string | null;
  question: QuestionView;
  answer: AnswerSlot;
  recordings: RecordingView[];
  confirmation: Confirmation | null;
  preparation: PreparationView;
}

export interface PracticeAnswerTarget {
  owner: CaptureOwner;
  recordingId: Id;
  selectionVersion: Version;
}

export interface SavePracticeReview {
  requestId: Id;
  target: PracticeAnswerTarget;
  expectedReviewVersion: Version;
  sourceTranscriptId: Id | null;
  origin: ReviewOrigin;
  text: string;
}

export interface ConfirmPracticeAnswer extends SavePracticeReview {
  expectedPracticeVersion: Version;
}

/** Estado de prática observável, sem simular uma pergunta da entrevista. */
export interface ExerciseSnapshot {
  revision: number;
  activePractice: PracticeDetail | null;
  activeOwner: CaptureOwner | null;
  pendingPreparation: Id | null;
}

export interface RewriteCommand {
  requestId: Id;
  origin: ResponseOrigin;
  level: RewriteLevel;
}

export interface EvaluatePractice {
  requestId: Id;
  practiceId: Id;
  expectedVersion: Version;
}

export type ReportState = "pending" | "running" | "failed" | "ready";

export type NetworkAttemptState =
  | "running"
  | "completed"
  | "failed"
  | "interrupted"
  | "discarded";

export interface ReportVersions {
  rubricVersion: string;
  promptVersion: string;
  schemaVersion: string;
}

/** Consumo normalizado; `null` significa desconhecido, nunca zero. */
export interface NormalizedUsage {
  warnings: string[];
  input: number | null;
  output: number | null;
  cache: number | null;
  reported: number | null;
  estimated: number | null;
  conservative: boolean;
  billingSource: string | null;
  providerEstimated: boolean | null;
}

export interface SessionUsage {
  knownReportedSubtotal: number;
  knownEstimatedSubtotal: number;
  unknownAttemptCount: number;
  input: number | null;
  output: number | null;
  complete: boolean;
}

export interface NetworkAttemptView {
  networkAttemptId: Id;
  requestId: Id;
  purpose: string;
  origin: ResponseOrigin;
  state: NetworkAttemptState;
  generation: number;
  usage: NormalizedUsage;
  createdAt: string;
  finishedAt: string | null;
}

export interface PracticeReportView {
  practiceId: Id;
  state: ReportState;
  report: QuestionReport | null;
  rubricVersion: string | null;
  promptVersion: string | null;
  schemaVersion: string | null;
  usage: SessionUsage;
  networkAttempts: NetworkAttemptView[];
}

export interface RewriteQuote {
  confirmationId: Id;
  role: string;
  quote: string;
}

export interface RewriteSegment {
  id: string;
  text: string;
  evidence: RewriteQuote[];
}

export interface RewriteChange {
  source: RewriteQuote;
  segmentIds: string[];
  explanation: string;
}

export interface RewriteMissingInformation {
  dimension: string;
  assessedConfirmationIds: Id[];
  preparationQuestion: string;
  explanation: string;
}

export interface RewriteView {
  id: Id | null;
  origin: ResponseOrigin;
  level: RewriteLevel;
  state: ReportState;
  segments: RewriteSegment[];
  spokenText: string | null;
  necessaryChanges: RewriteChange[];
  optionalStyle: RewriteChange[];
  missingInformation: RewriteMissingInformation[];
  versions: ReportVersions;
  usage: SessionUsage;
  networkAttempts: NetworkAttemptView[];
}

export interface RewritePreference {
  version: Version;
  level: RewriteLevel;
  updatedAt: string;
}

export interface SetRewriteLevel {
  requestId: Id;
  expectedVersion: Version;
  level: RewriteLevel;
}

export interface ComparisonRequest {
  left: ResponseOrigin;
  right: ResponseOrigin;
}

export interface ComparisonAnswer {
  role: string;
  recordings: RecordingView[];
  confirmation: Confirmation | null;
}

export type ComparisonReportState =
  | "pending"
  | "running"
  | "failed"
  | "ready"
  | "not_available";

export interface ComparisonEntry {
  origin: ResponseOrigin;
  createdAt: string;
  confirmedAt: string | null;
  question: QuestionView;
  answers: ComparisonAnswer[];
  report: { content: QuestionReport; versions: ReportVersions } | null;
  reportState: ComparisonReportState;
  rewrites: RewriteView[];
  preparation: PreparationView;
}

export type RubricCompatibility = "same" | "different" | "unknown";

export interface ComparisonView {
  left: ComparisonEntry;
  right: ComparisonEntry;
  rubricCompatibility: RubricCompatibility;
}

export interface PurposeUsage {
  purpose: string;
  usage: SessionUsage;
  networkAttemptCount: number;
}

export interface UsageGroup {
  totals: SessionUsage;
  networkAttemptCount: number;
  byPurpose: PurposeUsage[];
}

export interface TrainingUsageView {
  sessionId: Id;
  originalInterview: UsageGroup;
  laterPractice: UsageGroup;
  combined: UsageGroup;
}

/** Identidade textual estável de uma origem, para chaves e comparações. */
export function originKey(origin: ResponseOrigin): string {
  return origin.type === "original"
    ? `original:${origin.sessionQuestionId}`
    : `practice:${origin.practiceId}`;
}

export function sameOrigin(a: ResponseOrigin, b: ResponseOrigin): boolean {
  return originKey(a) === originKey(b);
}

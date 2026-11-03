export interface Tariff {
  model: string;
  currency: string;
  inputPerMillion: number;
  outputPerMillion: number;
  cachedInputPerMillion: number | null;
  observedAt: string;
  sourceUrl: string;
}
export interface AiSettingsInput {
  baseUrl: string;
  model: string;
  tariff: Tariff | null;
}
export interface AiSettingsView extends AiSettingsInput {
  version: number;
  credentialConfigured: boolean;
  consent: {
    destination: string;
    noticeVersion: string;
    acceptedAt: string;
  } | null;
}
export interface SaveAiSettings {
  requestId: string;
  expectedVersion: number;
  input: AiSettingsInput;
}
export interface AcceptAiConsent {
  requestId: string;
  expectedSettingsVersion: number;
  destination: string;
  noticeVersion: string;
}
export interface AiQuestionCommand {
  requestId: string;
  sessionId: string;
  sessionQuestionId: string;
  expectedSessionVersion: number;
}
export interface WaiveFollowUp extends AiQuestionCommand {
  reason: "unavailable" | "request_failed" | "user_cancelled_wait";
}
export interface AiSessionCommand {
  requestId: string;
  sessionId: string;
}
export interface AiJobView {
  attemptId: string;
  purpose: string;
  sessionId: string;
  sessionQuestionId: string;
  stage: string;
}
export interface Evidence {
  type: "quote" | "absence";
  confirmationId?: string;
  role?: string;
  quote?: string;
  dimension?: string;
  assessedConfirmationIds?: string[];
}
export interface Observation {
  id: string;
  explanation: string;
  evidence: Evidence[];
  dimension?: string;
  rating?: string;
}
export interface Correction {
  id: string;
  explanation: string;
  evidence: Evidence;
  replacement: string;
}
export interface Priority {
  observationId: string;
  action: string;
}
export interface QuestionReport {
  schemaVersion: string;
  content: Observation[];
  strengths: Observation[];
  gaps: Observation[];
  english: { necessaryCorrections: Correction[]; optionalStyle: Correction[] };
  priorities: Priority[];
  fewerPrioritiesReason: string | null;
}
export interface FeedbackView {
  sessionId: string;
  state: string;
  reports: {
    questionId: string;
    position: number;
    questionText: string;
    report: QuestionReport;
    sources: { confirmationId: string; role: string; text: string }[];
  }[];
  priorities: { questionId: string; priority: Priority }[];
  usage: {
    knownReportedSubtotal: number;
    knownEstimatedSubtotal: number;
    unknownAttemptCount: number;
    input: number | null;
    output: number | null;
    complete: boolean;
  };
  attempts: {
    id: string;
    state: string;
    requestedModel?: string;
    returnedModel?: string | null;
    usage: {
      input: number | null;
      output: number | null;
      reported: number | null;
      estimated: number | null;
    };
    error: string | null;
    purpose: string;
  }[];
}

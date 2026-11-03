/**
 * Dados partilhados pelos testes do treino posterior (fase 4).
 */

import { vi } from "vitest";

import type {
  SessionDetail,
  SessionMode,
} from "../lib/interview-contracts";
import type { PracticeDetail } from "../lib/practice-contracts";
import {
  fakeInterviewClient,
  makeConfirmation,
  makePractice,
  makeQuestionDetail,
  makeRecording,
  makeSessionQuestion,
  makeTranscript,
  sessionSummary,
} from "./fake-interview-client";
import type { FakeInterviewClient } from "./fake-interview-client";

/** Sessão concluída com uma pergunta principal confirmada (sq-1). */
export function completedDetail(mode: SessionMode = "local_v2"): SessionDetail {
  return {
    summary: sessionSummary({
      id: "session-1",
      count: 1,
      confirmedCount: 1,
      position: 1,
      status: "completed",
      mode,
      completedAt: "2026-11-02T11:00:00Z",
      version: 6,
    }),
    questions: [
      makeQuestionDetail({
        question: makeSessionQuestion({
          id: "sq-1",
          sessionId: "session-1",
          position: 1,
          selectedRecordingId: "recording-1",
        }),
        recordings: [
          makeRecording({
            id: "recording-1",
            sessionQuestionId: "sq-1",
            transcripts: [makeTranscript({ text: "Original raw." })],
          }),
        ],
        confirmation: makeConfirmation({
          sessionQuestionId: "sq-1",
          recordingId: "recording-1",
          text: "Original confirmed answer.",
        }),
      }),
    ],
  };
}

/** Cliente falso cujo histórico contém a sessão concluída. */
export function fakeWithCompletedSession(
  options: Parameters<typeof fakeInterviewClient>[0] = {},
  mode: SessionMode = "local_v2",
): FakeInterviewClient {
  const fake = fakeInterviewClient(options);
  const detail = completedDetail(mode);
  fake.client.listSessions = vi.fn(async () => [detail.summary]);
  fake.client.loadSession = vi.fn(async () => detail);
  return fake;
}

/** Prática a responder, com uma tomada transcrita e selecionada. */
export function answeringPractice(
  overrides: Partial<PracticeDetail> = {},
): PracticeDetail {
  const id = overrides.id ?? "practice-1";
  const answerId = `answer-${id}`;
  return makePractice({
    id,
    status: "answering",
    answer: {
      id: answerId,
      sessionQuestionId: "sq-1",
      role: "attempt",
      text: "",
      selectedRecordingId: "rec-p1",
      selectionVersion: 1,
      practiceId: id,
    },
    recordings: [
      makeRecording({
        id: "rec-p1",
        sessionQuestionId: answerId,
        transcripts: [
          makeTranscript({ id: "tr-p1", text: "Practice raw transcript." }),
        ],
      }),
    ],
    ...overrides,
  });
}

/** Prática confirmada com a sua resposta imutável. */
export function confirmedPractice(
  overrides: Partial<PracticeDetail> = {},
): PracticeDetail {
  const id = overrides.id ?? "practice-1";
  return makePractice({
    id,
    status: "confirmed",
    version: 3,
    confirmedAt: "2026-11-03T12:30:00Z",
    recordings: [
      makeRecording({
        id: `rec-${id}`,
        sessionQuestionId: `answer-${id}`,
        transcripts: [makeTranscript({ id: `tr-${id}`, text: "Raw try." })],
      }),
    ],
    confirmation: makeConfirmation({
      id: `confirmation-${id}`,
      sessionQuestionId: "sq-1",
      recordingId: `rec-${id}`,
      text: "Practice confirmed answer.",
    }),
    ...overrides,
  });
}

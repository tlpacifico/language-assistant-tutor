import { vi } from "vitest";

import type { ArticleClient } from "../lib/article-client";
import type {
  ArticleSnapshot,
  ArticleSummary,
  ArticleTurnView,
} from "../lib/article-contracts";

export function makeTurn(
  index: number,
  speaker: "tutor" | "user",
  text: string,
  overrides: Partial<ArticleTurnView> = {},
): ArticleTurnView {
  return {
    id: `turn-${index}`,
    branchId: "branch-1",
    index,
    speaker,
    version: {
      id: `version-${index}`,
      source: speaker === "user" ? "automatic_sent" : null,
      text,
    },
    audioAvailable: speaker === "user",
    fromPreviousBranch: false,
    ...overrides,
  };
}

export function makeSnapshot(
  overrides: {
    session?: Partial<ArticleSnapshot["session"]>;
    turns?: ArticleTurnView[];
    report?: ArticleSnapshot["report"];
    audio?: Partial<ArticleSnapshot["audio"]>;
  } = {},
): ArticleSnapshot {
  return {
    session: {
      id: "session-1",
      title: "Remote work",
      articleText: "Remote work changed how teams collaborate.",
      version: 3,
      state: "ready",
      activeBranchId: "branch-1",
      authorization: {
        destination: "https://opencode.ai/inference/go/openai/v1",
        noticeVersion: "article-auto-v1",
      },
      answers: 0,
      suggestFinish: false,
      limitReached: false,
      error: null,
      usage: null,
      ...overrides.session,
    },
    turns: overrides.turns ?? [
      makeTurn(1, "tutor", "What is the main argument?"),
    ],
    report: overrides.report ?? null,
    audio: { playback: "idle", turnId: null, ...overrides.audio },
  };
}

/** Cliente falso: fotografias controladas e eventos emitidos à vontade (também fora de ordem). */
export function fakeArticleClient(initial: ArticleSnapshot = makeSnapshot()) {
  const handlers = new Set<(snapshot: ArticleSnapshot) => void>();
  let current = initial;
  const summaries: ArticleSummary[] = [
    {
      id: initial.session.id,
      title: initial.session.title,
      state: initial.session.state,
      updatedAt: "2026-11-05T10:00:00.000Z",
    },
  ];
  const respond = () => Promise.resolve(current);
  const client = {
    destination: vi.fn(() =>
      Promise.resolve("https://opencode.ai/inference/go/openai/v1"),
    ),
    list: vi.fn(() => Promise.resolve(summaries)),
    snapshot: vi.fn(() => Promise.resolve(current)),
    create: vi.fn(respond),
    authorize: vi.fn(respond),
    revoke: vi.fn(respond),
    startRecording: vi.fn(respond),
    stopRecording: vi.fn(respond),
    submitText: vi.fn(respond),
    retry: vi.fn(respond),
    finish: vi.fn(respond),
    correct: vi.fn(respond),
    resume: vi.fn(respond),
    delete: vi.fn(() => Promise.resolve()),
    playTurn: vi.fn(() => Promise.resolve()),
    stopVoice: vi.fn(() => Promise.resolve()),
    speakTurn: vi.fn(() => Promise.resolve()),
    subscribe: vi.fn((handler: (snapshot: ArticleSnapshot) => void) => {
      handlers.add(handler);
      return Promise.resolve(() => {
        handlers.delete(handler);
      });
    }),
  };
  return {
    client: client as unknown as ArticleClient & typeof client,
    /** Emite e passa a ser o que os comandos devolvem. */
    emit(snapshot: ArticleSnapshot) {
      current = snapshot;
      handlers.forEach((handler) => handler(snapshot));
    },
    /** Emite sem alterar o que os comandos devolvem (evento atrasado). */
    emitLate(snapshot: ArticleSnapshot) {
      handlers.forEach((handler) => handler(snapshot));
    },
    set(snapshot: ArticleSnapshot) {
      current = snapshot;
    },
    subscribers: () => handlers.size,
  };
}

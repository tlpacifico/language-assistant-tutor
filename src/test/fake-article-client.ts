import { vi } from "vitest";

import type { ArticleClient } from "../lib/article-client";
import type {
  ArticlePlayerView,
  ArticleRecordingView,
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
    recording?: Partial<ArticleSnapshot["recording"]>;
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
      recovery: null,
      ...overrides.session,
    },
    turns: overrides.turns ?? [
      makeTurn(1, "tutor", "What is the main argument?"),
    ],
    report: overrides.report ?? null,
    audio: {
      sessionId: "session-1",
      turnId: null,
      versionId: null,
      generation: 0,
      sequence: 0,
      state: "idle",
      positionMs: 0,
      durationMs: null,
      bufferedMs: null,
      source: null,
      trigger: null,
      failure: null,
      ...overrides.audio,
    },
    recording: {
      sessionId: "session-1",
      turnId: "",
      captureId: "",
      sequence: 0,
      state: "inactive",
      elapsedMs: 0,
      level: null,
      available: true,
      ...overrides.recording,
    },
  };
}

/** Cliente falso: fotografias controladas e eventos emitidos à vontade (também fora de ordem). */
export function fakeArticleClient(initial: ArticleSnapshot = makeSnapshot()) {
  const handlers = new Set<(snapshot: ArticleSnapshot) => void>();
  const playerHandlers = new Set<(view: ArticlePlayerView) => void>();
  const recordingHandlers = new Set<(view: ArticleRecordingView) => void>();
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
    resume: vi.fn((arg: unknown, _generation?: number) =>
      typeof arg === "string"
        ? Promise.resolve(current.audio)
        : Promise.resolve(current),
    ),
    delete: vi.fn(() => Promise.resolve()),
    preferences: vi.fn(() =>
      Promise.resolve({ autoplay: true, speedX10: 10 }),
    ),
    savePreferences: vi.fn(
      (input: { autoplay: boolean; speedX10: 8 | 10 | 12 }) =>
        Promise.resolve(input),
    ),
    play: vi.fn(() => Promise.resolve(current.audio)),
    pause: vi.fn(() => Promise.resolve(current.audio)),
    stop: vi.fn(() => Promise.resolve(current.audio)),
    focusView: vi.fn(() => Promise.resolve()),
    blurView: vi.fn(() => Promise.resolve()),
    subscribePlayer: vi.fn((handler: (view: ArticlePlayerView) => void) => {
      playerHandlers.add(handler);
      return Promise.resolve(() => {
        playerHandlers.delete(handler);
      });
    }),
    subscribeRecording: vi.fn(
      (handler: (view: ArticleRecordingView) => void) => {
        recordingHandlers.add(handler);
        return Promise.resolve(() => {
          recordingHandlers.delete(handler);
        });
      },
    ),
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
    /** Emite um estado de reprodução; a fotografia seguinte reflete-o. */
    emitPlayer(view: ArticlePlayerView) {
      current = { ...current, audio: view };
      playerHandlers.forEach((handler) => handler(view));
    },
    /** Emite um estado de gravação; a fotografia seguinte reflete-o. */
    emitRecording(view: ArticleRecordingView) {
      current = { ...current, recording: view };
      recordingHandlers.forEach((handler) => handler(view));
    },
    set(snapshot: ArticleSnapshot) {
      current = snapshot;
    },
    subscribers: () => handlers.size,
  };
}

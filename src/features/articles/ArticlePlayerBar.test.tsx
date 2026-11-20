import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type {
  ArticlePlayerView,
  ArticleRecordingView,
  ArticleSessionView,
} from "../../lib/article-contracts";
import { makeSnapshot } from "../../test/fake-article-client";
import ArticlePlayerBar from "./ArticlePlayerBar";
import {
  formatClock,
  primaryAction,
  recoveryLabel,
  voiceRetryLabel,
} from "./player";

const session = (s: Partial<ArticleSessionView> = {}): ArticleSessionView =>
  makeSnapshot({ session: s }).session;
const player = (p: Partial<ArticlePlayerView> = {}): ArticlePlayerView =>
  makeSnapshot({ audio: p }).audio;

function renderBar(overrides: {
  session?: Partial<ArticleSessionView>;
  player?: Partial<ArticlePlayerView>;
  recording?: Partial<ArticleRecordingView>;
  busy?: boolean;
} = {}) {
  const snapshot = makeSnapshot({
    session: overrides.session,
    audio: overrides.player,
    recording: overrides.recording,
  });
  const handlers = {
    onPlay: vi.fn(),
    onPause: vi.fn(),
    onResume: vi.fn(),
    onStop: vi.fn(),
    onRecord: vi.fn(),
    onStopRecording: vi.fn(),
    onFinish: vi.fn(),
    onRetry: vi.fn(),
    onResumeSession: vi.fn(),
  };
  const view = render(
    <ArticlePlayerBar
      session={snapshot.session}
      player={snapshot.audio}
      recording={snapshot.recording}
      busy={overrides.busy ?? false}
      {...handlers}
    />,
  );
  return { ...handlers, ...view };
}

describe("primaryAction", () => {
  it("bar_has_one_primary_action_for_each_conversation_state", () => {
    const record = {
      kind: "record",
      label: "Gravar resposta",
      disabled: false,
      shortcut: "Ctrl+Enter",
    };

    expect(primaryAction(session({ state: "ready" }), player())).toEqual(record);

    for (const state of ["preparing", "playing", "paused", "buffering"] as const) {
      expect(
        primaryAction(session({ state: "ready" }), player({ state, source: "tutor_voice" })),
      ).toEqual({
        kind: "respond",
        label: "Responder agora",
        disabled: false,
        shortcut: "Ctrl+Enter",
      });
    }

    expect(
      primaryAction(session({ state: "ready" }), player({ state: "playing", source: "user_recording" })),
    ).toEqual(record);

    expect(primaryAction(session({ state: "recording" }), player())).toEqual({
      kind: "stop",
      label: "Parar e enviar",
      disabled: false,
      shortcut: "Ctrl+Enter",
    });

    expect(primaryAction(session({ state: "transcribing" }), player())).toEqual({
      kind: "none",
      label: "A transcrever a resposta…",
      disabled: true,
      shortcut: null,
    });

    expect(primaryAction(session({ state: "awaiting_tutor" }), player())).toEqual({
      kind: "none",
      label: "O tutor está a preparar a resposta…",
      disabled: true,
      shortcut: null,
    });

    expect(primaryAction(session({ state: "needs_review" }), player())).toEqual({
      kind: "record",
      label: "Gravar novamente",
      disabled: false,
      shortcut: "Ctrl+Enter",
    });

    expect(
      primaryAction(session({ state: "recoverable_error", recovery: "retry_send" }), player()),
    ).toEqual({
      kind: "retry",
      label: "Tentar enviar novamente",
      disabled: false,
      shortcut: null,
    });

    expect(primaryAction(session({ state: "finishing" }), player())).toEqual({
      kind: "none",
      label: "A gerar o feedback…",
      disabled: true,
      shortcut: null,
    });

    expect(primaryAction(session({ state: "completed" }), player())).toEqual({
      kind: "none",
      label: "Concluída",
      disabled: true,
      shortcut: null,
    });

    expect(primaryAction(session({ state: "draft" }), player())).toEqual({
      kind: "none",
      label: "Por autorizar",
      disabled: true,
      shortcut: null,
    });

    expect(primaryAction(session({ state: "paused" }), player())).toEqual({
      kind: "resume",
      label: "Retomar",
      disabled: false,
      shortcut: null,
    });

    expect(primaryAction(session({ state: "ready", limitReached: true }), player())).toEqual({
      kind: "finish",
      label: "Terminar e ver feedback",
      disabled: false,
      shortcut: null,
    });
  });
});

describe("formatClock", () => {
  it("formats as m:ss", () => {
    expect(formatClock(0)).toBe("0:00");
    expect(formatClock(3200)).toBe("0:03");
    expect(formatClock(65000)).toBe("1:05");
    expect(formatClock(125000)).toBe("2:05");
  });
});

describe("ArticlePlayerBar", () => {
  it("preparing_is_never_presented_as_playing", () => {
    renderBar({
      player: { state: "preparing", source: "tutor_voice", turnId: "turn-1", bufferedMs: 5000 },
    });
    expect(screen.queryByText(/A reproduzir áudio/)).not.toBeInTheDocument();
    expect(screen.getByText(/A preparar o restante áudio…/)).toBeInTheDocument();

    // A tocar mostra «Pausar», distinto da preparação.
    const second = renderBar({
      player: { state: "playing", source: "tutor_voice", turnId: "turn-1" },
    });
    expect(second.getByText(/A reproduzir áudio/)).toBeInTheDocument();
  });

  it("paused_offers_continue_with_the_same_position", () => {
    renderBar({
      player: {
        state: "paused",
        source: "tutor_voice",
        turnId: "turn-1",
        positionMs: 65000,
        bufferedMs: 90000,
      },
    });
    expect(screen.getByRole("button", { name: "Continuar" })).toBeInTheDocument();
    expect(screen.getByText(/1:05/)).toBeInTheDocument();
  });

  it("pausing_audio_does_not_change_the_session_consent_or_answer_count", async () => {
    const user = userEvent.setup();
    const handlers = renderBar({
      session: {
        state: "ready",
        answers: 3,
        authorization: {
          destination: "https://opencode.ai/inference/go/openai/v1",
          noticeVersion: "article-auto-v1",
        },
      },
      player: { state: "playing", source: "tutor_voice", turnId: "turn-1" },
    });
    await user.click(screen.getByRole("button", { name: "Pausar" }));
    expect(handlers.onPause).toHaveBeenCalledTimes(1);
    expect(handlers.onRecord).not.toHaveBeenCalled();
    expect(handlers.onFinish).not.toHaveBeenCalled();
    expect(handlers.onRetry).not.toHaveBeenCalled();
    expect(handlers.onStopRecording).not.toHaveBeenCalled();
    // A ação principal continua a mesma: pausar não muda a conversa.
    expect(screen.getByRole("button", { name: "Responder agora" })).toBeInTheDocument();
  });

  it("generating_shows_elapsed_and_buffered_without_a_final_total", () => {
    renderBar({
      player: {
        state: "playing",
        source: "tutor_voice",
        turnId: "turn-1",
        positionMs: 5000,
        durationMs: null,
        bufferedMs: 12000,
      },
    });
    expect(screen.getByText(/A preparar o restante áudio…/)).toBeInTheDocument();
    expect(screen.getByText(/Preparado: 0:12/)).toBeInTheDocument();
    const track = screen.getByRole("progressbar", { name: "Progresso da reprodução" });
    expect(track).not.toHaveAttribute("aria-valuenow");
  });

  it("complete_audio_shows_a_read_only_progress_bar", () => {
    renderBar({
      player: {
        state: "playing",
        source: "tutor_voice",
        turnId: "turn-1",
        positionMs: 65000,
        durationMs: 120000,
        bufferedMs: 120000,
      },
    });
    const track = screen.getByRole("progressbar", { name: "Progresso da reprodução" });
    expect(track).toHaveAttribute("aria-valuemax", "120000");
    expect(track).toHaveAttribute("aria-valuenow", "65000");
    expect(screen.getByText(/1:05 \/ 2:00/)).toBeInTheDocument();
    expect(screen.queryByRole("slider")).not.toBeInTheDocument();
  });

  it("recording_shows_elapsed_and_level_or_explicit_unavailable", () => {
    const first = renderBar({
      session: { state: "recording" },
      recording: { state: "active", elapsedMs: 3200, level: 0.5 },
    });
    expect(first.getByText("0:03")).toBeInTheDocument();
    expect(first.getByRole("progressbar", { name: "Nível do microfone" })).toHaveAttribute(
      "aria-valuenow",
      "50",
    );
    first.unmount();

    const second = renderBar({
      session: { state: "recording" },
      recording: { state: "active", elapsedMs: 3200, level: null },
    });
    expect(second.getByText("Nível indisponível")).toBeInTheDocument();
    expect(
      second.queryByRole("progressbar", { name: "Nível do microfone" }),
    ).not.toBeInTheDocument();
  });

  it("the_indicator_disappears_when_the_recording_view_is_ended", () => {
    renderBar({
      session: { state: "transcribing" },
      recording: { state: "ended", elapsedMs: 5000, level: null },
    });
    expect(
      screen.queryByRole("progressbar", { name: "Nível do microfone" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("Nível indisponível")).not.toBeInTheDocument();
  });
});

describe("recovery labels", () => {
  it("recovery_labels_come_from_the_persisted_operation", () => {
    expect(recoveryLabel(session({ recovery: "retry_send" }))).toBe("Tentar enviar novamente");
    expect(recoveryLabel(session({ recovery: "retry_feedback" }))).toBe("Gerar feedback novamente");
    expect(recoveryLabel(session({ recovery: "re_record" }))).toBe("Gravar novamente");
    expect(recoveryLabel(session({ recovery: null }))).toBeNull();
  });

  it("failed_voice_and_failed_feedback_offer_distinct_retry_labels", () => {
    expect(
      voiceRetryLabel(player({ state: "failed", failure: { operation: "synthesis", retryable: true } })),
    ).toBe("Gerar voz novamente");
    expect(
      voiceRetryLabel(player({ state: "failed", failure: { operation: "output", retryable: true } })),
    ).toBe("Tentar reproduzir novamente");
    const feedback = recoveryLabel(session({ recovery: "retry_feedback" }));
    expect(feedback).toBe("Gerar feedback novamente");
    expect(feedback).not.toBe("Gerar voz novamente");
    expect(feedback).not.toBe("Tentar reproduzir novamente");
  });
});

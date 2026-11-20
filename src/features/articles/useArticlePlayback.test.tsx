import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { ArticleClient } from "../../lib/article-client";
import {
  mergePlayer,
  mergeRecording,
  type ArticlePlayerView,
  type ArticleRecordingView,
  type ArticleSnapshot,
} from "../../lib/article-contracts";
import { fakeArticleClient, makeSnapshot } from "../../test/fake-article-client";
import { useArticlePlayback } from "./useArticlePlayback";

function playerView(
  overrides: Partial<ArticlePlayerView> = {},
): ArticlePlayerView {
  return {
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
    ...overrides,
  };
}

function recordingView(
  overrides: Partial<ArticleRecordingView> = {},
): ArticleRecordingView {
  return {
    sessionId: "session-1",
    turnId: "",
    captureId: "",
    sequence: 0,
    state: "inactive",
    elapsedMs: 0,
    level: null,
    available: true,
    ...overrides,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

/** Sonda mínima: expõe o estado e as ações do hook em nós acessíveis. */
function Probe({
  client,
  sessionId,
  initial,
}: {
  client: ArticleClient;
  sessionId: string;
  initial: ArticleSnapshot;
}) {
  const playback = useArticlePlayback(client, sessionId, initial);
  return (
    <div>
      <span data-testid="player-state">{playback.player.state}</span>
      <span data-testid="player-generation">{playback.player.generation}</span>
      <span data-testid="player-sequence">{playback.player.sequence}</span>
      <span data-testid="player-turn">{playback.player.turnId ?? ""}</span>
      <span data-testid="recording-state">{playback.recording.state}</span>
      <span data-testid="recording-capture">{playback.recording.captureId}</span>
      <span data-testid="recording-sequence">
        {playback.recording.sequence}
      </span>
      <button
        type="button"
        data-testid="pause"
        onClick={() => void playback.pause()}
      >
        pause
      </button>
      <button
        type="button"
        data-testid="resume"
        onClick={() => void playback.resume()}
      >
        resume
      </button>
      <button
        type="button"
        data-testid="stop"
        onClick={() => void playback.stop()}
      >
        stop
      </button>
    </div>
  );
}

describe("useArticlePlayback", () => {
  it("an_event_between_open_and_subscribe_is_recovered_by_the_resync_snapshot", async () => {
    const initial = makeSnapshot();
    const fake = fakeArticleClient(initial);
    // Evento publicado entre a abertura e a subscrição: ninguém o ouve, mas a
    // fotografia seguinte já o reflete.
    fake.emitPlayer(
      playerView({
        state: "playing",
        generation: 1,
        sequence: 1,
        turnId: "turn-1",
        source: "tutor_voice",
      }),
    );

    render(
      <Probe client={fake.client} sessionId="session-1" initial={initial} />,
    );

    await waitFor(() =>
      expect(screen.getByTestId("player-state")).toHaveTextContent("playing"),
    );
  });

  it("a_snapshot_older_than_an_applied_event_is_ignored", async () => {
    const initial = makeSnapshot();
    const fake = fakeArticleClient(initial);
    const pending = deferred<ArticleSnapshot>();
    fake.client.snapshot.mockReturnValue(pending.promise);

    render(
      <Probe client={fake.client} sessionId="session-1" initial={initial} />,
    );

    // Um evento mais recente chega e aplica-se.
    act(() => {
      fake.emitPlayer(
        playerView({ state: "playing", generation: 1, sequence: 1, turnId: "turn-1" }),
      );
    });
    expect(screen.getByTestId("player-state")).toHaveTextContent("playing");

    // A fotografia mais antiga resolve depois: não regride o evento aplicado.
    await act(async () => {
      pending.resolve(makeSnapshot({ audio: playerView({ sequence: 0 }) }));
      await pending.promise;
    });
    expect(screen.getByTestId("player-state")).toHaveTextContent("playing");
  });

  it("an_event_from_another_session_is_ignored", async () => {
    const initial = makeSnapshot();
    const fake = fakeArticleClient(initial);
    render(
      <Probe client={fake.client} sessionId="session-1" initial={initial} />,
    );
    await waitFor(() => expect(fake.client.focusView).toHaveBeenCalled());

    act(() => {
      fake.emitPlayer(
        playerView({
          sessionId: "session-2",
          state: "playing",
          generation: 1,
          sequence: 99,
        }),
      );
    });
    expect(screen.getByTestId("player-state")).toHaveTextContent("idle");
  });

  it("an_older_recording_capture_is_ignored", async () => {
    const initial = makeSnapshot({
      recording: recordingView({
        state: "active",
        captureId: "cap-2",
        sequence: 5,
        turnId: "turn-2",
        elapsedMs: 1200,
      }),
    });
    const fake = fakeArticleClient(initial);
    render(
      <Probe client={fake.client} sessionId="session-1" initial={initial} />,
    );
    await waitFor(() => expect(fake.client.focusView).toHaveBeenCalled());

    // Progresso de uma captura antiga: sequência mais alta mas captureId velho.
    act(() => {
      fake.emitRecording(
        recordingView({
          state: "active",
          captureId: "cap-1",
          sequence: 6,
          turnId: "turn-1",
          elapsedMs: 99,
        }),
      );
    });
    expect(screen.getByTestId("recording-capture")).toHaveTextContent("cap-2");
  });

  it("focus_is_set_after_the_listeners_and_the_resync", async () => {
    const initial = makeSnapshot();
    const fake = fakeArticleClient(initial);
    render(
      <Probe client={fake.client} sessionId="session-1" initial={initial} />,
    );

    await waitFor(() => expect(fake.client.focusView).toHaveBeenCalled());

    const order = [
      fake.client.subscribePlayer.mock.invocationCallOrder[0],
      fake.client.subscribeRecording.mock.invocationCallOrder[0],
      fake.client.snapshot.mock.invocationCallOrder[0],
      fake.client.focusView.mock.invocationCallOrder[0],
    ];
    expect(order[0]).toBeLessThan(order[1]);
    expect(order[1]).toBeLessThan(order[2]);
    expect(order[2]).toBeLessThan(order[3]);
    expect(fake.client.focusView).toHaveBeenCalledWith(
      "session-1",
      expect.any(String),
    );
  });

  it("a_stale_blur_token_does_not_clear_a_newer_view", async () => {
    const initial = makeSnapshot();
    const fake = fakeArticleClient(initial);
    const first = render(
      <Probe client={fake.client} sessionId="session-1" initial={initial} />,
    );
    await waitFor(() => expect(fake.client.focusView).toHaveBeenCalledTimes(1));
    const token1 = (
      fake.client.focusView.mock.calls[0] as unknown as [string, string]
    )[1];
    first.unmount();
    await waitFor(() => expect(fake.client.blurView).toHaveBeenCalledWith(token1));

    // Uma nova montagem usa outro token: o blur antigo não limpa a vista nova.
    const second = render(
      <Probe client={fake.client} sessionId="session-1" initial={initial} />,
    );
    await waitFor(() => expect(fake.client.focusView).toHaveBeenCalledTimes(2));
    const token2 = (
      fake.client.focusView.mock.calls[1] as unknown as [string, string]
    )[1];
    expect(token2).not.toBe(token1);
    second.unmount();
    await waitFor(() => expect(fake.client.blurView).toHaveBeenCalledWith(token2));
  });

  it("unmount_before_the_resync_never_focuses_and_removes_late_listeners", async () => {
    const initial = makeSnapshot();
    const fake = fakeArticleClient(initial);
    const playerSub = deferred<() => void>();
    const recordingSub = deferred<() => void>();
    const snapshot = deferred<ArticleSnapshot>();
    fake.client.subscribePlayer.mockReturnValue(playerSub.promise);
    fake.client.subscribeRecording.mockReturnValue(recordingSub.promise);
    fake.client.snapshot.mockReturnValue(snapshot.promise);

    const unP = vi.fn();
    const unR = vi.fn();

    const { unmount } = render(
      <Probe client={fake.client} sessionId="session-1" initial={initial} />,
    );
    expect(fake.client.snapshot).toHaveBeenCalledTimes(1);
    unmount();
    expect(fake.client.blurView).toHaveBeenCalledTimes(1);

    // Resolve depois da desmontagem: cancela os listeners e nunca toma foco.
    await act(async () => {
      playerSub.resolve(unP);
      recordingSub.resolve(unR);
      snapshot.resolve(
        makeSnapshot({
          audio: playerView({ state: "playing", generation: 1, sequence: 1 }),
        }),
      );
      await Promise.all([
        playerSub.promise,
        recordingSub.promise,
        snapshot.promise,
      ]);
    });

    expect(unP).toHaveBeenCalledTimes(1);
    expect(unR).toHaveBeenCalledTimes(1);
    expect(fake.client.focusView).not.toHaveBeenCalled();
    expect(screen.queryByTestId("player-state")).not.toBeInTheDocument();
  });

  it("pause_and_resume_use_the_current_generation", async () => {
    const user = userEvent.setup();
    const initial = makeSnapshot({
      audio: playerView({
        state: "playing",
        generation: 7,
        sequence: 4,
        turnId: "turn-1",
      }),
    });
    const fake = fakeArticleClient(initial);
    render(
      <Probe client={fake.client} sessionId="session-1" initial={initial} />,
    );
    await waitFor(() => expect(fake.client.focusView).toHaveBeenCalled());

    await user.click(screen.getByTestId("pause"));
    await user.click(screen.getByTestId("resume"));

    expect(fake.client.pause).toHaveBeenCalledWith("session-1", 7);
    expect(fake.client.resume).toHaveBeenCalledWith("session-1", 7);
  });

  it("stop_is_a_no_op_without_a_real_selection", async () => {
    const user = userEvent.setup();
    const initial = makeSnapshot({
      audio: playerView({ state: "idle", generation: 0, sequence: 0 }),
    });
    const fake = fakeArticleClient(initial);
    render(
      <Probe client={fake.client} sessionId="session-1" initial={initial} />,
    );
    await waitFor(() => expect(fake.client.focusView).toHaveBeenCalled());

    // Sem seleção real (geração 0), parar é um no-op.
    await user.click(screen.getByTestId("stop"));
    expect(fake.client.stop).not.toHaveBeenCalled();

    // Uma seleção real (geração > 0) para pela geração atual.
    act(() => {
      fake.emitPlayer(
        playerView({ state: "playing", generation: 3, sequence: 2, turnId: "turn-1" }),
      );
    });
    await user.click(screen.getByTestId("stop"));
    await waitFor(() =>
      expect(fake.client.stop).toHaveBeenCalledWith("session-1", 3),
    );
  });

  it("events_delivered_before_the_subscribe_resolves_do_not_update_an_unmounted_hook", async () => {
    const initial = makeSnapshot();
    const fake = fakeArticleClient(initial);
    const playerSub = deferred<() => void>();
    const recordingSub = deferred<() => void>();
    let delivered: ((view: ArticlePlayerView) => void) | undefined;
    fake.client.subscribePlayer.mockImplementation((handler) => {
      delivered = handler;
      return playerSub.promise;
    });
    fake.client.subscribeRecording.mockReturnValue(recordingSub.promise);

    const unP = vi.fn();
    const unR = vi.fn();

    const { unmount } = render(
      <Probe client={fake.client} sessionId="session-1" initial={initial} />,
    );
    unmount();

    // Evento entregue na janela entre a desmontagem e a resolução da subscrição:
    // o handler já não atualiza estado nem regista nada.
    act(() => {
      delivered?.(
        playerView({
          state: "playing",
          generation: 1,
          sequence: 1,
          turnId: "turn-1",
        }),
      );
    });

    await act(async () => {
      playerSub.resolve(unP);
      recordingSub.resolve(unR);
      await Promise.all([playerSub.promise, recordingSub.promise]);
    });

    expect(unP).toHaveBeenCalledTimes(1);
    expect(unR).toHaveBeenCalledTimes(1);
    expect(fake.client.focusView).not.toHaveBeenCalled();
    expect(screen.queryByTestId("player-state")).not.toBeInTheDocument();
  });

  it("a_focus_that_resolves_after_unmount_is_blurred", async () => {
    const initial = makeSnapshot();
    const fake = fakeArticleClient(initial);
    const focus = deferred<void>();
    fake.client.focusView.mockReturnValue(focus.promise);

    const { unmount } = render(
      <Probe client={fake.client} sessionId="session-1" initial={initial} />,
    );
    await waitFor(() => expect(fake.client.focusView).toHaveBeenCalledTimes(1));
    const token = (
      fake.client.focusView.mock.calls[0] as unknown as [string, string]
    )[1];

    unmount();
    await waitFor(() => expect(fake.client.blurView).toHaveBeenCalledWith(token));

    await act(async () => {
      focus.resolve();
      await focus.promise;
    });

    // O foco em voo é limpo depois de resolver, com o mesmo token.
    expect(fake.client.blurView).toHaveBeenCalledTimes(2);
    expect(fake.client.blurView).toHaveBeenCalledWith(token);
  });
});

describe("merge helpers", () => {
  it("a_snapshot_with_equal_sequence_applies", () => {
    const currentPlayer = playerView({ state: "idle", sequence: 3 });
    const nextPlayer = playerView({ state: "playing", generation: 1, sequence: 3 });
    expect(mergePlayer(currentPlayer, nextPlayer, "snapshot")).toBe(nextPlayer);

    const currentRecording = recordingView({ state: "inactive", sequence: 3 });
    const nextRecording = recordingView({
      state: "ended",
      sequence: 3,
      captureId: "cap",
    });
    expect(mergeRecording(currentRecording, nextRecording, "snapshot")).toBe(
      nextRecording,
    );
  });

  it("an_event_with_equal_sequence_is_rejected", () => {
    const currentPlayer = playerView({ state: "playing", generation: 1, sequence: 3 });
    const nextPlayer = playerView({ state: "ended", generation: 1, sequence: 3 });
    expect(mergePlayer(currentPlayer, nextPlayer, "event")).toBe(currentPlayer);

    const currentRecording = recordingView({ state: "ended", sequence: 3, captureId: "cap" });
    const nextRecording = recordingView({ state: "inactive", sequence: 3 });
    expect(mergeRecording(currentRecording, nextRecording, "event")).toBe(
      currentRecording,
    );
  });
});

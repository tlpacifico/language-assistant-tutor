/**
 * Reprodução e gravação de uma conversa sobre um artigo na interface.
 *
 * O núcleo é dono da máquina de estados; este hook apenas funde os eventos
 * `article://player` e `article://recording` e a fotografia, filtrando por
 * sessão, geração, `captureId` e sequência. Na montagem instala os dois
 * listeners, relê a fotografia para recuperar eventos entre a abertura e a
 * subscrição e só então toma o foco da vista com um token novo.
 */

import { useCallback, useEffect, useRef, useState } from "react";

import { toAppError } from "../../lib/app-error";
import type { ArticleClient } from "../../lib/article-client";
import {
  mergePlayer,
  mergeRecording,
  type ArticlePlayerView,
  type ArticleRecordingView,
  type ArticleSnapshot,
} from "../../lib/article-contracts";
import type { AppError } from "../../lib/contracts";
import { newRequestId } from "../../lib/request-id";

export interface ArticlePlaybackApi {
  player: ArticlePlayerView;
  recording: ArticleRecordingView;
  play: (turnId: string) => Promise<void>;
  pause: () => Promise<void>;
  resume: () => Promise<void>;
  stop: () => Promise<void>;
  pending: boolean;
  ready: boolean;
  conversationReady: boolean;
  error: AppError | null;
}

export function useArticlePlayback(
  client: ArticleClient,
  sessionId: string,
  initial: ArticleSnapshot,
  onSnapshot?: (snapshot: ArticleSnapshot) => void,
): ArticlePlaybackApi {
  const [player, setPlayer] = useState(initial.audio);
  const [recording, setRecording] = useState(initial.recording);
  const [error, setError] = useState<AppError | null>(null);
  const [pending, setPending] = useState(false);
  const [conversationReady, setConversationReady] = useState(false);
  const [ready, setReady] = useState(false);

  const playerRef = useRef(initial.audio);
  const recordingRef = useRef(initial.recording);
  // Verdadeiro enquanto a vista está montada; usado pelas ações para nunca
  // aplicar estado depois da desmontagem.
  const mountedRef = useRef(true);
  const focusedRef = useRef(false);

  useEffect(() => {
    let cancelled = false;
    mountedRef.current = true;
    setReady(false);
    setConversationReady(false);
    const token = newRequestId();
    const unlisteners: (() => void)[] = [];

    async function trackSubscription(subscription: Promise<() => void>) {
      const unlisten = await subscription;
      if (cancelled) {
        unlisten();
      } else {
        unlisteners.push(unlisten);
      }
    }

    function removeListeners() {
      cancelled = true;
      for (const unlisten of unlisteners.splice(0)) {
        unlisten();
      }
    }

    const handlePlayer = (view: ArticlePlayerView) => {
      if (cancelled) {
        return;
      }
      const current = playerRef.current;
      const next = mergePlayer(current, view);
      if (next !== current) {
        playerRef.current = next;
        setPlayer(next);
      }
    };
    const handleRecording = (view: ArticleRecordingView) => {
      if (cancelled) {
        return;
      }
      const current = recordingRef.current;
      const next = mergeRecording(current, view);
      if (next !== current) {
        recordingRef.current = next;
        setRecording(next);
      }
    };

    void (async () => {
      await Promise.all([
        onSnapshot && trackSubscription(client.subscribe((snapshot) => {
          if (!cancelled) onSnapshot(snapshot);
        })).then(() => {
          if (!cancelled) setConversationReady(true);
        }),
        trackSubscription(client.subscribePlayer(handlePlayer)),
        trackSubscription(client.subscribeRecording(handleRecording)),
      ]);
      if (cancelled) {
        return;
      }

      const snapshot = await client.snapshot(sessionId);
      if (cancelled) {
        return;
      }
      onSnapshot?.(snapshot);
      if (snapshot) {
        const currentPlayer = playerRef.current;
        const nextPlayer = mergePlayer(currentPlayer, snapshot.audio, "snapshot");
        if (nextPlayer !== currentPlayer) {
          playerRef.current = nextPlayer;
          setPlayer(nextPlayer);
        }
        const currentRecording = recordingRef.current;
        const nextRecording = mergeRecording(
          currentRecording,
          snapshot.recording,
          "snapshot",
        );
        if (nextRecording !== currentRecording) {
          recordingRef.current = nextRecording;
          setRecording(nextRecording);
        }
      }

      // (3) Só depois de ouvir e de reler a fotografia é que a vista toma foco.
      await client.focusView(sessionId, token);
      // Um foco em voo não pode sobreviver à desmontagem: limpa-o, se a vista
      // já tiver sido desmontada enquanto o foco resolvia.
      if (cancelled) {
        void client.blurView(token).catch(() => undefined);
        return;
      }
      focusedRef.current = true;
      setReady(true);
    })().catch((failure) => {
      if (!cancelled) {
        setError(toAppError(failure));
        // Keep receiving conversation snapshots even if media setup failed.
      }
    });

    return () => {
      mountedRef.current = false;
      focusedRef.current = false;
      removeListeners();
      void client.blurView(token).catch(() => undefined);
    };
  }, [client, sessionId, onSnapshot]);

  const runAction = useCallback(async (action: () => Promise<ArticlePlayerView>) => {
    setPending(true);
    setError(null);
    try {
      const next = await action();
      if (!mountedRef.current) {
        return;
      }
      const current = playerRef.current;
      const merged = mergePlayer(current, next, "snapshot");
      if (merged !== current) {
        playerRef.current = merged;
        setPlayer(merged);
      }
    } catch (failure) {
      if (mountedRef.current) {
        setError(toAppError(failure));
      }
    } finally {
      if (mountedRef.current) {
        setPending(false);
      }
    }
  }, []);

  const play = useCallback(
    (turnId: string) => focusedRef.current
      ? runAction(() => client.play(sessionId, turnId))
      : Promise.resolve(),
    [client, sessionId, runAction],
  );
  const pause = useCallback(
    () => runAction(() => client.pause(sessionId, playerRef.current.generation)),
    [client, sessionId, runAction],
  );
  const resume = useCallback(
    () => runAction(() => client.resume(sessionId, playerRef.current.generation)),
    [client, sessionId, runAction],
  );
  const stop = useCallback(() => {
    const generation = playerRef.current.generation;
    if (generation === 0) {
      return Promise.resolve();
    }
    return runAction(() => client.stop(sessionId, generation));
  }, [client, sessionId, runAction]);

  return { player, recording, play, pause, resume, stop, pending, ready, conversationReady, error };
}

/**
 * Barra persistente da conversa sobre artigos. Fica colada ao fundo da vista,
 * com espaço reservado, e nunca cobre mensagens, erros ou foco.
 *
 * Mostra o texto de estado, o item selecionado com os controlos de reprodução,
 * o progresso honesto (elapsed/total ou elapsed + «A preparar o restante
 * áudio…»), a quantidade preparada rotulada, o nível do microfone durante a
 * gravação, a ação principal e a ação secundária «Terminar e ver feedback».
 */

import { useCallback, useLayoutEffect, useRef, type Ref } from "react";
import {
  ARTICLE_STATE_LABELS,
  type ArticlePlayerView,
  type ArticleRecordingView,
  type ArticleSessionView,
} from "../../lib/article-contracts";
import {
  canFinish,
  canReplayAudio,
  formatClock,
  playbackActive,
  primaryAction,
  voiceRetryLabel,
  type PrimaryAction,
} from "./player";

function PrimaryButton({
  buttonRef,
  action,
  busy,
  onRecord,
  onStopRecording,
  onFinish,
  onRetry,
  onResumeSession,
}: {
  buttonRef: Ref<HTMLButtonElement>;
  action: PrimaryAction;
  busy: boolean;
  onRecord: () => void;
  onStopRecording: () => void;
  onFinish: () => void;
  onRetry: () => void;
  onResumeSession: () => void;
}) {
  if (action.kind === "none") {
    return null;
  }
  const onClick =
    action.kind === "record" || action.kind === "respond"
      ? onRecord
      : action.kind === "stop"
        ? onStopRecording
        : action.kind === "resume"
          ? onResumeSession
          : action.kind === "finish"
            ? onFinish
            : onRetry;
  const disabled = action.disabled || busy;
  return (
    <button ref={buttonRef} type="button" aria-disabled={disabled || undefined}
      aria-keyshortcuts={action.shortcut ? "Control+Enter" : undefined}
      onClick={() => { if (!disabled) onClick(); }}>
      {action.label}
    </button>
  );
}

const PLAYBACK_ANNOUNCEMENTS: Record<ArticlePlayerView["state"], string> = {
  idle: "Reprodução parada.",
  preparing: "A preparar áudio.",
  playing: "A reproduzir áudio.",
  paused: "Reprodução em pausa.",
  buffering: "À espera de mais áudio.",
  ended: "Reprodução concluída.",
  failed: "Falha na reprodução.",
};

export default function ArticlePlayerBar({
  session,
  player,
  recording,
  busy,
  mediaDisabled = false,
  recordDisabled = false,
  onPlay,
  onPause,
  onResume,
  onStop,
  onRecord,
  onStopRecording,
  onFinish,
  onRetry,
  onResumeSession,
}: {
  session: ArticleSessionView;
  player: ArticlePlayerView;
  recording: ArticleRecordingView;
  busy: boolean;
  mediaDisabled?: boolean;
  recordDisabled?: boolean;
  onPlay: (turnId: string) => void;
  onPause: () => void;
  onResume: () => void;
  onStop: () => void;
  onRecord: () => void;
  onStopRecording: () => void;
  onFinish: () => void;
  onRetry: () => void;
  onResumeSession: () => void;
}) {
  const state = session.state;
  const action = primaryAction(session, player);
  const stateText =
    action.kind === "none" ? action.label : ARTICLE_STATE_LABELS[state];
  const statusRef = useRef<HTMLParagraphElement>(null);
  const primaryRef = useRef<HTMLButtonElement>(null);
  const trackPrimary = useCallback((button: HTMLButtonElement | null) => {
    primaryRef.current = button;
    if (button) return () => {
      primaryRef.current = null;
      if (document.activeElement === button) {
        statusRef.current?.focus({ preventScroll: true });
      }
    };
  }, []);

  useLayoutEffect(() => {
    if (primaryRef.current && document.activeElement === statusRef.current) {
      primaryRef.current.focus({ preventScroll: true });
    }
  }, [action.kind]);

  const hasSelection = player.turnId !== null && player.state !== "idle";
  const showMedia = canReplayAudio(state) && hasSelection;
  const selectedLabel =
    player.source === "tutor_voice" ? "Pergunta do tutor" : "A sua resposta";
  const retryLabel = voiceRetryLabel(player);
  const active = playbackActive(player);
  const knownDuration = player.durationMs !== null;
  const duration = player.durationMs ?? 0;
  const progress = duration > 0 ? Math.min(1, player.positionMs / duration) : 0;

  return (
    <section className="article__bar" aria-label="Controlos da conversa">
      <p ref={statusRef} tabIndex={-1} className="article__bar-state">{stateText}</p>
      <div className="article__visually-hidden" role="log" aria-label="Estado da reprodução"
        aria-live="polite" aria-atomic="true">
        {player.generation > 0 ? PLAYBACK_ANNOUNCEMENTS[player.state] : ""}
      </div>

      {showMedia ? (
        <div className="article__bar-media">
          <span className="article__bar-item">
            {selectedLabel} · {PLAYBACK_ANNOUNCEMENTS[player.state]}
          </span>

          {player.state === "failed" && retryLabel ? (
            <button
              type="button"
              disabled={busy || mediaDisabled}
              onClick={() => onPlay(player.turnId ?? "")}
            >
              {retryLabel}
            </button>
          ) : null}

          {active ? (
            <div className="article__bar-controls">
              {player.state !== "paused" ? (
                <button type="button" disabled={busy || mediaDisabled} onClick={onPause}>
                  Pausar
                </button>
              ) : null}
              {player.state === "paused" ? (
                <button type="button" disabled={busy || mediaDisabled} onClick={onResume}>
                  Continuar
                </button>
              ) : null}
              <button type="button" disabled={busy || mediaDisabled} onClick={onStop}>
                Parar
              </button>
            </div>
          ) : null}

          {active ? (
            <div className="article__bar-progress">
              {knownDuration ? (
                <div
                  role="progressbar"
                  className="article__bar-track"
                  aria-label="Progresso da reprodução"
                  aria-valuemin={0}
                  aria-valuemax={player.durationMs ?? 0}
                  aria-valuenow={Math.min(
                    player.positionMs,
                    player.durationMs ?? 0,
                  )}
                  aria-valuetext={`${formatClock(player.positionMs)} de ${formatClock(player.durationMs ?? 0)}`}
                >
                  <span style={{ width: `${progress * 100}%` }} />
                </div>
              ) : (
                <div
                  role="progressbar"
                  className="article__bar-track article__bar-track--indeterminate"
                  aria-label="Progresso da reprodução"
                />
              )}
              <span className="article__bar-time">
                {knownDuration
                  ? `${formatClock(player.positionMs)} / ${formatClock(player.durationMs ?? 0)}`
                  : `${formatClock(player.positionMs)} · A preparar o restante áudio…`}
              </span>
              {!knownDuration && player.bufferedMs !== null ? (
                <span className="article__bar-buffered">
                  Preparado: {formatClock(player.bufferedMs)}
                </span>
              ) : null}
            </div>
          ) : null}
        </div>
      ) : null}

      {recording.state === "active" ? (
        <div className="article__bar-recording">
          <span className="article__bar-time">
            {formatClock(recording.elapsedMs)}
          </span>
          {recording.level !== null ? (
            <div
              role="progressbar"
              className="article__meter"
              aria-label="Nível do microfone"
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={Math.round(recording.level * 100)}
            >
              <span style={{ width: `${recording.level * 100}%` }} />
            </div>
          ) : (
            <span className="article__bar-unavailable">Nível indisponível</span>
          )}
        </div>
      ) : null}

      <div className="article__bar-actions">
        <PrimaryButton
          buttonRef={trackPrimary}
          action={action}
          busy={busy || recordDisabled}
          onRecord={onRecord}
          onStopRecording={onStopRecording}
          onFinish={onFinish}
          onRetry={onRetry}
          onResumeSession={onResumeSession}
        />
        {canFinish(state) && action.kind !== "finish" ? (
          <button type="button" disabled={busy} onClick={onFinish}>
            Terminar e ver feedback
          </button>
        ) : null}
      </div>
      {state === "recording" ? (
        <p className="practice__hint">
          A transcrição será enviada automaticamente ao tutor depois de parar.
        </p>
      ) : null}
    </section>
  );
}

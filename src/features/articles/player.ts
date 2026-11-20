/**
 * Lógica pura da barra de reprodução da conversa sobre artigos.
 *
 * Sem React: só contratos e rótulos. A matriz da ação principal segue o
 * capítulo 4 da especificação; a pausa de áudio nunca muda o estado, o
 * consentimento ou o número de respostas da conversa.
 */

import type {
  ArticlePlayerView,
  ArticleSessionView,
  ArticleState,
} from "../../lib/article-contracts";

export type PrimaryActionKind =
  | "record"
  | "respond"
  | "stop"
  | "resume"
  | "finish"
  | "retry"
  | "none";

export interface PrimaryAction {
  kind: PrimaryActionKind;
  label: string;
  disabled: boolean;
  shortcut: string | null;
}

/** `mm:ss` sem horas; o áudio de uma resposta nunca chega a uma hora. */
export function formatClock(ms: number): string {
  const totalSeconds = Math.max(0, Math.floor(ms / 1000));
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${String(seconds).padStart(2, "0")}`;
}

/** Rótulo de recuperação a partir da operação persistida, nunca da mensagem. */
export function recoveryLabel(session: ArticleSessionView): string | null {
  switch (session.recovery) {
    case "retry_send":
      return "Tentar enviar novamente";
    case "retry_feedback":
      return "Gerar feedback novamente";
    case "re_record":
      return "Gravar novamente";
    default:
      return null;
  }
}

/** Rótulo da repetição de voz, a partir da operação que falhou. */
export function voiceRetryLabel(player: ArticlePlayerView): string | null {
  if (player.state !== "failed" || player.failure === null) {
    return null;
  }
  return player.failure.operation === "synthesis"
    ? "Gerar voz novamente"
    : "Tentar reproduzir novamente";
}

/** A reprodução histórica só é permitida nestes estados da conversa. */
export function canReplayAudio(state: ArticleState): boolean {
  return state === "ready" || state === "needs_review" || state === "completed";
}

/** Terminar continua possível nestes estados (e em `recoverable_error`). */
export function canFinish(state: ArticleState): boolean {
  return (
    state === "ready" ||
    state === "needs_review" ||
    state === "awaiting_tutor" ||
    state === "recoverable_error"
  );
}

const PLAYBACK_ACTIVE: ArticlePlayerView["state"][] = [
  "preparing",
  "playing",
  "paused",
  "buffering",
];

function playbackActive(player: ArticlePlayerView): boolean {
  return PLAYBACK_ACTIVE.includes(player.state);
}

/**
 * A ação principal para cada estado da conversa. `disabled` é verdadeiro só
 * para os estados sem ação (`none`); a ocupação/pendência é aplicada pelo
 * componente que recebe o resultado.
 */
export function primaryAction(
  session: ArticleSessionView,
  player: ArticlePlayerView,
): PrimaryAction {
  const finish: PrimaryAction = {
    kind: "finish",
    label: "Terminar e ver feedback",
    disabled: false,
    shortcut: null,
  };

  switch (session.state) {
    case "draft":
      return { kind: "none", label: "Por autorizar", disabled: true, shortcut: null };
    case "awaiting_tutor":
      return {
        kind: "none",
        label: "O tutor está a preparar a resposta…",
        disabled: true,
        shortcut: null,
      };
    case "transcribing":
      return {
        kind: "none",
        label: "A transcrever a resposta…",
        disabled: true,
        shortcut: null,
      };
    case "recording":
      return { kind: "stop", label: "Parar e enviar", disabled: false, shortcut: null };
    case "needs_review":
      return session.limitReached
        ? finish
        : { kind: "record", label: "Gravar novamente", disabled: false, shortcut: "Ctrl+Enter" };
    case "recoverable_error": {
      const label = recoveryLabel(session);
      return label === null
        ? finish
        : { kind: "retry", label, disabled: false, shortcut: null };
    }
    case "finishing":
      return {
        kind: "none",
        label: "A gerar o feedback…",
        disabled: true,
        shortcut: null,
      };
    case "completed":
      return { kind: "none", label: "Concluída", disabled: true, shortcut: null };
    case "paused":
      return { kind: "resume", label: "Retomar", disabled: false, shortcut: null };
    case "ready":
      if (session.limitReached) {
        return finish;
      }
      if (playbackActive(player) && player.source === "tutor_voice") {
        return { kind: "respond", label: "Responder agora", disabled: false, shortcut: "Ctrl+Enter" };
      }
      return { kind: "record", label: "Gravar resposta", disabled: false, shortcut: "Ctrl+Enter" };
  }
}

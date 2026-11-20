/**
 * Controlos de reprodução de uma fala do tutor ou da gravação de uma resposta.
 *
 * O turno ativo espelha a barra: «Pausar» a tocar, «Continuar» em pausa e
 * «Ouvir do início» para recomeçar. Uma gravação original em falta é rotulada
 * como indisponível, sem substituição nem repetição de API.
 */

import type {
  ArticlePlayerView,
  ArticleTurnView,
} from "../../lib/article-contracts";
import { playbackActive } from "./player";

export default function ArticleTurnControls({
  turn,
  player,
  disabled = false,
  onPlay,
  onPause,
  onResume,
  onRestart,
}: {
  turn: ArticleTurnView;
  player: ArticlePlayerView;
  disabled?: boolean;
  onPlay: (turnId: string) => void;
  onPause: () => void;
  onResume: () => void;
  onRestart: (turnId: string) => void;
}) {
  // A voz do tutor está sempre disponível (cache ou síntese local); a gravação
  // original de uma resposta só existe quando há áudio guardado.
  if (turn.speaker === "user" && !turn.audioAvailable) {
    return <span className="article__turn-unavailable">Áudio indisponível</span>;
  }

  const active = player.turnId === turn.id;
  const paused = active && player.state === "paused";
  const activePlayback = active && playbackActive(player);

  if (!activePlayback) {
    return (
      <span className="article__turn-controls">
        <button type="button" disabled={disabled} onClick={() => onPlay(turn.id)}>
          Ouvir
        </button>
      </span>
    );
  }

  return (
    <span className="article__turn-controls">
      {!paused ? (
        <button type="button" disabled={disabled} onClick={onPause}>
          Pausar
        </button>
      ) : null}
      {paused ? (
        <button type="button" disabled={disabled} onClick={onResume}>
          Continuar
        </button>
      ) : null}
      <button type="button" disabled={disabled} onClick={() => onRestart(turn.id)}>
        Ouvir do início
      </button>
    </span>
  );
}

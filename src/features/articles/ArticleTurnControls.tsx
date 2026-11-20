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

export default function ArticleTurnControls({
  turn,
  player,
  onPlay,
  onPause,
  onResume,
  onRestart,
}: {
  turn: ArticleTurnView;
  player: ArticlePlayerView;
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
  const playing = active && player.state === "playing";
  const paused = active && player.state === "paused";
  const activeish =
    active &&
    (player.state === "preparing" ||
      player.state === "playing" ||
      player.state === "paused" ||
      player.state === "buffering");

  if (!activeish) {
    return (
      <span className="article__turn-controls">
        <button type="button" onClick={() => onPlay(turn.id)}>
          Ouvir
        </button>
      </span>
    );
  }

  return (
    <span className="article__turn-controls">
      {playing ? (
        <button type="button" onClick={onPause}>
          Pausar
        </button>
      ) : null}
      {paused ? (
        <button type="button" onClick={onResume}>
          Continuar
        </button>
      ) : null}
      <button type="button" onClick={() => onRestart(turn.id)}>
        Ouvir do início
      </button>
    </span>
  );
}

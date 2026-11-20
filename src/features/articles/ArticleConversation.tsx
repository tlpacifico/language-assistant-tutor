import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";

import type { ArticleClient } from "../../lib/article-client";
import {
  ARTICLE_NOTICE_VERSION,
  MAX_ANSWER_CHARS,
  SUGGEST_FINISH_AFTER,
  type ArticlePlayerView,
  type ArticlePreferences as ArticlePreferencesView,
  type ArticleSnapshot,
  type ArticleState,
  type ArticleTurnView,
} from "../../lib/article-contracts";
import ArticleConsent from "./ArticleConsent";
import ArticlePlayerBar from "./ArticlePlayerBar";
import ArticlePreferences from "./ArticlePreferences";
import ArticleReport from "./ArticleReport";
import ArticleTurnControls from "./ArticleTurnControls";
import { canFinish, canReplayAudio, playbackActive, primaryAction } from "./player";
import { useArticle } from "./useArticle";
import { useArticlePlayback } from "./useArticlePlayback";
import { useArticleShortcuts } from "./useArticleShortcuts";
import { useFollowTail } from "./useFollowTail";

/** Comentário e pergunta chegam num só turno, separados por uma linha em branco. */
function paragraphs(text: string): string[] {
  return text
    .split(/\n{2,}/)
    .map((part) => part.trim())
    .filter((part) => part.length > 0);
}

/** Estados em que o microfone ou a transcrição estão ocupados. */
function isBusy(state: ArticleSnapshot["session"]["state"]): boolean {
  return state === "recording" || state === "transcribing";
}

function Turn({
  turn,
  anchor,
  player,
  state,
  mediaDisabled,
  action,
  onPlay,
  onPause,
  onResume,
  onRestart,
}: {
  turn: ArticleTurnView;
  anchor?: string;
  player: ArticlePlayerView;
  state: ArticleState;
  mediaDisabled: boolean;
  action?: React.ReactNode;
  onPlay: (turnId: string) => void;
  onPause: () => void;
  onResume: () => void;
  onRestart: (turnId: string) => void;
}) {
  const tutor = turn.speaker === "tutor";
  const emptyUserTurn = !tutor && turn.version.text === "";
  const active = player.turnId === turn.id;
  return (
    <li
      id={anchor}
      tabIndex={anchor ? -1 : undefined}
      data-testid={tutor ? "turn-tutor" : "turn-user"}
      aria-label={tutor ? "Tutor" : "Você"}
      className={`article__turn article__turn--${turn.speaker}${
        active ? " article__turn--active" : ""
      }`}
    >
      {emptyUserTurn ? (
        <p className="article__turn-placeholder">
          {state === "transcribing" ? "A transcrever…" : "Transcrição indisponível."}
        </p>
      ) : (
        paragraphs(turn.version.text).map((part, index) => (
          <p key={index} lang="en">
            {part}
          </p>
        ))
      )}
      {turn.fromPreviousBranch ? (
        <small>Ramo anterior (só leitura)</small>
      ) : null}
      {canReplayAudio(state) ? (
        <ArticleTurnControls
          turn={turn}
          player={player}
          disabled={mediaDisabled}
          onPlay={onPlay}
          onPause={onPause}
          onResume={onResume}
          onRestart={onRestart}
        />
      ) : null}
      {action}
    </li>
  );
}

/**
 * Conversa: perguntas do tutor, respostas do utilizador e ações permitidas
 * pelo estado persistido. Sem editor obrigatório e sem correções entre turnos;
 * a correção só existe depois de concluída e abre um ramo novo.
 */
export default function ArticleConversation({
  client,
  initial,
  onBusyChange,
}: {
  client: ArticleClient;
  initial: ArticleSnapshot;
  onBusyChange?: (busy: boolean) => void;
}) {
  const { snapshot, error, pending, capturePending, run, apply } = useArticle(initial);
  const playback = useArticlePlayback(client, initial.session.id, initial, apply);
  const { session, turns, report } = snapshot;
  const [destination, setDestination] = useState<string | null>(null);
  const [consent, setConsent] = useState(false);
  const [manual, setManual] = useState("");
  const [correcting, setCorrecting] = useState<string | null>(null);
  const [corrected, setCorrected] = useState("");
  const [preferences, setPreferences] = useState<ArticlePreferencesView | null>(null);
  const [panelOpen, setPanelOpen] = useState(false);
  const headingRef = useRef<HTMLHeadingElement>(null);
  const chatRef = useRef<HTMLDivElement>(null);
  const panelId = useId();
  const latestQuestionId = useId();

  const shownError = error ?? session.error ?? playback.error;
  const busy = isBusy(session.state) || capturePending;

  useEffect(() => {
    onBusyChange?.(busy);
  }, [busy, onBusyChange]);

  useLayoutEffect(() => {
    headingRef.current?.focus({ preventScroll: true });
  }, [session.id]);

  useEffect(() => {
    if (session.state !== "draft") {
      return;
    }
    let active = true;
    void client.destination().then((value) => active && setDestination(value));
    return () => {
      active = false;
    };
  }, [client, session.state]);

  useEffect(() => {
    let active = true;
    void client
      .preferences()
      .then((value) => active && setPreferences(value))
      .catch(() => undefined);
    return () => {
      active = false;
    };
  }, [client]);

  function savePreferences(next: ArticlePreferencesView): void {
    const previous = preferences;
    setPreferences(next);
    void client
      .savePreferences(next)
      .then(setPreferences)
      .catch(() => previous && setPreferences(previous));
  }

  const userTurns = turns.filter((t) => t.speaker === "user");
  const lastUser = userTurns[userTurns.length - 1];
  const state = session.state;
  const canRevoke = session.authorization !== null && canFinish(state);

  // Um turno do utilizador sem texto só aparece como «A transcrever…» enquanto
  // a transcrição está em curso; o texto nunca é fabricado.
  const visibleTurns = turns.filter(
    (t) =>
      t.version.text !== "" ||
      t.speaker === "tutor" ||
      (t.speaker === "user" && (state === "transcribing" ||
        (state === "needs_review" && t.id === lastUser?.id))),
  );
  const latestQuestion = visibleTurns.filter((turn) => turn.speaker === "tutor").at(-1);
  const follow = useFollowTail(chatRef, visibleTurns.map((turn) => turn.id));

  function record() {
    void run("startRecording", (command) => client.startRecording(command));
  }
  function stopRecording() {
    void run("stopRecording", (command) => client.stopRecording(command));
  }

  const primary = primaryAction(session, playback.player);
  const captureUnavailable = !playback.conversationReady &&
    (primary.kind === "record" || primary.kind === "respond");
  const canPause = canReplayAudio(state) && playback.player.turnId !== null &&
    playbackActive(playback.player);
  useArticleShortcuts({
    onRecord: pending || captureUnavailable || primary.disabled || !primary.shortcut
      ? null
      : primary.kind === "stop" ? stopRecording : record,
    onPauseToggle: pending || playback.pending || !playback.ready || !canPause
      ? null
      : playback.player.state === "paused" ? playback.resume : playback.pause,
  });

  const statusLines: string[] = [];
  if (session.limitReached) {
    statusLines.push(
      "Limite da conversa atingido. Termine para obter o feedback.",
    );
  } else if (session.suggestFinish && session.answers >= SUGGEST_FINISH_AFTER) {
    statusLines.push(
      `Já deu ${session.answers} respostas. Pode terminar para ver o feedback.`,
    );
  }

  return (
    <div className="article__conversation">
      <header>
        <h2 ref={headingRef} tabIndex={-1}>{session.title ?? "Conversa sobre um artigo"}</h2>
        {session.usage ? (
          <p className="practice__hint">
            Consumo: {session.usage.inputTokens ?? "?"} tokens de entrada, {session.usage.outputTokens ?? "?"} tokens de saída
          </p>
        ) : null}
        <div className="article__actions">
          <button type="button" aria-expanded={panelOpen} aria-controls={panelId}
            onClick={() => setPanelOpen(!panelOpen)}>
            Ver o artigo
          </button>
          {latestQuestion ? (
            <a href={`#${latestQuestionId}`} onClick={(event) => {
              const question = document.getElementById(latestQuestionId);
              if (question) {
                event.preventDefault();
                question.focus();
              }
            }}>
              Ir para a última pergunta
            </a>
          ) : null}
        </div>
        {preferences ? (
          <details className="article__audio-preferences">
            <summary>Preferências de áudio</summary>
            <ArticlePreferences
              preferences={preferences}
              onAutoplayChange={(autoplay) => savePreferences({ ...preferences, autoplay })}
              onSpeedChange={(speedX10) => savePreferences({ ...preferences, speedX10 })}
            />
          </details>
        ) : null}
      </header>

      <div className={panelOpen ? "article__reading article__reading--open" : "article__reading"}>
        <aside id={panelId} role="region" aria-label="Artigo" hidden={!panelOpen}
          tabIndex={panelOpen ? 0 : undefined} className="article__reading-panel">
          <p lang="en" className="article__text">{session.articleText}</p>
        </aside>
        <div ref={chatRef} role="region" aria-label="Mensagens da conversa"
          tabIndex={0} className="article__scroll" onScroll={follow.onScroll}>
          <ol aria-label="Conversa" className="article__turns">
            {visibleTurns.map((turn) => (
              <Turn
                key={turn.id}
                anchor={turn.id === latestQuestion?.id ? latestQuestionId : undefined}
                turn={turn}
                player={playback.player}
                state={state}
                mediaDisabled={pending || !playback.ready}
                onPlay={(turnId) => void playback.play(turnId)}
                onPause={() => void playback.pause()}
                onResume={() => void playback.resume()}
                onRestart={(turnId) => void playback.play(turnId)}
                action={
                  state === "completed" &&
                  turn.speaker === "user" &&
                  !turn.fromPreviousBranch ? (
                    <button
                      type="button"
                      disabled={pending}
                      onClick={() => {
                        setCorrecting(turn.id);
                        setCorrected(turn.version.text);
                      }}
                    >
                      Corrigir esta resposta
                    </button>
                  ) : undefined
                }
              />
            ))}
          </ol>

          {statusLines.length > 0 ? (
            <div role="status" className="practice__hint">
              {statusLines.map((line) => (
                <p key={line}>{line}</p>
              ))}
            </div>
          ) : null}

          {shownError ? (
            <div
              role="alert"
              className="practice__notice practice__notice--error"
            >
              <p>{shownError.message}</p>
            </div>
          ) : null}

          {state === "draft" ? (
            <section className="practice__panel">
              <p>A autorização foi revogada. Para continuar, autorize outra vez.</p>
              <ArticleConsent
                destination={destination}
                checked={consent}
                onChange={setConsent}
              />
              <button
                type="button"
                disabled={!consent || destination === null || pending}
                onClick={() =>
                  void run("authorize", (command) =>
                    client.authorize({
                      command,
                      destination: destination ?? "",
                      noticeVersion: ARTICLE_NOTICE_VERSION,
                    }),
                  )
                }
              >
                Autorizar e continuar
              </button>
            </section>
          ) : null}

          {state === "needs_review" && lastUser ? (
            <section className="practice__panel">
              <label>
                A sua resposta (a transcrição falhou)
                <textarea
                  rows={4}
                  value={manual}
                  onChange={(event) => setManual(event.target.value)}
                />
              </label>
              <button
                type="button"
                disabled={
                  pending ||
                  manual.trim() === "" ||
                  [...manual.trim()].length > MAX_ANSWER_CHARS
                }
                onClick={() =>
                  void run("submitText", (command) =>
                    client.submitText({
                      command,
                      turnId: lastUser.id,
                      text: manual.trim(),
                    }),
                  ).then(() => setManual(""))
                }
              >
                Enviar texto
              </button>
            </section>
          ) : null}

          {canRevoke ? (
            <div className="article__actions">
              <button
                type="button"
                disabled={pending}
                onClick={() => void run("revoke", (command) => client.revoke(command))}
              >
                Revogar autorização
              </button>
            </div>
          ) : null}

          {correcting ? (
            <section className="practice__panel">
              <label>
                Texto corrigido
                <textarea
                  rows={4}
                  value={corrected}
                  onChange={(event) => setCorrected(event.target.value)}
                />
              </label>
              <p className="practice__hint">
                Regenerar o relatório pode consumir a API. A conversa anterior e o
                seu relatório ficam guardados.
              </p>
              <button
                type="button"
                disabled={pending || corrected.trim() === ""}
                onClick={() =>
                  void run("correct", (command) =>
                    client.correct({
                      command,
                      turnId: correcting,
                      text: corrected.trim(),
                    }),
                  ).then(() => setCorrecting(null))
                }
              >
                Guardar correção
              </button>{" "}
              <button type="button" onClick={() => setCorrecting(null)}>
                Cancelar
              </button>
            </section>
          ) : null}

          {state === "completed" ? (
            report ? (
              <ArticleReport
                report={report}
                turns={turns}
                player={playback.player}
                mediaDisabled={pending || !playback.ready}
                onPlay={(turnId) => void playback.play(turnId)}
                onPause={() => void playback.pause()}
                onResume={() => void playback.resume()}
                onRestart={(turnId) => void playback.play(turnId)}
              />
            ) : (
              <p className="practice__hint">
                Sem relatório: não há avaliação para mostrar porque não houve
                respostas.
              </p>
            )
          ) : null}

        </div>
      </div>
      {!follow.atEnd && follow.newCount > 0 ? (
        <button type="button" onClick={() => {
          follow.jumpToLatest();
          chatRef.current?.focus({ preventScroll: true });
        }}>
          Nova resposta ↓ ({follow.newCount})
        </button>
      ) : null}
      <ArticlePlayerBar
        session={session}
        player={playback.player}
        recording={playback.recording}
        busy={pending}
        mediaDisabled={!playback.ready}
        onPlay={(turnId) => void playback.play(turnId)}
        onPause={() => void playback.pause()}
        onResume={() => void playback.resume()}
        onStop={() => void playback.stop()}
        recordDisabled={captureUnavailable}
        onRecord={record}
        onStopRecording={stopRecording}
        onFinish={() => void run("finish", (command) => client.finish(command))}
        onRetry={() => void run("retry", (command) => client.retry(command))}
        onResumeSession={() =>
          void run("resume", (command) => client.resume(command))
        }
      />
    </div>
  );
}

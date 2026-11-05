import { useEffect, useRef, useState } from "react";

import type { ArticleClient } from "../../lib/article-client";
import {
  ARTICLE_NOTICE_VERSION,
  ARTICLE_STATE_LABELS,
  MAX_ANSWER_CHARS,
  SUGGEST_FINISH_AFTER,
  type ArticleSnapshot,
  type ArticleTurnView,
} from "../../lib/article-contracts";
import ArticleConsent from "./ArticleConsent";
import ArticleReport from "./ArticleReport";
import { useArticle } from "./useArticle";

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
  action,
}: {
  turn: ArticleTurnView;
  action?: React.ReactNode;
}) {
  const tutor = turn.speaker === "tutor";
  return (
    <li
      data-testid={tutor ? "turn-tutor" : "turn-user"}
      aria-label={tutor ? "Tutor" : "Você"}
      className={`article__turn article__turn--${turn.speaker}`}
    >
      {paragraphs(turn.version.text).map((part, index) => (
        <p key={index} lang="en">
          {part}
        </p>
      ))}
      {turn.fromPreviousBranch ? (
        <small>Ramo anterior (só leitura)</small>
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
  const { snapshot, error, pending, run } = useArticle(client, initial);
  const { session, turns, report, audio } = snapshot;
  const [destination, setDestination] = useState<string | null>(null);
  const [consent, setConsent] = useState(false);
  const [manual, setManual] = useState("");
  const [correcting, setCorrecting] = useState<string | null>(null);
  const [corrected, setCorrected] = useState("");
  const alertRef = useRef<HTMLDivElement>(null);

  const shownError = error ?? session.error;
  const busy = isBusy(session.state);

  useEffect(() => {
    onBusyChange?.(busy);
  }, [busy, onBusyChange]);

  useEffect(() => {
    if (shownError) {
      alertRef.current?.focus();
    }
  }, [shownError?.message, shownError?.code]); // eslint-disable-line react-hooks/exhaustive-deps

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

  const userTurns = turns.filter((t) => t.speaker === "user");
  const lastUser = userTurns[userTurns.length - 1];
  const state = session.state;
  const canRecord = state === "ready" || state === "needs_review";
  const canFinish =
    state === "ready" ||
    state === "needs_review" ||
    state === "awaiting_tutor" ||
    state === "recoverable_error";
  const canRevoke = session.authorization !== null && canFinish;

  const statusLines: string[] = [];
  if (state === "awaiting_tutor") {
    statusLines.push("O tutor está a preparar a resposta…");
  }
  if (state === "transcribing") {
    statusLines.push("A transcrever a resposta…");
  }
  if (state === "finishing") {
    statusLines.push("A gerar o feedback…");
  }
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
        <h2>{session.title ?? "Conversa sobre um artigo"}</h2>
        <p className="practice__hint">
          {ARTICLE_STATE_LABELS[state]}
          {session.usage
            ? ` · consumo: ${session.usage.inputTokens ?? "?"} tokens de entrada, ${session.usage.outputTokens ?? "?"} de saída`
            : ""}
        </p>
        <details>
          <summary>Ver o artigo</summary>
          <p lang="en" className="article__text">
            {session.articleText}
          </p>
        </details>
      </header>

      <ol aria-label="Conversa" className="article__turns">
        {turns
          .filter((t) => t.version.text !== "" || t.speaker === "tutor")
          .map((turn) => (
            <Turn
              key={turn.id}
              turn={turn}
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
          ref={alertRef}
          tabIndex={-1}
          role="alert"
          className="practice__notice practice__notice--error"
        >
          <p>{shownError.message}</p>
        </div>
      ) : null}

      {audio.playback === "playing" ? (
        <button type="button" onClick={() => void client.stopVoice()}>
          Parar voz
        </button>
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

      <div className="article__actions">
        {state === "recording" ? (
          <button
            type="button"
            disabled={pending}
            onClick={() => void run("stopRecording", (command) => client.stopRecording(command))}
          >
            Parar gravação
          </button>
        ) : null}
        {canRecord ? (
          <button
            type="button"
            disabled={pending || session.limitReached}
            onClick={() =>
              void run("startRecording", (command) => client.startRecording(command))
            }
          >
            Gravar resposta
          </button>
        ) : null}
        {state === "recoverable_error" && session.error?.retryable ? (
          <button
            type="button"
            disabled={pending}
            onClick={() => void run("retry", (command) => client.retry(command))}
          >
            Repetir
          </button>
        ) : null}
        {canFinish ? (
          <button
            type="button"
            disabled={pending}
            onClick={() => void run("finish", (command) => client.finish(command))}
          >
            Terminar e ver feedback
          </button>
        ) : null}
        {canRevoke ? (
          <button
            type="button"
            disabled={pending}
            onClick={() => void run("revoke", (command) => client.revoke(command))}
          >
            Revogar autorização
          </button>
        ) : null}
      </div>

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
            onPlay={(turnId) =>
              void client.playTurn(session.id, turnId).catch(() => undefined)
            }
          />
        ) : (
          <p className="practice__hint">
            Sem relatório: não há avaliação para mostrar porque não houve
            respostas.
          </p>
        )
      ) : null}
    </div>
  );
}

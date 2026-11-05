import { useEffect, useRef, useState } from "react";

import { toAppError } from "../../lib/app-error";
import type { ArticleClient } from "../../lib/article-client";
import {
  ARTICLE_NOTICE_VERSION,
  MAX_ARTICLE_CHARS,
  MAX_TITLE_CHARS,
  type ArticleSnapshot,
} from "../../lib/article-contracts";
import type { AppError } from "../../lib/contracts";
import { newRequestId } from "../../lib/request-id";
import ArticleConsent from "./ArticleConsent";

const grouped = new Intl.NumberFormat("fr-FR", { useGrouping: true });

/** Número com espaço fino entre milhares, como no resto da interface. */
function format(value: number): string {
  return grouped.format(value).replace(/[  ]/g, " ");
}

/**
 * Preparação: artigo colado, título opcional e autorização explícita.
 *
 * O texto nunca é cortado: acima do limite o botão fica desativado e a
 * mensagem explica o excesso. Criar e autorizar são dois comandos distintos; a
 * conversa só é entregue depois de ambos.
 */
export default function ArticleSetup({
  client,
  onStarted,
}: {
  client: ArticleClient;
  onStarted: (snapshot: ArticleSnapshot) => void;
}) {
  const [text, setText] = useState("");
  const [title, setTitle] = useState("");
  const [consent, setConsent] = useState(false);
  const [destination, setDestination] = useState<string | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [busy, setBusy] = useState(false);
  const alertRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let active = true;
    void client
      .destination()
      .then((value) => active && setDestination(value))
      .catch((failure) => active && setError(toAppError(failure)));
    return () => {
      active = false;
    };
  }, [client]);

  useEffect(() => {
    if (error) {
      alertRef.current?.focus();
    }
  }, [error]);

  const length = [...text.trim()].length;
  const tooLong = length > MAX_ARTICLE_CHARS;
  const titleTooLong = [...title.trim()].length > MAX_TITLE_CHARS;
  const valid = length > 0 && !tooLong && !titleTooLong;

  async function start(): Promise<void> {
    if (!valid || !consent || destination === null || busy) {
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const created = await client.create({
        requestId: newRequestId(),
        title: title.trim() === "" ? null : title.trim(),
        text,
      });
      const started = await client.authorize({
        command: {
          requestId: newRequestId(),
          sessionId: created.session.id,
          expectedVersion: created.session.version,
        },
        destination,
        noticeVersion: ARTICLE_NOTICE_VERSION,
      });
      onStarted(started);
    } catch (failure) {
      setError(toAppError(failure));
    } finally {
      setBusy(false);
    }
  }

  return (
    <form
      className="practice__panel article__setup"
      onSubmit={(event) => {
        event.preventDefault();
        void start();
      }}
    >
      <h2>Conversar sobre um artigo</h2>
      <p className="practice__hint">
        Cole o texto de um artigo em inglês. O tutor faz perguntas, responde às
        suas ideias e, no fim, comenta a coerência, a coesão, o desenvolvimento e
        a interação das suas respostas.
      </p>
      <label>
        Título (opcional)
        <input
          type="text"
          value={title}
          onChange={(event) => setTitle(event.target.value)}
        />
      </label>
      <label>
        Texto do artigo
        <textarea
          rows={10}
          value={text}
          onChange={(event) => setText(event.target.value)}
          aria-invalid={tooLong}
        />
      </label>
      <p className="practice__hint" aria-live="polite">
        {format(length)} / {format(MAX_ARTICLE_CHARS)}
      </p>
      {tooLong ? (
        <p role="alert">
          O artigo excede {format(MAX_ARTICLE_CHARS)} caracteres. Reduza o texto:
          a aplicação não o corta.
        </p>
      ) : null}
      {titleTooLong ? (
        <p role="alert">O título excede {MAX_TITLE_CHARS} caracteres.</p>
      ) : null}
      <ArticleConsent
        destination={destination}
        checked={consent}
        onChange={setConsent}
      />
      {error ? (
        <div
          ref={alertRef}
          tabIndex={-1}
          role="alert"
          className="practice__notice practice__notice--error"
        >
          <p>{error.message}</p>
        </div>
      ) : null}
      <button type="submit" disabled={!valid || !consent || busy}>
        Começar a conversa
      </button>
    </form>
  );
}

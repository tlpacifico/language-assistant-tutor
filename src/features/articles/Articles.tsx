import { useCallback, useEffect, useRef, useState } from "react";

import { toAppError } from "../../lib/app-error";
import type { ArticleClient } from "../../lib/article-client";
import {
  ARTICLE_STATE_LABELS,
  type ArticleSnapshot,
  type ArticleSummary,
} from "../../lib/article-contracts";
import type { AppError } from "../../lib/contracts";
import { newRequestId } from "../../lib/request-id";
import ArticleConversation from "./ArticleConversation";
import ArticleSetup from "./ArticleSetup";
import "./articles.css";

type Mode =
  | { kind: "list" }
  | { kind: "new" }
  | { kind: "open"; snapshot: ArticleSnapshot };

/**
 * Página Artigos: lista de conversas, preparação de uma nova e conversa aberta.
 *
 * Abrir, retomar ou montar a página nunca inicia envios, gravações nem som: só
 * lê o estado persistido (a retoma recupera estado local, sem rede). O estado
 * ocupado (gravação ou transcrição) é entregue à aplicação para bloquear a
 * navegação.
 */
export default function Articles({
  client,
  onBusyChange,
}: {
  client: ArticleClient;
  onBusyChange?: (busy: boolean) => void;
}) {
  const [mode, setMode] = useState<Mode>({ kind: "list" });
  const [items, setItems] = useState<ArticleSummary[] | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [confirming, setConfirming] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const alertRef = useRef<HTMLDivElement>(null);

  const reload = useCallback(async () => {
    try {
      setItems(await client.list());
    } catch (failure) {
      setError(toAppError(failure));
    }
  }, [client]);

  useEffect(() => {
    if (mode.kind === "list") {
      void reload();
    }
  }, [mode.kind, reload]);

  useEffect(() => {
    if (error) {
      alertRef.current?.focus();
    }
  }, [error]);

  useEffect(() => {
    onBusyChange?.(busy);
    return () => onBusyChange?.(false);
  }, [busy, onBusyChange]);

  async function open(id: string, resume: boolean): Promise<void> {
    setError(null);
    try {
      const current = await client.snapshot(id);
      const snapshot = resume
        ? await client.resume({
            requestId: newRequestId(),
            sessionId: id,
            expectedVersion: current.session.version,
          })
        : current;
      setMode({ kind: "open", snapshot });
    } catch (failure) {
      setError(toAppError(failure));
    }
  }

  async function remove(item: ArticleSummary): Promise<void> {
    setError(null);
    try {
      const current = await client.snapshot(item.id);
      await client.delete({
        requestId: newRequestId(),
        sessionId: item.id,
        expectedVersion: current.session.version,
      });
      setConfirming(null);
      await reload();
    } catch (failure) {
      setError(toAppError(failure));
    }
  }

  const errorBox = error ? (
    <div
      ref={alertRef}
      tabIndex={-1}
      role="alert"
      className="practice__notice practice__notice--error"
    >
      <p>{error.message}</p>
    </div>
  ) : null;

  if (mode.kind === "new") {
    return (
      <div className="article">
        <button type="button" onClick={() => setMode({ kind: "list" })}>
          Voltar à lista
        </button>
        <ArticleSetup
          client={client}
          onStarted={(snapshot) => setMode({ kind: "open", snapshot })}
        />
      </div>
    );
  }

  if (mode.kind === "open") {
    return (
      <div className="article">
        <button
          type="button"
          disabled={busy}
          onClick={() => setMode({ kind: "list" })}
        >
          Voltar à lista
        </button>
        <ArticleConversation
          key={mode.snapshot.session.id}
          client={client}
          initial={mode.snapshot}
          onBusyChange={setBusy}
        />
      </div>
    );
  }

  return (
    <div className="article">
      <section className="practice__panel">
        <h2>Conversas sobre artigos</h2>
        <p className="practice__hint">
          Converse em inglês sobre um artigo e receba feedback sobre coerência,
          coesão, desenvolvimento e interação. Só pode haver uma conversa em
          curso de cada vez.
        </p>
        <button type="button" onClick={() => setMode({ kind: "new" })}>
          Nova conversa
        </button>
      </section>
      {errorBox}
      {items === null ? (
        <p className="practice__hint">A carregar as conversas…</p>
      ) : items.length === 0 ? (
        <p className="practice__hint">Ainda não tem conversas sobre artigos.</p>
      ) : (
        <ul aria-label="Conversas" className="article__list">
          {items.map((item) => (
            <li key={item.id}>
              <strong>{item.title ?? "Sem título"}</strong>
              <span className="practice__hint">
                {" "}
                · {ARTICLE_STATE_LABELS[item.state]}
              </span>
              <div>
                <button type="button" onClick={() => void open(item.id, false)}>
                  Abrir
                </button>
                {item.state !== "completed" ? (
                  <button type="button" onClick={() => void open(item.id, true)}>
                    Retomar
                  </button>
                ) : null}
                {confirming === item.id ? (
                  <>
                    <button type="button" onClick={() => void remove(item)}>
                      Confirmar eliminação
                    </button>
                    <button type="button" onClick={() => setConfirming(null)}>
                      Cancelar
                    </button>
                  </>
                ) : (
                  <button type="button" onClick={() => setConfirming(item.id)}>
                    Eliminar
                  </button>
                )}
              </div>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

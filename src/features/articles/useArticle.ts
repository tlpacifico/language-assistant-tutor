/**
 * Estado de uma conversa sobre um artigo na interface.
 *
 * A interface só apresenta fotografias persistidas pelo núcleo: não há lógica
 * de transição aqui. Eventos de outra conversa, atrasados (versão anterior) ou
 * duplicados são ignorados; um comando de cada vez, com um `requestId` novo por
 * clique (um clique duplo não duplica o efeito).
 */

import { useCallback, useEffect, useRef, useState } from "react";

import { toAppError } from "../../lib/app-error";
import type { ArticleClient } from "../../lib/article-client";
import type {
  ArticleCommand,
  ArticleSnapshot,
} from "../../lib/article-contracts";
import type { AppError } from "../../lib/contracts";
import { newRequestId } from "../../lib/request-id";

export type ArticleAction = (
  command: ArticleCommand,
) => Promise<ArticleSnapshot | void>;

export function useArticle(client: ArticleClient, initial: ArticleSnapshot) {
  const [snapshot, setSnapshot] = useState(initial);
  const [error, setError] = useState<AppError | null>(null);
  const [pending, setPending] = useState(false);
  const latest = useRef(initial);
  const busy = useRef(false);
  // Mesmo gesto sobre a mesma versão reutiliza o `requestId`: um clique duplo
  // repete a intenção e o núcleo aplica-a uma só vez.
  const lastRequest = useRef<{ key: string; id: string } | null>(null);

  const apply = useCallback((next: ArticleSnapshot) => {
    const current = latest.current;
    if (next.session.id !== current.session.id) {
      return;
    }
    if (next.session.version < current.session.version) {
      return;
    }
    latest.current = next;
    setSnapshot(next);
  }, []);

  useEffect(() => {
    let cancelled = false;
    let remove: (() => void) | null = null;
    void client.subscribe(apply).then((unsubscribe) => {
      if (cancelled) {
        unsubscribe();
      } else {
        remove = unsubscribe;
      }
    });
    return () => {
      cancelled = true;
      remove?.();
    };
  }, [client, apply]);

  const run = useCallback(
    async (gesture: string, action: ArticleAction) => {
      if (busy.current) {
        return;
      }
      const key = `${gesture}:${latest.current.session.version}`;
      if (lastRequest.current?.key !== key) {
        lastRequest.current = { key, id: newRequestId() };
      }
      const requestId = lastRequest.current.id;
      busy.current = true;
      setPending(true);
      setError(null);
      try {
        const next = await action({
          requestId,
          sessionId: latest.current.session.id,
          expectedVersion: latest.current.session.version,
        });
        if (next) {
          apply(next);
        }
      } catch (failure) {
        setError(toAppError(failure));
      } finally {
        busy.current = false;
        setPending(false);
      }
    },
    [apply],
  );

  return {
    snapshot,
    error,
    pending,
    run,
    dismissError: () => setError(null),
  };
}

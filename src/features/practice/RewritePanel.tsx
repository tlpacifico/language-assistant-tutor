/**
 * Sugestões de reformulação de uma resposta confirmada.
 *
 * O painel nunca pede uma sugestão por iniciativa própria: o pedido (que envia
 * o texto confirmado e os critérios à API) só parte de um clique explícito. As
 * variantes válidas são imutáveis e ficam por nível; uma resposta que chega
 * para outro nível guarda-se na sua variante e não troca a seleção do
 * utilizador. A voz usa apenas o texto sugerido, nunca as explicações.
 */

import { useCallback, useEffect, useRef, useState } from "react";

import { toAppError } from "../../lib/app-error";
import type { AppError } from "../../lib/contracts";
import type { InterviewClient } from "../../lib/interview-client";
import type { InterviewSnapshot } from "../../lib/interview-contracts";
import {
  originKey,
  REWRITE_LEVELS,
  type RewriteLevel,
  type RewritePreference,
  type RewriteView,
  type ResponseOrigin,
} from "../../lib/practice-contracts";
import { newRequestId } from "../../lib/request-id";
import { UsageLine } from "./shared";

const DIMENSION_LABELS: Record<string, string> = {
  relevance: "Relevância",
  situation: "Situação",
  task: "Tarefa",
  individual_responsibility: "Responsabilidade individual",
  actions: "Ações",
  result: "Resultado",
  learning: "Aprendizagem",
};

function ChangeList({
  title,
  changes,
}: {
  title: string;
  changes: RewriteView["necessaryChanges"];
}) {
  return (
    <section aria-label={title}>
      <h5>{title}</h5>
      {changes.length === 0 ? (
        <p className="practice__hint">Nenhuma.</p>
      ) : (
        <ul className="practice__sublist">
          {changes.map((change, index) => (
            <li key={`${change.source.confirmationId}-${index}`}>
              <blockquote lang="en">{change.source.quote}</blockquote>
              <p>{change.explanation}</p>
              <p className="practice__hint">
                Segmentos afetados: {change.segmentIds.join(", ")}.
              </p>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

export interface RewritePanelProps {
  client: InterviewClient;
  origin: ResponseOrigin;
  /** Fotografia atual: reprodução, captura e publicação pendente. */
  snapshot?: InterviewSnapshot | null;
  onSnapshot?: (snapshot: InterviewSnapshot) => void;
  /** Mostra a ação de guardar um resultado já recebido (por omissão, sim). */
  showPendingPublication?: boolean;
}

export default function RewritePanel({
  client,
  origin,
  snapshot = null,
  onSnapshot,
  showPendingPublication = true,
}: RewritePanelProps) {
  const key = originKey(origin);
  const [preference, setPreference] = useState<RewritePreference | null>(null);
  // Nível visível: começa em B1 e só muda por ação do utilizador ou pela
  // preferência local guardada, nunca por uma resposta remota.
  const [level, setLevel] = useState<RewriteLevel>("B1");
  const [variants, setVariants] = useState<RewriteView[]>([]);
  const [outcomes, setOutcomes] = useState<
    Partial<Record<RewriteLevel, { view?: RewriteView; error?: AppError }>>
  >({});
  const [requesting, setRequesting] = useState<RewriteLevel | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [retryingPublication, setRetryingPublication] = useState(false);
  const intention = useRef<{ key: string; requestId: string } | null>(null);
  const originRef = useRef(key);
  originRef.current = key;

  const pendingPublication = snapshot?.pendingAiPublication ?? null;
  const capturing =
    snapshot !== null && (snapshot.recordingId !== null || snapshot.finalizing);

  const loadVariants = useCallback(async (): Promise<void> => {
    try {
      const rows = await client.listRewrites(origin);
      if (originRef.current === originKey(origin)) {
        setVariants(rows.filter((row) => row.state === "ready"));
      }
    } catch (failure) {
      setError(toAppError(failure));
    }
  }, [client, key]);

  useEffect(() => {
    let active = true;
    setVariants([]);
    setOutcomes({});
    setError(null);
    client
      .loadRewritePreference()
      .then((next) => {
        if (active) {
          setPreference(next);
          setLevel(next.level);
        }
      })
      .catch((failure: unknown) => {
        if (active) {
          setError(toAppError(failure));
        }
      });
    void loadVariants();
    return () => {
      active = false;
    };
  }, [client, key, loadVariants]);

  // Um resultado guardado depois de uma falha de publicação passa a aparecer.
  useEffect(() => {
    if (pendingPublication === null) {
      void loadVariants();
    }
  }, [pendingPublication, loadVariants]);

  async function chooseLevel(next: RewriteLevel): Promise<void> {
    setLevel(next);
    if (!preference || preference.level === next) {
      return;
    }
    // Preferência local: não envia nada para a API e não altera sugestões.
    try {
      setPreference(
        await client.setRewriteLevel({
          requestId: newRequestId(),
          expectedVersion: preference.version,
          level: next,
        }),
      );
    } catch (failure) {
      setError(toAppError(failure));
    }
  }

  async function request(target: RewriteLevel, fresh: boolean): Promise<void> {
    if (requesting !== null) {
      return;
    }
    const intentionKey = `${key}:${target}`;
    if (fresh || intention.current?.key !== intentionKey) {
      intention.current = { key: intentionKey, requestId: newRequestId() };
    }
    const requestId = intention.current.requestId;
    const requestedOrigin = key;
    setRequesting(target);
    setError(null);
    try {
      const view = await client.requestRewrite({
        requestId,
        origin,
        level: target,
      });
      intention.current = null;
      if (originRef.current !== requestedOrigin) {
        return;
      }
      setOutcomes((previous) => ({ ...previous, [target]: { view } }));
      if (view.state === "ready" && view.id !== null) {
        // Guarda na variante do nível pedido, sem trocar o nível visível.
        setVariants((previous) => [
          ...previous.filter((entry) => entry.level !== target),
          view,
        ]);
      }
    } catch (failure) {
      intention.current = null;
      if (originRef.current === requestedOrigin) {
        setOutcomes((previous) => ({
          ...previous,
          [target]: { error: toAppError(failure) },
        }));
      }
    } finally {
      setRequesting(null);
    }
  }

  async function listen(view: RewriteView): Promise<void> {
    if (view.id === null) {
      return;
    }
    setError(null);
    try {
      const next = await client.playAudio({
        type: "rewrite",
        rewriteId: view.id,
      });
      onSnapshot?.(next);
    } catch (failure) {
      setError(toAppError(failure));
    }
  }

  async function stop(): Promise<void> {
    try {
      const next = await client.stopAudio();
      onSnapshot?.(next);
    } catch (failure) {
      setError(toAppError(failure));
    }
  }

  async function savePending(id: string): Promise<void> {
    setRetryingPublication(true);
    setError(null);
    try {
      const next = await client.retryAiPublication(id);
      onSnapshot?.(next);
      await loadVariants();
    } catch (failure) {
      setError(toAppError(failure));
    } finally {
      setRetryingPublication(false);
    }
  }

  const variant = variants.find((entry) => entry.level === level) ?? null;
  const outcome = outcomes[level];
  const failed =
    outcome?.error !== undefined || outcome?.view?.state === "failed";
  const waiting =
    requesting === level ||
    outcome?.view?.state === "running" ||
    outcome?.view?.state === "pending";

  return (
    <section
      className="practice__panel practice__panel--wide"
      aria-labelledby={`rewrite-title-${key}`}
    >
      <h4 id={`rewrite-title-${key}`}>Reformulação por nível</h4>
      <p className="practice__hint">
        O nível é um objetivo de linguagem para a reformulação e não um
        diagnóstico CEFR. Mudar o nível não envia nada à API.
      </p>

      <div role="radiogroup" aria-label="Nível da reformulação">
        {REWRITE_LEVELS.map((entry) => (
          <label key={entry} className="practice__choice">
            <input
              type="radio"
              name={`rewrite-level-${key}`}
              value={entry}
              checked={level === entry}
              onChange={() => void chooseLevel(entry)}
            />{" "}
            {entry}
            {variants.some((candidate) => candidate.level === entry)
              ? " (guardada)"
              : ""}
          </label>
        ))}
      </div>

      {pendingPublication && showPendingPublication ? (
        <div className="practice__notice" role="status">
          <p>
            Existe um resultado já recebido que ainda não foi guardado. Guardar
            não faz um novo pedido à rede.
          </p>
          <div className="practice__actions">
            <button
              type="button"
              disabled={retryingPublication}
              onClick={() => void savePending(pendingPublication)}
            >
              Guardar resultado recebido sem novo pedido
            </button>
          </div>
        </div>
      ) : null}

      {variant ? (
        <article aria-label={`Sugestão ${variant.level}`}>
          <h5>Sugestão da IA — confirme os factos</h5>
          <p className="practice__warning">
            Esta sugestão é uma paráfrase automática e pode alterar o sentido.
            Confirme que preserva os seus factos antes de a usar. Não é uma
            avaliação nem um diagnóstico de pronúncia.
          </p>
          <div className="practice__field" aria-label="Texto sugerido">
            {variant.segments.map((segment) => (
              <p key={segment.id} lang="en">
                {segment.text}
              </p>
            ))}
          </div>
          <div className="practice__actions">
            <button
              type="button"
              disabled={capturing}
              onClick={() => void listen(variant)}
            >
              Ouvir só a sugestão
            </button>
            {snapshot?.playback ? (
              <button type="button" onClick={() => void stop()}>
                Parar áudio
              </button>
            ) : null}
          </div>
          <ChangeList
            title="Alterações necessárias"
            changes={variant.necessaryChanges}
          />
          <ChangeList
            title="Alternativas opcionais de estilo"
            changes={variant.optionalStyle}
          />
          <section aria-label="Informação em falta">
            <h5>Informação em falta (fora da voz)</h5>
            {variant.missingInformation.length === 0 ? (
              <p className="practice__hint">Nenhuma lacuna indicada.</p>
            ) : (
              <ul className="practice__sublist">
                {variant.missingInformation.map((gap, index) => (
                  <li key={`${gap.dimension}-${index}`}>
                    <p>
                      <strong>
                        {DIMENSION_LABELS[gap.dimension] ?? gap.dimension}
                      </strong>
                      : {gap.explanation}
                    </p>
                    <p lang="en">{gap.preparationQuestion}</p>
                  </li>
                ))}
              </ul>
            )}
          </section>
          <p className="practice__hint">
            Versões: prompt {variant.versions.promptVersion}; esquema{" "}
            {variant.versions.schemaVersion}; rubrica{" "}
            {variant.versions.rubricVersion}.
          </p>
          <details>
            <summary>Consumo desta sugestão</summary>
            <UsageLine
              usage={variant.usage}
              networkAttemptCount={variant.networkAttempts.length}
            />
          </details>
        </article>
      ) : (
        <div>
          <p className="practice__hint">
            Ainda não existe uma sugestão {level} para esta resposta.
          </p>
          {waiting ? (
            <p role="status">A aguardar o serviço externo…</p>
          ) : null}
          {failed ? (
            <div
              className="practice__notice practice__notice--error"
              role="alert"
            >
              <p>
                O pedido de sugestão {level} falhou.{" "}
                {outcome?.error?.message ?? ""}
              </p>
              <p className="practice__hint">
                Repetir é um novo pedido e pode gerar novo custo se o
                fornecedor já processou o anterior.
              </p>
            </div>
          ) : null}
          <div className="practice__actions">
            <button
              type="button"
              disabled={requesting !== null || waiting}
              onClick={() => void request(level, failed)}
            >
              {failed
                ? `Repetir pedido de sugestão ${level}`
                : `Pedir sugestão ${level}`}
            </button>
          </div>
          <p className="practice__hint">
            O pedido envia à API do OpenCode apenas o texto confirmado e os
            critérios da pergunta. Pode ter custo.
          </p>
        </div>
      )}

      {error ? (
        <div className="practice__notice practice__notice--error" role="alert">
          <p>{error.message}</p>
          <p className="practice__hint">Código: {error.code}.</p>
        </div>
      ) : null}
    </section>
  );
}

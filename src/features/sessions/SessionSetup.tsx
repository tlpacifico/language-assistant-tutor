import { useEffect, useRef, useState } from "react";

import type { AppError } from "../../lib/contracts";
import { toAppError } from "../../lib/app-error";
import type { InterviewClient } from "../../lib/interview-client";
import {
  CATEGORY_LABELS,
  type Category,
  type InterviewSnapshot,
  type Question,
  type StartSession,
} from "../../lib/interview-contracts";
import { newRequestId } from "../../lib/request-id";

/** Sugestão inicial de perguntas por sessão. */
const DEFAULT_COUNT = 3;

function questionsLabel(total: number): string {
  return total === 1 ? "1 pergunta elegível" : `${total} perguntas elegíveis`;
}

/** Interpreta a quantidade como um número de dígitos; vazio ou decimal é inválido. */
function parseCount(value: string): number | null {
  const trimmed = value.trim();
  if (!/^\d+$/.test(trimmed)) {
    return null;
  }
  return Number(trimmed);
}

/**
 * Preparação de uma sessão guiada.
 *
 * A escolha começa em «Todas as categorias» e três perguntas. O total elegível
 * vem do banco da categoria escolhida; quando é inferior a três, a sugestão
 * mantém-se e o início fica bloqueado até o utilizador ajustar a quantidade —
 * o total nunca é reduzido nem há perguntas repetidas em silêncio. O núcleo
 * revalida a quantidade e sorteia as perguntas ao iniciar.
 *
 * Com uma sessão em curso, a preparação oferece retomá-la em vez de iniciar
 * outra. O `requestId` é conservado ao repetir um pedido falhado e substituído
 * quando a intenção muda.
 */
export default function SessionSetup({
  client,
  snapshot,
  onStarted,
}: {
  client: InterviewClient;
  snapshot: InterviewSnapshot;
  onStarted: (snapshot: InterviewSnapshot) => void;
}) {
  const [category, setCategory] = useState<Category | null>(null);
  const [countText, setCountText] = useState(String(DEFAULT_COUNT));
  const [questions, setQuestions] = useState<Question[] | null>(null);
  const [loadError, setLoadError] = useState<AppError | null>(null);
  const [loading, setLoading] = useState(true);
  const [reloadKey, setReloadKey] = useState(0);
  const [starting, setStarting] = useState(false);
  const [startError, setStartError] = useState<AppError | null>(null);
  const intention = useRef<{
    requestId: string;
    category: Category | null;
    count: number;
  } | null>(null);

  useEffect(() => {
    let active = true;
    setLoading(true);
    setLoadError(null);
    client
      .listQuestions(category)
      .then((rows) => {
        if (active) {
          setQuestions(rows);
          setLoading(false);
        }
      })
      .catch((error: unknown) => {
        if (active) {
          setQuestions(null);
          setLoadError(toAppError(error));
          setLoading(false);
        }
      });
    return () => {
      active = false;
    };
  }, [client, category, reloadKey]);

  const activeSession =
    snapshot.activeSession && snapshot.activeSession.status === "in_progress"
      ? snapshot.activeSession
      : null;

  const eligible = questions ? questions.length : 0;
  const count = parseCount(countText);
  const quantityError = (() => {
    if (questions === null) {
      return null;
    }
    if (eligible === 0) {
      return "Não existem perguntas elegíveis para esta escolha. Adicione perguntas ao banco antes de iniciar a entrevista.";
    }
    if (count === null) {
      return "Indique um número inteiro de perguntas.";
    }
    if (count < 1 || count > eligible) {
      return `Só existem ${eligible} perguntas elegíveis. Escolha uma quantidade entre 1 e ${eligible}.`;
    }
    return null;
  })();
  const suggestionTooHigh =
    questions !== null &&
    eligible > 0 &&
    eligible < DEFAULT_COUNT &&
    count !== null &&
    count > eligible;
  const canStart =
    !loading &&
    !starting &&
    questions !== null &&
    count !== null &&
    count >= 1 &&
    count <= eligible &&
    eligible > 0;

  async function start(): Promise<void> {
    if (!canStart || starting || count === null) {
      return;
    }
    if (
      !intention.current ||
      intention.current.category !== category ||
      intention.current.count !== count
    ) {
      intention.current = { requestId: newRequestId(), category, count };
    }
    const request: StartSession = {
      requestId: intention.current.requestId,
      category,
      count,
    };
    setStarting(true);
    setStartError(null);
    try {
      const next = await client.startSession(request);
      intention.current = null;
      onStarted(next);
    } catch (error) {
      // O mesmo pedido conserva o identificador numa nova tentativa.
      setStartError(toAppError(error));
    } finally {
      setStarting(false);
    }
  }

  return (
    <section className="practice__page" aria-labelledby="setup-title">
      <h2 id="setup-title">Preparar entrevista</h2>

      {activeSession ? (
        <section className="practice__panel" aria-labelledby="resume-title">
          <h3 id="resume-title">Retomar sessão</h3>
          <p className="practice__state">
            Sessão em curso: pergunta {activeSession.position} de{" "}
            {activeSession.count}.
          </p>
          <p>Respostas confirmadas: {activeSession.confirmedCount}.</p>
          <p className="practice__hint">
            Categoria:{" "}
            {activeSession.category
              ? CATEGORY_LABELS[activeSession.category]
              : "todas as categorias"}
            .
          </p>
          <div className="practice__actions">
            <button type="button" onClick={() => onStarted(snapshot)}>
              Retomar sessão
            </button>
          </div>
        </section>
      ) : loadError ? (
        <div className="practice__notice practice__notice--error" role="alert">
          <p>Não foi possível ler o banco de perguntas.</p>
          <p>{loadError.message}</p>
          <p className="practice__hint">Código: {loadError.code}.</p>
          <div className="practice__actions">
            <button
              type="button"
              onClick={() => setReloadKey((value) => value + 1)}
            >
              Tentar ler novamente
            </button>
          </div>
        </div>
      ) : (
        <form
          className="practice__form"
          onSubmit={(event) => {
            event.preventDefault();
            void start();
          }}
        >
          <div className="practice__field">
            <label htmlFor="setup-category">Categoria</label>
            <select
              id="setup-category"
              className="practice__select"
              value={category ?? ""}
              onChange={(event) => {
                const next = event.target.value;
                setCategory(next === "" ? null : (next as Category));
                setStartError(null);
              }}
              disabled={starting}
            >
              <option value="">Todas as categorias</option>
              {(Object.keys(CATEGORY_LABELS) as Category[]).map((value) => (
                <option key={value} value={value}>
                  {CATEGORY_LABELS[value]}
                </option>
              ))}
            </select>
          </div>

          <div className="practice__field">
            <label htmlFor="setup-count">Quantidade de perguntas</label>
            <input
              id="setup-count"
              className="practice__input"
              type="number"
              min={1}
              step={1}
              inputMode="numeric"
              value={countText}
              onChange={(event) => {
                setCountText(event.target.value);
                setStartError(null);
              }}
              disabled={starting || loading}
              aria-invalid={quantityError !== null}
              aria-describedby={[
                "setup-eligible",
                quantityError ? "setup-count-error" : null,
                suggestionTooHigh ? "setup-count-warning" : null,
              ]
                .filter(Boolean)
                .join(" ")}
            />
            <p className="practice__hint" id="setup-eligible">
              {loading
                ? "A carregar perguntas…"
                : `${questionsLabel(eligible)} para esta escolha.`}
            </p>
          </div>

          {quantityError ? (
            <p className="practice__error-text" id="setup-count-error">
              {quantityError}
            </p>
          ) : null}

          {suggestionTooHigh ? (
            <p className="practice__warning" id="setup-count-warning">
              Só existem {eligible} perguntas elegíveis. Ajuste a quantidade
              antes de iniciar a entrevista.
            </p>
          ) : null}

          <div className="practice__actions">
            <button type="submit" disabled={!canStart}>
              {starting ? "A iniciar entrevista…" : "Iniciar entrevista"}
            </button>
          </div>

          {startError ? (
            <div className="practice__notice practice__notice--error" role="alert">
              <p>{startError.message}</p>
              <p className="practice__hint">Código: {startError.code}.</p>
            </div>
          ) : null}
        </form>
      )}
    </section>
  );
}

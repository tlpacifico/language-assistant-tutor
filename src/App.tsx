import { useEffect, useState } from "react";

import type { AppError } from "./lib/contracts";
import type { InterviewClient } from "./lib/interview-client";
import { createInterviewClient } from "./lib/interview-client";
import type { InterviewSnapshot } from "./lib/interview-contracts";
import "./features/practice/practice.css";

/** Rótulos dos quatro componentes de disponibilidade. */
const COMPONENT_LABELS = {
  microphone: "Microfone",
  playback: "Reprodução",
  transcription: "Transcrição",
  synthesis: "Síntese",
} as const;

const STATUS_LABELS: Record<string, string> = {
  ready: "Disponível",
  missing: "Em falta",
  loading: "A carregar",
  error: "Com erro",
};

const DATABASE_LABELS: Record<InterviewSnapshot["database"], string> = {
  opening: "A abrir a base de dados…",
  ready: "Base de dados pronta",
  error: "A base de dados não abriu",
};

/**
 * Converter uma rejeição do cliente num erro estável para a interface.
 *
 * Os comandos devolvem sempre um `AppError`; uma falha inesperada de IPC é
 * apresentada como erro repetível, sem inventar códigos de domínio.
 */
function toAppError(error: unknown): AppError {
  if (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    "message" in error &&
    "retryable" in error
  ) {
    return error as AppError;
  }
  return {
    code: "UNKNOWN",
    message:
      "Não foi possível comunicar com o núcleo da aplicação. Tente novamente.",
    retryable: true,
  };
}

/**
 * Janela mínima da fase 2 enquanto as páginas são construídas.
 *
 * Mostra a disponibilidade, o estado da base de dados e a recuperação de uma
 * abertura falhada. O cliente é injetado para os testes usarem um adaptador
 * controlado, sem IPC Tauri nem base de dados reais.
 */
export default function App({
  client = createInterviewClient(),
}: {
  client?: InterviewClient;
}) {
  const [snapshot, setSnapshot] = useState<InterviewSnapshot | null>(null);
  const [loadError, setLoadError] = useState<AppError | null>(null);
  const [actionError, setActionError] = useState<AppError | null>(null);
  const [retrying, setRetrying] = useState(false);

  useEffect(() => {
    let active = true;
    const unsubscribe = client.subscribe((next) => {
      if (active) {
        setSnapshot(next);
      }
    });
    client
      .snapshot()
      .then((next) => {
        if (active) {
          setSnapshot(next);
          setLoadError(null);
        }
      })
      .catch((error: unknown) => {
        if (active) {
          setLoadError(toAppError(error));
        }
      });
    return () => {
      active = false;
      unsubscribe();
    };
  }, [client]);

  async function retryDatabase(): Promise<void> {
    setRetrying(true);
    setActionError(null);
    try {
      setSnapshot(await client.retryDatabase());
    } catch (error) {
      setActionError(toAppError(error));
    } finally {
      setRetrying(false);
    }
  }

  async function run(
    action: () => Promise<InterviewSnapshot>,
  ): Promise<void> {
    setActionError(null);
    try {
      setSnapshot(await action());
    } catch (error) {
      setActionError(toAppError(error));
    }
  }

  const database = snapshot?.database ?? "opening";
  const databaseMessage = snapshot?.lastError?.message ?? null;

  return (
    <main className="practice" aria-labelledby="app-title">
      <header className="practice__header">
        <p className="practice__eyebrow">Tutor de entrevistas</p>
        <h1 className="practice__question" id="app-title">
          A preparar as entrevistas guiadas…
        </h1>
        <p className="practice__hint">
          As páginas de preparação, entrevista e histórico estão a ser
          construídas.
        </p>
      </header>

      {loadError && !snapshot ? (
        <div className="practice__notice practice__notice--error" role="alert">
          <p>{loadError.message}</p>
        </div>
      ) : null}

      {snapshot ? (
        <>
          <section className="practice__panel" aria-labelledby="database-title">
            <h2 id="database-title">Base de dados</h2>
            <p
              className={`practice__status practice__status--${database}`}
              role="status"
            >
              {DATABASE_LABELS[database]}
            </p>
            {database === "error" ? (
              <>
                {databaseMessage ? <p>{databaseMessage}</p> : null}
                <div className="practice__actions">
                  <button
                    type="button"
                    onClick={() => void retryDatabase()}
                    disabled={retrying}
                  >
                    {retrying
                      ? "A tentar abrir novamente…"
                      : "Tentar abrir novamente"}
                  </button>
                </div>
              </>
            ) : null}
          </section>

          <section className="practice__panel" aria-labelledby="availability-title">
            <h2 id="availability-title">Disponibilidade</h2>
            <ul className="practice__availability">
              {(
                Object.keys(COMPONENT_LABELS) as Array<
                  keyof typeof COMPONENT_LABELS
                >
              ).map((key) => {
                const component = snapshot.availability[key];
                return (
                  <li key={key}>
                    <span className="practice__component">
                      {COMPONENT_LABELS[key]}
                    </span>
                    <span
                      className={`practice__status practice__status--${component.status}`}
                    >
                      {STATUS_LABELS[component.status] ?? component.status}
                    </span>
                    {component.message ? (
                      <span className="practice__hint">{component.message}</span>
                    ) : null}
                  </li>
                );
              })}
            </ul>
            <div className="practice__actions">
              <button
                type="button"
                onClick={() => void run(() => client.refreshAvailability())}
                disabled={retrying}
              >
                Verificar disponibilidade
              </button>
              <button
                type="button"
                onClick={() => void run(() => client.restartWorker())}
                disabled={retrying}
              >
                Reiniciar áudio local
              </button>
            </div>
          </section>
        </>
      ) : null}

      {actionError ? (
        <div className="practice__notice practice__notice--error" role="alert">
          <p>{actionError.message}</p>
          <p className="practice__hint">Código: {actionError.code}.</p>
        </div>
      ) : null}
    </main>
  );
}

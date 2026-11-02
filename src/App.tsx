import { useCallback, useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { toAppError } from "./lib/app-error";
import type { AppError } from "./lib/contracts";
import { createInterviewClient } from "./lib/interview-client";
import type { InterviewClient } from "./lib/interview-client";
import type { InterviewSnapshot } from "./lib/interview-contracts";
import Interview from "./features/interview/Interview";
import { useInterview } from "./features/interview/useInterview";
import NavigationGuard from "./features/navigation/NavigationGuard";
import type {
  GuardIntention,
  GuardRequest,
} from "./features/navigation/NavigationGuard";
import QuestionBank from "./features/questions/QuestionBank";
import SessionSetup from "./features/sessions/SessionSetup";
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

/** Páginas da aplicação, sem router nem gestor global de estado. */
type Page = "setup" | "bank" | "interview" | "history";

const PAGES: Array<{ id: Page; label: string }> = [
  { id: "setup", label: "Preparação" },
  { id: "bank", label: "Banco de perguntas" },
  { id: "interview", label: "Entrevista" },
  { id: "history", label: "Histórico" },
];

/** Motivo apresentado quando uma página é pedida durante captura/finalização. */
const NAVIGATION_BLOCKED_MESSAGE =
  "Não é possível mudar de página enquanto a gravação está a ser capturada ou finalizada.";

/**
 * Janela do sistema, injetável para os testes.
 *
 * O fecho é sempre intercetado: só depois de `prepareClose` e da decisão sobre
 * a revisão é que `destroy` fecha a janela.
 */
export interface AppWindow {
  onCloseRequested: (handler: () => void) => Promise<() => void>;
  destroy: () => Promise<void>;
}

/** Janela Tauri em execução; nula quando não existe (testes ou web). */
function tauriWindow(): AppWindow | null {
  try {
    const current = getCurrentWindow();
    return {
      onCloseRequested(handler) {
        return current.onCloseRequested((event) => {
          // O fecho imediato é sempre bloqueado; `destroy` fecha quando a
          // revisão e o `prepareClose` estiverem resolvidos.
          event.preventDefault();
          handler();
        });
      },
      destroy: () => current.destroy(),
    };
  } catch {
    return null;
  }
}

/** Página de histórico: o espaço reservado até a consulta estar disponível. */
function HistoryPage() {
  return (
    <section className="practice__page" aria-labelledby="history-title">
      <h2 id="history-title">Histórico — fase 2</h2>
      <p className="practice__hint">
        A lista de sessões, a retoma e a consulta dos treinos anteriores ficam
        disponíveis numa fase seguinte. Nada é apagado nem convertido.
      </p>
    </section>
  );
}

/**
 * Janela das entrevistas guiadas.
 *
 * Mantém uma única subscrição ao cliente e uma única decisão de
 * guardar/descartar/cancelar, usada pela navegação, pela captura de uma nova
 * tomada e pelo fecho. A identidade da sessão ativa vem sempre do núcleo.
 * Enquanto a base de dados não abre, as páginas que dependem dela não são
 * montadas e a repetição da abertura fica disponível.
 */
export default function App({
  client,
  windowHandle,
}: {
  client?: InterviewClient;
  windowHandle?: AppWindow;
}) {
  // O cliente por omissão é criado uma única vez: sem prop, cada render
  // recriaria o adaptador e a subscrição seria refeita a cada fotografia.
  const [fallbackClient] = useState(createInterviewClient);
  const activeClient = client ?? fallbackClient;
  const [fallbackWindow] = useState(() => windowHandle ?? tauriWindow());
  const activeWindow = windowHandle ?? fallbackWindow;
  const [snapshot, setSnapshot] = useState<InterviewSnapshot | null>(null);
  const [loadError, setLoadError] = useState<AppError | null>(null);
  const [actionError, setActionError] = useState<AppError | null>(null);
  const [retrying, setRetrying] = useState(false);
  const [page, setPage] = useState<Page>("setup");
  const [navigationBlocked, setNavigationBlocked] = useState(false);
  const [guardRequest, setGuardRequest] = useState<GuardRequest | null>(null);
  const [closing, setClosing] = useState(false);
  const closingRef = useRef(false);

  // Uma fotografia com revisão inferior à conhecida é ignorada; nenhuma
  // fotografia apaga o rascunho local (o hook trata da revisão).
  const applySnapshot = useCallback((next: InterviewSnapshot) => {
    setSnapshot((previous) =>
      previous && next.revision < previous.revision ? previous : next,
    );
  }, []);

  const review = useInterview(activeClient, snapshot, applySnapshot, {
    locked: closing,
  });
  const reviewRef = useRef(review);
  reviewRef.current = review;

  useEffect(() => {
    let active = true;
    const unsubscribe = activeClient.subscribe((next) => {
      if (active) {
        applySnapshot(next);
      }
    });
    activeClient
      .snapshot()
      .then((next) => {
        if (active) {
          applySnapshot(next);
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
  }, [activeClient, applySnapshot]);

  const busy =
    snapshot !== null &&
    (snapshot.recordingId !== null || snapshot.finalizing);

  useEffect(() => {
    if (!busy) {
      setNavigationBlocked(false);
    }
  }, [busy]);

  function requestDecision(
    intention: GuardIntention,
    proceed: () => void | Promise<void>,
  ): void {
    if (review.confirming) {
      setActionError({
        code: "BUSY",
        message:
          "A confirmação está a ser guardada. Aguarde que termine antes de continuar.",
        retryable: true,
      });
      return;
    }
    if (!review.dirty) {
      void proceed();
      return;
    }
    setGuardRequest({
      intention,
      proceed: async () => {
        setGuardRequest(null);
        await proceed();
      },
    });
  }

  function navigate(next: Page): void {
    if (next === page) {
      return;
    }
    if (busy) {
      // A captura e a finalização não podem ser interrompidas por navegação.
      setNavigationBlocked(true);
      return;
    }
    setNavigationBlocked(false);
    requestDecision("navigate", () => setPage(next));
  }

  async function closeNow(): Promise<void> {
    if (closingRef.current) {
      return;
    }
    closingRef.current = true;
    setClosing(true);
    setActionError(null);
    try {
      // Finaliza uma captura ativa e recusa o fecho com uma publicação
      // pendente: nada fecha em silêncio.
      applySnapshot(await activeClient.prepareClose());
      if (reviewRef.current.isDirty()) {
        // Uma edição que tenha escapado ao bloqueio do editor não pode ser
        // perdida em `destroy`: a janela fica e a decisão reaparece.
        closingRef.current = false;
        setClosing(false);
        setGuardRequest({
          intention: "close",
          proceed: async () => {
            setGuardRequest(null);
            await closeNow();
          },
        });
        return;
      }
      if (activeWindow) {
        await activeWindow.destroy();
      }
    } catch (error) {
      setActionError(toAppError(error));
      closingRef.current = false;
      setClosing(false);
    }
  }

  function requestClose(): void {
    if (closingRef.current) {
      return;
    }
    if (review.saving) {
      setActionError({
        code: "BUSY",
        message:
          "A revisão está a ser guardada. Tente fechar novamente quando a escrita terminar.",
        retryable: true,
      });
      return;
    }
    if (review.confirming) {
      setActionError({
        code: "BUSY",
        message:
          "A confirmação está a ser guardada. Tente fechar novamente quando terminar.",
        retryable: true,
      });
      return;
    }
    if (review.dirty) {
      setGuardRequest({
        intention: "close",
        proceed: async () => {
          setGuardRequest(null);
          await closeNow();
        },
      });
      return;
    }
    void closeNow();
  }

  const closeActionRef = useRef(requestClose);
  closeActionRef.current = requestClose;

  useEffect(() => {
    if (!activeWindow) {
      return;
    }
    let disposed = false;
    let unlisten: (() => void) | null = null;

    activeWindow
      .onCloseRequested(() => closeActionRef.current())
      .then((remove) => {
        if (disposed) {
          remove();
        } else {
          unlisten = remove;
        }
      })
      .catch(() => {
        // Sem janela do sistema (ambiente web ou testes) não há o que fechar.
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [activeWindow]);

  async function retryDatabase(): Promise<void> {
    setRetrying(true);
    setActionError(null);
    try {
      applySnapshot(await activeClient.retryDatabase());
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
      applySnapshot(await action());
    } catch (error) {
      setActionError(toAppError(error));
    }
  }

  const database = snapshot?.database ?? "opening";
  const databaseMessage = snapshot?.lastError?.message ?? null;

  let content: ReactNode;
  if (page === "history") {
    content = <HistoryPage />;
  } else if (!snapshot) {
    content = (
      <p className="practice__hint">A carregar o estado da aplicação…</p>
    );
  } else if (snapshot.database !== "ready") {
    content = (
      <section className="practice__panel" aria-labelledby="gate-title">
        <h2 id="gate-title">Base de dados indisponível</h2>
        <p className="practice__hint">
          A preparação, o banco de perguntas e a entrevista ficam disponíveis
          quando a base de dados abrir.
        </p>
      </section>
    );
  } else if (page === "bank") {
    content = <QuestionBank client={activeClient} />;
  } else if (page === "interview") {
    content = (
      <Interview
        client={activeClient}
        snapshot={snapshot}
        onSnapshot={applySnapshot}
        review={review}
        requestDecision={requestDecision}
        closing={closing}
      />
    );
  } else {
    content = (
      <SessionSetup
        client={activeClient}
        snapshot={snapshot}
        onStarted={(next) => {
          applySnapshot(next);
          setPage("interview");
        }}
      />
    );
  }

  return (
    <main className="practice" aria-labelledby="app-title">
      <header className="practice__header">
        <p className="practice__eyebrow">Entrevistas comportamentais em inglês</p>
        <h1 className="practice__question" id="app-title">
          Tutor de entrevistas
        </h1>
        <p className="practice__hint">
          Prepare a sessão, edite o banco de perguntas e consulte o histórico.
        </p>
      </header>

      <nav className="practice__nav" aria-label="Navegação principal">
        {PAGES.map((entry) => (
          <button
            key={entry.id}
            type="button"
            aria-current={page === entry.id ? "page" : undefined}
            onClick={() => navigate(entry.id)}
          >
            {entry.label}
          </button>
        ))}
      </nav>

      {navigationBlocked ? (
        <div className="practice__notice practice__notice--error" role="alert">
          <p>{NAVIGATION_BLOCKED_MESSAGE}</p>
        </div>
      ) : null}

      {loadError && !snapshot ? (
        <div className="practice__notice practice__notice--error" role="alert">
          <p>{loadError.message}</p>
        </div>
      ) : null}

      {closing ? (
        <div className="practice__notice" role="status">
          <p>A preparar o fecho da aplicação…</p>
        </div>
      ) : null}

      {content}

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
            const component = snapshot?.availability[key];
            return (
              <li key={key}>
                <span className="practice__component">
                  {COMPONENT_LABELS[key]}
                </span>
                {component ? (
                  <>
                    <span
                      className={`practice__status practice__status--${component.status}`}
                    >
                      {STATUS_LABELS[component.status] ?? component.status}
                    </span>
                    {component.message ? (
                      <span className="practice__hint">{component.message}</span>
                    ) : null}
                  </>
                ) : (
                  <span className="practice__hint">
                    Ainda sem informação do núcleo.
                  </span>
                )}
              </li>
            );
          })}
        </ul>
        <div className="practice__actions">
          <button
            type="button"
            onClick={() => void run(() => activeClient.refreshAvailability())}
            disabled={!snapshot || retrying}
          >
            Verificar disponibilidade
          </button>
          <button
            type="button"
            onClick={() => void run(() => activeClient.restartWorker())}
            disabled={!snapshot || retrying}
          >
            Reiniciar áudio local
          </button>
        </div>
      </section>

      <NavigationGuard
        request={guardRequest}
        busy={busy}
        dirty={review.dirty}
        saving={review.saving}
        saveError={review.error}
        onSave={review.saveReview}
        onDiscard={review.discardReview}
        onCancel={() => setGuardRequest(null)}
      />

      {actionError ? (
        <div className="practice__notice practice__notice--error" role="alert">
          <p>{actionError.message}</p>
          <p className="practice__hint">Código: {actionError.code}.</p>
        </div>
      ) : null}
    </main>
  );
}

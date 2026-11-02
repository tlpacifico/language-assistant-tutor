import { useEffect, useState } from "react";
import type { ReactNode } from "react";

import { toAppError } from "./lib/app-error";
import type { AppError } from "./lib/contracts";
import { createInterviewClient } from "./lib/interview-client";
import type { InterviewClient } from "./lib/interview-client";
import { CATEGORY_LABELS } from "./lib/interview-contracts";
import type { InterviewSnapshot } from "./lib/interview-contracts";
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
 * Página da entrevista enquanto a captura, revisão e confirmação são
 * construídas na tarefa seguinte. Lê apenas a fotografia do núcleo: não cria
 * sessões, gravações ou resultados.
 */
function InterviewPage({ snapshot }: { snapshot: InterviewSnapshot }) {
  if (!snapshot.activeSession) {
    return (
      <section className="practice__page" aria-labelledby="interview-title">
        <h2 id="interview-title">Entrevista</h2>
        <p>
          Não existe uma sessão em curso. Comece ou retome uma entrevista na
          preparação.
        </p>
      </section>
    );
  }

  const { position, count, category } = snapshot.activeSession;
  return (
    <section className="practice__page" aria-labelledby="interview-title">
      <h2 id="interview-title">Entrevista</h2>
      <p className="practice__state">
        Pergunta {position} de {count}.
      </p>
      <p className="practice__hint">
        Categoria: {category ? CATEGORY_LABELS[category] : "todas as categorias"}.
      </p>
      {snapshot.current ? (
        <p className="practice__question" lang="en">
          {snapshot.current.question.text}
        </p>
      ) : null}
      <p className="practice__hint">
        A captura e a revisão da resposta são apresentadas na próxima versão
        desta página.
      </p>
    </section>
  );
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
 * Mantém uma única subscrição ao cliente durante toda a navegação e guarda a
 * página atual em estado local. A identidade da sessão ativa vem sempre do
 * núcleo: a interface não escolhe nem inventa sessões. Enquanto a base de dados
 * não abre, as páginas que dependem dela não são montadas e a repetição da
 * abertura fica disponível. Durante a captura ou a finalização, a mudança de
 * página é recusada com o motivo visível; a decisão partilhada de guardar,
 * descartar ou cancelar pertence à proteção de alterações da tarefa seguinte.
 */
export default function App({
  client,
}: {
  client?: InterviewClient;
}) {
  // O cliente por omissão é criado uma única vez: sem prop, cada render
  // recriaria o adaptador e a subscrição seria refeita a cada fotografia.
  const [fallbackClient] = useState(createInterviewClient);
  const activeClient = client ?? fallbackClient;
  const [snapshot, setSnapshot] = useState<InterviewSnapshot | null>(null);
  const [loadError, setLoadError] = useState<AppError | null>(null);
  const [actionError, setActionError] = useState<AppError | null>(null);
  const [retrying, setRetrying] = useState(false);
  const [page, setPage] = useState<Page>("setup");
  const [navigationBlocked, setNavigationBlocked] = useState(false);

  useEffect(() => {
    let active = true;
    const unsubscribe = activeClient.subscribe((next) => {
      if (active) {
        setSnapshot(next);
      }
    });
    activeClient
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
  }, [activeClient]);

  const busy =
    snapshot !== null &&
    (snapshot.recordingId !== null || snapshot.finalizing);

  useEffect(() => {
    if (!busy) {
      setNavigationBlocked(false);
    }
  }, [busy]);

  function navigate(next: Page): void {
    if (next === page) {
      return;
    }
    if (busy) {
      setNavigationBlocked(true);
      return;
    }
    setNavigationBlocked(false);
    setPage(next);
  }

  async function retryDatabase(): Promise<void> {
    setRetrying(true);
    setActionError(null);
    try {
      setSnapshot(await activeClient.retryDatabase());
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
    content = <InterviewPage snapshot={snapshot} />;
  } else {
    content = (
      <SessionSetup
        client={activeClient}
        snapshot={snapshot}
        onStarted={(next) => {
          setSnapshot(next);
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

      {actionError ? (
        <div className="practice__notice practice__notice--error" role="alert">
          <p>{actionError.message}</p>
          <p className="practice__hint">Código: {actionError.code}.</p>
        </div>
      ) : null}
    </main>
  );
}

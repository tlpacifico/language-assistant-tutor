import AiSettings from "./features/ai/AiSettings";
import DiagnosticsPanel from "./features/diagnostics/DiagnosticsPanel";
import { useCallback, useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { toAppError } from "./lib/app-error";
import type { AppError } from "./lib/contracts";
import { createInterviewClient } from "./lib/interview-client";
import type { InterviewClient } from "./lib/interview-client";
import type { InterviewSnapshot } from "./lib/interview-contracts";
import History from "./features/history/History";
import Interview from "./features/interview/Interview";
import { useInterview } from "./features/interview/useInterview";
import Articles from "./features/articles/Articles";
import { createArticleClient, type ArticleClient } from "./lib/article-client";
import NavigationGuard from "./features/navigation/NavigationGuard";
import type {
  GuardIntention,
  GuardRequest,
  GuardSource,
} from "./features/navigation/NavigationGuard";
import Practice from "./features/practice/Practice";
import QuestionBank from "./features/questions/QuestionBank";
import StoryBank from "./features/stories/StoryBank";
import { PendingPreparationNotice } from "./features/stories/StorySupport";
import SessionSetup from "./features/sessions/SessionSetup";
import "./features/layout/styles.css";
import "./features/layout/fonts.css";
import TutorNavigation, {
  PAGE_COPY,
  type Page,
} from "./features/layout/TutorNavigation";

import EnvironmentIcon, {
  ENVIRONMENT_HINTS,
} from "./features/layout/EnvironmentIcon";

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

/** Motivo apresentado quando uma página é pedida durante captura/finalização. */
const NAVIGATION_BLOCKED_MESSAGE =
  "Não é possível mudar de página enquanto a gravação está a começar, a ser capturada ou a finalizar.";

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
  articleClient,
  windowHandle,
}: {
  client?: InterviewClient;
  articleClient?: ArticleClient;
  windowHandle?: AppWindow;
}) {
  // O cliente por omissão é criado uma única vez: sem prop, cada render
  // recriaria o adaptador e a subscrição seria refeita a cada fotografia.
  const [fallbackClient] = useState(createInterviewClient);
  const activeClient = client ?? fallbackClient;
  const [fallbackArticleClient] = useState(createArticleClient);
  const activeArticleClient = articleClient ?? fallbackArticleClient;
  // Gravação ou transcrição de um artigo: a navegação fica bloqueada.
  const [articleBusy, setArticleBusy] = useState(false);
  const [fallbackWindow] = useState(() => windowHandle ?? tauriWindow());
  const activeWindow = windowHandle ?? fallbackWindow;
  const [snapshot, setSnapshot] = useState<InterviewSnapshot | null>(null);
  const [loadError, setLoadError] = useState<AppError | null>(null);
  const [actionError, setActionError] = useState<AppError | null>(null);
  const [retrying, setRetrying] = useState(false);
  const [page, setPage] = useState<Page>("setup");
  const mainRef = useRef<HTMLElement>(null);
  const previousPage = useRef(page);
  useEffect(() => {
    if (previousPage.current !== page) {
      previousPage.current = page;
      mainRef.current?.focus();
    }
  }, [page]);
  const [practiceId, setPracticeId] = useState<string | null>(null);
  const [navigationBlocked, setNavigationBlocked] = useState(false);
  const [guardRequest, setGuardRequest] = useState<GuardRequest | null>(null);
  const [bankGuard, setBankGuard] = useState<GuardSource | null>(null);
  const bankGuardRef = useRef<GuardSource | null>(null);
  const [recordPending, setRecordPending] = useState(false);
  const [closing, setClosing] = useState(false);
  const closingRef = useRef(false);

  /** Registra o estado do editor do banco na decisão única desta janela. */
  const reportBankGuard = useCallback((guard: GuardSource | null) => {
    bankGuardRef.current = guard;
    setBankGuard(guard);
  }, []);

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

  // Durante o pedido de início a fotografia ainda não tem `recordingId`:
  // `recordPending` cobre essa janela e mantém a navegação bloqueada.
  const busy =
    recordPending ||
    (snapshot !== null &&
      (snapshot.recordingId !== null || snapshot.finalizing));

  useEffect(() => {
    if (!busy) {
      setNavigationBlocked(false);
    }
  }, [busy]);

  // A mesma decisão cobre todos os editores com alterações por guardar: a
  // revisão da resposta e o rascunho do banco de perguntas.
  const guardSources: GuardSource[] = [
    {
      key: "review",
      label: "revisão da resposta",
      dirty: review.dirty,
      saving: review.saving,
      error: review.error,
      save: review.saveReview,
      discard: review.discardReview,
    },
  ];
  if (bankGuard) {
    guardSources.push(bankGuard);
  }
  const dirty = guardSources.some((source) => source.dirty);

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
    const anyDirty = review.dirty || (bankGuardRef.current?.dirty ?? false);
    if (!anyDirty) {
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
    // A entrevista é sempre um destino seguro: é onde vivem a captura e o
    // botão de terminar. Sem esta exceção, ficar noutra página durante uma
    // captura tornaria o botão inacessível.
    // Com uma captura de prática, a página da prática é o destino seguro.
    const safeHome: Page =
      snapshot?.current || snapshot?.activeSession
        ? "interview"
        : snapshot?.practice?.activePractice
          ? "practice"
          : "interview";
    if (articleBusy && next !== "articles") {
      setNavigationBlocked(true);
      return;
    }
    if (busy && next !== safeHome) {
      // A captura e a finalização não podem ser interrompidas por navegação.
      setNavigationBlocked(true);
      return;
    }
    setNavigationBlocked(false);
    requestDecision("navigate", () => setPage(next));
  }

  /**
   * Abre a entrevista na etapa persistida da sessão escolhida no histórico.
   *
   * O histórico já releu o núcleo e confirmou que a sessão continua em curso;
   * aqui só se muda de página, sem avançar nem confirmar nenhuma pergunta.
   */
  function resumeSession(sessionId: string): void {
    if (busy) {
      setNavigationBlocked(true);
      return;
    }
    if (snapshot?.activeSession?.id !== sessionId) {
      setActionError({
        code: "STALE_SOURCE",
        message:
          "A sessão indicada já não está em curso. Atualize o histórico antes de retomar.",
        retryable: false,
      });
      return;
    }
    setNavigationBlocked(false);
    requestDecision("navigate", () => setPage("interview"));
  }

  /**
   * Abre uma prática (nova, pendente ou já feita). Pode ser pedida com a
   * revisão por guardar: a decisão partilhada resolve-a antes de mudar.
   */
  function openPractice(id: string): void {
    if (busy) {
      setNavigationBlocked(true);
      return;
    }
    setNavigationBlocked(false);
    requestDecision("navigate", () => {
      setPracticeId(id);
      setPage("practice");
    });
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
      // Lido no momento do fecho: uma edição feita durante a escrita conta pela
      // versão mais recente do rascunho, não pela fotografia anterior.
      const stillDirty =
        reviewRef.current.isDirty() || (bankGuardRef.current?.dirty ?? false);
      if (stillDirty) {
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
    if (recordPending) {
      setActionError({
        code: "BUSY",
        message:
          "A gravação está a começar. Tente fechar novamente quando a captura estiver em curso ou tiver terminado.",
        retryable: true,
      });
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
    if (bankGuard?.saving) {
      setActionError({
        code: "BUSY",
        message:
          "A alteração do banco está a ser guardada. Tente fechar novamente quando a escrita terminar.",
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
    if (dirty) {
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

  async function run(action: () => Promise<InterviewSnapshot>): Promise<void> {
    setActionError(null);
    try {
      applySnapshot(await action());
    } catch (error) {
      setActionError(toAppError(error));
    }
  }

  const database = snapshot?.database ?? "opening";
  const databaseMessage = snapshot?.lastError?.message ?? null;
  const pendingRecordingId = snapshot?.pendingRecordingId ?? null;
  const pendingPracticeId = snapshot?.practice?.activePractice?.id ?? null;

  let content: ReactNode;
  if (page === "ai") {
    content = <AiSettings client={activeClient} />;
  } else if (page === "history") {
    content = (
      <History
        client={activeClient}
        onResume={resumeSession}
        snapshot={snapshot}
        onSnapshot={applySnapshot}
        onOpenPractice={openPractice}
      />
    );
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
  } else if (page === "articles") {
    content = (
      <Articles client={activeArticleClient} onBusyChange={setArticleBusy} />
    );
  } else if (page === "bank") {
    content = (
      <QuestionBank
        client={activeClient}
        reportGuard={reportBankGuard}
        onReplaceDraft={(proceed) => requestDecision("switch", proceed)}
      />
    );
  } else if (page === "stories") {
    content = (
      <StoryBank
        client={activeClient}
        reportGuard={reportBankGuard}
        onReplaceDraft={(proceed) => requestDecision("switch", proceed)}
      />
    );
  } else if (page === "practice" && practiceId !== null) {
    content = (
      <Practice
        client={activeClient}
        snapshot={snapshot}
        onSnapshot={applySnapshot}
        review={review}
        practiceId={practiceId}
        requestDecision={requestDecision}
        closing={closing}
        onRecordingPending={setRecordPending}
        onBack={() => requestDecision("navigate", () => setPage("history"))}
        onOpenPractice={(id) => {
          setPracticeId(id);
          setPage("practice");
        }}
      />
    );
  } else if (page === "interview") {
    content = (
      <Interview
        client={activeClient}
        snapshot={snapshot}
        onSnapshot={applySnapshot}
        review={review}
        requestDecision={requestDecision}
        closing={closing}
        onRecordingPending={setRecordPending}
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
    <div className="practice tutor-app">
      <a className="tutor-skip" href="#main-content">
        Saltar para o conteúdo
      </a>
      <TutorNavigation
        page={page}
        session={snapshot?.activeSession ?? null}
        onNavigate={navigate}
      />
      <main
        className={`tutor-main tutor-main--${page}`}
        ref={mainRef}
        id="main-content"
        aria-labelledby="app-title"
        tabIndex={-1}
      >
        <div className="tutor-content">
          <header className="tutor-page-head">
            <p className="tutor-eyebrow">{PAGE_COPY[page].label}</p>
            <h1 id="app-title">{PAGE_COPY[page].title}</h1>
            <p>{PAGE_COPY[page].description}</p>
          </header>

          <DiagnosticsPanel />
          {page !== "setup" && (
            <details className="tutor-audio-recovery">
              <summary>Áudio e recuperação</summary>
              <p className="practice__hint">
                Verifique os dispositivos ou reinicie o áudio local sem sair
                desta página. O núcleo preserva a gravação em curso e recusa o
                reinício do auxiliar durante transcrição ou síntese.
              </p>
              <div className="practice__actions">
                <button
                  type="button"
                  disabled={!snapshot || retrying || closing}
                  onClick={() =>
                    void run(() => activeClient.refreshAvailability())
                  }
                >
                  Verificar disponibilidade
                </button>
                <button
                  type="button"
                  disabled={!snapshot || retrying || closing}
                  onClick={() => void run(() => activeClient.restartWorker())}
                >
                  Reiniciar áudio local
                </button>
              </div>
            </details>
          )}

          {navigationBlocked ? (
            <div
              className="practice__notice practice__notice--error"
              role="alert"
            >
              <p>{NAVIGATION_BLOCKED_MESSAGE}</p>
            </div>
          ) : null}

          {loadError && !snapshot ? (
            <div
              className="practice__notice practice__notice--error"
              role="alert"
            >
              <p>{loadError.message}</p>
            </div>
          ) : null}

          {closing ? (
            <div className="practice__notice" role="status">
              <p>A preparar o fecho da aplicação…</p>
            </div>
          ) : null}

          {snapshot ? (
            <PendingPreparationNotice
              client={activeClient}
              snapshot={snapshot}
              onSnapshot={applySnapshot}
            />
          ) : null}

          {pendingPracticeId && page !== "practice" ? (
            <div className="practice__notice" role="status">
              <p>
                Existe uma prática pendente. Retome-a, confirme-a ou abandone-a
                antes de começar uma entrevista ou outra prática.
              </p>
              <div className="practice__actions">
                <button
                  type="button"
                  onClick={() => openPractice(pendingPracticeId)}
                >
                  Retomar prática pendente
                </button>
              </div>
            </div>
          ) : null}

          {pendingRecordingId ? (
            <div
              className="practice__notice practice__notice--error"
              role="alert"
            >
              <p>
                A gravação terminada ainda não foi guardada. Repita o guardar
                antes de gravar outra resposta ou fechar a aplicação.
              </p>
              <div className="practice__actions">
                <button
                  type="button"
                  onClick={() =>
                    void run(() =>
                      activeClient.retrySaveRecording(pendingRecordingId),
                    )
                  }
                  disabled={retrying}
                >
                  Repetir guardar gravação
                </button>
              </div>
            </div>
          ) : null}

          {content}

          {page === "setup" && (
            <div className="tutor-setup-grid">
              <section
                className="practice__panel tutor-database"
                aria-labelledby="database-title"
              >
                <h2 id="database-title">Base de dados</h2>
                <p
                  className={`practice__status practice__status--${database}`}
                  role="status"
                >
                  {DATABASE_LABELS[database]}
                </p>
                <p className="practice__hint">
                  As sessões, histórias e gravações ficam neste computador.
                </p>
                <div className="practice__actions">
                  <button type="button" onClick={() => navigate("bank")}>
                    Abrir banco
                  </button>
                  <button type="button" onClick={() => navigate("stories")}>
                    Ver histórias
                  </button>
                </div>
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

              <section
                className="practice__panel tutor-environment"
                aria-labelledby="availability-title"
              >
                <h2 id="availability-title">Ambiente</h2>
                <p className="practice__hint">
                  Verifique o microfone e o áudio antes de gravar a primeira
                  resposta.
                </p>
                <ul className="practice__availability">
                  {(
                    Object.keys(COMPONENT_LABELS) as Array<
                      keyof typeof COMPONENT_LABELS
                    >
                  ).map((key) => {
                    const component = snapshot?.availability[key];
                    return (
                      <li key={key}>
                        <EnvironmentIcon name={key} />
                        <div className="tutor-env-copy">
                          <span className="practice__component">
                            {COMPONENT_LABELS[key]}
                          </span>
                          <span className="practice__hint">
                            {ENVIRONMENT_HINTS[key]}
                          </span>
                        </div>
                        {component ? (
                          <>
                            <span
                              className={`practice__status practice__status--${component.status}`}
                            >
                              {STATUS_LABELS[component.status] ??
                                component.status}
                            </span>
                            {component.message ? (
                              <span className="practice__hint">
                                {component.message}
                              </span>
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
                    onClick={() =>
                      void run(() => activeClient.refreshAvailability())
                    }
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

              <section
                className="practice__panel tutor-quick-guide"
                aria-labelledby="guide-title"
              >
                <h2 id="guide-title">Roteiro rápido</h2>
                <ol className="tutor-steps">
                  <li>
                    <strong>Prepare as suas histórias</strong>
                    <span>
                      Recorde o contexto, a sua responsabilidade e o que fez.
                    </span>
                  </li>
                  <li>
                    <strong>Ouça e responda em inglês</strong>
                    <span>
                      Comece e termine a gravação quando estiver preparado.
                    </span>
                  </li>
                  <li>
                    <strong>Reveja e pratique de novo</strong>
                    <span>
                      Confirme o texto e melhore sem perder o original.
                    </span>
                  </li>
                </ol>
              </section>
            </div>
          )}
          {page !== "setup" && database === "error" && (
            <div
              className="practice__notice practice__notice--error"
              role="alert"
            >
              <p>{DATABASE_LABELS[database]}</p>
              <p>{databaseMessage}</p>
              <button
                type="button"
                onClick={() => void retryDatabase()}
                disabled={retrying}
              >
                Tentar abrir novamente
              </button>
            </div>
          )}

          <NavigationGuard
            request={guardRequest}
            busy={busy}
            sources={guardSources}
            onCancel={() => setGuardRequest(null)}
          />

          {actionError ? (
            <div
              className="practice__notice practice__notice--error"
              role="alert"
            >
              <p>{actionError.message}</p>
              <p className="practice__hint">Código: {actionError.code}.</p>
            </div>
          ) : null}
        </div>
      </main>
    </div>
  );
}

import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import App from "./App";
import type { AppWindow } from "./App";
import type { InterviewClient } from "./lib/interview-client";
import type {
  AdvanceSession,
  Category,
  ConfirmAnswer,
  Confirmation,
  InterviewSnapshot,
  Question,
  QuestionDetail,
  RecordingView,
  SaveReview,
  SessionDetail,
  SessionQuestion,
  SessionStatus,
  SessionSummary,
  StartSession,
} from "./lib/interview-contracts";
import {
  appError,
  fakeInterviewClient,
  makeQuestion,
  makeQuestionDetail,
  makeRecording,
  makeSessionQuestion,
  makeTranscript,
  readyAvailability,
  sessionSummary,
  snapshot,
} from "./test/fake-interview-client";

const QUESTIONS = [
  makeQuestion({ id: "q-1" }),
  makeQuestion({
    id: "q-8",
    category: "failure_learning",
    text: "Tell me about a time you made a mistake at work.",
  }),
];

function renderApp(initial = snapshot(), questions = QUESTIONS) {
  const fake = fakeInterviewClient({ initial, questions });
  render(<App client={fake.client} />);
  return fake;
}

/** Janela controlada: guarda o handler de fecho e a ordem dos pedidos. */
function fakeWindow(order: string[] = []) {
  let closeHandler: (() => void) | null = null;
  const destroy = vi.fn(async () => {
    order.push("destroy");
  });
  const windowHandle: AppWindow = {
    onCloseRequested: vi.fn(async (handler: () => void) => {
      closeHandler = handler;
      return () => {
        closeHandler = null;
      };
    }),
    destroy,
  };
  return {
    windowHandle,
    destroy,
    requestClose() {
      closeHandler?.();
    },
  };
}

/** Sessão em curso com uma resposta transcrita e o editor disponível. */
function interviewInitial(): InterviewSnapshot {
  return snapshot({
    revision: 2,
    activeSession: sessionSummary({ version: 5 }),
    current: makeQuestionDetail({
      question: makeSessionQuestion({
        id: "sq-2",
        selectedRecordingId: "recording-1",
      }),
      recordings: [
        makeRecording({
          id: "recording-1",
          sessionQuestionId: "sq-2",
          transcripts: [
            makeTranscript({ id: "transcript-1", text: "My stored answer." }),
          ],
        }),
      ],
    }),
  });
}

/** Fotografia devolvida pelo núcleo depois de guardar a revisão. */
function reviewedSnapshot(
  base: InterviewSnapshot,
  text: string,
  version = 1,
): InterviewSnapshot {
  const current = base.current;
  if (!current) {
    throw new Error("A fotografia base não tem pergunta corrente.");
  }
  return snapshot({
    ...base,
    revision: base.revision + 1,
    current: {
      ...current,
      recordings: current.recordings.map((recording) =>
        recording.id === current.question.selectedRecordingId
          ? {
              ...recording,
              review: {
                recordingId: recording.id,
                sourceTranscriptId: "transcript-1",
                origin: "automatic" as const,
                text,
                version,
                savedAt: "2026-11-02T10:20:00Z",
              },
            }
          : recording,
      ),
    },
  });
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

/** Textos fixos das três perguntas do percurso integrado. */
const CONTROLLED_TEXTS = [
  "Tell me about a time you solved a difficult problem at work.",
  "Tell me about a time you made a mistake at work.",
  "Tell me about a time you led an initiative.",
];

/** Pergunta de uma sessão controlada, com os textos que lhe pertencem. */
interface ControlledQuestion {
  id: string;
  sourceId: string;
  text: string;
  category: Category;
  position: number;
  selectionVersion: number;
  ordinal: number;
  recording: RecordingView | null;
  confirmation: Confirmation | null;
}

interface ControlledSession {
  id: string;
  category: Category | null;
  questions: ControlledQuestion[];
  position: number;
  version: number;
  status: SessionStatus;
  createdAt: string;
  completedAt: string | null;
}

/**
 * Cliente controlado para o percurso integrado.
 *
 * Tem a mesma superfície de `InterviewClient` e mantém uma única sessão em
 * memória, como o núcleo: a confirmação guarda a versão do texto, o avanço é
 * explícito e a sessão concluída deixa de estar ativa. Não existe chamada de
 * IA nem dispensa de seguimento — a transcrição automática é simulada na
 * publicação da gravação e `transcribeResponse` nunca é usado pela interface.
 */
function controlledInterviewClient() {
  const bank: Question[] = [
    makeQuestion({ id: "q-1", text: CONTROLLED_TEXTS[0] }),
    makeQuestion({
      id: "q-2",
      category: "failure_learning",
      text: CONTROLLED_TEXTS[1],
    }),
    makeQuestion({
      id: "q-3",
      category: "leadership_initiative",
      text: CONTROLLED_TEXTS[2],
    }),
  ];
  let revision = 0;
  let session: ControlledSession | null = null;
  let recordingId: string | null = null;
  let listener: ((next: InterviewSnapshot) => void) | null = null;
  let transcripts = 0;

  function find(questionId: string, current: ControlledSession): ControlledQuestion {
    const question = current.questions.find((entry) => entry.id === questionId);
    if (!question) {
      throw appError(
        "STALE_SOURCE",
        "A pergunta indicada não pertence à sessão.",
        false,
      );
    }
    return question;
  }

  function questionView(question: ControlledQuestion): SessionQuestion {
    return makeSessionQuestion({
      id: question.id,
      sessionId: "session-1",
      sourceQuestionId: question.sourceId,
      position: question.position,
      text: question.text,
      category: question.category,
      selectedRecordingId: question.recording?.id ?? null,
      selectionVersion: question.selectionVersion,
    });
  }

  function questionDetail(question: ControlledQuestion): QuestionDetail {
    return {
      question: questionView(question),
      recordings: question.recording ? [question.recording] : [],
      confirmation: question.confirmation,
    };
  }

  function summary(): SessionSummary | null {
    if (!session) {
      return null;
    }
    return sessionSummary({
      id: session.id,
      category: session.category,
      count: session.questions.length,
      confirmedCount: session.questions.filter(
        (entry) => entry.confirmation !== null,
      ).length,
      position: session.position,
      status: session.status,
      version: session.version,
      createdAt: session.createdAt,
      completedAt: session.completedAt,
    });
  }

  function detail(): SessionDetail {
    if (!session) {
      throw appError("DATABASE_UNAVAILABLE", "Ainda não existe sessão.", false);
    }
    return {
      summary: summary() as SessionSummary,
      questions: session.questions.map(questionDetail),
    };
  }

  function state(): InterviewSnapshot {
    const active = session && session.status === "in_progress" ? summary() : null;
    const current =
      session && session.status === "in_progress"
        ? session.questions.find((entry) => entry.position === session?.position)
        : undefined;
    return snapshot({
      revision,
      activeSession: active,
      current: current ? questionDetail(current) : null,
      recordingId,
    });
  }

  function publish(): InterviewSnapshot {
    revision += 1;
    const next = state();
    listener?.(next);
    return next;
  }

  /** Guarda a revisão pedida e devolve a versão resultante. */
  function storeReview(question: ControlledQuestion, request: SaveReview): number {
    if (!question.recording) {
      throw appError(
        "ANSWER_REQUIRED",
        "Esta pergunta não tem uma gravação guardada.",
        false,
      );
    }
    const version = request.expectedReviewVersion + 1;
    question.recording = {
      ...question.recording,
      review: {
        recordingId: question.recording.id,
        sourceTranscriptId: request.sourceTranscriptId,
        origin: request.origin,
        text: request.text,
        version,
        savedAt: "2026-11-02T10:05:00.000Z",
      },
    };
    return version;
  }

  const transcribeResponse = vi.fn(async () => state());

  const client: InterviewClient = {
    snapshot: vi.fn(async () => state()),
    listQuestions: vi.fn(async (category) =>
      bank
        .filter((question) => category == null || question.category === category)
        .map((question) => ({ ...question })),
    ),
    createQuestion: vi.fn(async () => {
      throw appError("INVALID_INPUT", "O banco não é editado neste teste.", false);
    }),
    updateQuestion: vi.fn(async () => {
      throw appError("INVALID_INPUT", "O banco não é editado neste teste.", false);
    }),
    startSession: vi.fn(async (request: StartSession) => {
      session = {
        id: "session-1",
        category: request.category,
        questions: bank.slice(0, request.count).map((question, index) => ({
          id: `sq-${index + 1}`,
          sourceId: question.id,
          text: question.text,
          category: question.category,
          position: index + 1,
          selectionVersion: 0,
          ordinal: 0,
          recording: null,
          confirmation: null,
        })),
        position: 1,
        version: 1,
        status: "in_progress",
        createdAt: "2026-11-02T10:00:00.000Z",
        completedAt: null,
      };
      return publish();
    }),
    listSessions: vi.fn(async () => {
      const current = summary();
      return current ? [current] : [];
    }),
    loadSession: vi.fn(async (sessionId: string) => {
      if (!session || session.id !== sessionId) {
        throw appError("INVALID_INPUT", "Sessão desconhecida.", false);
      }
      return detail();
    }),
    listLegacy: vi.fn(async () => []),
    loadLegacy: vi.fn(async () => {
      throw appError("INVALID_INPUT", "Sem treinos anteriores.", false);
    }),
    startRecording: vi.fn(async (sessionQuestionId: string) => {
      if (!session) {
        throw appError("STALE_SOURCE", "Sem sessão em curso.", false);
      }
      const question = find(sessionQuestionId, session);
      recordingId = `recording-${question.position}-${question.ordinal + 1}`;
      return publish();
    }),
    stopRecording: vi.fn(async () => {
      if (!session || recordingId === null) {
        throw appError("CAPTURE_INTERRUPTED", "Sem gravação em curso.", false);
      }
      const current = session;
      const question = current.questions.find(
        (entry) => entry.position === current.position,
      );
      if (!question) {
        throw appError("STALE_SOURCE", "Sem pergunta corrente.", false);
      }
      transcripts += 1;
      question.ordinal += 1;
      question.selectionVersion += 1;
      question.recording = makeRecording({
        id: recordingId,
        sessionQuestionId: question.id,
        ordinal: question.ordinal,
        transcripts: [
          makeTranscript({
            id: `transcript-${transcripts}`,
            text: `Automatic answer for question ${question.position}`,
          }),
        ],
        originalFile: `sessions/session-1/${question.id}/${recordingId}/original.wav`,
      });
      recordingId = null;
      return publish();
    }),
    retrySaveRecording: vi.fn(async () => state()),
    transcribeResponse,
    saveReview: vi.fn(async (request: SaveReview) => {
      if (!session) {
        throw appError("STALE_SOURCE", "Sem sessão em curso.", false);
      }
      storeReview(find(request.target.sessionQuestionId, session), request);
      return publish();
    }),
    confirmAnswer: vi.fn(async (request: ConfirmAnswer) => {
      if (!session) {
        throw appError("STALE_SOURCE", "Sem sessão em curso.", false);
      }
      const current = session;
      const question = find(request.target.sessionQuestionId, current);
      if (question.confirmation) {
        throw appError(
          "ANSWER_CONFIRMED",
          "A resposta já foi confirmada.",
          false,
        );
      }
      const version = storeReview(question, request);
      const recording = question.recording as RecordingView;
      question.confirmation = {
        id: `confirmation-${question.position}`,
        sessionQuestionId: question.id,
        recordingId: recording.id,
        reviewVersion: version,
        sourceTranscriptId: request.sourceTranscriptId,
        origin: request.origin,
        text: request.text,
        confirmedAt: "2026-11-02T10:10:00.000Z",
      };
      current.version += 1;
      return publish();
    }),
    advanceSession: vi.fn(async (request: AdvanceSession) => {
      if (!session) {
        throw appError("STALE_SOURCE", "Sem sessão em curso.", false);
      }
      const current = session;
      if (request.expectedSessionVersion !== current.version) {
        throw appError("STALE_SOURCE", "A sessão mudou entretanto.", false);
      }
      const question = find(request.sessionQuestionId, current);
      if (!question.confirmation) {
        throw appError(
          "ANSWER_REQUIRED",
          "Confirme a resposta antes de avançar.",
          false,
        );
      }
      if (question.position !== current.position) {
        throw appError("STALE_SOURCE", "A pergunta já não é a atual.", false);
      }
      if (current.position < current.questions.length) {
        current.position += 1;
        current.version += 1;
      } else {
        current.status = "completed";
        current.completedAt = "2026-11-02T10:30:00.000Z";
        current.version += 1;
      }
      return publish();
    }),
    playAudio: vi.fn(async () => state()),
    stopAudio: vi.fn(async () => state()),
    refreshAvailability: vi.fn(async () => state()),
    restartWorker: vi.fn(async () => state()),
    prepareClose: vi.fn(async () => state()),
    retryDatabase: vi.fn(async () => state()),
    subscribe: vi.fn((next: (value: InterviewSnapshot) => void) => {
      listener = next;
      return () => {
        listener = null;
      };
    }),
  };

  return {
    client,
    transcribeResponse,
    currentSession: () => session,
  };
}

describe("App", () => {
  it("mostra o estado de abertura, a disponibilidade e a preparação", async () => {
    renderApp(snapshot({ database: "opening" }));

    expect(
      screen.getByRole("heading", { name: "Tutor de entrevistas" }),
    ).toBeInTheDocument();
    expect(
      await screen.findByText("A abrir a base de dados…"),
    ).toBeInTheDocument();
    expect(screen.getByText("Microfone")).toBeInTheDocument();
    expect(screen.getByText("Síntese")).toBeInTheDocument();
    // Quatro componentes prontos.
    expect(screen.getAllByText("Disponível")).toHaveLength(4);
  });

  it("acompanha as fotografias publicadas pelo núcleo", async () => {
    const fake = renderApp(snapshot({ database: "opening" }));
    await screen.findByText("A abrir a base de dados…");

    act(() => {
      fake.publish(snapshot({ revision: 2, database: "ready" }));
    });

    expect(await screen.findByText("Base de dados pronta")).toBeInTheDocument();
    expect(
      await screen.findByRole("heading", { name: "Preparar entrevista" }),
    ).toBeInTheDocument();
  });

  it("permite repetir a abertura da base de dados", async () => {
    const user = userEvent.setup();
    const error = appError(
      "DATABASE_UNAVAILABLE",
      "Não foi possível abrir a base de dados.",
    );
    renderApp(snapshot({ database: "error", lastError: error }));

    expect(
      await screen.findByText("A base de dados não abriu"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Não foi possível abrir a base de dados."),
    ).toBeInTheDocument();

    await user.click(
      screen.getByRole("button", { name: "Tentar abrir novamente" }),
    );

    await waitFor(() =>
      expect(screen.getByText("Base de dados pronta")).toBeInTheDocument(),
    );
    expect(
      await screen.findByRole("heading", { name: "Preparar entrevista" }),
    ).toBeInTheDocument();
  });

  it("mostra uma falha de comunicação com o núcleo", async () => {
    let listener: ((snapshot: import("./lib/interview-contracts").InterviewSnapshot) => void) | null =
      null;
    const base = fakeInterviewClient({ questions: QUESTIONS });
    const client: InterviewClient = {
      ...base.client,
      snapshot: async () => {
        throw new Error("IPC indisponível");
      },
      subscribe: vi.fn((next) => {
        listener = next;
        return () => {
          listener = null;
        };
      }),
    };
    render(<App client={client} />);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Não foi possível comunicar com o núcleo da aplicação.",
    );
    expect(listener).not.toBeNull();
  });

  it("navega entre páginas e mantém uma única subscrição", async () => {
    const user = userEvent.setup();
    const fake = renderApp();
    expect(
      await screen.findByRole("heading", { name: "Preparar entrevista" }),
    ).toBeInTheDocument();

    await user.click(
      screen.getByRole("button", { name: "Banco de perguntas" }),
    );
    expect(
      await screen.findByRole("heading", { name: "Banco de perguntas" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Banco de perguntas" }),
    ).toHaveAttribute("aria-current", "page");

    // A fotografia continua a chegar com a página do banco montada.
    act(() => {
      fake.publish(
        snapshot({
          revision: 4,
          availability: {
            ...readyAvailability(),
            synthesis: { status: "ready", message: "Voz local pronta" },
          },
        }),
      );
    });
    expect(await screen.findByText("Voz local pronta")).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "Banco de perguntas" }),
    ).toBeInTheDocument();
    expect(fake.client.subscribe).toHaveBeenCalledTimes(1);

    await user.click(screen.getByRole("button", { name: "Entrevista" }));
    expect(
      await screen.findByText(
        "Não existe uma sessão em curso. Comece ou retome uma entrevista na preparação.",
      ),
    ).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Histórico" }));
    expect(
      await screen.findByRole("heading", { name: "Histórico" }),
    ).toBeInTheDocument();
    expect(fake.client.subscribe).toHaveBeenCalledTimes(1);
  });

  it("bloqueia a mudança de página durante a captura", async () => {
    const user = userEvent.setup();
    renderApp(
      snapshot({ recordingId: "recording-1", revision: 1 }),
      QUESTIONS,
    );
    expect(
      await screen.findByRole("heading", { name: "Preparar entrevista" }),
    ).toBeInTheDocument();

    await user.click(
      screen.getByRole("button", { name: "Banco de perguntas" }),
    );

    expect(
      screen.getByRole("alert"),
    ).toHaveTextContent(
      "Não é possível mudar de página enquanto a gravação está a ser capturada ou finalizada.",
    );
    expect(
      screen.getByRole("heading", { name: "Preparar entrevista" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Preparação" }),
    ).toHaveAttribute("aria-current", "page");
  });

  it("bloqueia a mudança de página durante a finalização", async () => {
    const user = userEvent.setup();
    renderApp(snapshot({ finalizing: true, revision: 1 }), QUESTIONS);
    await screen.findByRole("heading", { name: "Preparar entrevista" });

    await user.click(screen.getByRole("button", { name: "Histórico" }));

    expect(screen.getByRole("alert")).toHaveTextContent(
      "Não é possível mudar de página",
    );
    expect(
      screen.queryByRole("heading", { name: "Histórico" }),
    ).not.toBeInTheDocument();
  });

  it("resolve a revisão por guardar antes de fechar a janela", async () => {
    const user = userEvent.setup();
    const initial = interviewInitial();
    const order: string[] = [];
    const controller = fakeWindow(order);
    const fake = fakeInterviewClient({ initial, questions: QUESTIONS });
    vi.mocked(fake.client.prepareClose).mockImplementation(async () => {
      order.push("prepareClose");
      return initial;
    });
    render(
      <App client={fake.client} windowHandle={controller.windowHandle} />,
    );

    await user.click(screen.getByRole("button", { name: "Entrevista" }));
    const editor = await screen.findByLabelText("Revisão editável");
    await user.clear(editor);
    await user.type(editor, "Text still unsaved.");

    await act(async () => {
      controller.requestClose();
    });

    expect(screen.getByText("Revisão por guardar")).toBeInTheDocument();
    expect(controller.destroy).not.toHaveBeenCalled();
    expect(order).toEqual([]);

    await user.click(
      screen.getByRole("button", { name: "Descartar e fechar" }),
    );

    await waitFor(() => expect(controller.destroy).toHaveBeenCalledTimes(1));
    expect(order).toEqual(["prepareClose", "destroy"]);
    expect(fake.client.subscribe).toHaveBeenCalledTimes(1);
  });

  it("finaliza a captura antes de destruir a janela", async () => {
    const order: string[] = [];
    const controller = fakeWindow(order);
    const fake = fakeInterviewClient({
      initial: snapshot({ revision: 1, recordingId: "recording-1" }),
      questions: QUESTIONS,
    });
    vi.mocked(fake.client.prepareClose).mockImplementation(async () => {
      order.push("prepareClose");
      return snapshot({ revision: 2, recordingId: null });
    });
    render(
      <App client={fake.client} windowHandle={controller.windowHandle} />,
    );

    await act(async () => {
      controller.requestClose();
    });

    await waitFor(() => expect(controller.destroy).toHaveBeenCalledTimes(1));
    expect(order).toEqual(["prepareClose", "destroy"]);
  });

  it("mantém a janela aberta quando o fecho falha", async () => {
    const controller = fakeWindow();
    const fake = fakeInterviewClient({
      initial: snapshot({ revision: 1 }),
      questions: QUESTIONS,
    });
    vi.mocked(fake.client.prepareClose).mockRejectedValue(
      appError(
        "STORAGE_WRITE_FAILED",
        "Há uma gravação por guardar. Repita o guardar antes de fechar.",
      ),
    );
    render(
      <App client={fake.client} windowHandle={controller.windowHandle} />,
    );

    await act(async () => {
      controller.requestClose();
    });

    expect(
      await screen.findByText(
        "Há uma gravação por guardar. Repita o guardar antes de fechar.",
      ),
    ).toBeInTheDocument();
    expect(controller.destroy).not.toHaveBeenCalled();
  });

  it("não fecha com texto escrito durante a preparação do fecho", async () => {
    const user = userEvent.setup();
    const initial = interviewInitial();
    const order: string[] = [];
    const controller = fakeWindow(order);
    const fake = fakeInterviewClient({ initial, questions: QUESTIONS });
    const saved = deferred<InterviewSnapshot>();
    const closing = deferred<InterviewSnapshot>();
    let saves = 0;
    vi.mocked(fake.client.saveReview).mockImplementation((request) => {
      saves += 1;
      if (saves === 1) {
        return saved.promise;
      }
      return Promise.resolve(reviewedSnapshot(initial, request.text, 2));
    });
    vi.mocked(fake.client.prepareClose).mockImplementation(() => {
      order.push("prepareClose");
      return closing.promise;
    });
    render(
      <App client={fake.client} windowHandle={controller.windowHandle} />,
    );

    await user.click(screen.getByRole("button", { name: "Entrevista" }));
    const editor = await screen.findByLabelText("Revisão editável");
    await user.clear(editor);
    await user.type(editor, "Saved before closing.");

    await act(async () => {
      controller.requestClose();
    });
    await user.click(screen.getByRole("button", { name: "Guardar e fechar" }));
    expect(fake.client.saveReview).toHaveBeenCalledTimes(1);

    // A escrita termina e o fecho chega a `prepareClose`; a edição seguinte
    // acontece antes do render que bloqueia o editor, como numa corrida real.
    await act(async () => {
      saved.resolve(reviewedSnapshot(initial, "Saved before closing.", 1));
      await saved.promise;
      for (let hops = 0; hops < 12; hops += 1) {
        if (vi.mocked(fake.client.prepareClose).mock.calls.length > 0) {
          break;
        }
        await Promise.resolve();
      }
      fireEvent.change(editor, {
        target: { value: "Saved before closing. Typed while closing." },
      });
    });

    expect(fake.client.prepareClose).toHaveBeenCalledTimes(1);
    expect(editor).toHaveAttribute("readonly");
    expect(editor).toHaveValue("Saved before closing. Typed while closing.");

    await act(async () => {
      closing.resolve(snapshot({ ...initial, revision: 20 }));
      await closing.promise;
    });

    // A versão mais recente está por guardar: a janela mantém-se e a decisão
    // reaparece com o texto preservado.
    await waitFor(() =>
      expect(screen.getByText("Revisão por guardar")).toBeInTheDocument(),
    );
    expect(controller.destroy).not.toHaveBeenCalled();
    expect(screen.getByLabelText("Revisão editável")).toHaveValue(
      "Saved before closing. Typed while closing.",
    );

    // Guardar a versão nova liberta o fecho.
    await user.click(screen.getByRole("button", { name: "Guardar e fechar" }));
    await waitFor(() => expect(controller.destroy).toHaveBeenCalledTimes(1));
    expect(order).toEqual(["prepareClose", "prepareClose", "destroy"]);
  });

  it("three_question_session_can_resume_and_finish", async () => {
    const user = userEvent.setup();
    const controlled = controlledInterviewClient();
    const first = render(<App client={controlled.client} />);

    // Preparação: a sugestão inicial é de três perguntas e todas são elegíveis.
    expect(
      await screen.findByText("3 perguntas elegíveis para esta escolha."),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Iniciar entrevista" }));

    // Pergunta 1: gravar, confirmar e deixar o avanço por decidir.
    expect(await screen.findByText("Pergunta 1 de 3")).toBeInTheDocument();
    await user.click(
      await screen.findByRole("button", { name: "Iniciar gravação" }),
    );
    await user.click(
      await screen.findByRole("button", { name: "Terminar gravação" }),
    );
    await waitFor(() =>
      expect(screen.getByLabelText("Revisão editável")).toHaveValue(
        "Automatic answer for question 1",
      ),
    );
    await user.click(screen.getByRole("button", { name: "Confirmar resposta" }));
    await waitFor(() =>
      expect(screen.getByText(/Resposta confirmada em/)).toBeInTheDocument(),
    );
    // Confirmar não avança: a próxima pergunta continua por pedir.
    expect(controlled.client.advanceSession).not.toHaveBeenCalled();
    expect(screen.getByText("Pergunta 1 de 3")).toBeInTheDocument();

    // Reabertura: uma janela nova lê a etapa persistida do núcleo.
    first.unmount();
    render(<App client={controlled.client} />);
    expect(
      await screen.findByText("Sessão em curso: pergunta 1 de 3."),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Retomar sessão" }));

    // A resposta confirmada aguarda um avanço explícito: nada salta sozinho.
    expect(await screen.findByText("Pergunta 1 de 3")).toBeInTheDocument();
    expect(screen.queryByText("Pergunta 2 de 3")).toBeNull();
    expect(controlled.client.advanceSession).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Próxima pergunta" }));

    // Pergunta 2.
    expect(await screen.findByText("Pergunta 2 de 3")).toBeInTheDocument();
    await user.click(
      await screen.findByRole("button", { name: "Iniciar gravação" }),
    );
    await user.click(
      await screen.findByRole("button", { name: "Terminar gravação" }),
    );
    await waitFor(() =>
      expect(screen.getByLabelText("Revisão editável")).toHaveValue(
        "Automatic answer for question 2",
      ),
    );
    await user.click(screen.getByRole("button", { name: "Confirmar resposta" }));
    await user.click(
      await screen.findByRole("button", { name: "Próxima pergunta" }),
    );

    // Pergunta 3 e fim da sessão.
    expect(await screen.findByText("Pergunta 3 de 3")).toBeInTheDocument();
    await user.click(
      await screen.findByRole("button", { name: "Iniciar gravação" }),
    );
    await user.click(
      await screen.findByRole("button", { name: "Terminar gravação" }),
    );
    await waitFor(() =>
      expect(screen.getByLabelText("Revisão editável")).toHaveValue(
        "Automatic answer for question 3",
      ),
    );
    await user.click(screen.getByRole("button", { name: "Confirmar resposta" }));
    await user.click(
      await screen.findByRole("button", { name: "Terminar sessão" }),
    );

    expect(
      await screen.findByRole("heading", { name: "Sessão concluída" }),
    ).toBeInTheDocument();
    expect(controlled.currentSession()?.status).toBe("completed");

    // O histórico lê as três confirmações, cada uma na sua área.
    await user.click(screen.getByRole("button", { name: "Histórico" }));
    await user.click(
      await screen.findByRole("button", { name: "Abrir sessão" }),
    );
    for (const position of [1, 2, 3]) {
      const confirmed = await screen.findByRole("region", {
        name: `Resposta confirmada da pergunta ${position} de 3`,
      });
      expect(
        within(confirmed).getByText(
          `Automatic answer for question ${position}`,
        ),
      ).toBeInTheDocument();
    }
    expect(
      screen.getByText("Respostas confirmadas: 3 de 3."),
    ).toBeInTheDocument();
    expect(screen.queryByText(/feedback pendente/i)).toBeNull();

    // Nenhum pedido de IA e nenhuma dispensa de seguimento existe no cliente.
    expect(controlled.transcribeResponse).not.toHaveBeenCalled();
    expect(controlled.client).not.toHaveProperty("dismissFollowUp");
    expect(controlled.client).not.toHaveProperty("requestFeedback");
  });
});

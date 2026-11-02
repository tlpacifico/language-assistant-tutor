import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import App from "./App";
import type { AppWindow } from "./App";
import type { InterviewClient } from "./lib/interview-client";
import type { InterviewSnapshot } from "./lib/interview-contracts";
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
      await screen.findByRole("heading", { name: "Histórico — fase 2" }),
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
      screen.queryByRole("heading", { name: "Histórico — fase 2" }),
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
});

import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import App from "./App";
import type { InterviewClient } from "./lib/interview-client";
import type { InterviewSnapshot } from "./lib/interview-contracts";

function readyAvailability(): InterviewSnapshot["availability"] {
  const ready = { status: "ready" as const, message: null };
  return {
    microphone: ready,
    playback: ready,
    transcription: ready,
    synthesis: ready,
  };
}

function snapshot(
  overrides: Partial<InterviewSnapshot> = {},
): InterviewSnapshot {
  return {
    revision: 0,
    database: "opening",
    activeSession: null,
    current: null,
    availability: readyAvailability(),
    recordingId: null,
    finalizing: false,
    modelJob: null,
    playback: null,
    warnings: [],
    lastError: null,
    ...overrides,
  };
}

/**
 * Cliente controlado: a janela mínima só precisa de ler a fotografia, receber
 * publicações e repetir a abertura da base. Os restantes comandos existem para
 * respeitar o contrato e falham se forem chamados sem preparação.
 */
function fakeClient(initial: InterviewSnapshot) {
  let listener: ((snapshot: InterviewSnapshot) => void) | null = null;
  let current = initial;
  const client: InterviewClient = {
    snapshot: async () => current,
    listQuestions: async () => [],
    createQuestion: async () => {
      throw new Error("criar pergunta não é usado nesta janela");
    },
    updateQuestion: async () => {
      throw new Error("editar pergunta não é usado nesta janela");
    },
    startSession: async () => current,
    listSessions: async () => [],
    loadSession: async () => {
      throw new Error("abrir sessão não é usado nesta janela");
    },
    startRecording: async () => current,
    stopRecording: async () => current,
    retrySaveRecording: async () => current,
    transcribeResponse: async () => current,
    saveReview: async () => current,
    confirmAnswer: async () => current,
    advanceSession: async () => current,
    playAudio: async () => current,
    stopAudio: async () => current,
    refreshAvailability: async () => current,
    restartWorker: async () => current,
    prepareClose: async () => current,
    retryDatabase: async () => {
      current = snapshot({ database: "ready", revision: current.revision + 1 });
      return current;
    },
    subscribe(next) {
      listener = next;
      return () => {
        listener = null;
      };
    },
  };
  return {
    client,
    publish(next: InterviewSnapshot) {
      current = next;
      listener?.(next);
    },
  };
}

describe("App", () => {
  it("mostra o estado de abertura e a disponibilidade", async () => {
    const { client } = fakeClient(snapshot());
    render(<App client={client} />);

    expect(
      screen.getByRole("heading", {
        name: "A preparar as entrevistas guiadas…",
      }),
    ).toBeInTheDocument();
    expect(await screen.findByText("A abrir a base de dados…")).toBeInTheDocument();
    expect(screen.getByText("Microfone")).toBeInTheDocument();
    expect(screen.getByText("Síntese")).toBeInTheDocument();
    // Quatro componentes prontos.
    expect(screen.getAllByText("Disponível")).toHaveLength(4);
  });

  it("acompanha as fotografias publicadas pelo núcleo", async () => {
    const control = fakeClient(snapshot());
    render(<App client={control.client} />);
    await screen.findByText("A abrir a base de dados…");

    act(() => {
      control.publish(snapshot({ revision: 2, database: "ready" }));
    });

    expect(await screen.findByText("Base de dados pronta")).toBeInTheDocument();
  });

  it("permite repetir a abertura da base de dados", async () => {
    const error = {
      code: "DATABASE_UNAVAILABLE",
      message: "Não foi possível abrir a base de dados.",
      retryable: true,
    };
    const control = fakeClient(
      snapshot({ database: "error", lastError: error }),
    );
    render(<App client={control.client} />);

    expect(await screen.findByText("A base de dados não abriu")).toBeInTheDocument();
    expect(
      screen.getByText("Não foi possível abrir a base de dados."),
    ).toBeInTheDocument();

    await userEvent.click(
      screen.getByRole("button", { name: "Tentar abrir novamente" }),
    );

    await waitFor(() =>
      expect(screen.getByText("Base de dados pronta")).toBeInTheDocument(),
    );
  });

  it("mostra uma falha de comunicação com o núcleo", async () => {
    let listener: ((snapshot: InterviewSnapshot) => void) | null = null;
    const client: InterviewClient = {
      ...fakeClient(snapshot()).client,
      snapshot: async () => {
        throw new Error("IPC indisponível");
      },
      subscribe(next) {
        listener = next;
        return () => {
          listener = null;
        };
      },
    };
    render(<App client={client} />);

    expect(
      await screen.findByRole("alert"),
    ).toHaveTextContent("Não foi possível comunicar com o núcleo da aplicação.");
    expect(listener).not.toBeNull();
  });
});

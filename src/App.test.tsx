import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import App from "./App";
import type { InterviewClient } from "./lib/interview-client";
import {
  appError,
  fakeInterviewClient,
  makeQuestion,
  readyAvailability,
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
});

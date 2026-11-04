import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { InterviewSnapshot } from "../../lib/interview-contracts";
import {
  appError,
  fakeInterviewClient,
  makeQuestion,
  sessionSummary,
  snapshot,
} from "../../test/fake-interview-client";

import SessionSetup from "./SessionSetup";

function renderSetup(options: Parameters<typeof fakeInterviewClient>[0] = {}) {
  const fake = fakeInterviewClient(options);
  const onStarted = vi.fn();
  render(
    <SessionSetup
      client={fake.client}
      snapshot={options.initial ?? snapshot()}
      onStarted={onStarted}
    />,
  );
  return { ...fake, onStarted };
}

function failureQuestions() {
  return [
    makeQuestion({
      id: "q-8",
      category: "failure_learning",
      text: "Tell me about a time you made a mistake at work.",
    }),
    makeQuestion({
      id: "q-9",
      category: "failure_learning",
      text: "Describe a time you received feedback that was difficult to hear.",
    }),
  ];
}

function problemQuestions() {
  return [
    makeQuestion({ id: "q-11", category: "problem_solving" }),
    makeQuestion({
      id: "q-12",
      category: "problem_solving",
      text: "Describe a time you investigated a problem with incomplete information.",
    }),
    makeQuestion({
      id: "q-13",
      category: "problem_solving",
      text: "Tell me about a time you had to choose between different solutions.",
    }),
  ];
}

describe("SessionSetup", () => {
  it("defaults_to_three_and_all_categories", async () => {
    const fake = renderSetup({
      questions: [...failureQuestions(), ...problemQuestions()],
    });

    expect(screen.getByLabelText("Categoria")).toHaveValue("");
    expect(
      (screen.getByLabelText("Categoria") as HTMLSelectElement)
        .selectedOptions[0],
    ).toHaveTextContent("Todas as categorias");
    expect(screen.getByLabelText("Quantidade de perguntas")).toHaveValue(3);
    expect(
      await screen.findByText("5 perguntas elegíveis para esta escolha."),
    ).toBeInTheDocument();

    const start = screen.getByRole("button", { name: "Iniciar entrevista" });
    expect(start).toBeEnabled();
    expect(fake.client.listQuestions).toHaveBeenCalledWith(null);

    await userEvent.click(start);
    await waitFor(() => expect(fake.onStarted).toHaveBeenCalledTimes(1));
    expect(fake.starts).toHaveLength(1);
    expect(fake.starts[0]).toMatchObject({ category: null, count: 3 });
    expect(fake.starts[0].requestId.length).toBeGreaterThan(0);
  });

  it("insufficient_questions_require_explicit_adjustment", async () => {
    const user = userEvent.setup();
    const fake = renderSetup({
      questions: [
        ...failureQuestions(),
        makeQuestion({
          id: "q-18",
          category: "change_adaptation",
          text: "Tell me about a time your priorities changed unexpectedly.",
        }),
      ],
    });

    expect(
      await screen.findByText("3 perguntas elegíveis para esta escolha."),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Iniciar entrevista" }),
    ).toBeEnabled();

    await user.selectOptions(
      screen.getByLabelText("Categoria"),
      "failure_learning",
    );

    expect(
      await screen.findByText("2 perguntas elegíveis para esta escolha."),
    ).toBeInTheDocument();
    expect(fake.client.listQuestions).toHaveBeenCalledWith("failure_learning");
    // A sugestão continua em três: nunca é reduzida em silêncio.
    expect(screen.getByLabelText("Quantidade de perguntas")).toHaveValue(3);
    expect(
      screen.getByText(
        "Só existem 2 perguntas elegíveis. Ajuste a quantidade antes de iniciar a entrevista.",
      ),
    ).toBeInTheDocument();

    const start = screen.getByRole("button", { name: "Iniciar entrevista" });
    expect(start).toBeDisabled();
    expect(fake.client.startSession).not.toHaveBeenCalled();

    const count = screen.getByLabelText("Quantidade de perguntas");
    await user.clear(count);
    await user.type(count, "2");

    expect(
      screen.queryByText(
        "Só existem 2 perguntas elegíveis. Ajuste a quantidade antes de iniciar a entrevista.",
      ),
    ).not.toBeInTheDocument();
    expect(start).toBeEnabled();

    await user.click(start);
    await waitFor(() => expect(fake.onStarted).toHaveBeenCalledTimes(1));
    expect(fake.starts[0]).toMatchObject({
      category: "failure_learning",
      count: 2,
    });
  });

  it("active_session_offers_resume", async () => {
    const user = userEvent.setup();
    const initial = snapshot({
      activeSession: sessionSummary({
        category: "problem_solving",
        count: 3,
        confirmedCount: 1,
        position: 2,
      }),
    });
    const fake = renderSetup({
      initial,
      questions: [...failureQuestions(), ...problemQuestions()],
    });

    expect(
      screen.queryByRole("button", { name: "Iniciar entrevista" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByText("Sessão em curso: pergunta 2 de 3."),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("progressbar", { name: "Progresso da sessão" }),
    ).toHaveAttribute("value", "1");
    expect(
      screen.getByRole("progressbar", { name: "Progresso da sessão" }),
    ).toHaveAttribute("max", "3");
    expect(screen.getByText(/Resolução de problemas/)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Retomar sessão" }));

    expect(fake.onStarted).toHaveBeenCalledWith(initial);
    expect(fake.client.startSession).not.toHaveBeenCalled();
  });

  it("keeps_the_request_id_when_retrying_a_failed_start", async () => {
    const user = userEvent.setup();
    const fake = renderSetup({ questions: problemQuestions() });
    await screen.findByText("3 perguntas elegíveis para esta escolha.");

    const failure = appError("BUSY", "O núcleo está ocupado. Tente novamente.");
    const startSession = vi
      .fn()
      .mockRejectedValueOnce(failure)
      .mockImplementation(async () => snapshot());
    fake.client.startSession = startSession;

    const start = screen.getByRole("button", { name: "Iniciar entrevista" });
    await user.click(start);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "O núcleo está ocupado. Tente novamente.",
    );

    await user.click(start);
    await waitFor(() => expect(startSession).toHaveBeenCalledTimes(2));
    const first = startSession.mock.calls[0][0];
    const second = startSession.mock.calls[1][0];
    expect(second.requestId).toBe(first.requestId);

    // Uma intenção nova (outra quantidade) recebe outro identificador.
    const count = screen.getByLabelText("Quantidade de perguntas");
    await user.clear(count);
    await user.type(count, "2");
    await user.click(start);
    await waitFor(() => expect(startSession).toHaveBeenCalledTimes(3));
    const third = startSession.mock.calls[2][0];
    expect(third.requestId).not.toBe(first.requestId);
  });

  it("blocks_repeated_submissions_while_starting", async () => {
    const user = userEvent.setup();
    const fake = renderSetup({ questions: problemQuestions() });
    await screen.findByText("3 perguntas elegíveis para esta escolha.");

    let finish: (value: InterviewSnapshot) => void = () => {};
    fake.client.startSession = vi.fn(
      () =>
        new Promise<InterviewSnapshot>((resolve) => {
          finish = resolve;
        }),
    );

    const start = screen.getByRole("button", { name: "Iniciar entrevista" });
    await user.click(start);
    const pending = screen.getByRole("button", {
      name: "A iniciar entrevista…",
    });
    expect(pending).toBeDisabled();
    await user.click(pending);
    expect(fake.client.startSession).toHaveBeenCalledTimes(1);

    finish(snapshot({ activeSession: sessionSummary() }));
    await waitFor(() => expect(fake.onStarted).toHaveBeenCalledTimes(1));
  });

  it("reports_a_failed_bank_read_with_a_retry", async () => {
    const user = userEvent.setup();
    const fake = fakeInterviewClient({ questions: problemQuestions() });
    fake.client.listQuestions = vi
      .fn()
      .mockRejectedValueOnce(
        appError(
          "DATABASE_UNAVAILABLE",
          "Não foi possível abrir a base de dados.",
        ),
      )
      .mockResolvedValue(problemQuestions());
    render(
      <SessionSetup
        client={fake.client}
        snapshot={snapshot()}
        onStarted={vi.fn()}
      />,
    );

    expect(
      await screen.findByText("Não foi possível abrir a base de dados."),
    ).toBeInTheDocument();

    await user.click(
      screen.getByRole("button", { name: "Tentar ler novamente" }),
    );
    expect(
      await screen.findByText("3 perguntas elegíveis para esta escolha."),
    ).toBeInTheDocument();
  });
});

import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { FeedbackView } from "../../lib/ai-contracts";
import {
  fakeInterviewClient,
  makeConfirmation,
  makeQuestionDetail,
  sessionSummary,
  snapshot,
} from "../../test/fake-interview-client";
import QuestionCoaching from "./QuestionCoaching";

function state() {
  return snapshot({
    activeSession: sessionSummary({ mode: "ai_v3" }),
    current: makeQuestionDetail({
      confirmation: makeConfirmation(),
      followUpStage: "waived",
    }),
  });
}
function feedback(ready = true): FeedbackView {
  const s = state();
  return {
    sessionId: s.activeSession!.id,
    state: ready ? "partial" : "pending",
    reports: ready
      ? [
          {
            questionId: s.current!.question.id,
            position: 1,
            questionText: "Tell me about a migration.",
            sources: [
              {
                confirmationId: "confirmation-1",
                role: "initial",
                text: "I had plan a migration.",
              },
            ],
            report: {
              schemaVersion: "1",
              content: [],
              strengths: [],
              gaps: [],
              priorities: [],
              fewerPrioritiesReason: null,
              english: {
                necessaryCorrections: [
                  {
                    id: "fix-1",
                    explanation: "Use o passado simples.",
                    replacement: "I planned a migration.",
                    evidence: {
                      type: "quote",
                      confirmationId: "confirmation-1",
                      role: "initial",
                      quote: "I had plan a migration.",
                    },
                  },
                ],
                optionalStyle: [],
              },
            },
          },
        ]
      : [],
    priorities: [],
    attempts: [],
    usage: {
      knownReportedSubtotal: 0,
      knownEstimatedSubtotal: 0,
      unknownAttemptCount: 0,
      input: null,
      output: null,
      complete: false,
    },
  };
}
function setup(ready = false) {
  const f = fakeInterviewClient({ initial: state() });
  vi.mocked(f.client.loadFeedback).mockResolvedValue(feedback(ready));
  vi.mocked(f.client.evaluateQuestion).mockImplementation(async () => {
    vi.mocked(f.client.loadFeedback).mockResolvedValue(feedback());
    return feedback();
  });
  return f;
}

describe("Avaliação por pergunta", () => {
  it("avalia e reformula apenas por clique, apresentando correção e versão separadas", async () => {
    const f = setup();
    render(
      <QuestionCoaching
        client={f.client}
        snapshot={state()}
        onSnapshot={vi.fn()}
      />,
    );
    const button = await screen.findByRole("button", {
      name: "Avaliar e sugerir melhorias",
    });
    expect(f.client.evaluateQuestion).not.toHaveBeenCalled();
    expect(f.client.requestRewrite).not.toHaveBeenCalled();
    fireEvent.click(button);
    expect(await screen.findByText("Use o passado simples.")).toBeVisible();
    expect(await screen.findByText("Suggested B1 sentence one.")).toBeVisible();
    expect(
      screen.getByText("I had plan a migration.", { selector: "blockquote" }),
    ).toBeVisible();
    expect(f.client.evaluateQuestion).toHaveBeenCalledWith(
      expect.objectContaining({
        sessionId: state().activeSession!.id,
        sessionQuestionId: state().current!.question.id,
      }),
    );
    expect(f.client.requestRewrite).toHaveBeenCalledTimes(1);
    expect(f.client.generateFeedback).not.toHaveBeenCalled();
  });

  it("retoma avaliação guardada sem novo envio e repete só reformulação falhada", async () => {
    const f = setup(true);
    vi.mocked(f.client.requestRewrite).mockRejectedValueOnce({
      code: "AI_NETWORK",
      message: "Rede indisponível",
      retryable: true,
    });
    render(
      <QuestionCoaching
        client={f.client}
        snapshot={state()}
        onSnapshot={vi.fn()}
      />,
    );
    expect(await screen.findByText("Use o passado simples.")).toBeVisible();
    expect(f.client.requestRewrite).not.toHaveBeenCalled();
    fireEvent.click(
      screen.getByRole("button", { name: "Sugerir versão melhorada" }),
    );
    expect(await screen.findByText("Rede indisponível")).toBeVisible();
    expect(screen.getByText("Use o passado simples.")).toBeVisible();
    fireEvent.click(
      screen.getByRole("button", { name: "Sugerir versão melhorada" }),
    );
    expect(await screen.findByText("Suggested B1 sentence one.")).toBeVisible();
    expect(f.client.evaluateQuestion).not.toHaveBeenCalled();
  });

  it("não inicia reformulação atrasada depois de sair da pergunta", async () => {
    const f = setup();
    let finish!: (v: FeedbackView) => void;
    vi.mocked(f.client.evaluateQuestion).mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    const view = render(
      <QuestionCoaching
        client={f.client}
        snapshot={state()}
        onSnapshot={vi.fn()}
      />,
    );
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Avaliar e sugerir melhorias",
      }),
    );
    await waitFor(() =>
      expect(f.client.evaluateQuestion).toHaveBeenCalledTimes(1),
    );
    view.unmount();
    await act(async () => finish(feedback()));
    expect(f.client.requestRewrite).not.toHaveBeenCalled();
  });

  it("uma avaliação falhada não inicia reformulação e mostra consumo parcial", async () => {
    const f = setup();
    vi.mocked(f.client.evaluateQuestion).mockRejectedValue({
      code: "AI_NETWORK",
      message: "Avaliação indisponível",
      retryable: true,
    });
    const failed = feedback(false);
    failed.state = "failed";
    failed.usage.unknownAttemptCount = 1;
    vi.mocked(f.client.loadFeedback).mockResolvedValue(failed);
    render(
      <QuestionCoaching
        client={f.client}
        snapshot={state()}
        onSnapshot={vi.fn()}
      />,
    );
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Avaliar e sugerir melhorias",
      }),
    );
    expect(await screen.findByText("Avaliação indisponível")).toBeVisible();
    expect(f.client.requestRewrite).not.toHaveBeenCalled();
    fireEvent.click(
      screen.getByText("Consumo da entrevista (sem reformulações)"),
    );
    expect(
      screen.getByText(/Chamadas com custo desconhecido: 1/),
    ).toBeVisible();
    expect(screen.getByText(/Valores parciais/)).toBeVisible();
  });

  it("não oferece avaliação enquanto o seguimento estiver pendente", async () => {
    const f = setup();
    const s = state();
    s.current!.followUpStage = "not_requested";
    render(
      <QuestionCoaching client={f.client} snapshot={s} onSnapshot={vi.fn()} />,
    );
    expect(
      screen.queryByRole("button", { name: "Avaliar e sugerir melhorias" }),
    ).toBeNull();
    expect(f.client.evaluateQuestion).not.toHaveBeenCalled();
  });
});

import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import type {
  ComparisonEntry,
  ComparisonRequest,
  ComparisonView,
} from "../../lib/practice-contracts";
import {
  fakeInterviewClient,
  makeConfirmation,
  makePreparation,
  makeRecording,
  makeRewrite,
  makeTranscript,
} from "../../test/fake-interview-client";
import Comparison from "./Comparison";

const REQUEST: ComparisonRequest = {
  left: { type: "original", sessionId: "session-1", sessionQuestionId: "sq-1" },
  right: { type: "practice", practiceId: "practice-1" },
};

const QUESTION = {
  text: "Tell me about a time you solved a difficult problem at work.",
  category: "problem_solving" as const,
  competencies: ["Resolução de problemas"],
  criteria: ["Explicar o problema."],
};

function original(overrides: Partial<ComparisonEntry> = {}): ComparisonEntry {
  return {
    origin: REQUEST.left,
    createdAt: "2026-11-02T10:00:00Z",
    confirmedAt: "2026-11-02T10:10:00Z",
    question: QUESTION,
    answers: [
      {
        role: "initial",
        recordings: [
          makeRecording({
            id: "rec-initial",
            transcripts: [makeTranscript({ text: "Raw initial automatic." })],
          }),
        ],
        confirmation: makeConfirmation({ text: "Initial confirmed text." }),
      },
      {
        role: "follow_up",
        recordings: [
          makeRecording({
            id: "rec-follow",
            transcripts: [makeTranscript({ id: "t2", text: "Raw follow automatic." })],
          }),
        ],
        confirmation: makeConfirmation({ id: "c2", text: "Follow-up confirmed text." }),
      },
    ],
    report: null,
    reportState: "not_available",
    rewrites: [],
    preparation: makePreparation(),
    ...overrides,
  };
}

function attempt(overrides: Partial<ComparisonEntry> = {}): ComparisonEntry {
  return {
    origin: REQUEST.right,
    createdAt: "2026-11-03T10:00:00Z",
    confirmedAt: "2026-11-03T10:20:00Z",
    question: QUESTION,
    answers: [
      {
        role: "attempt",
        recordings: [
          makeRecording({
            id: "rec-attempt",
            transcripts: [makeTranscript({ id: "t3", text: "Raw attempt automatic." })],
          }),
        ],
        confirmation: makeConfirmation({ id: "c3", text: "Attempt confirmed text." }),
      },
    ],
    report: null,
    reportState: "pending",
    rewrites: [],
    preparation: makePreparation(),
    ...overrides,
  };
}

function view(overrides: Partial<ComparisonView> = {}): ComparisonView {
  return {
    left: original(),
    right: attempt(),
    rubricCompatibility: "unknown",
    ...overrides,
  };
}

function renderComparison(comparison: ComparisonView) {
  const fake = fakeInterviewClient();
  fake.setComparison(comparison);
  render(<Comparison client={fake.client} request={REQUEST} />);
  return fake;
}

describe("Comparison", () => {
  it("separa inicial, seguimento e tentativa e distingue automático de confirmado", async () => {
    renderComparison(view());

    const left = await screen.findByRole("article", { name: /Resposta original/ });
    const right = screen.getByRole("article", { name: /Prática/ });
    expect(within(left).getByText("Resposta inicial")).toBeInTheDocument();
    expect(within(left).getByText("Seguimento")).toBeInTheDocument();
    expect(within(left).getByText("Raw initial automatic.")).toBeInTheDocument();
    expect(within(left).getByText("Initial confirmed text.")).toBeInTheDocument();
    expect(within(left).getByText("Follow-up confirmed text.")).toBeInTheDocument();
    expect(within(left).getAllByText("Texto confirmado")).toHaveLength(2);
    expect(within(left).getAllByText(/Transcrição automática \(tomada/)).toHaveLength(2);
    expect(within(right).getByText("Tentativa de prática")).toBeInTheDocument();
    expect(within(right).getByText("Attempt confirmed text.")).toBeInTheDocument();
    expect(within(right).queryByText("Seguimento")).toBeNull();
  });

  it("não cria texto de melhoria nem pede avaliação, e ouve originais", async () => {
    const user = userEvent.setup();
    const fake = renderComparison(view());

    await screen.findByRole("article", { name: /Prática/ });
    expect(screen.queryByText(/melhorou|melhoria comprovada|progresso/i)).toBeNull();
    await user.click(
      screen.getByRole("button", { name: /Ouvir original da tomada 1 \(Prática/ }),
    );

    expect(fake.client.playAudio).toHaveBeenCalledWith({
      type: "recording",
      recordingId: "rec-attempt",
    });
    expect(fake.client.compareResponses).toHaveBeenCalledTimes(1);
    expect(fake.client.compareResponses).toHaveBeenCalledWith(REQUEST);
    expect(fake.client.evaluatePractice).not.toHaveBeenCalled();
    expect(fake.client.requestRewrite).not.toHaveBeenCalled();
  });

  it("rubrica diferente ou relatório em falta não parece melhoria comprovada", async () => {
    renderComparison(view({ rubricCompatibility: "different" }));

    expect(
      await screen.findByText(/não indicam melhoria comprovada/),
    ).toBeInTheDocument();
    expect(screen.getByText(/Esta comparação não pede avaliação/)).toBeInTheDocument();
    expect(screen.getByText(/Relatório ainda não pedido/)).toBeInTheDocument();
  });

  it("mostra versões do relatório quando existe e rubrica igual", async () => {
    const report = {
      content: {
        schemaVersion: "ai-v3-1",
        content: [
          {
            id: "o1",
            explanation: "Explicação da observação.",
            dimension: "actions",
            rating: "clear",
            evidence: [],
          },
        ],
        strengths: [],
        gaps: [],
        english: { necessaryCorrections: [], optionalStyle: [] },
        priorities: [],
        fewerPrioritiesReason: null,
      },
      versions: {
        rubricVersion: "star-text-v1",
        promptVersion: "feedback-v3",
        schemaVersion: "ai-v3-1",
      },
    };
    renderComparison(
      view({
        left: original({ report, reportState: "ready" }),
        right: attempt({ report, reportState: "ready" }),
        rubricCompatibility: "same",
      }),
    );

    expect(
      await screen.findByText("Os dois relatórios usam a mesma rubrica."),
    ).toBeInTheDocument();
    expect(screen.getAllByText(/Rubrica star-text-v1; prompt feedback-v3/)).toHaveLength(2);
  });

  it("apresenta a sugestão da IA numa zona separada com aviso de factos", async () => {
    renderComparison(
      view({
        right: attempt({
          rewrites: [makeRewrite({ origin: REQUEST.right, level: "B2" })],
        }),
      }),
    );

    const right = await screen.findByRole("article", { name: /Prática/ });
    const zone = within(right).getByRole("complementary");
    expect(
      within(zone).getByText("Sugestão da IA — confirme os factos"),
    ).toBeInTheDocument();
    expect(within(zone).getByText("Suggested B2 sentence one.")).toBeInTheDocument();
    // O texto sugerido não aparece misturado com a resposta confirmada.
    const confirmed = within(right).getByText("Attempt confirmed text.");
    expect(zone.contains(confirmed)).toBe(false);
  });
});

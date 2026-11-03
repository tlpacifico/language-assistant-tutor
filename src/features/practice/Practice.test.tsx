import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import App from "../../App";
import {
  appError,
  emptyUsage,
  makeTrainingUsage,
  usageGroup,
  fakeInterviewClient,
  makePractice,
  makePreparation,
  makeStory,
  sessionSummary,
  snapshot,
} from "../../test/fake-interview-client";
import {
  answeringPractice,
  confirmedPractice,
  fakeWithCompletedSession,
} from "../../test/practice-fixtures";
import type { InterviewSnapshot } from "../../lib/interview-contracts";
import type { PracticeDetail } from "../../lib/practice-contracts";

/** Fotografia com uma prática pendente e sem sessão de entrevista. */
function withActivePractice(detail: PracticeDetail): InterviewSnapshot {
  return snapshot({
    revision: 3,
    practice: {
      revision: 3,
      activePractice: detail,
      activeOwner: {
        type: "practice",
        practiceId: detail.id,
        answerId: detail.answer.id,
      },
      pendingPreparation: null,
    },
  });
}

async function openHistorySession(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("button", { name: "Histórico" }));
  await user.click(await screen.findByRole("button", { name: "Abrir sessão" }));
  return screen.findByRole("heading", { name: "Treino posterior" });
}

function noAutomaticApiCalls(fake: ReturnType<typeof fakeInterviewClient>) {
  expect(fake.client.requestRewrite).not.toHaveBeenCalled();
  expect(fake.client.evaluatePractice).not.toHaveBeenCalled();
  expect(fake.client.requestFollowUp).not.toHaveBeenCalled();
  expect(fake.client.generateFeedback).not.toHaveBeenCalled();
}

describe("treino posterior — criar e bloquear", () => {
  it("Tentar novamente cria a prática sem feedback, reformulação nem seguimento", async () => {
    const user = userEvent.setup();
    const fake = fakeWithCompletedSession({ initial: snapshot({ revision: 1 }) });
    render(<App client={fake.client} />);

    await openHistorySession(user);
    await user.click(screen.getByRole("button", { name: "Tentar novamente" }));

    expect(fake.client.createPractice).toHaveBeenCalledTimes(1);
    expect(fake.client.createPractice).toHaveBeenCalledWith(
      expect.objectContaining({
        sessionId: "session-1",
        sessionQuestionId: "sq-1",
        previousPracticeId: null,
      }),
    );
    expect(
      await screen.findByRole("heading", { name: "Prática 1" }),
    ).toBeInTheDocument();
    noAutomaticApiCalls(fake);
    // Sem sessão fictícia: não existe progresso de entrevista.
    expect(screen.queryByText(/Pergunta \d+ de \d+/)).not.toBeInTheDocument();
  });

  it("bloqueia a prática com uma entrevista em curso", async () => {
    const user = userEvent.setup();
    const fake = fakeWithCompletedSession({
      initial: snapshot({ revision: 1, activeSession: sessionSummary() }),
    });
    render(<App client={fake.client} />);

    await openHistorySession(user);

    expect(screen.getByRole("button", { name: "Tentar novamente" })).toBeDisabled();
    expect(
      screen.getByText("Termine a entrevista em curso antes de praticar."),
    ).toBeInTheDocument();
    expect(fake.client.createPractice).not.toHaveBeenCalled();
  });

  it("bloqueia nova prática com outra pendente e oferece retomar", async () => {
    const user = userEvent.setup();
    const pending = makePractice({ id: "practice-9" });
    const fake = fakeWithCompletedSession({ initial: withActivePractice(pending) });
    render(<App client={fake.client} />);

    await openHistorySession(user);

    expect(screen.getByRole("button", { name: "Tentar novamente" })).toBeDisabled();
    expect(
      screen.getByText(/Já existe uma prática pendente/),
    ).toBeInTheDocument();
    await user.click(
      screen.getAllByRole("button", { name: "Retomar prática pendente" })[0],
    );
    expect(
      await screen.findByRole("heading", { name: "Prática 1" }),
    ).toBeInTheDocument();
  });

  it("mostra a recusa de domínio do núcleo", async () => {
    const user = userEvent.setup();
    const fake = fakeWithCompletedSession({ initial: snapshot({ revision: 1 }) });
    fake.client.createPractice = async () => {
      throw appError("STALE_SOURCE", "Origem sem confirmação.", false);
    };
    render(<App client={fake.client} />);

    await openHistorySession(user);
    await user.click(screen.getByRole("button", { name: "Tentar novamente" }));

    expect(await screen.findByText("Origem sem confirmação.")).toBeInTheDocument();
  });
});

describe("treino posterior — retoma e captura", () => {
  it("retoma a prática pendente sem sessão de entrevista", async () => {
    const user = userEvent.setup();
    const fake = fakeInterviewClient({
      initial: withActivePractice(makePractice({ id: "practice-1" })),
    });
    fake.practices.push(makePractice({ id: "practice-1" }));
    render(<App client={fake.client} />);

    await user.click(
      await screen.findByRole("button", { name: "Retomar prática pendente" }),
    );

    expect(
      await screen.findByRole("heading", { name: "Prática 1" }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Tell me about a time you solved a difficult problem/),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Iniciar gravação" })).toBeEnabled();
    expect(fake.client.startPracticeRecording).not.toHaveBeenCalled();
  });

  it("inicia a gravação com o dono da prática e a versão de seleção", async () => {
    const user = userEvent.setup();
    const practice = makePractice({ id: "practice-1" });
    const fake = fakeInterviewClient({ initial: withActivePractice(practice) });
    fake.practices.push(practice);
    render(<App client={fake.client} />);

    await user.click(
      await screen.findByRole("button", { name: "Retomar prática pendente" }),
    );
    await user.click(await screen.findByRole("button", { name: "Iniciar gravação" }));

    expect(fake.client.startPracticeRecording).toHaveBeenCalledWith(
      {
        type: "practice",
        practiceId: "practice-1",
        answerId: practice.answer.id,
      },
      0,
    );
  });

  it("erro de metadados do apoio não esconde Terminar gravação", async () => {
    const user = userEvent.setup();
    const practice = answeringPractice();
    const fake = fakeInterviewClient({ initial: withActivePractice(practice) });
    fake.practices.push(practice);
    render(<App client={fake.client} />);

    await user.click(
      await screen.findByRole("button", { name: "Retomar prática pendente" }),
    );
    await screen.findByRole("heading", { name: "Prática 1" });

    // O microfone arranca, mas a escrita dos metadados do apoio falha.
    act(() =>
      fake.publish({
        ...withActivePractice(practice),
        revision: 9,
        recordingId: "rec-p2",
        practice: {
          revision: 9,
          activePractice: practice,
          activeOwner: {
            type: "practice",
            practiceId: practice.id,
            answerId: practice.answer.id,
          },
          pendingPreparation: "rec-p2",
        },
      }),
    );

    expect(
      await screen.findByRole("button", { name: "Terminar gravação" }),
    ).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "Repetir guardar apoio" }));
    expect(fake.client.retryPreparationPublication).toHaveBeenCalledWith("rec-p2");
    expect(
      screen.getByRole("button", { name: "Terminar gravação" }),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Terminar gravação" }));
    expect(fake.client.stopRecording).toHaveBeenCalled();
  });
});

describe("treino posterior — revisão, confirmação e abandono", () => {
  it("guarda e confirma a revisão pelos comandos da prática", async () => {
    const user = userEvent.setup();
    const practice = answeringPractice();
    const fake = fakeInterviewClient({ initial: withActivePractice(practice) });
    fake.practices.push(practice);
    render(<App client={fake.client} />);

    await user.click(
      await screen.findByRole("button", { name: "Retomar prática pendente" }),
    );
    const editor = await screen.findByLabelText("Revisão editável");
    // A revisão parte apenas da transcrição automática.
    expect(editor).toHaveValue("Practice raw transcript.");
    expect(screen.getByLabelText("Transcrição automática")).toHaveValue(
      "Practice raw transcript.",
    );

    await user.clear(editor);
    await user.type(editor, "My reviewed practice answer.");
    await user.click(screen.getByRole("button", { name: "Guardar revisão" }));

    expect(fake.client.savePracticeReview).toHaveBeenCalledWith(
      expect.objectContaining({
        target: {
          owner: {
            type: "practice",
            practiceId: "practice-1",
            answerId: practice.answer.id,
          },
          recordingId: "rec-p1",
          selectionVersion: 1,
        },
        expectedReviewVersion: 0,
        sourceTranscriptId: "tr-p1",
        origin: "automatic",
        text: "My reviewed practice answer.",
      }),
    );
    expect(fake.client.saveReview).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Confirmar resposta" }));

    await waitFor(() =>
      expect(fake.client.confirmPracticeAnswer).toHaveBeenCalledWith(
        expect.objectContaining({
          expectedPracticeVersion: 1,
          text: "My reviewed practice answer.",
        }),
      ),
    );
    expect(fake.client.confirmAnswer).not.toHaveBeenCalled();
    expect(
      await screen.findByRole("heading", {
        name: "Resposta confirmada da tentativa",
      }),
    ).toBeInTheDocument();
    expect(screen.getByText("My reviewed practice answer.")).toBeInTheDocument();
    noAutomaticApiCalls(fake);
  });

  it("abandono pede a decisão guardar/descartar/cancelar e conserva os dados", async () => {
    const user = userEvent.setup();
    const practice = answeringPractice();
    const fake = fakeInterviewClient({ initial: withActivePractice(practice) });
    fake.practices.push(practice);
    render(<App client={fake.client} />);

    await user.click(
      await screen.findByRole("button", { name: "Retomar prática pendente" }),
    );
    const editor = await screen.findByLabelText("Revisão editável");
    await user.type(editor, " edited");

    await user.click(screen.getByRole("button", { name: "Abandonar prática" }));
    expect(
      screen.getByText(/conserva as gravações e as revisões para leitura futura/),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Confirmar abandono" }));

    // Há texto por guardar: nada é abandonado sem decisão.
    expect(fake.client.abandonPractice).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Cancelar" }));
    expect(fake.client.abandonPractice).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Confirmar abandono" }));
    await user.click(screen.getByRole("button", { name: "Descartar e sair" }));

    await waitFor(() =>
      expect(fake.client.abandonPractice).toHaveBeenCalledWith(
        expect.objectContaining({ practiceId: "practice-1", expectedVersion: 1 }),
      ),
    );
    expect(
      await screen.findByRole("heading", { name: "Prática abandonada" }),
    ).toBeInTheDocument();
    // As gravações continuam consultáveis em leitura.
    expect(
      screen.getByRole("button", { name: "Ouvir tomada 1" }),
    ).toBeInTheDocument();
    expect(fake.client.saveReview).not.toHaveBeenCalled();
    expect(fake.client.savePracticeReview).not.toHaveBeenCalled();
  });

  it("guardar antes de sair usa o comando de revisão da prática", async () => {
    const user = userEvent.setup();
    const practice = answeringPractice();
    const fake = fakeInterviewClient({ initial: withActivePractice(practice) });
    fake.practices.push(practice);
    render(<App client={fake.client} />);

    await user.click(
      await screen.findByRole("button", { name: "Retomar prática pendente" }),
    );
    await user.type(await screen.findByLabelText("Revisão editável"), " more");
    await user.click(screen.getByRole("button", { name: "Histórico" }));
    await user.click(screen.getByRole("button", { name: "Guardar e sair" }));

    await waitFor(() =>
      expect(fake.client.savePracticeReview).toHaveBeenCalledTimes(1),
    );
  });
});

describe("treino posterior — avaliação, publicação e consumo", () => {
  it("avalia a tentativa explicitamente e mostra a rubrica", async () => {
    const user = userEvent.setup();
    const practice = confirmedPractice();
    const fake = fakeWithCompletedSession({ initial: snapshot({ revision: 1 }) });
    fake.practices.push(practice);
    fake.reports.set("practice-1", {
      practiceId: "practice-1",
      state: "ready",
      report: {
        schemaVersion: "ai-v3-1",
        content: [
          {
            id: "o1",
            explanation: "Descreve a ação.",
            dimension: "actions",
            rating: "clear",
            evidence: [
              {
                type: "quote",
                confirmationId: "confirmation-practice-1",
                role: "attempt",
                quote: "Practice confirmed answer.",
              },
            ],
          },
        ],
        strengths: [],
        gaps: [],
        english: { necessaryCorrections: [], optionalStyle: [] },
        priorities: [],
        fewerPrioritiesReason: null,
      },
      rubricVersion: "star-text-v1",
      promptVersion: "practice-feedback-v1",
      schemaVersion: "ai-v3-1",
      usage: emptyUsage({ input: null, complete: false, unknownAttemptCount: 1 }),
      networkAttempts: [],
    });
    // Antes de pedir, o relatório está pendente.
    const ready = fake.reports.get("practice-1")!;
    fake.reports.delete("practice-1");
    render(<App client={fake.client} />);

    await openHistorySession(user);
    await user.click(screen.getByRole("button", { name: "Abrir prática 1" }));
    expect(
      await screen.findByText("Ainda sem avaliação"),
    ).toBeInTheDocument();
    expect(fake.client.evaluatePractice).not.toHaveBeenCalled();

    fake.reports.set("practice-1", ready);
    await user.click(screen.getByRole("button", { name: "Avaliar tentativa" }));

    expect(fake.client.evaluatePractice).toHaveBeenCalledWith(
      expect.objectContaining({ practiceId: "practice-1", expectedVersion: 3 }),
    );
    expect(await screen.findByText(/Rubrica star-text-v1/)).toBeInTheDocument();
    expect(screen.getByText(/Descreve a ação/)).toBeInTheDocument();
    // Nenhum seguimento nasce de uma prática.
    expect(fake.client.requestFollowUp).not.toHaveBeenCalled();
  });

  it("retry de publicação é separado do pedido de rede", async () => {
    const user = userEvent.setup();
    const practice = confirmedPractice();
    const fake = fakeWithCompletedSession({
      initial: snapshot({ revision: 1, pendingAiPublication: "attempt-7" }),
    });
    fake.practices.push(practice);
    render(<App client={fake.client} />);

    await openHistorySession(user);
    await user.click(screen.getByRole("button", { name: "Abrir prática 1" }));
    await user.click(
      await screen.findByRole("button", {
        name: "Guardar resultado recebido sem novo pedido",
      }),
    );

    expect(fake.client.retryAiPublication).toHaveBeenCalledWith("attempt-7");
    expect(fake.client.evaluatePractice).not.toHaveBeenCalled();
    expect(fake.client.requestRewrite).not.toHaveBeenCalled();
  });

  it("consulta, comparação e edição de história não enviam pedidos à API", async () => {
    const user = userEvent.setup();
    const practice = confirmedPractice();
    const fake = fakeWithCompletedSession({ initial: snapshot({ revision: 1 }) });
    fake.practices.push(practice);
    fake.stories.push(makeStory());
    render(<App client={fake.client} />);

    await openHistorySession(user);
    await user.click(screen.getByRole("button", { name: "Abrir prática 1" }));
    await screen.findByRole("heading", { name: "Resposta confirmada da tentativa" });
    await user.click(screen.getByRole("button", { name: "Histórias" }));
    await user.click(await screen.findByRole("button", { name: /Editar história/ }));
    await user.type(screen.getByLabelText("Título"), " v2");
    await user.click(screen.getByRole("button", { name: "Guardar alterações" }));

    await waitFor(() => expect(fake.client.updateStory).toHaveBeenCalled());
    noAutomaticApiCalls(fake);
  });

  it("não deixa uma prática confirmada ser tratada como pendente", async () => {
    const user = userEvent.setup();
    const practice = confirmedPractice({ preparation: makePreparation() });
    const fake = fakeWithCompletedSession({ initial: snapshot({ revision: 1 }) });
    fake.practices.push(practice);
    render(<App client={fake.client} />);

    await openHistorySession(user);
    await user.click(screen.getByRole("button", { name: "Abrir prática 1" }));
    const confirmedSection = await screen.findByRole("heading", {
      name: "Resposta confirmada da tentativa",
    });
    expect(confirmedSection).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Iniciar gravação" }),
    ).not.toBeInTheDocument();
    expect(
      within(screen.getByRole("main")).queryByLabelText("Revisão editável"),
    ).not.toBeInTheDocument();
  });
});

describe("treino posterior — consumo separado", () => {
  it("separa entrevista original e treino posterior e rotula o desconhecido", async () => {
    const user = userEvent.setup();
    const fake = fakeWithCompletedSession({ initial: snapshot({ revision: 1 }) });
    fake.setTrainingUsage(
      makeTrainingUsage({
        originalInterview: usageGroup({
          networkAttemptCount: 3,
          totals: emptyUsage({ knownReportedSubtotal: 0.012, input: 1200, output: 300 }),
        }),
        laterPractice: usageGroup({
          networkAttemptCount: 2,
          totals: emptyUsage({
            knownReportedSubtotal: 0.004,
            input: null,
            output: null,
            complete: false,
            unknownAttemptCount: 1,
          }),
          byPurpose: [
            {
              purpose: "rewrite",
              networkAttemptCount: 2,
              usage: emptyUsage({ complete: false, unknownAttemptCount: 1 }),
            },
          ],
        }),
        combined: usageGroup({
          networkAttemptCount: 5,
          totals: emptyUsage({
            knownReportedSubtotal: 0.016,
            complete: false,
            unknownAttemptCount: 1,
          }),
        }),
      }),
    );
    render(<App client={fake.client} />);

    await openHistorySession(user);

    const later = await screen.findByLabelText("Consumo — treino posterior");
    const original = screen.getByLabelText("Consumo — entrevista original");
    expect(within(original).getByText("Chamadas de rede: 3.")).toBeInTheDocument();
    expect(within(original).getByText(/Tokens de entrada: 1200/)).toBeInTheDocument();
    expect(within(later).getByText("Chamadas de rede: 2.")).toBeInTheDocument();
    // Consumo desconhecido nunca aparece como zero.
    expect(within(later).getByText(/Tokens de entrada: desconhecidos/)).toBeInTheDocument();
    expect(within(later).getByText(/Valores parciais/)).toBeInTheDocument();
    expect(within(later).getByText(/rewrite: 2 chamada/)).toBeInTheDocument();
    expect(screen.getByLabelText("Consumo — total combinado")).toBeInTheDocument();
    expect(fake.client.requestRewrite).not.toHaveBeenCalled();
  });
});

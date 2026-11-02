import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { Attempt, RestoreResult } from "../../lib/contracts";
import type { SessionDetail } from "../../lib/interview-contracts";
import {
  appError,
  fakeInterviewClient,
  makeConfirmation,
  makeQuestionDetail,
  makeRecording,
  makeSessionQuestion,
  makeTranscript,
  sessionSummary,
  snapshot,
} from "../../test/fake-interview-client";
import History from "./History";

/** Resposta guardada e confirmada da primeira pergunta. */
function confirmedDetail(): SessionDetail {
  const summary = sessionSummary({
    id: "session-1",
    count: 2,
    confirmedCount: 1,
    position: 1,
    version: 4,
  });
  return {
    summary,
    questions: [
      makeQuestionDetail({
        question: makeSessionQuestion({
          id: "sq-1",
          sessionId: "session-1",
          position: 1,
          selectedRecordingId: "recording-1",
        }),
        recordings: [
          makeRecording({
            id: "recording-1",
            sessionQuestionId: "sq-1",
            transcripts: [
              makeTranscript({ id: "transcript-1", text: "Raw automatic answer." }),
            ],
            review: {
              recordingId: "recording-1",
              sourceTranscriptId: "transcript-1",
              origin: "automatic",
              text: "Edited saved review.",
              version: 2,
              savedAt: "2026-11-02T10:05:00.000Z",
            },
          }),
        ],
        confirmation: makeConfirmation({
          sessionQuestionId: "sq-1",
          recordingId: "recording-1",
          reviewVersion: 2,
          text: "Confirmed immutable answer.",
        }),
      }),
    ],
  };
}

/** Treino da fase 1 com transcrição automática e revisão guardada. */
function legacyAttempt(): Attempt {
  return {
    id: "attempt-1",
    question: "Tell me about a time you solved a difficult problem at work.",
    createdAt: "2026-10-20T10:00:00.000Z",
    capture: {
      sampleRate: 48_000,
      channels: 1,
      durationMs: 9_000,
      bytes: 1_000,
      incomplete: false,
    },
    originalFile: "original.wav",
    normalizedFile: null,
    transcripts: [
      makeTranscript({ id: "legacy-transcript", text: "Legacy automatic answer." }),
    ],
    review: {
      sourceTranscriptId: "legacy-transcript",
      text: "Legacy saved review.",
      savedAt: "2026-10-20T10:05:00.000Z",
    },
    lastOperation: null,
  };
}

/** Treinos da fase 1 distinguíveis pelo texto, para provar qual ficou aberto. */
function legacyPair(): [Attempt, Attempt] {
  const first = legacyAttempt();
  const second: Attempt = {
    ...legacyAttempt(),
    id: "attempt-2",
    question: "Tell me about a time you had competing deadlines.",
    createdAt: "2026-10-21T10:00:00.000Z",
    transcripts: [
      makeTranscript({
        id: "legacy-transcript-2",
        text: "Second legacy automatic answer.",
      }),
    ],
    review: null,
  };
  return [first, second];
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

describe("History", () => {
  it("history_separates_raw_review_and_confirmed", async () => {
    const user = userEvent.setup();
    const fake = fakeInterviewClient();
    const detail = confirmedDetail();
    vi.mocked(fake.client.listSessions).mockResolvedValue([detail.summary]);
    vi.mocked(fake.client.loadSession).mockResolvedValue(detail);
    render(<History client={fake.client} onResume={vi.fn()} />);

    await user.click(await screen.findByRole("button", { name: "Abrir sessão" }));

    const label = "pergunta 1 de 2";
    const raw = await screen.findByRole("region", {
      name: `Transcrição automática da ${label}`,
    });
    const review = screen.getByRole("region", {
      name: `Revisão guardada da ${label}`,
    });
    const confirmed = screen.getByRole("region", {
      name: `Resposta confirmada da ${label}`,
    });

    expect(within(raw).getByText("Raw automatic answer.")).toBeInTheDocument();
    expect(within(review).getByText("Edited saved review.")).toBeInTheDocument();
    expect(
      within(confirmed).getByText("Confirmed immutable answer."),
    ).toBeInTheDocument();

    // Nenhum texto aparece na área de outro: automático, revisão e confirmação
    // são versões distintas e não se substituem.
    expect(within(raw).queryByText("Edited saved review.")).toBeNull();
    expect(within(raw).queryByText("Confirmed immutable answer.")).toBeNull();
    expect(within(review).queryByText("Raw automatic answer.")).toBeNull();
    expect(within(review).queryByText("Confirmed immutable answer.")).toBeNull();
    expect(within(confirmed).queryByText("Raw automatic answer.")).toBeNull();
    expect(within(confirmed).queryByText("Edited saved review.")).toBeNull();

    // Abrir a sessão é uma leitura: não muda a sessão em curso nem avança nada.
    expect(fake.client.snapshot).not.toHaveBeenCalled();
    expect(fake.client.advanceSession).not.toHaveBeenCalled();
    expect(fake.client.confirmAnswer).not.toHaveBeenCalled();
  });

  it("resume_keeps_confirmed_question_awaiting_next", async () => {
    const user = userEvent.setup();
    const fake = fakeInterviewClient();
    const detail = confirmedDetail();
    const summary = detail.summary;
    const current = detail.questions[0];
    expect(current).toBeDefined();
    vi.mocked(fake.client.listSessions).mockResolvedValue([summary]);
    vi.mocked(fake.client.loadSession).mockResolvedValue(detail);
    vi.mocked(fake.client.snapshot).mockResolvedValue(
      snapshot({ revision: 9, activeSession: summary, current }),
    );
    const onResume = vi.fn();
    render(<History client={fake.client} onResume={onResume} />);

    await user.click(await screen.findByRole("button", { name: "Retomar" }));

    await waitFor(() => expect(onResume).toHaveBeenCalledWith("session-1"));
    // A etapa persistida é a pergunta 1, com a resposta confirmada à espera de
    // um avanço explícito: retomar não confirma nem avança nada.
    expect(
      await screen.findByText(/Sessão retomada na pergunta 1 de 2/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/A resposta confirmada aguarda que avance explicitamente/),
    ).toBeInTheDocument();
    expect(fake.client.advanceSession).not.toHaveBeenCalled();
    expect(fake.client.confirmAnswer).not.toHaveBeenCalled();
    expect(fake.client.loadSession).not.toHaveBeenCalled();
    expect(
      screen.queryByRole("button", { name: /confirmar/i }),
    ).not.toBeInTheDocument();
  });

  it("missing_audio_keeps_confirmed_text", async () => {
    const user = userEvent.setup();
    const fake = fakeInterviewClient();
    const summary = sessionSummary({
      id: "session-1",
      count: 1,
      confirmedCount: 1,
      position: 1,
      status: "completed",
      completedAt: "2026-11-02T11:00:00.000Z",
    });
    const detail: SessionDetail = {
      summary,
      questions: [
        makeQuestionDetail({
          question: makeSessionQuestion({
            id: "sq-1",
            sessionId: "session-1",
            position: 1,
            selectedRecordingId: "recording-1",
          }),
          recordings: [
            makeRecording({
              id: "recording-1",
              sessionQuestionId: "sq-1",
              state: "interrupted",
              originalFile: null,
              warnings: [
                appError(
                  "ARTIFACT_MISSING",
                  "O áudio original da tomada não foi encontrado; os textos foram conservados.",
                  false,
                ),
              ],
            }),
          ],
          confirmation: makeConfirmation({
            sessionQuestionId: "sq-1",
            recordingId: "recording-1",
            text: "Confirmed despite missing audio.",
          }),
        }),
      ],
    };
    vi.mocked(fake.client.listSessions).mockResolvedValue([summary]);
    vi.mocked(fake.client.loadSession).mockResolvedValue(detail);
    render(<History client={fake.client} onResume={vi.fn()} />);

    await user.click(await screen.findByRole("button", { name: "Abrir sessão" }));

    // O áudio em falta é um aviso por artefacto; o texto confirmado mantém-se.
    expect(
      await screen.findByText("Confirmed despite missing audio."),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/não foi encontrado; os textos foram conservados/),
    ).toBeInTheDocument();
    expect(screen.getByText(/Sem áudio original guardado/)).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Ouvir original da tomada/ }),
    ).not.toBeInTheDocument();

    // Uma sessão concluída mostra um resumo e nada de feedback pendente.
    expect(
      screen.getByRole("region", { name: "Resumo da sessão concluída" }),
    ).toBeInTheDocument();
    expect(screen.queryByText(/feedback pendente/i)).toBeNull();
  });

  it("legacy_has_no_confirmation_action", async () => {
    const user = userEvent.setup();
    const fake = fakeInterviewClient();
    const attempt = legacyAttempt();
    const result: RestoreResult = { attempt, warnings: [] };
    vi.mocked(fake.client.listLegacy).mockResolvedValue([result]);
    vi.mocked(fake.client.loadLegacy).mockResolvedValue(result);
    render(<History client={fake.client} onResume={vi.fn()} />);

    const legacy = await screen.findByRole("region", {
      name: "Treinos da fase 1",
    });
    await user.click(
      within(legacy).getByRole("button", { name: "Abrir detalhes" }),
    );

    expect(
      await within(legacy).findByText("Legacy automatic answer."),
    ).toBeInTheDocument();
    expect(within(legacy).getByText("Legacy saved review.")).toBeInTheDocument();
    expect(fake.client.loadLegacy).toHaveBeenCalledWith("attempt-1");

    await user.click(within(legacy).getByRole("button", { name: "Ouvir original" }));
    expect(fake.client.playAudio).toHaveBeenCalledWith({
      type: "legacy",
      attemptId: "attempt-1",
    });

    // Os treinos anteriores não têm confirmação nem qualquer ação de escrita.
    expect(
      within(legacy).queryByRole("button", { name: /confirmar/i }),
    ).toBeNull();
    expect(within(legacy).queryByRole("button", { name: /gravar/i })).toBeNull();
    expect(fake.client.confirmAnswer).not.toHaveBeenCalled();
    expect(fake.client.saveReview).not.toHaveBeenCalled();
    expect(fake.client.advanceSession).not.toHaveBeenCalled();
  });

  it("legacy_ignores_a_late_response_from_the_previous_item", async () => {
    const user = userEvent.setup();
    const fake = fakeInterviewClient();
    const [first, second] = legacyPair();
    const firstLoad = deferred<RestoreResult>();
    const secondLoad = deferred<RestoreResult>();
    vi.mocked(fake.client.listLegacy).mockResolvedValue([
      { attempt: first, warnings: [] },
      { attempt: second, warnings: [] },
    ]);
    vi.mocked(fake.client.loadLegacy).mockImplementation((attemptId) =>
      attemptId === first.id ? firstLoad.promise : secondLoad.promise,
    );
    render(<History client={fake.client} onResume={vi.fn()} />);

    const legacy = await screen.findByRole("region", {
      name: "Treinos da fase 1",
    });
    const openButtons = await within(legacy).findAllByRole("button", {
      name: "Abrir detalhes",
    });
    expect(openButtons).toHaveLength(2);

    // Abrir A e logo B: a resposta de A chega depois da de B.
    await user.click(openButtons[0]);
    await user.click(openButtons[1]);

    await act(async () => {
      secondLoad.resolve({ attempt: second, warnings: [] });
      await secondLoad.promise;
    });
    await act(async () => {
      firstLoad.resolve({ attempt: first, warnings: [] });
      await firstLoad.promise;
    });

    // O detalhe visível pertence a B; a resposta tardia de A não o substitui.
    expect(
      within(legacy).getByText("Second legacy automatic answer."),
    ).toBeInTheDocument();
    expect(
      within(legacy).queryByText("Legacy automatic answer."),
    ).toBeNull();
    expect(fake.client.loadLegacy).toHaveBeenCalledTimes(2);
  });
});

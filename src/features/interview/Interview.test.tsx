import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useCallback, useEffect, useState } from "react";
import { describe, expect, it, vi } from "vitest";

import type { InterviewClient } from "../../lib/interview-client";
import type {
  AdvanceSession,
  ConfirmAnswer,
  InterviewSnapshot,
  QuestionDetail,
  RecordingView,
  SaveReview,
  SessionDetail,
  SessionQuestion,
} from "../../lib/interview-contracts";
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
import Interview from "./Interview";
import { useInterview } from "./useInterview";

const QUESTION_TEXT =
  "Tell me about a time you solved a difficult problem at work.";

/** Fotografia com uma sessão em curso e a pergunta corrente. */
function interviewSnapshot(options: {
  position?: number;
  count?: number;
  sessionVersion?: number;
  confirmedCount?: number;
  question?: Partial<SessionQuestion>;
  recordings?: RecordingView[];
  confirmation?: QuestionDetail["confirmation"];
  revision?: number;
  recordingId?: string | null;
  pendingRecordingId?: string | null;
} = {}): InterviewSnapshot {
  const recordings = options.recordings ?? [];
  const position = options.position ?? 2;
  const count = options.count ?? 3;
  return snapshot({
    revision: options.revision ?? 3,
    recordingId: options.recordingId ?? null,
    pendingRecordingId: options.pendingRecordingId ?? null,
    activeSession: sessionSummary({
      position,
      count,
      version: options.sessionVersion ?? 5,
      confirmedCount: options.confirmedCount ?? 1,
    }),
    current: makeQuestionDetail({
      question: makeSessionQuestion({
        id: "sq-2",
        position,
        text: QUESTION_TEXT,
        selectedRecordingId:
          recordings.length > 0
            ? recordings[recordings.length - 1].id
            : null,
        selectionVersion: 2,
        ...options.question,
      }),
      recordings,
      confirmation: options.confirmation ?? null,
    }),
  });
}

/** Fotografia de uma gravação com transcrição automática concluída. */
function recordedSnapshot(transcriptText = "My first automatic answer.") {
  return interviewSnapshot({
    recordings: [
      makeRecording({
        id: "recording-1",
        sessionQuestionId: "sq-2",
        transcripts: [makeTranscript({ id: "transcript-1", text: transcriptText })],
      }),
    ],
  });
}

/** Fotografia após a revisão ter sido guardada na versão indicada. */
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
                sourceTranscriptId: recording.transcripts[0]?.id ?? null,
                origin:
                  recording.transcripts[0]?.status === "completed" &&
                  recording.transcripts[0]?.text.trim() !== ""
                    ? "automatic"
                    : "manual_explicit",
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

/** Fotografia após a confirmação, com o texto exatamente revisto. */
function confirmedSnapshot(
  base: InterviewSnapshot,
  text: string,
  reviewVersion = 1,
): InterviewSnapshot {
  const reviewed = reviewedSnapshot(base, text, reviewVersion);
  const current = reviewed.current;
  const summary = reviewed.activeSession;
  if (!current || !summary) {
    throw new Error("A fotografia revista não tem sessão ou pergunta.");
  }
  const recordingId = current.question.selectedRecordingId ?? "recording-1";
  return snapshot({
    ...reviewed,
    revision: reviewed.revision + 1,
    activeSession: {
      ...summary,
      confirmedCount: summary.confirmedCount + 1,
      version: summary.version + 1,
    },
    current: {
      ...current,
      confirmation: makeConfirmation({
        sessionQuestionId: current.question.id,
        recordingId,
        reviewVersion,
        text,
      }),
    },
  });
}

/** Página ligada ao cliente como a aplicação a liga: um único rascunho. */
function Harness({
  client,
  initial,
}: {
  client: InterviewClient;
  initial: InterviewSnapshot;
}) {
  const [current, setCurrent] = useState(initial);
  const apply = useCallback((next: InterviewSnapshot) => {
    setCurrent((previous) =>
      next.revision < previous.revision ? previous : next,
    );
  }, []);

  useEffect(() => client.subscribe(apply), [client, apply]);

  const review = useInterview(client, current, apply);
  return (
    <Interview
      client={client}
      snapshot={current}
      onSnapshot={apply}
      review={review}
      requestDecision={(_intention, proceed) => void proceed()}
    />
  );
}

function renderPage(initial: InterviewSnapshot) {
  const fake = fakeInterviewClient({ initial });
  const view = render(<Harness client={fake.client} initial={initial} />);
  return {
    fake,
    ...view,
    publish(next: InterviewSnapshot) {
      act(() => fake.publish(next));
    },
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("Interview", () => {
  it("shows_question_two_of_three", async () => {
    renderPage(recordedSnapshot());

    expect(await screen.findByText("Pergunta 2 de 3")).toBeInTheDocument();
    expect(screen.getByText(QUESTION_TEXT)).toHaveAttribute("lang", "en");
    expect(screen.getByText("Tomada 1")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Gravar novamente" }),
    ).toBeEnabled();
    expect(screen.queryByText(/pontuação|nota|feedback/i)).toBeNull();
  });

  it("save_does_not_enable_next_until_confirmation", async () => {
    const user = userEvent.setup();
    const base = recordedSnapshot();
    const { fake } = renderPage(base);
    const saves: SaveReview[] = [];
    vi.mocked(fake.client.saveReview).mockImplementation(async (request) => {
      saves.push(request);
      return reviewedSnapshot(base, request.text, 1);
    });
    vi.mocked(fake.client.confirmAnswer).mockImplementation(async (request) =>
      confirmedSnapshot(base, request.text, request.expectedReviewVersion + 1),
    );

    const editor = await screen.findByLabelText("Revisão editável");
    await user.clear(editor);
    await user.type(editor, "I turned the incident into a checklist.");
    await user.click(screen.getByRole("button", { name: "Guardar revisão" }));

    expect(saves).toHaveLength(1);
    expect(saves[0].text).toBe("I turned the incident into a checklist.");
    expect(fake.client.confirmAnswer).not.toHaveBeenCalled();
    expect(fake.client.advanceSession).not.toHaveBeenCalled();
    expect(
      screen.getByRole("button", { name: "Próxima pergunta" }),
    ).toBeDisabled();

    await user.click(screen.getByRole("button", { name: "Confirmar resposta" }));

    expect(fake.client.confirmAnswer).toHaveBeenCalledTimes(1);
    expect(fake.client.advanceSession).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Próxima pergunta" }),
      ).toBeEnabled(),
    );
  });

  it("confirmation_locks_exact_editor_text", async () => {
    const user = userEvent.setup();
    const base = recordedSnapshot();
    const { fake } = renderPage(base);
    const confirmations: ConfirmAnswer[] = [];
    vi.mocked(fake.client.confirmAnswer).mockImplementation(async (request) => {
      confirmations.push(request);
      return confirmedSnapshot(base, request.text, 1);
    });

    const text = "I split the billing migration into three releases.";
    const editor = await screen.findByLabelText("Revisão editável");
    await user.clear(editor);
    await user.type(editor, text);
    await user.click(screen.getByRole("button", { name: "Confirmar resposta" }));

    expect(confirmations).toHaveLength(1);
    const request = confirmations[0];
    expect(request.text).toBe(text);
    expect(request.expectedReviewVersion).toBe(0);
    expect(request.expectedSessionVersion).toBe(5);
    expect(request.origin).toBe("automatic");
    expect(request.sourceTranscriptId).toBe("transcript-1");
    expect(request.target).toEqual({
      sessionId: "session-1",
      sessionQuestionId: "sq-2",
      recordingId: "recording-1",
      selectionVersion: 2,
    });
    expect(request.requestId.length).toBeGreaterThan(0);

    const locked = screen.getByLabelText("Revisão editável");
    await waitFor(() => expect(locked).toHaveValue(text));
    expect(locked).toHaveAttribute("readonly");
    expect(
      screen.queryByRole("button", { name: "Confirmar resposta" }),
    ).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Gravar novamente" }),
    ).toBeNull();
    expect(screen.queryByText(/pontuação|nota|feedback/i)).toBeNull();
  });

  it("last_question_uses_finish_label", async () => {
    const user = userEvent.setup();
    const base = interviewSnapshot({
      position: 3,
      count: 3,
      confirmedCount: 2,
      recordings: [
        makeRecording({
          id: "recording-1",
          sessionQuestionId: "sq-2",
          transcripts: [
            makeTranscript({ id: "transcript-1", text: "A confirmed answer." }),
          ],
          review: {
            recordingId: "recording-1",
            sourceTranscriptId: "transcript-1",
            origin: "automatic",
            text: "A confirmed answer.",
            version: 1,
            savedAt: "2026-11-02T10:20:00Z",
          },
        }),
      ],
      confirmation: makeConfirmation({
        text: "A confirmed answer.",
        sessionQuestionId: "sq-2",
      }),
    });
    const { fake } = renderPage(base);
    const advances: AdvanceSession[] = [];
    vi.mocked(fake.client.advanceSession).mockImplementation(async (request) => {
      advances.push(request);
      return snapshot({ revision: 9, activeSession: null, current: null });
    });
    const detail: SessionDetail = {
      summary: sessionSummary({
        status: "completed",
        position: 3,
        count: 3,
        confirmedCount: 3,
        completedAt: "2026-11-02T11:00:00Z",
      }),
      questions: [],
    };
    vi.mocked(fake.client.loadSession).mockResolvedValue(detail);

    expect(screen.queryByRole("button", { name: "Próxima pergunta" })).toBeNull();
    const finish = await screen.findByRole("button", { name: "Terminar sessão" });
    expect(finish).toBeEnabled();
    await user.click(finish);

    expect(advances).toHaveLength(1);
    expect(advances[0].sessionId).toBe("session-1");
    expect(advances[0].sessionQuestionId).toBe("sq-2");
    expect(advances[0].expectedSessionVersion).toBe(5);
    expect(fake.client.loadSession).toHaveBeenCalledWith("session-1");
    expect(
      await screen.findByRole("heading", { name: "Sessão concluída" }),
    ).toBeInTheDocument();
  });

  it("manual_revision_of_no_speech_is_explicit", async () => {
    const user = userEvent.setup();
    const base = interviewSnapshot({
      recordings: [
        makeRecording({
          id: "recording-1",
          sessionQuestionId: "sq-2",
          transcripts: [
            makeTranscript({
              id: "transcript-1",
              status: "no_speech",
              text: "",
            }),
          ],
        }),
      ],
    });
    const { fake } = renderPage(base);
    const saves: SaveReview[] = [];
    vi.mocked(fake.client.saveReview).mockImplementation(async (request) => {
      saves.push(request);
      return reviewedSnapshot(base, request.text, 1);
    });

    expect(
      await screen.findByText(/não reconheceu fala nesta gravação/i),
    ).toBeInTheDocument();
    expect(screen.getByText(/introduzido manualmente/i)).toBeInTheDocument();

    const editor = screen.getByLabelText("Revisão editável");
    expect(editor).toBeEnabled();
    await user.type(editor, "I described the outage from memory.");
    await user.click(screen.getByRole("button", { name: "Guardar revisão" }));

    expect(saves).toHaveLength(1);
    expect(saves[0].text).toBe("I described the outage from memory.");
    expect(saves[0].origin).toBe("manual_explicit");
    expect(saves[0].sourceTranscriptId).toBe("transcript-1");
    expect(saves[0].expectedReviewVersion).toBe(0);
    expect(fake.client.confirmAnswer).not.toHaveBeenCalled();
  });

  it("prior_recordings_remain_readable", async () => {
    const user = userEvent.setup();
    const base = interviewSnapshot({
      recordings: [
        makeRecording({
          id: "recording-1",
          sessionQuestionId: "sq-2",
          ordinal: 1,
          transcripts: [
            makeTranscript({ id: "transcript-1", text: "First take answer." }),
          ],
        }),
        makeRecording({
          id: "recording-2",
          sessionQuestionId: "sq-2",
          ordinal: 2,
          transcripts: [
            makeTranscript({ id: "transcript-2", text: "Second take answer." }),
          ],
        }),
      ],
    });
    const { fake } = renderPage(base);

    expect(await screen.findByText("Tomada 1")).toBeInTheDocument();
    expect(screen.getByText("Tomada 2")).toBeInTheDocument();
    const takes = screen.getAllByRole("listitem");
    expect(within(takes[0]).getByText("First take answer.")).toBeInTheDocument();
    expect(
      within(takes[1]).getByText("Second take answer."),
    ).toBeInTheDocument();
    expect(screen.getByText("Selecionada")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Ouvir tomada 1" }));
    expect(fake.client.playAudio).toHaveBeenCalledWith({
      type: "recording",
      recordingId: "recording-1",
    });
  });

  it("old_snapshot_does_not_erase_draft", async () => {
    const user = userEvent.setup();
    const base = recordedSnapshot();
    const { fake } = renderPage(base);
    const editor = await screen.findByLabelText("Revisão editável");
    await user.clear(editor);
    await user.type(editor, "Draft typed by the user.");

    act(() => {
      fake.publish(
        snapshot({
          ...reviewedSnapshot(base, "A server version.", 7),
          revision: 1,
        }),
      );
    });

    expect(screen.getByLabelText("Revisão editável")).toHaveValue(
      "Draft typed by the user.",
    );
    expect(screen.getByText("Pergunta 2 de 3")).toBeInTheDocument();
  });

  it("confirm_failure_preserves_text", async () => {
    const user = userEvent.setup();
    const base = recordedSnapshot();
    const { fake } = renderPage(base);
    vi.mocked(fake.client.confirmAnswer).mockRejectedValue(
      appError("STORAGE_WRITE_FAILED", "Não foi possível confirmar a resposta."),
    );

    const editor = await screen.findByLabelText("Revisão editável");
    await user.clear(editor);
    await user.type(editor, "Text that must survive a failed confirmation.");
    await user.click(screen.getByRole("button", { name: "Confirmar resposta" }));

    expect(
      await screen.findByText("Não foi possível confirmar a resposta."),
    ).toBeInTheDocument();
    const kept = screen.getByLabelText("Revisão editável");
    expect(kept).toHaveValue("Text that must survive a failed confirmation.");
    expect(kept).not.toHaveAttribute("readonly");
    expect(
      screen.getByRole("button", { name: "Confirmar resposta" }),
    ).toBeEnabled();
    expect(
      screen.getByRole("button", { name: "Próxima pergunta" }),
    ).toBeDisabled();
  });

  it("double_next_click_sends_one_request", async () => {
    const base = interviewSnapshot({
      recordings: [
        makeRecording({
          id: "recording-1",
          sessionQuestionId: "sq-2",
          transcripts: [
            makeTranscript({ id: "transcript-1", text: "A confirmed answer." }),
          ],
        }),
      ],
      confirmation: makeConfirmation({
        text: "A confirmed answer.",
        sessionQuestionId: "sq-2",
      }),
    });
    const { fake } = renderPage(base);
    const pending = deferred<InterviewSnapshot>();
    vi.mocked(fake.client.advanceSession).mockReturnValue(pending.promise);

    const next = await screen.findByRole("button", { name: "Próxima pergunta" });
    act(() => {
      fireEvent.click(next);
      fireEvent.click(next);
    });

    expect(fake.client.advanceSession).toHaveBeenCalledTimes(1);

    await act(async () => {
      pending.resolve(
        snapshot({
          revision: 10,
          activeSession: sessionSummary({
            position: 3,
            count: 3,
            version: 6,
            confirmedCount: 2,
          }),
          current: makeQuestionDetail({
            question: makeSessionQuestion({ id: "sq-3", position: 3 }),
          }),
        }),
      );
      await pending.promise;
    });
  });

  it("synthesis_failure_still_allows_recording", async () => {
    const user = userEvent.setup();
    const base = interviewSnapshot({ recordings: [] });
    const { fake } = renderPage(base);
    vi.mocked(fake.client.playAudio).mockRejectedValue(
      appError("WORKER_UNAVAILABLE", "A voz local não está disponível."),
    );

    await user.click(await screen.findByRole("button", { name: "Ouvir pergunta" }));

    expect(
      await screen.findByText("A voz local não está disponível."),
    ).toBeInTheDocument();
    expect(screen.getByText(QUESTION_TEXT)).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Iniciar gravação" }),
    ).toBeEnabled();
  });

  it("failed_publication_keeps_the_take_retryable", async () => {
    const user = userEvent.setup();
    const base = interviewSnapshot({ recordingId: "recording-2" });
    const page = renderPage(base);
    vi.mocked(page.fake.client.stopRecording).mockRejectedValue(
      appError(
        "INVALID_AUDIO",
        "O áudio original não pôde ser guardado; nada foi publicado.",
      ),
    );

    await user.click(
      await screen.findByRole("button", { name: "Terminar gravação" }),
    );
    expect(
      await screen.findByText(
        "O áudio original não pôde ser guardado; nada foi publicado.",
      ),
    ).toBeInTheDocument();

    // A fotografia publicada depois da falha relê a sessão: a tomada reservada
    // aparece na pergunta atual e o identificador pendente fica exposto.
    const reserved = makeRecording({
      id: "recording-2",
      sessionQuestionId: "sq-2",
      ordinal: 2,
      state: "reserved",
      capture: null,
      originalFile: null,
    });
    page.publish(
      interviewSnapshot({
        revision: 12,
        recordingId: null,
        pendingRecordingId: "recording-2",
        question: { selectedRecordingId: null },
        recordings: [reserved],
      }),
    );

    expect(
      await screen.findByText("Reservada"),
    ).toBeInTheDocument();
    const retry = screen.getByRole("button", {
      name: "Repetir guardar gravação",
    });
    await user.click(retry);

    expect(page.fake.client.retrySaveRecording).toHaveBeenCalledWith(
      "recording-2",
    );
  });

  it("review_is_read_only_while_a_new_take_is_recording", async () => {
    const user = userEvent.setup();
    const initial = recordedSnapshot("My first automatic answer.");
    const page = renderPage(initial);

    const editor = await screen.findByLabelText("Revisão editável");
    expect(editor).not.toHaveAttribute("readonly");
    await user.clear(editor);
    await user.type(editor, "edited before the new take");
    expect(editor).toHaveValue("edited before the new take");

    // A captura começou: a seleção pode passar para a tomada nova e o núcleo
    // deixaria de conseguir guardar o texto desta. O editor fecha, para não
    // ficar texto inacessível.
    page.publish(
      interviewSnapshot({
        revision: 4,
        recordingId: "recording-2",
        recordings: initial.current?.recordings ?? [],
      }),
    );

    const locked = screen.getByLabelText("Revisão editável");
    expect(locked).toHaveAttribute("readonly");
    expect(locked).toHaveValue("edited before the new take");
  });

  it("local_edit_adopts_the_review_version_created_by_the_core", async () => {
    const user = userEvent.setup();
    const initial = interviewSnapshot({
      recordings: [
        makeRecording({
          id: "recording-1",
          sessionQuestionId: "sq-2",
          transcripts: [],
        }),
      ],
    });
    const page = renderPage(initial);

    const editor = await screen.findByLabelText("Revisão editável");
    await user.type(editor, "edited before the transcript");

    // O núcleo criou sozinho a revisão 1 ao terminar a transcrição. O rascunho
    // tem de adotar essa versão sem perder o texto editado, senão os pedidos
    // seguintes ficam presos na versão zero.
    page.publish(reviewedSnapshot(initial, "My automatic answer."));

    expect(screen.getByLabelText("Revisão editável")).toHaveValue(
      "edited before the transcript",
    );

    await user.click(screen.getByRole("button", { name: "Guardar revisão" }));

    await waitFor(() =>
      expect(page.fake.client.saveReview).toHaveBeenCalledTimes(1),
    );
    const request = vi.mocked(page.fake.client.saveReview).mock.calls[0][0];
    expect(request.expectedReviewVersion).toBe(1);
    expect(request.text).toBe("edited before the transcript");
  });
});

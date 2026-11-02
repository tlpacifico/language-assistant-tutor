import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useCallback, useState } from "react";
import { describe, expect, it, vi } from "vitest";

import type { InterviewClient } from "../../lib/interview-client";
import type {
  InterviewSnapshot,
  QuestionDetail,
  RecordingView,
} from "../../lib/interview-contracts";
import {
  appError,
  fakeInterviewClient,
  makeQuestionDetail,
  makeRecording,
  makeSessionQuestion,
  makeTranscript,
  sessionSummary,
  snapshot,
  type FakeInterviewClient,
} from "../../test/fake-interview-client";
import NavigationGuard from "./NavigationGuard";
import type { GuardIntention, GuardRequest } from "./NavigationGuard";
import { useInterview } from "../interview/useInterview";

const AUTOMATIC_TEXT = "Initial automatic answer.";

/** Fotografia com uma resposta transcrita e ainda por confirmar. */
function recordedSnapshot(): InterviewSnapshot {
  const recordings: RecordingView[] = [
    makeRecording({
      id: "recording-1",
      sessionQuestionId: "sq-2",
      transcripts: [
        makeTranscript({ id: "transcript-1", text: AUTOMATIC_TEXT }),
      ],
    }),
  ];
  return snapshot({
    revision: 3,
    activeSession: sessionSummary({ version: 5 }),
    current: makeQuestionDetail({
      question: makeSessionQuestion({
        id: "sq-2",
        selectedRecordingId: "recording-1",
      }),
      recordings,
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
  const updated: QuestionDetail = {
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
  };
  return snapshot({ ...base, revision: base.revision + 1, current: updated });
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

/**
 * Ensaio da decisão partilhada: o editor usa o rascunho real do hook e o guard
 * recebe o mesmo estado que a aplicação lhe entrega.
 */
function GuardHarness({
  client,
  initial,
  intention = "close",
  busy = false,
  proceed,
}: {
  client: InterviewClient;
  initial: InterviewSnapshot;
  intention?: GuardIntention;
  busy?: boolean;
  proceed: () => void;
}) {
  const [current, setCurrent] = useState(initial);
  const [request, setRequest] = useState<GuardRequest | null>(null);
  const apply = useCallback((next: InterviewSnapshot) => {
    setCurrent((previous) =>
      next.revision < previous.revision ? previous : next,
    );
  }, []);
  const review = useInterview(client, current, apply);

  return (
    <div>
      <label htmlFor="draft">Revisão</label>
      <textarea
        id="draft"
        value={review.text}
        onChange={(event) => review.setReviewText(event.target.value)}
      />
      <p role="status">
        Rascunho: {review.dirty ? "por guardar" : "guardado"}
      </p>
      <button
        type="button"
        onClick={() =>
          setRequest({
            intention,
            proceed: () => {
              proceed();
              setRequest(null);
            },
          })
        }
      >
        Pedir decisão
      </button>
      <NavigationGuard
        request={request}
        busy={busy}
        dirty={review.dirty}
        saving={review.saving}
        saveError={review.error}
        onSave={review.saveReview}
        onDiscard={review.discardReview}
        onCancel={() => setRequest(null)}
      />
    </div>
  );
}

function renderGuard(options: {
  initial?: InterviewSnapshot;
  intention?: GuardIntention;
  busy?: boolean;
  configure?: (fake: FakeInterviewClient) => void;
}) {
  const initial = options.initial ?? recordedSnapshot();
  const fake = fakeInterviewClient({ initial });
  options.configure?.(fake);
  const proceed = vi.fn();
  render(
    <GuardHarness
      client={fake.client}
      initial={initial}
      intention={options.intention}
      busy={options.busy}
      proceed={proceed}
    />,
  );
  return { proceed, fake };
}

describe("NavigationGuard", () => {
  it("save_and_leave_keeps_newer_edit", async () => {
    const user = userEvent.setup();
    const initial = recordedSnapshot();
    const pending = deferred<InterviewSnapshot>();
    const { fake, proceed } = renderGuard({
      initial,
      intention: "navigate",
      configure: (fakeClient) => {
        vi.mocked(fakeClient.client.saveReview).mockReturnValue(pending.promise);
      },
    });

    const editor = screen.getByLabelText("Revisão");
    await user.clear(editor);
    await user.type(editor, "First version.");
    await user.click(screen.getByRole("button", { name: "Pedir decisão" }));
    expect(screen.getByText("Revisão por guardar")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Guardar e sair" }));
    expect(fake.client.saveReview).toHaveBeenCalledTimes(1);

    // Edição durante a escrita: a versão nova continua por guardar.
    await user.type(editor, " Second version.");

    await act(async () => {
      pending.resolve(reviewedSnapshot(initial, "First version.", 1));
      await pending.promise;
    });

    expect(proceed).not.toHaveBeenCalled();
    expect(screen.getByText("Revisão por guardar")).toBeInTheDocument();
    expect(screen.getByLabelText("Revisão")).toHaveValue(
      "First version. Second version.",
    );
    expect(screen.getByText("Rascunho: por guardar")).toBeInTheDocument();
  });

  it("failed_save_does_not_destroy_window", async () => {
    const user = userEvent.setup();
    const { fake, proceed } = renderGuard({
      initial: recordedSnapshot(),
      configure: (fakeClient) => {
        vi.mocked(fakeClient.client.saveReview).mockRejectedValue(
          appError(
            "STORAGE_WRITE_FAILED",
            "Não foi possível guardar a revisão.",
          ),
        );
      },
    });

    const editor = screen.getByLabelText("Revisão");
    await user.clear(editor);
    await user.type(editor, "Text kept after the failure.");
    await user.click(screen.getByRole("button", { name: "Pedir decisão" }));
    await user.click(screen.getByRole("button", { name: "Guardar e fechar" }));

    expect(fake.client.saveReview).toHaveBeenCalledTimes(1);
    expect(proceed).not.toHaveBeenCalled();

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Não foi possível guardar a revisão.");
    expect(screen.getByLabelText("Revisão")).toHaveValue(
      "Text kept after the failure.",
    );
    // A janela mantém-se e a decisão continua disponível para repetir.
    expect(screen.getByText("Revisão por guardar")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Guardar e fechar" }),
    ).toBeEnabled();
  });

  it("cancel_keeps_editor", async () => {
    const user = userEvent.setup();
    const { proceed } = renderGuard({ initial: recordedSnapshot() });

    const editor = screen.getByLabelText("Revisão");
    await user.clear(editor);
    await user.type(editor, "Edited but not saved.");
    await user.click(screen.getByRole("button", { name: "Pedir decisão" }));
    await user.click(screen.getByRole("button", { name: "Cancelar" }));

    expect(screen.queryByText("Revisão por guardar")).toBeNull();
    expect(screen.getByLabelText("Revisão")).toHaveValue(
      "Edited but not saved.",
    );
    expect(proceed).not.toHaveBeenCalled();
  });

  it("discard_requires_explicit_choice", async () => {
    const user = userEvent.setup();
    const { proceed } = renderGuard({ initial: recordedSnapshot() });

    const editor = screen.getByLabelText("Revisão");
    await user.clear(editor);
    await user.type(editor, "Discard this text.");
    await user.click(screen.getByRole("button", { name: "Pedir decisão" }));

    // Abrir a decisão não descarta nada.
    expect(screen.getByLabelText("Revisão")).toHaveValue("Discard this text.");
    expect(proceed).not.toHaveBeenCalled();

    await user.click(
      screen.getByRole("button", { name: "Descartar e fechar" }),
    );

    expect(proceed).toHaveBeenCalledTimes(1);
    await waitFor(() =>
      expect(screen.getByLabelText("Revisão")).toHaveValue(AUTOMATIC_TEXT),
    );
  });

  it("capture_blocks_navigation", async () => {
    const user = userEvent.setup();
    const { proceed } = renderGuard({
      initial: recordedSnapshot(),
      intention: "navigate",
      busy: true,
    });

    await user.click(screen.getByRole("button", { name: "Pedir decisão" }));

    expect(screen.getByRole("alert")).toHaveTextContent(
      "Não é possível mudar de página enquanto a gravação está a ser capturada ou finalizada.",
    );
    expect(
      screen.queryByRole("button", { name: "Guardar e sair" }),
    ).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Descartar e sair" }),
    ).toBeNull();
    expect(proceed).not.toHaveBeenCalled();
  });
});

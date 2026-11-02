import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type {
  AppError,
  AppSnapshot,
  Attempt,
  Availability,
  ComponentState,
  PlaybackTarget,
  Transcript,
} from "../../lib/contracts";
import { INITIAL_QUESTION } from "../../lib/contracts";
import type { PracticeClient } from "../../lib/tauri";
import Practice from "./Practice";

/**
 * A janela Tauri não existe no jsdom. O teste captura o handler registado por
 * `onCloseRequested` e controla o `destroy`, sem abrir uma janela real.
 */
const windowMock = vi.hoisted(() => ({
  onCloseRequested: vi.fn(),
  destroy: vi.fn(),
  handler: null as ((event: { preventDefault: () => void }) => void) | null,
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    onCloseRequested: windowMock.onCloseRequested,
    destroy: windowMock.destroy,
  }),
}));

const STORAGE_ERROR: AppError = {
  code: "STORAGE_WRITE_FAILED",
  message: "Não foi possível guardar a revisão no disco.",
  retryable: true,
};

const PREPARE_CLOSE_ERROR: AppError = {
  code: "STORAGE_WRITE_FAILED",
  message: "Não foi possível preparar o fecho sem perder dados.",
  retryable: true,
};

const readyComponent = (): ComponentState => ({ status: "ready", message: null });

function readyAvailability(): Availability {
  return {
    microphone: readyComponent(),
    playback: readyComponent(),
    transcription: readyComponent(),
    synthesis: readyComponent(),
  };
}

function snapshot(overrides: Partial<AppSnapshot> = {}): AppSnapshot {
  return {
    revision: 1,
    question: INITIAL_QUESTION,
    availability: readyAvailability(),
    attempt: null,
    recordingId: null,
    modelJob: null,
    playback: null,
    lastError: null,
    ...overrides,
  };
}

function transcript(overrides: Partial<Transcript> = {}): Transcript {
  return {
    id: "transcript-1",
    status: "completed",
    text: "I solved a difficult bug in the billing service.",
    segments: [],
    model: "small",
    language: "en",
    createdAt: "2026-11-01T10:00:00Z",
    loadMs: 120,
    processingMs: 420,
    ...overrides,
  };
}

function attempt(overrides: Partial<Attempt> = {}): Attempt {
  return {
    id: "attempt-1",
    question: INITIAL_QUESTION,
    createdAt: "2026-11-01T10:00:00Z",
    capture: {
      sampleRate: 48_000,
      channels: 1,
      durationMs: 2_500,
      bytes: 240_000,
      incomplete: false,
    },
    originalFile: "original.wav",
    normalizedFile: "normalized.wav",
    transcripts: [transcript()],
    review: null,
    lastOperation: null,
    ...overrides,
  };
}

function createFakeClient(initial: AppSnapshot) {
  let listener: ((next: AppSnapshot) => void) | null = null;

  const mocks = {
    snapshot: vi.fn(async () => initial),
    refresh: vi.fn(async () => initial),
    startRecording: vi.fn(async () => initial),
    stopRecording: vi.fn(async () => initial),
    retrySaveRecording: vi.fn(async () => initial),
    transcribe: vi.fn(async (_attemptId: string) => initial),
    play: vi.fn(async (_target: PlaybackTarget) => initial),
    stop: vi.fn(async () => initial),
    saveReview: vi.fn(
      async (_attemptId: string, _sourceId: string, _text: string) => initial,
    ),
    restartWorker: vi.fn(async () => initial),
    prepareClose: vi.fn(async () => initial),
    subscribe: vi.fn((next: (value: AppSnapshot) => void) => {
      listener = next;
      return () => {
        listener = null;
      };
    }),
  };

  const client: PracticeClient = {
    snapshot: mocks.snapshot,
    refresh: mocks.refresh,
    startRecording: mocks.startRecording,
    stopRecording: mocks.stopRecording,
    retrySaveRecording: mocks.retrySaveRecording,
    transcribe: mocks.transcribe,
    play: mocks.play,
    stop: mocks.stop,
    saveReview: mocks.saveReview,
    restartWorker: mocks.restartWorker,
    prepareClose: mocks.prepareClose,
    subscribe: mocks.subscribe,
  };

  return {
    client,
    mocks,
    emit(next: AppSnapshot) {
      listener?.(next);
    },
  };
}

async function renderPractice(initial: AppSnapshot) {
  const fake = createFakeClient(initial);
  const user = userEvent.setup();
  render(<Practice client={fake.client} />);
  await screen.findByRole("button", { name: "Iniciar gravação" });
  return { ...fake, user };
}

function reviewEditor() {
  return screen.getByLabelText("Revisão da transcrição");
}

beforeEach(() => {
  windowMock.handler = null;
  windowMock.onCloseRequested.mockReset();
  windowMock.onCloseRequested.mockImplementation(
    (handler: (event: { preventDefault: () => void }) => void) => {
      windowMock.handler = handler;
      return Promise.resolve(() => {});
    },
  );
  windowMock.destroy.mockReset();
  windowMock.destroy.mockResolvedValue(undefined);
});

describe("Practice", () => {
  it("shows_the_fixed_question", async () => {
    await renderPractice(snapshot({ revision: 1 }));

    const question = screen.getByRole("heading", { level: 1 });
    expect(question).toHaveTextContent(
      "Tell me about a time you solved a difficult problem at work.",
    );
    expect(question).toHaveAttribute("lang", "en");
  });

  it("manual_start_stop", async () => {
    const finished = attempt();
    const { client, mocks, user } = await renderPractice(snapshot({ revision: 1 }));

    mocks.startRecording.mockResolvedValueOnce(
      snapshot({ revision: 2, recordingId: "recording-1" }),
    );
    mocks.stopRecording.mockResolvedValueOnce(
      snapshot({ revision: 3, attempt: finished }),
    );

    await user.click(screen.getByRole("button", { name: "Iniciar gravação" }));

    const stop = await screen.findByRole("button", { name: "Terminar gravação" });
    expect(stop).toBeEnabled();
    expect(mocks.startRecording).toHaveBeenCalledTimes(1);
    expect(screen.getByText("Gravação em curso")).toBeInTheDocument();

    await user.click(stop);

    await screen.findByRole("button", { name: "Iniciar gravação" });
    expect(mocks.stopRecording).toHaveBeenCalledTimes(1);
    expect(screen.getByText("Duração da gravação: 2,5 s")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Ouvir resposta" })).toBeEnabled();
    expect(reviewEditor()).toHaveValue(finished.transcripts[0].text);
    expect(client.stopRecording).toHaveBeenCalledTimes(1);
  });

  it("playback_is_allowed_while_transcribing", async () => {
    await renderPractice(
      snapshot({
        revision: 2,
        attempt: attempt(),
        modelJob: {
          id: "job-1",
          attemptId: "attempt-1",
          kind: "transcribe",
          stage: "processing",
        },
      }),
    );

    expect(screen.getByRole("button", { name: "Ouvir pergunta" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Ouvir resposta" })).toBeEnabled();
    // Transcrever continua a impedir outra gravação e outro trabalho de modelo.
    expect(screen.getByRole("button", { name: "Iniciar gravação" })).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "Transcrever resposta" }),
    ).toBeDisabled();
  });

  it("playback_is_refused_during_synthesis", async () => {
    await renderPractice(
      snapshot({
        revision: 2,
        attempt: attempt(),
        modelJob: {
          id: "job-2",
          attemptId: null,
          kind: "synthesize",
          stage: "loading",
        },
      }),
    );

    expect(screen.getByRole("button", { name: "Ouvir pergunta" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Ouvir resposta" })).toBeDisabled();
  });

  it("starting_recording_stops_playback", async () => {
    const { mocks, user } = await renderPractice(
      snapshot({ revision: 2, attempt: attempt(), playback: "response" }),
    );

    // A reprodução não obriga a carregar em «Parar áudio» primeiro: o núcleo
    // pára a saída ao abrir o microfone.
    const start = screen.getByRole("button", { name: "Iniciar gravação" });
    expect(start).toBeEnabled();
    await user.click(start);
    expect(mocks.startRecording).toHaveBeenCalledTimes(1);
  });

  it("refresh_availability_is_reachable", async () => {
    const { mocks, user } = await renderPractice(snapshot({ revision: 1 }));

    await user.click(
      screen.getByRole("button", { name: "Verificar disponibilidade" }),
    );
    expect(mocks.refresh).toHaveBeenCalledTimes(1);
  });

  it("shows_loading_without_freezing_editor", async () => {
    const { mocks, user } = await renderPractice(
      snapshot({
        revision: 2,
        attempt: attempt(),
        modelJob: {
          id: "job-1",
          attemptId: "attempt-1",
          kind: "transcribe",
          stage: "loading",
        },
      }),
    );

    expect(
      screen.getByText("A carregar o modelo de transcrição…"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Transcrever resposta" }),
    ).toBeDisabled();
    expect(screen.getByRole("button", { name: "Iniciar gravação" })).toBeDisabled();

    const editor = reviewEditor();
    expect(editor).not.toHaveAttribute("readonly");
    await user.clear(editor);
    await user.type(editor, "edited while the model loads");
    expect(editor).toHaveValue("edited while the model loads");
    expect(mocks.transcribe).not.toHaveBeenCalled();
  });

  it("raw_transcript_is_readonly", async () => {
    const automatic = transcript({ text: "raw words from the engine" });
    const { user } = await renderPractice(
      snapshot({ revision: 2, attempt: attempt({ transcripts: [automatic] }) }),
    );

    const raw = screen.getByLabelText("Transcrição automática");
    expect(raw).toHaveAttribute("readonly");
    expect(raw).toHaveValue("raw words from the engine");
    expect(reviewEditor()).toHaveValue("raw words from the engine");

    await user.clear(reviewEditor());
    await user.type(reviewEditor(), "my version");

    expect(reviewEditor()).toHaveValue("my version");
    expect(screen.getByLabelText("Transcrição automática")).toHaveValue(
      "raw words from the engine",
    );
    expect(screen.getByText("Alterações por guardar")).toBeInTheDocument();
  });

  it("save_failure_keeps_dirty_text", async () => {
    const { mocks, user } = await renderPractice(
      snapshot({ revision: 2, attempt: attempt() }),
    );
    mocks.saveReview.mockRejectedValueOnce(STORAGE_ERROR);

    const editor = reviewEditor();
    await user.clear(editor);
    await user.type(editor, "my edited words");

    await user.click(screen.getByRole("button", { name: "Guardar revisão" }));

    await screen.findByText(STORAGE_ERROR.message);
    expect(screen.getByLabelText("Revisão da transcrição")).toHaveValue(
      "my edited words",
    );
    expect(screen.getByText("Alterações por guardar")).toBeInTheDocument();
    expect(
      screen.queryByText("Revisão guardada"),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Guardar revisão" })).toBeEnabled();
    expect(mocks.saveReview).toHaveBeenCalledWith(
      "attempt-1",
      "transcript-1",
      "my edited words",
    );

    // Uma gravação nova com a revisão por guardar só começa depois da decisão;
    // uma escrita falhada mantém o texto e não inicia a captura.
    mocks.saveReview.mockRejectedValueOnce(STORAGE_ERROR);
    await user.click(screen.getByRole("button", { name: "Iniciar gravação" }));
    await user.click(
      screen.getByRole("button", { name: "Guardar e iniciar gravação" }),
    );

    await waitFor(() => expect(mocks.saveReview).toHaveBeenCalledTimes(2));
    expect(mocks.startRecording).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog", { name: "Revisão por guardar" })).toBeInTheDocument();
    expect(reviewEditor()).toHaveValue("my edited words");
    expect(screen.getByText(STORAGE_ERROR.message)).toBeInTheDocument();
  });

  it("new_recording_with_dirty_review", async () => {
    const active = attempt();
    const { client, mocks, user } = await renderPractice(
      snapshot({ revision: 2, attempt: active }),
    );
    mocks.saveReview.mockResolvedValueOnce(
      snapshot({
        revision: 3,
        attempt: {
          ...active,
          review: {
            sourceTranscriptId: "transcript-1",
            text: "my edited words",
            savedAt: "2026-11-01T10:05:00Z",
          },
        },
      }),
    );
    mocks.startRecording.mockResolvedValueOnce(
      snapshot({ revision: 4, recordingId: "recording-2", attempt: active }),
    );

    await user.clear(reviewEditor());
    await user.type(reviewEditor(), "my edited words");
    await user.click(screen.getByRole("button", { name: "Iniciar gravação" }));

    const dialog = screen.getByRole("dialog", { name: "Revisão por guardar" });
    expect(dialog).toBeInTheDocument();
    expect(client.startRecording).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Cancelar" }));
    expect(screen.queryByRole("dialog", { name: "Revisão por guardar" })).not.toBeInTheDocument();
    expect(client.startRecording).not.toHaveBeenCalled();
    expect(mocks.saveReview).not.toHaveBeenCalled();
    expect(reviewEditor()).toHaveValue("my edited words");

    await user.click(screen.getByRole("button", { name: "Iniciar gravação" }));
    await user.click(
      screen.getByRole("button", { name: "Guardar e iniciar gravação" }),
    );

    await screen.findByRole("button", { name: "Terminar gravação" });
    expect(mocks.saveReview).toHaveBeenCalledWith(
      "attempt-1",
      "transcript-1",
      "my edited words",
    );
    expect(client.startRecording).toHaveBeenCalledTimes(1);
    expect(mocks.saveReview.mock.invocationCallOrder[0]).toBeLessThan(
      mocks.startRecording.mock.invocationCallOrder[0],
    );
    expect(screen.queryByRole("dialog", { name: "Revisão por guardar" })).not.toBeInTheDocument();
  });

  it("discard_dirty_review_starts_recording", async () => {
    const active = attempt();
    const { client, mocks, user } = await renderPractice(
      snapshot({ revision: 2, attempt: active }),
    );
    mocks.startRecording.mockResolvedValueOnce(
      snapshot({ revision: 3, recordingId: "recording-3", attempt: active }),
    );

    await user.clear(reviewEditor());
    await user.type(reviewEditor(), "temporary words");
    await user.click(screen.getByRole("button", { name: "Iniciar gravação" }));
    await user.click(
      screen.getByRole("button", { name: "Descartar e iniciar gravação" }),
    );

    await screen.findByRole("button", { name: "Terminar gravação" });
    expect(mocks.saveReview).not.toHaveBeenCalled();
    expect(client.startRecording).toHaveBeenCalledTimes(1);
    expect(reviewEditor()).toHaveValue(active.transcripts[0].text);
  });

  it("dirty_review_is_not_attached_to_a_new_attempt", async () => {
    const first = attempt({
      id: "attempt-1",
      transcripts: [
        transcript({ id: "transcript-1", text: "first attempt text" }),
      ],
    });
    const second = attempt({
      id: "attempt-2",
      transcripts: [
        transcript({ id: "transcript-2", text: "second attempt text" }),
      ],
    });
    const { mocks, user } = await renderPractice(
      snapshot({ revision: 1, attempt: first }),
    );

    mocks.startRecording.mockResolvedValueOnce(
      snapshot({ revision: 2, attempt: first, recordingId: "recording-1" }),
    );
    mocks.stopRecording.mockResolvedValueOnce(
      snapshot({ revision: 3, attempt: second }),
    );

    await user.click(screen.getByRole("button", { name: "Iniciar gravação" }));
    await screen.findByRole("button", { name: "Terminar gravação" });

    // A edição acontece durante a captura, enquanto a resposta A ainda é a
    // ativa. A resposta B só aparece depois de terminar.
    await user.clear(reviewEditor());
    await user.type(reviewEditor(), "draft written during the first answer");

    await user.click(screen.getByRole("button", { name: "Terminar gravação" }));
    await screen.findByRole("button", { name: "Iniciar gravação" });

    expect(screen.getByLabelText("Transcrição automática")).toHaveValue(
      "second attempt text",
    );
    expect(reviewEditor()).toHaveValue("draft written during the first answer");
    expect(
      screen.getByText("Revisão de outra resposta por guardar"),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Guardar revisão" })).toBeDisabled();
    expect(mocks.saveReview).not.toHaveBeenCalled();
    expect(mocks.saveReview).not.toHaveBeenCalledWith(
      "attempt-2",
      "transcript-2",
      "draft written during the first answer",
    );

    // Descartar a revisão antiga reinicia a cópia a partir da resposta B.
    await user.click(
      screen.getByRole("button", { name: "Descartar revisão anterior" }),
    );

    expect(reviewEditor()).toHaveValue("second attempt text");
    expect(screen.getByText("Revisão por guardar")).toBeInTheDocument();
    expect(mocks.saveReview).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Guardar revisão" }));
    await waitFor(() => expect(mocks.saveReview).toHaveBeenCalledTimes(1));
    expect(mocks.saveReview).toHaveBeenCalledWith(
      "attempt-2",
      "transcript-2",
      "second attempt text",
    );
  });

  it("close_with_dirty_review", async () => {
    const active = attempt();
    const { client, mocks, user } = await renderPractice(
      snapshot({ revision: 2, attempt: active }),
    );

    await user.clear(reviewEditor());
    await user.type(reviewEditor(), "my edited words");
    await waitFor(() => expect(windowMock.handler).not.toBeNull());

    const closeEvent = { preventDefault: vi.fn() };
    act(() => {
      windowMock.handler?.(closeEvent);
    });

    expect(closeEvent.preventDefault).toHaveBeenCalledTimes(1);
    expect(client.prepareClose).not.toHaveBeenCalled();
    expect(windowMock.destroy).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog", { name: "Revisão por guardar" })).toBeInTheDocument();

    // Cancelar mantém a janela, os dados e a revisão editável.
    await user.click(screen.getByRole("button", { name: "Cancelar" }));
    expect(screen.queryByRole("dialog", { name: "Revisão por guardar" })).not.toBeInTheDocument();
    expect(client.prepareClose).not.toHaveBeenCalled();
    expect(windowMock.destroy).not.toHaveBeenCalled();
    expect(reviewEditor()).toHaveValue("my edited words");

    // Uma escrita falhada mantém a janela aberta, o texto e o erro.
    mocks.saveReview.mockRejectedValueOnce(STORAGE_ERROR);
    act(() => {
      windowMock.handler?.(closeEvent);
    });
    await user.click(screen.getByRole("button", { name: "Guardar e fechar" }));

    await screen.findByText(STORAGE_ERROR.message);
    expect(client.prepareClose).not.toHaveBeenCalled();
    expect(windowMock.destroy).not.toHaveBeenCalled();
    expect(reviewEditor()).toHaveValue("my edited words");
    expect(screen.getByRole("dialog", { name: "Revisão por guardar" })).toBeInTheDocument();

    // Guardar com sucesso e preparar o fecho; uma falha no fecho mantém a
    // janela, os dados e permite repetir.
    mocks.saveReview.mockResolvedValueOnce(
      snapshot({
        revision: 3,
        attempt: {
          ...active,
          review: {
            sourceTranscriptId: "transcript-1",
            text: "my edited words",
            savedAt: "2026-11-01T10:06:00Z",
          },
        },
      }),
    );
    mocks.prepareClose.mockRejectedValueOnce(PREPARE_CLOSE_ERROR);

    await user.click(screen.getByRole("button", { name: "Guardar e fechar" }));

    await screen.findByText(PREPARE_CLOSE_ERROR.message);
    expect(windowMock.destroy).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog", { name: "Revisão por guardar" })).toBeInTheDocument();
    expect(reviewEditor()).toHaveValue("my edited words");

    mocks.saveReview.mockResolvedValueOnce(
      snapshot({
        revision: 4,
        attempt: {
          ...active,
          review: {
            sourceTranscriptId: "transcript-1",
            text: "my edited words",
            savedAt: "2026-11-01T10:07:00Z",
          },
        },
      }),
    );
    mocks.prepareClose.mockResolvedValueOnce(snapshot({ revision: 5, attempt: active }));

    await user.click(screen.getByRole("button", { name: "Guardar e fechar" }));

    await waitFor(() => expect(windowMock.destroy).toHaveBeenCalledTimes(1));
    expect(client.prepareClose).toHaveBeenCalledTimes(2);
    expect(mocks.saveReview).toHaveBeenCalledTimes(3);
    expect(mocks.prepareClose.mock.invocationCallOrder[1]).toBeLessThan(
      windowMock.destroy.mock.invocationCallOrder[0],
    );
  });

  it("older_snapshot_is_ignored", async () => {
    const active = attempt({
      transcripts: [transcript({ text: "first transcript" })],
    });
    const { emit, user } = await renderPractice(
      snapshot({ revision: 5, attempt: active }),
    );

    expect(screen.getByLabelText("Transcrição automática")).toHaveValue(
      "first transcript",
    );

    act(() => {
      emit(
        snapshot({
          revision: 4,
          attempt: attempt({
            transcripts: [transcript({ id: "transcript-0", text: "stale transcript" })],
          }),
        }),
      );
    });

    expect(screen.getByLabelText("Transcrição automática")).toHaveValue(
      "first transcript",
    );
    expect(reviewEditor()).toHaveValue("first transcript");
    expect(
      screen.queryByRole("button", { name: "Terminar gravação" }),
    ).not.toBeInTheDocument();

    // Uma edição local nunca é substituída por uma fotografia mais recente.
    await user.clear(reviewEditor());
    await user.type(reviewEditor(), "my edited words");
    act(() => {
      emit(
        snapshot({
          revision: 6,
          attempt: attempt({
            transcripts: [transcript({ id: "transcript-2", text: "new automatic words" })],
          }),
        }),
      );
    });

    expect(screen.getByLabelText("Transcrição automática")).toHaveValue(
      "new automatic words",
    );
    expect(screen.getByLabelText("Revisão da transcrição")).toHaveValue(
      "my edited words",
    );
  });
});

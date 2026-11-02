/**
 * Cliente controlado para os testes da interface da fase 2.
 *
 * Reproduz a forma de `InterviewClient` sem IPC, microfone, modelos nem base de
 * dados: guarda as perguntas em memória, registra os pedidos recebidos e
 * publica fotografias quando o teste o pede.
 */

import { vi } from "vitest";

import type { AppError, Availability, Transcript } from "../lib/contracts";
import type { InterviewClient } from "../lib/interview-client";
import type {
  Category,
  Confirmation,
  InterviewSnapshot,
  Question,
  QuestionDetail,
  QuestionInput,
  RecordingView,
  SessionQuestion,
  SessionSummary,
  StartSession,
} from "../lib/interview-contracts";

/** Disponibilidade com os quatro componentes prontos. */
export function readyAvailability(): Availability {
  const ready = { status: "ready" as const, message: null };
  return {
    microphone: ready,
    playback: ready,
    transcription: ready,
    synthesis: ready,
  };
}

/** Fotografia pronta por omissão, com substituições pontuais. */
export function snapshot(
  overrides: Partial<InterviewSnapshot> = {},
): InterviewSnapshot {
  return {
    revision: 0,
    database: "ready",
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

/** Pergunta do banco com valores estáveis, substituíveis campo a campo. */
export function makeQuestion(overrides: Partial<Question> = {}): Question {
  return {
    id: overrides.id ?? "q-1",
    text:
      overrides.text ??
      "Tell me about a time you solved a difficult problem at work.",
    category: overrides.category ?? "problem_solving",
    competencies: overrides.competencies ?? ["Resolução de problemas"],
    criteria: overrides.criteria ?? [
      "Explicar o problema, o raciocínio e o desfecho.",
    ],
    version: overrides.version ?? 1,
    createdAt: overrides.createdAt ?? "2026-11-02T10:00:00Z",
    updatedAt: overrides.updatedAt ?? "2026-11-02T10:00:00Z",
  };
}

/** Resumo de sessão em curso, baseado nos valores da spec. */
export function sessionSummary(
  overrides: Partial<SessionSummary> = {},
): SessionSummary {
  return {
    id: overrides.id ?? "session-1",
    category: overrides.category ?? null,
    count: overrides.count ?? 3,
    confirmedCount: overrides.confirmedCount ?? 1,
    position: overrides.position ?? 2,
    status: overrides.status ?? "in_progress",
    mode: "local_v2",
    version: overrides.version ?? 3,
    createdAt: overrides.createdAt ?? "2026-11-02T10:00:00Z",
    completedAt: overrides.completedAt ?? null,
  };
}

/** Erro estável para falhas simuladas, com mensagem em português de Portugal. */
export function appError(
  code: string,
  message: string,
  retryable = true,
): AppError {
  return { code, message, retryable };
}

/** Transcrição automática imutável, substituível campo a campo. */
export function makeTranscript(overrides: Partial<Transcript> = {}): Transcript {
  return {
    id: overrides.id ?? "transcript-1",
    status: overrides.status ?? "completed",
    text:
      overrides.text ??
      "I solved a difficult problem by splitting it into smaller steps.",
    segments: overrides.segments ?? [],
    model: overrides.model ?? "whisper-small",
    language: overrides.language ?? "en",
    createdAt: overrides.createdAt ?? "2026-11-02T10:05:00Z",
    loadMs: overrides.loadMs ?? 120,
    processingMs: overrides.processingMs ?? 900,
  };
}

/** Gravação publicada da pergunta, com os resultados que lhe pertencem. */
export function makeRecording(
  overrides: Partial<RecordingView> = {},
): RecordingView {
  return {
    id: overrides.id ?? "recording-1",
    sessionQuestionId: overrides.sessionQuestionId ?? "sq-1",
    ordinal: overrides.ordinal ?? 1,
    state: overrides.state ?? "ready",
    capture:
      overrides.capture === undefined
        ? {
            sampleRate: 48000,
            channels: 1,
            durationMs: 12500,
            bytes: 2400000,
            incomplete: false,
          }
        : overrides.capture,
    originalFile:
      overrides.originalFile === undefined
        ? "sessions/session-1/sq-1/recording-1/original.wav"
        : overrides.originalFile,
    normalizedFile: overrides.normalizedFile ?? null,
    transcripts: overrides.transcripts ?? [],
    review: overrides.review ?? null,
    warnings: overrides.warnings ?? [],
  };
}

/** Cópia fixada da pergunta na sessão, baseada nos valores da spec. */
export function makeSessionQuestion(
  overrides: Partial<SessionQuestion> = {},
): SessionQuestion {
  return {
    id: overrides.id ?? "sq-1",
    sessionId: overrides.sessionId ?? "session-1",
    sourceQuestionId: overrides.sourceQuestionId ?? "q-1",
    sourceVersion: overrides.sourceVersion ?? 1,
    position: overrides.position ?? 2,
    text:
      overrides.text ??
      "Tell me about a time you solved a difficult problem at work.",
    category: overrides.category ?? "problem_solving",
    competencies: overrides.competencies ?? ["Resolução de problemas"],
    criteria: overrides.criteria ?? [
      "Explicar o problema, o raciocínio e o desfecho.",
    ],
    selectedRecordingId:
      overrides.selectedRecordingId === undefined
        ? "recording-1"
        : overrides.selectedRecordingId,
    selectionVersion: overrides.selectionVersion ?? 2,
  };
}

/** Texto confirmado e as suas referências. */
export function makeConfirmation(
  overrides: Partial<Confirmation> = {},
): Confirmation {
  return {
    id: overrides.id ?? "confirmation-1",
    sessionQuestionId: overrides.sessionQuestionId ?? "sq-1",
    recordingId: overrides.recordingId ?? "recording-1",
    reviewVersion: overrides.reviewVersion ?? 1,
    sourceTranscriptId:
      overrides.sourceTranscriptId === undefined
        ? "transcript-1"
        : overrides.sourceTranscriptId,
    origin: overrides.origin ?? "automatic",
    text: overrides.text ?? "I solved a difficult problem by splitting it.",
    confirmedAt: overrides.confirmedAt ?? "2026-11-02T10:10:00Z",
  };
}

/** Pergunta corrente da entrevista, com gravações e confirmação. */
export function makeQuestionDetail(
  overrides: Partial<QuestionDetail> = {},
): QuestionDetail {
  return {
    question: overrides.question ?? makeSessionQuestion(),
    recordings: overrides.recordings ?? [],
    confirmation: overrides.confirmation ?? null,
  };
}

export interface FakeInterviewClient {
  client: InterviewClient;
  /** Banco em memória, alterado por criações e edições do teste. */
  questions: Question[];
  /** Pedidos de início recebidos, na ordem em que chegaram. */
  starts: StartSession[];
  /** Perguntas criadas, com o texto exatamente como foi enviado. */
  creates: QuestionInput[];
  /** Edições recebidas, com a versão esperada. */
  updates: Array<{ id: string; expectedVersion: number; input: QuestionInput }>;
  /** Publica uma fotografia para o listener ativo. */
  publish(next: InterviewSnapshot): void;
}

/**
 * Cria o cliente falso.
 *
 * `startSession` devolve uma sessão ativa coerente com o pedido; os testes que
 * precisam de falhas substituem o método com `vi.fn` e restauram depois.
 */
export function fakeInterviewClient(options: {
  initial?: InterviewSnapshot;
  questions?: Question[];
} = {}): FakeInterviewClient {
  let current = options.initial ?? snapshot();
  const questions = [...(options.questions ?? [])];
  const starts: StartSession[] = [];
  const creates: QuestionInput[] = [];
  const updates: FakeInterviewClient["updates"] = [];
  let listener: ((snapshot: InterviewSnapshot) => void) | null = null;

  function selected(category: Category | null): Question[] {
    return questions
      .filter((question) => category === null || question.category === category)
      .map((question) => ({ ...question }));
  }

  const client: InterviewClient = {
    snapshot: vi.fn(async () => current),
    listQuestions: vi.fn(async (category) =>
      selected(category ?? null),
    ),
    createQuestion: vi.fn(async (input: QuestionInput) => {
      creates.push(input);
      const created = makeQuestion({
        ...input,
        id: `q-${questions.length + 1}`,
        version: 1,
        updatedAt: "2026-11-02T11:00:00Z",
      });
      questions.push(created);
      return { ...created };
    }),
    updateQuestion: vi.fn(
      async (id: string, expectedVersion: number, input: QuestionInput) => {
        updates.push({ id, expectedVersion, input });
        const index = questions.findIndex((question) => question.id === id);
        if (index < 0) {
          throw appError(
            "STALE_SOURCE",
            "A pergunta já não existe no banco.",
            false,
          );
        }
        if (questions[index].version !== expectedVersion) {
          throw appError(
            "STALE_SOURCE",
            "A pergunta foi alterada entretanto. Recarregue o banco.",
            false,
          );
        }
        const updated = makeQuestion({
          ...questions[index],
          ...input,
          version: expectedVersion + 1,
          updatedAt: "2026-11-02T11:00:00Z",
        });
        questions[index] = updated;
        return { ...updated };
      },
    ),
    startSession: vi.fn(async (request: StartSession) => {
      starts.push(request);
      current = snapshot({
        revision: current.revision + 1,
        activeSession: sessionSummary({
          category: request.category,
          count: request.count,
          confirmedCount: 0,
          position: 1,
          version: 1,
        }),
        current: null,
      });
      return current;
    }),
    listSessions: vi.fn(async () => []),
    loadSession: vi.fn(async () => {
      throw appError("INVALID_INPUT", "Abrir sessão não é usado neste teste.");
    }),
    listLegacy: vi.fn(async () => []),
    loadLegacy: vi.fn(async () => {
      throw appError("INVALID_INPUT", "Abrir treino não é usado neste teste.");
    }),
    startRecording: vi.fn(async () => current),
    stopRecording: vi.fn(async () => current),
    retrySaveRecording: vi.fn(async () => current),
    transcribeResponse: vi.fn(async () => current),
    saveReview: vi.fn(async () => current),
    confirmAnswer: vi.fn(async () => current),
    advanceSession: vi.fn(async () => current),
    playAudio: vi.fn(async () => current),
    stopAudio: vi.fn(async () => current),
    refreshAvailability: vi.fn(async () => current),
    restartWorker: vi.fn(async () => current),
    prepareClose: vi.fn(async () => current),
    retryDatabase: vi.fn(async () => {
      current = snapshot({
        ...current,
        database: "ready",
        lastError: null,
        revision: current.revision + 1,
      });
      return current;
    }),
    subscribe: vi.fn((next: (next: InterviewSnapshot) => void) => {
      listener = next;
      return () => {
        listener = null;
      };
    }),
  };

  return {
    client,
    questions,
    starts,
    creates,
    updates,
    publish(next: InterviewSnapshot) {
      current = next;
      listener?.(next);
    },
  };
}

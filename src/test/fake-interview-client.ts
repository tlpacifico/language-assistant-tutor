/**
 * Cliente controlado para os testes da interface da fase 2.
 *
 * Reproduz a forma de `InterviewClient` sem IPC, microfone, modelos nem base de
 * dados: guarda as perguntas em memória, registra os pedidos recebidos e
 * publica fotografias quando o teste o pede.
 */

import { vi } from "vitest";

import type { AppError, Availability, Transcript } from "../lib/contracts";
import type {
  ComparisonView,
  CreatePractice,
  PracticeDetail,
  PracticeReportView,
  PreparationView,
  RewriteCommand,
  RewritePreference,
  RewriteView,
  SessionUsage,
  StoryInput,
  StoryView,
  TrainingUsageView,
  UsageGroup,
} from "../lib/practice-contracts";
import { originKey } from "../lib/practice-contracts";
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
    pendingRecordingId: null,
    finalizing: false,
    modelJob: null,
    playback: null,
    warnings: [],
    lastError: null,
    practice: {
      revision: 0,
      activePractice: null,
      activeOwner: null,
      pendingPreparation: null,
    },
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
    mode: overrides.mode ?? "local_v2",
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
export function makeTranscript(
  overrides: Partial<Transcript> = {},
): Transcript {
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
    followUp: overrides.followUp,
    followUpStage: overrides.followUpStage,
    initialAnswerId: overrides.initialAnswerId,
    question: overrides.question ?? makeSessionQuestion(),
    recordings: overrides.recordings ?? [],
    confirmation: overrides.confirmation ?? null,
  };
}

/** Consumo vazio mas completo, para vistas sem chamadas de rede. */
export function emptyUsage(overrides: Partial<SessionUsage> = {}): SessionUsage {
  return {
    knownReportedSubtotal: 0,
    knownEstimatedSubtotal: 0,
    unknownAttemptCount: 0,
    input: 0,
    output: 0,
    complete: true,
    ...overrides,
  };
}

/** Grupo de consumo, substituível campo a campo. */
export function usageGroup(overrides: Partial<UsageGroup> = {}): UsageGroup {
  return {
    totals: emptyUsage(),
    networkAttemptCount: 0,
    byPurpose: [],
    ...overrides,
  };
}

/** Consumo de treino sem chamadas. */
export function makeTrainingUsage(
  overrides: Partial<TrainingUsageView> = {},
): TrainingUsageView {
  return {
    sessionId: "session-1",
    originalInterview: usageGroup(),
    laterPractice: usageGroup(),
    combined: usageGroup(),
    ...overrides,
  };
}

/** Preparação editável sem apoio. */
export function makePreparation(
  overrides: Partial<PreparationView> = {},
): PreparationView {
  return {
    answerId: "answer-practice-1",
    version: 0,
    lockState: "editable",
    selectedStoryId: null,
    selectedStoryVersion: null,
    snapshot: null,
    support: "none",
    lockedCaptureId: null,
    ...overrides,
  };
}

/** História local com os quatro campos STAR preenchidos por omissão. */
export function makeStory(overrides: Partial<StoryView> = {}): StoryView {
  const input: StoryInput = {
    title: overrides.title ?? "Migração do pipeline",
    situation: overrides.situation ?? "The deploy pipeline was failing.",
    task: overrides.task ?? "I had to restore it before the release.",
    actions: overrides.actions ?? "I split the jobs and added retries.",
    outcome: overrides.outcome ?? "Deploys became stable.",
    competencies: overrides.competencies ?? ["Resolução de problemas"],
  };
  const missingFields = (
    ["situation", "task", "actions", "outcome"] as const
  ).filter((field) => input[field].trim() === "");
  return {
    ...input,
    id: overrides.id ?? "story-1",
    version: overrides.version ?? 1,
    missingFields: overrides.missingFields ?? [...missingFields],
    createdAt: overrides.createdAt ?? "2026-11-03T09:00:00Z",
    updatedAt: overrides.updatedAt ?? "2026-11-03T09:00:00Z",
  };
}

/** Prática pendente (preparing) sobre a pergunta sq-1 por omissão. */
export function makePractice(
  overrides: Partial<PracticeDetail> = {},
): PracticeDetail {
  const id = overrides.id ?? "practice-1";
  const answerId = overrides.answer?.id ?? `answer-${id}`;
  return {
    id,
    sourceSessionId: overrides.sourceSessionId ?? "session-1",
    sessionQuestionId: overrides.sessionQuestionId ?? "sq-1",
    ordinal: overrides.ordinal ?? 1,
    previousPracticeId: overrides.previousPracticeId ?? null,
    status: overrides.status ?? "preparing",
    version: overrides.version ?? 1,
    createdAt: overrides.createdAt ?? "2026-11-03T10:00:00Z",
    confirmedAt: overrides.confirmedAt ?? null,
    abandonedAt: overrides.abandonedAt ?? null,
    question: overrides.question ?? {
      text: "Tell me about a time you solved a difficult problem at work.",
      category: "problem_solving",
      competencies: ["Resolução de problemas"],
      criteria: ["Explicar o problema, o raciocínio e o desfecho."],
    },
    answer: overrides.answer ?? {
      id: answerId,
      sessionQuestionId: overrides.sessionQuestionId ?? "sq-1",
      role: "attempt",
      text: "",
      selectedRecordingId: null,
      selectionVersion: 0,
      practiceId: id,
    },
    recordings: overrides.recordings ?? [],
    confirmation: overrides.confirmation ?? null,
    preparation:
      overrides.preparation ?? makePreparation({ answerId: answerId }),
  };
}

/** Reformulação pronta, com segmentos e listas distintas. */
export function makeRewrite(overrides: Partial<RewriteView> = {}): RewriteView {
  const level = overrides.level ?? "B1";
  return {
    id: overrides.id === undefined ? `rewrite-${level}` : overrides.id,
    origin: overrides.origin ?? {
      type: "original",
      sessionId: "session-1",
      sessionQuestionId: "sq-1",
    },
    level,
    state: overrides.state ?? "ready",
    segments: overrides.segments ?? [
      {
        id: "s1",
        text: `Suggested ${level} sentence one.`,
        evidence: [
          {
            confirmationId: "confirmation-1",
            role: "initial",
            quote: "I solved a difficult problem",
          },
        ],
      },
      {
        id: "s2",
        text: `Suggested ${level} sentence two.`,
        evidence: [
          {
            confirmationId: "confirmation-1",
            role: "initial",
            quote: "splitting it",
          },
        ],
      },
    ],
    spokenText:
      overrides.spokenText === undefined
        ? `Suggested ${level} sentence one.\n\nSuggested ${level} sentence two.`
        : overrides.spokenText,
    necessaryChanges: overrides.necessaryChanges ?? [
      {
        source: {
          confirmationId: "confirmation-1",
          role: "initial",
          quote: "splitting it",
        },
        segmentIds: ["s2"],
        explanation: "Corrige a concordância do verbo.",
      },
    ],
    optionalStyle: overrides.optionalStyle ?? [
      {
        source: {
          confirmationId: "confirmation-1",
          role: "initial",
          quote: "I solved",
        },
        segmentIds: ["s1"],
        explanation: "Alternativa de estilo opcional.",
      },
    ],
    missingInformation: overrides.missingInformation ?? [
      {
        dimension: "result",
        assessedConfirmationIds: ["confirmation-1"],
        preparationQuestion: "What was the measurable result?",
        explanation: "A resposta não refere o resultado.",
      },
    ],
    versions: overrides.versions ?? {
      rubricVersion: "factual-rewrite-v1",
      promptVersion: "rewrite-v1",
      schemaVersion: "rewrite-v1",
    },
    usage: overrides.usage ?? emptyUsage(),
    networkAttempts: overrides.networkAttempts ?? [],
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
  /** Histórias locais em memória. */
  stories: StoryView[];
  /** Práticas em memória, pela ordem de criação. */
  practices: PracticeDetail[];
  /** Preparações por identificador de resposta. */
  preparations: Map<string, PreparationView>;
  /** Reformulações prontas, reutilizadas por origem e nível. */
  rewrites: RewriteView[];
  /** Relatórios de prática por identificador. */
  reports: Map<string, PracticeReportView>;
  /** Substitui a resposta do pedido de reformulação (por exemplo, com atraso). */
  setRewriteResponder(
    responder: ((request: RewriteCommand) => Promise<RewriteView>) | null,
  ): void;
  /** Define a comparação devolvida por compareResponses. */
  setComparison(view: ComparisonView | null): void;
  setTrainingUsage(view: TrainingUsageView): void;
  /** Publica uma fotografia para o listener ativo. */
  publish(next: InterviewSnapshot): void;
}

/**
 * Cria o cliente falso.
 *
 * `startSession` devolve uma sessão ativa coerente com o pedido; os testes que
 * precisam de falhas substituem o método com `vi.fn` e restauram depois.
 */
export function fakeInterviewClient(
  options: {
    initial?: InterviewSnapshot;
    questions?: Question[];
  } = {},
): FakeInterviewClient {
  let current = options.initial ?? snapshot();
  const questions = [...(options.questions ?? [])];
  const starts: StartSession[] = [];
  const creates: QuestionInput[] = [];
  const updates: FakeInterviewClient["updates"] = [];
  let listener: ((snapshot: InterviewSnapshot) => void) | null = null;
  const stories: StoryView[] = [];
  const practices: PracticeDetail[] = [];
  const preparations = new Map<string, PreparationView>();
  const rewrites: RewriteView[] = [];
  const reports = new Map<string, PracticeReportView>();
  let rewriteResponder:
    | ((request: RewriteCommand) => Promise<RewriteView>)
    | null = null;
  let comparison: ComparisonView | null = null;
  let trainingUsage: TrainingUsageView = makeTrainingUsage();
  let preference: RewritePreference = {
    version: 1,
    level: "B1",
    updatedAt: "2026-11-03T08:00:00Z",
  };

  function publishPractice(detail: PracticeDetail | null): void {
    current = snapshot({
      ...current,
      revision: current.revision + 1,
      practice: {
        revision: current.revision + 1,
        activePractice: detail,
        activeOwner: detail
          ? {
              type: "practice",
              practiceId: detail.id,
              answerId: detail.answer.id,
            }
          : null,
        pendingPreparation: null,
      },
    });
    listener?.(current);
  }

  function selected(category: Category | null): Question[] {
    return questions
      .filter((question) => category === null || question.category === category)
      .map((question) => ({ ...question }));
  }

  const client: InterviewClient = {
    loadAiSettings: vi.fn(async () => ({
      version: 1,
      baseUrl: "https://opencode.ai/inference/go/openai/v1",
      model: "deepseek-v4.1-flash",
      tariff: null,
      credentialConfigured: false,
      consent: null,
    })),
    saveAiSettings: vi.fn(async () => {
      throw new Error("Configuração não simulada");
    }),
    saveAiKey: vi.fn(async () => {
      throw new Error("Credencial não simulada");
    }),
    removeAiKey: vi.fn(async () => {
      throw new Error("Credencial não simulada");
    }),
    acceptAiConsent: vi.fn(async () => {
      throw new Error("Consentimento não simulado");
    }),
    requestFollowUp: vi.fn(async () => {
      throw new Error("Seguimento não simulado");
    }),
    waiveFollowUp: vi.fn(async () => {
      throw new Error("Dispensa não simulada");
    }),
    generateFeedback: vi.fn(async () => {
      throw new Error("Feedback não simulado");
    }),
    evaluateQuestion: vi.fn(async () => {
      throw new Error("Avaliação individual não simulada");
    }),
    retryAiPublication: vi.fn(async () => {
      throw new Error("Publicação não simulada");
    }),
    loadFeedback: vi.fn(async (id) => ({
      sessionId: id,
      state: "local",
      reports: [],
      priorities: [],
      usage: {
        knownReportedSubtotal: 0,
        knownEstimatedSubtotal: 0,
        unknownAttemptCount: 0,
        input: 0,
        output: 0,
        complete: true,
      },
      attempts: [],
    })),

    snapshot: vi.fn(async () => current),
    listQuestions: vi.fn(async (category) => selected(category ?? null)),
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
    preloadModels: vi.fn(async () => current),
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
    listStories: vi.fn(async (competency) => {
      const filter = (competency ?? "").trim().toLowerCase();
      return stories
        .filter(
          (story) =>
            filter === "" ||
            story.competencies.some(
              (entry) => entry.trim().toLowerCase() === filter,
            ),
        )
        .map((story) => ({ ...story }));
    }),
    createStory: vi.fn(async (request) => {
      const created = makeStory({
        ...request.input,
        id: `story-${stories.length + 1}`,
        version: 1,
      });
      stories.push(created);
      return { ...created };
    }),
    updateStory: vi.fn(async (request) => {
      const index = stories.findIndex((story) => story.id === request.id);
      if (index < 0 || stories[index].version !== request.expectedVersion) {
        throw appError(
          "STALE_SOURCE",
          "A história foi alterada entretanto. Recarregue o banco.",
          false,
        );
      }
      stories[index] = makeStory({
        ...request.input,
        id: request.id,
        version: request.expectedVersion + 1,
        updatedAt: "2026-11-03T11:00:00Z",
      });
      return { ...stories[index] };
    }),
    loadPreparation: vi.fn(async (owner) => {
      return (
        preparations.get(owner.answerId) ??
        makePreparation({ answerId: owner.answerId })
      );
    }),
    selectStory: vi.fn(async (request) => {
      const answerId = request.owner.answerId;
      const existing =
        preparations.get(answerId) ?? makePreparation({ answerId });
      if (existing.lockState !== "editable") {
        throw appError(
          "STALE_SOURCE",
          "O apoio já ficou fixado pela primeira gravação e não pode ser alterado.",
          false,
        );
      }
      if (existing.version !== request.expectedPreparationVersion) {
        throw appError(
          "STALE_SOURCE",
          "O apoio foi alterado entretanto. Recarregue a preparação.",
          false,
        );
      }
      preparations.set(answerId, {
        ...existing,
        version: existing.version + 1,
        selectedStoryId: request.storyId,
        selectedStoryVersion: request.expectedStoryVersion,
        support: request.storyId === null ? "none" : "story",
      });
      current = snapshot({ ...current, revision: current.revision + 1 });
      return current;
    }),
    resolvePreparation: vi.fn(async () => current),
    retryPreparationPublication: vi.fn(async () => {
      current = snapshot({
        ...current,
        revision: current.revision + 1,
        practice: { ...current.practice, pendingPreparation: null },
      });
      return current;
    }),
    createPractice: vi.fn(async (request: CreatePractice) => {
      if (current.activeSession) {
        throw appError(
          "STALE_SOURCE",
          "Termine a entrevista em curso antes de praticar.",
          false,
        );
      }
      if (
        practices.some((entry) =>
          ["preparing", "answering"].includes(entry.status),
        )
      ) {
        throw appError(
          "STALE_SOURCE",
          "Já existe uma prática pendente. Confirme-a ou abandone-a antes de criar outra.",
          false,
        );
      }
      const siblings = practices.filter(
        (entry) => entry.sessionQuestionId === request.sessionQuestionId,
      );
      const created = makePractice({
        id: `practice-${practices.length + 1}`,
        sourceSessionId: request.sessionId,
        sessionQuestionId: request.sessionQuestionId,
        ordinal: siblings.length + 1,
        previousPracticeId: request.previousPracticeId,
      });
      practices.push(created);
      publishPractice(created);
      return { ...created };
    }),
    loadPractice: vi.fn(async (practiceId) => {
      const found = practices.find((entry) => entry.id === practiceId);
      if (!found) {
        throw appError("STALE_SOURCE", "A prática já não existe.", false);
      }
      return { ...found };
    }),
    listPractices: vi.fn(async (sessionQuestionId) =>
      practices
        .filter((entry) => entry.sessionQuestionId === sessionQuestionId)
        .map((entry) => ({ ...entry })),
    ),
    abandonPractice: vi.fn(async (request) => {
      const index = practices.findIndex(
        (entry) => entry.id === request.practiceId,
      );
      if (
        index < 0 ||
        !["preparing", "answering"].includes(practices[index].status)
      ) {
        throw appError(
          "STALE_SOURCE",
          "Só uma prática ainda pendente pode ser abandonada.",
          false,
        );
      }
      practices[index] = {
        ...practices[index],
        status: "abandoned",
        version: practices[index].version + 1,
        abandonedAt: "2026-11-03T12:00:00Z",
      };
      publishPractice(null);
      return { ...practices[index] };
    }),
    startPracticeRecording: vi.fn(async () => current),
    savePracticeReview: vi.fn(async () => current),
    confirmPracticeAnswer: vi.fn(async (request) => {
      const owner = request.target.owner;
      const index = practices.findIndex(
        (entry) => owner.type === "practice" && entry.id === owner.practiceId,
      );
      if (index < 0) {
        throw appError("STALE_SOURCE", "A prática já não existe.", false);
      }
      practices[index] = {
        ...practices[index],
        status: "confirmed",
        version: practices[index].version + 1,
        confirmedAt: "2026-11-03T12:30:00Z",
        confirmation: makeConfirmation({
          id: `confirmation-${practices[index].id}`,
          sessionQuestionId: practices[index].sessionQuestionId,
          recordingId: request.target.recordingId,
          reviewVersion: request.expectedReviewVersion,
          sourceTranscriptId: request.sourceTranscriptId,
          origin: request.origin,
          text: request.text,
        }),
      };
      publishPractice(null);
      return { ...practices[index] };
    }),
    transcribePracticeResponse: vi.fn(async () => current),
    evaluatePractice: vi.fn(async (request) => {
      const report = reports.get(request.practiceId);
      if (!report) {
        throw new Error("Avaliação não simulada");
      }
      return report;
    }),
    loadPracticeReport: vi.fn(async (practiceId) => {
      return (
        reports.get(practiceId) ?? {
          practiceId,
          state: "pending" as const,
          report: null,
          rubricVersion: null,
          promptVersion: null,
          schemaVersion: null,
          usage: emptyUsage(),
          networkAttempts: [],
        }
      );
    }),
    requestRewrite: vi.fn(async (request: RewriteCommand) => {
      if (rewriteResponder) {
        const made = await rewriteResponder(request);
        if (made.state === "ready") {
          rewrites.push(made);
        }
        return made;
      }
      const reused = rewrites.find(
        (entry) =>
          originKey(entry.origin) === originKey(request.origin) &&
          entry.level === request.level,
      );
      if (reused) {
        return { ...reused };
      }
      const made = makeRewrite({
        origin: request.origin,
        level: request.level,
      });
      rewrites.push(made);
      return { ...made };
    }),
    listRewrites: vi.fn(async (origin) =>
      rewrites
        .filter((entry) => originKey(entry.origin) === originKey(origin))
        .map((entry) => ({ ...entry })),
    ),
    compareResponses: vi.fn(async () => {
      if (!comparison) {
        throw new Error("Comparação não simulada");
      }
      return comparison;
    }),
    loadTrainingUsage: vi.fn(async () => trainingUsage),
    loadRewritePreference: vi.fn(async () => ({ ...preference })),
    setRewriteLevel: vi.fn(async (request) => {
      if (request.expectedVersion !== preference.version) {
        throw appError(
          "STALE_SOURCE",
          "A preferência foi alterada entretanto.",
          false,
        );
      }
      preference = {
        version: preference.version + 1,
        level: request.level,
        updatedAt: "2026-11-03T12:00:00Z",
      };
      return { ...preference };
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
    stories,
    practices,
    preparations,
    rewrites,
    reports,
    setRewriteResponder(responder) {
      rewriteResponder = responder;
    },
    setComparison(view) {
      comparison = view;
    },
    setTrainingUsage(view) {
      trainingUsage = view;
    },
    publish(next: InterviewSnapshot) {
      current = next;
      listener?.(next);
    },
  };
}

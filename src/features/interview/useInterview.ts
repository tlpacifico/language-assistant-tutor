import { activeAnswer } from "../../lib/interview-contracts";
/**
 * Estado local da revisão de uma resposta da entrevista guiada.
 *
 * O núcleo Rust continua a ser a autoridade da sessão, da seleção e das
 * versões; este hook guarda apenas o rascunho editável da gravação corrente e
 * o estado das escritas. Os rascunhos são mantidos por `recordingId`, para que
 * consultar o histórico durante uma inferência não apague trabalho nem o
 * aplique a outra tomada.
 *
 * Regras de proteção:
 * - Uma fotografia com revisão inferior à conhecida é ignorada.
 * - Uma fotografia nunca substitui uma edição local por guardar.
 * - Uma edição feita durante uma escrita mantém a versão nova por guardar.
 * - Uma confirmação recebida torna o texto apenas de leitura.
 */

import { useCallback, useLayoutEffect, useReducer, useRef } from "react";

import type { AppError, Transcript } from "../../lib/contracts";
import { toAppError } from "../../lib/app-error";
import type { InterviewClient } from "../../lib/interview-client";
import type {
  AnswerTarget,
  ConfirmAnswer,
  Id,
  InterviewSnapshot,
  RecordingView,
  ReviewOrigin,
  SaveReview,
  SessionQuestion,
  Version,
} from "../../lib/interview-contracts";
import { newRequestId } from "../../lib/request-id";

/** Resultado de uma tentativa de guardar a revisão. */
export type SaveOutcome = "saved" | "pending" | "failed";

/** Resposta selecionada e a sua gravação, quando existem. */
export interface SelectedAnswer {
  question: SessionQuestion;
  recording: RecordingView;
}

/** Rascunho local de uma gravação, com a versão que lhe corresponde. */
interface Draft {
  text: string;
  /** Texto cuja versão está guardada (ou o texto automático inicial). */
  baseline: string;
  version: Version;
  sourceTranscriptId: Id | null;
  origin: ReviewOrigin;
  savedAt: string | null;
}

interface InterviewState {
  snapshot: InterviewSnapshot | null;
  drafts: Record<Id, Draft>;
  saving: boolean;
  confirming: boolean;
  error: AppError | null;
}

type Action =
  | { type: "snapshot"; snapshot: InterviewSnapshot }
  | { type: "edit"; text: string }
  | { type: "save-start" }
  | {
      type: "save-success";
      snapshot: InterviewSnapshot;
      recordingId: Id;
      savedText: string;
    }
  | { type: "save-failure"; error: AppError }
  | { type: "discard" }
  | { type: "confirm-start" }
  | { type: "confirm-success"; snapshot: InterviewSnapshot }
  | { type: "confirm-failure"; error: AppError };

const EMPTY_STATE: InterviewState = {
  snapshot: null,
  drafts: {},
  saving: false,
  confirming: false,
  error: null,
};

/** Interpretação apresentável de uma falha de validação local. */
function invalid(message: string): AppError {
  return { code: "INVALID_INPUT", message, retryable: false };
}

/** Transcrição automática mais recente de uma gravação. */
export function latestTranscript(
  recording: RecordingView | null,
): Transcript | null {
  if (!recording || recording.transcripts.length === 0) {
    return null;
  }
  return recording.transcripts[recording.transcripts.length - 1] ?? null;
}

/** Gravação selecionada da pergunta corrente, com a pergunta a que pertence. */
export function selectedAnswer(
  snapshot: InterviewSnapshot | null,
): SelectedAnswer | null {
  const current = activeAnswer(snapshot?.current);
  const recordingId = current?.question.selectedRecordingId;
  if (!current || !recordingId) {
    return null;
  }
  const recording = current.recordings.find((view) => view.id === recordingId);
  if (!recording) {
    return null;
  }
  return { question: current.question, recording };
}

/** Texto automático utilizável como ponto de partida de uma revisão. */
function automaticText(recording: RecordingView): string | null {
  const latest = latestTranscript(recording);
  if (!latest || latest.status !== "completed") {
    return null;
  }
  return latest.text.trim() === "" ? null : latest.text;
}

function seedDraft(recording: RecordingView): Draft {
  const saved = recording.review;
  const automatic = automaticText(recording);
  const text = saved?.text ?? automatic ?? "";
  return {
    text,
    baseline: text,
    version: saved?.version ?? 0,
    sourceTranscriptId:
      saved?.sourceTranscriptId ?? latestTranscript(recording)?.id ?? null,
    origin:
      saved?.origin ?? (automatic !== null ? "automatic" : "manual_explicit"),
    savedAt: saved?.savedAt ?? null,
  };
}

/** Alinha o rascunho com a fotografia sem perder trabalho local. */
function reconcileDraft(
  draft: Draft | undefined,
  recording: RecordingView,
  confirmedText: string | null,
  lockedVersion: Version,
  lockedSource: Id | null,
  lockedOrigin: ReviewOrigin,
  lockedAt: string | null,
  busy: boolean,
): Draft | undefined {
  if (confirmedText !== null) {
    return {
      text: confirmedText,
      baseline: confirmedText,
      version: lockedVersion,
      sourceTranscriptId: lockedSource,
      origin: lockedOrigin,
      savedAt: lockedAt,
    };
  }

  const fresh = seedDraft(recording);
  if (!draft) {
    return fresh;
  }
  // Uma escrita em curso nunca é substituída por uma fotografia.
  if (busy) {
    return draft;
  }
  const saved = recording.review;
  // O núcleo pode criar uma revisão sozinho (por exemplo, ao terminar a
  // transcrição). Se o rascunho continuar na versão antiga, todos os pedidos
  // seguintes são recusados por versão desatualizada: adota-se a versão
  // guardada e conserva-se o texto editado como alteração por guardar.
  if (saved && saved.version > draft.version) {
    return {
      ...draft,
      baseline: saved.text,
      version: saved.version,
      savedAt: saved.savedAt,
      sourceTranscriptId: draft.sourceTranscriptId ?? saved.sourceTranscriptId,
    };
  }
  // Uma edição local nunca é substituída.
  if (draft.text !== draft.baseline) {
    return draft;
  }
  if (saved && saved.version !== draft.version) {
    return { ...fresh, text: saved.text, baseline: saved.text };
  }
  if (!saved && draft.version === 0 && draft.text === "" && fresh.text !== "") {
    return { ...fresh, text: fresh.text, baseline: fresh.text };
  }
  if (draft.version === 0) {
    // Sem versão guardada, a origem e a transcrição de referência acompanham
    // o núcleo até a primeira gravação.
    if (
      draft.origin !== fresh.origin ||
      draft.sourceTranscriptId !== fresh.sourceTranscriptId
    ) {
      return {
        ...draft,
        origin: fresh.origin,
        sourceTranscriptId: fresh.sourceTranscriptId,
      };
    }
  }
  return draft;
}

function reconcileDrafts(
  state: InterviewState,
  snapshot: InterviewSnapshot,
): Record<Id, Draft> {
  const current = activeAnswer(snapshot.current);
  if (!current) {
    return state.drafts;
  }
  const answer = selectedAnswer(snapshot);
  if (!answer) {
    return state.drafts;
  }
  const confirmation = current.confirmation;
  const confirmedHere =
    confirmation !== null && confirmation.recordingId === answer.recording.id
      ? confirmation
      : null;
  const reconciled = reconcileDraft(
    state.drafts[answer.recording.id],
    answer.recording,
    confirmedHere?.text ?? null,
    confirmedHere?.reviewVersion ?? 0,
    confirmedHere?.sourceTranscriptId ?? null,
    confirmedHere?.origin ?? "automatic",
    confirmedHere?.confirmedAt ?? null,
    state.saving || state.confirming,
  );
  if (!reconciled || reconciled === state.drafts[answer.recording.id]) {
    return state.drafts;
  }
  return { ...state.drafts, [answer.recording.id]: reconciled };
}

function applySnapshot(
  state: InterviewState,
  next: InterviewSnapshot,
): InterviewState {
  // As fotografias chegam com revisão crescente; uma mais antiga pode chegar
  // atrasada e não pode reverter o que o utilizador já vê.
  if (state.snapshot && next.revision < state.snapshot.revision) {
    return state;
  }
  return {
    ...state,
    snapshot: next,
    drafts: reconcileDrafts(state, next),
  };
}

function withDraft(
  state: InterviewState,
  recordingId: Id,
  draft: Draft,
): InterviewState {
  return { ...state, drafts: { ...state.drafts, [recordingId]: draft } };
}

function reducer(state: InterviewState, action: Action): InterviewState {
  switch (action.type) {
    case "snapshot":
      return applySnapshot(state, action.snapshot);
    case "edit": {
      const answer = selectedAnswer(state.snapshot);
      const draft = answer ? state.drafts[answer.recording.id] : undefined;
      if (!answer || !draft) {
        return state;
      }
      return {
        ...withDraft(state, answer.recording.id, {
          ...draft,
          text: action.text,
        }),
        error: null,
      };
    }
    case "save-start":
      return { ...state, saving: true, error: null };
    case "save-success": {
      const applied = applySnapshot(state, action.snapshot);
      const answer = selectedAnswer(action.snapshot);
      const saved = answer?.recording.review ?? null;
      const previous = applied.drafts[action.recordingId];
      const text = previous?.text ?? action.savedText;
      const next: Draft = {
        text,
        baseline: action.savedText,
        version: saved?.version ?? (previous?.version ?? 0) + 1,
        sourceTranscriptId:
          saved?.sourceTranscriptId ?? previous?.sourceTranscriptId ?? null,
        origin: saved?.origin ?? previous?.origin ?? "automatic",
        savedAt: saved?.savedAt ?? previous?.savedAt ?? null,
      };
      return {
        ...withDraft(applied, action.recordingId, next),
        saving: false,
        error: null,
      };
    }
    case "save-failure":
      return { ...state, saving: false, error: action.error };
    case "discard": {
      // Descartar resolve todas as edições por guardar, não só a da resposta
      // visível: um rascunho de outra tomada não pode ficar inacessível.
      const drafts: Record<Id, Draft> = {};
      for (const [id, entry] of Object.entries(state.drafts)) {
        drafts[id] =
          entry.text === entry.baseline
            ? entry
            : { ...entry, text: entry.baseline };
      }
      return { ...state, drafts, error: null };
    }
    case "confirm-start":
      return { ...state, confirming: true, error: null };
    case "confirm-success":
      return {
        ...applySnapshot(state, action.snapshot),
        saving: false,
        confirming: false,
        error: null,
      };
    case "confirm-failure":
      return { ...state, confirming: false, error: action.error };
    default:
      return state;
  }
}

export interface InterviewReview {
  /** Gravação revista; nula enquanto não existir uma resposta selecionada. */
  recordingId: Id | null;
  text: string;
  origin: ReviewOrigin;
  version: Version;
  savedAt: string | null;
  /** Verdadeiro quando o texto visível difere da versão guardada. */
  dirty: boolean;
  saving: boolean;
  confirming: boolean;
  /** Confirmada e por isso apenas de leitura. */
  confirmed: boolean;
  /** Texto da transcrição automática da gravação selecionada. */
  automaticText: string;
  /** O motor não reconheceu fala nesta gravação. */
  automaticNoSpeech: boolean;
  /** A revisão pode ser editada, guardada e confirmada. */
  canEdit: boolean;
  canSave: boolean;
  /**
   * Indica, a partir dos valores mais recentes (mesmo antes do próximo
   * render), se existe texto por guardar. Usado pela guarda de fecho para não
   * fechar com uma edição que tenha escapado ao bloqueio do editor.
   */
  isDirty: () => boolean;
  error: AppError | null;
  setReviewText: (text: string) => void;
  saveReview: () => Promise<SaveOutcome>;
  discardReview: () => void;
  confirm: () => Promise<boolean>;
}

/**
 * Liga o ecrã da entrevista ao cliente injetado.
 *
 * `snapshot` é a fotografia conhecida da aplicação; `onSnapshot` recebe o
 * estado devolvido por cada mutação para que a navegação e a guarda de fecho
 * vejam a confirmação e o avanço sem uma segunda subscrição.
 *
 * `options.locked` bloqueia o rascunho enquanto a aplicação se prepara para
 * fechar: o editor fica apenas de leitura e nenhuma edição nova entra no texto
 * que já foi guardado.
 */
export function useInterview(
  client: InterviewClient,
  snapshot: InterviewSnapshot | null,
  onSnapshot?: (snapshot: InterviewSnapshot) => void,
  options: { locked?: boolean } = {},
): InterviewReview {
  const locked = options.locked ?? false;
  const [state, dispatch] = useReducer(reducer, EMPTY_STATE);
  const stateRef = useRef(state);
  const lockedRef = useRef(locked);
  const textRef = useRef("");
  const baselineRef = useRef("");
  const draftsRef = useRef<Record<Id, Draft>>(state.drafts);
  const intentionRef = useRef<{ key: string; requestId: string } | null>(null);
  const saveInFlight = useRef<Promise<SaveOutcome> | null>(null);

  stateRef.current = state;
  lockedRef.current = locked;
  draftsRef.current = state.drafts;

  const answer = selectedAnswer(state.snapshot);
  const draft = answer ? state.drafts[answer.recording.id] : undefined;
  const confirmation =
    activeAnswer(state.snapshot?.current)?.confirmation ?? null;
  const confirmed =
    confirmation !== null &&
    answer !== null &&
    confirmation.recordingId === answer.recording.id;
  const text = draft?.text ?? "";
  if (draft) {
    textRef.current = draft.text;
    baselineRef.current = draft.baseline;
  }
  const latest = latestTranscript(answer?.recording ?? null);
  // Enquanto decorre uma captura ou a finalização, a seleção pode mudar para
  // uma gravação nova. Um texto editado na tomada anterior deixaria de poder
  // ser guardado (o núcleo só guarda a selecionada) e sairia do alcance da
  // guarda: o editor fica por isso bloqueado durante a captura.
  const captureBusy =
    (state.snapshot?.recordingId ?? null) !== null ||
    (state.snapshot?.finalizing ?? false);
  const canEdit =
    answer !== null &&
    !confirmed &&
    !state.confirming &&
    !captureBusy &&
    !locked;

  // O rascunho é alinhado antes de a página ser apresentada: a revisão começa
  // preenchida, sem um instante com o editor vazio.
  useLayoutEffect(() => {
    if (snapshot) {
      dispatch({ type: "snapshot", snapshot });
    }
  }, [snapshot]);

  const stableRequestId = useCallback((key: string): string => {
    const current = intentionRef.current;
    if (current?.key === key) {
      return current.requestId;
    }
    const requestId = newRequestId();
    intentionRef.current = { key, requestId };
    return requestId;
  }, []);

  const setReviewText = useCallback((nextText: string) => {
    const current = stateRef.current;
    if (lockedRef.current) {
      // A preparação do fecho bloqueia o editor: texto novo não entra num
      // rascunho que já foi guardado para o fecho.
      return;
    }
    const selected = selectedAnswer(current.snapshot);
    const currentDraft = selected
      ? current.drafts[selected.recording.id]
      : undefined;
    const capturing =
      (current.snapshot?.recordingId ?? null) !== null ||
      (current.snapshot?.finalizing ?? false);
    if (!selected || !currentDraft || current.confirming || capturing) {
      return;
    }
    if (activeAnswer(current.snapshot?.current)?.confirmation) {
      return;
    }
    textRef.current = nextText;
    dispatch({ type: "edit", text: nextText });
  }, []);

  const discardReview = useCallback(() => {
    const current = stateRef.current;
    if (lockedRef.current) {
      return;
    }
    if (
      current.confirming ||
      activeAnswer(current.snapshot?.current)?.confirmation
    ) {
      return;
    }
    const selected = selectedAnswer(current.snapshot);
    const currentDraft = selected
      ? current.drafts[selected.recording.id]
      : undefined;
    if (currentDraft) {
      textRef.current = currentDraft.baseline;
      baselineRef.current = currentDraft.baseline;
    }
    dispatch({ type: "discard" });
  }, []);

  const runSave = useCallback(async (): Promise<SaveOutcome> => {
    const current = stateRef.current;
    if (lockedRef.current) {
      dispatch({
        type: "save-failure",
        error: invalid(
          "A aplicação está a preparar o fecho. Aguarde que termine para guardar.",
        ),
      });
      return "failed";
    }
    const selected = selectedAnswer(current.snapshot);
    const currentDraft = selected
      ? current.drafts[selected.recording.id]
      : undefined;
    if (!selected || !currentDraft) {
      dispatch({
        type: "save-failure",
        error: invalid("Não existe uma resposta selecionada para guardar."),
      });
      return "failed";
    }
    if (activeAnswer(current.snapshot?.current)?.confirmation) {
      dispatch({
        type: "save-failure",
        error: invalid("A resposta confirmada é apenas de leitura."),
      });
      return "failed";
    }
    const text = textRef.current;
    if (text.trim() === "") {
      dispatch({
        type: "save-failure",
        error: invalid("O texto da revisão não pode estar vazio."),
      });
      return "failed";
    }

    const { question, recording } = selected;
    const target: AnswerTarget = {
      sessionId: question.sessionId,
      sessionQuestionId: current.snapshot?.current?.question.id ?? question.id,
      ...(current.snapshot?.current?.question.id !== question.id
        ? { answerId: question.id }
        : {}),
      recordingId: recording.id,
      selectionVersion: question.selectionVersion,
    };
    const request: SaveReview = {
      requestId: stableRequestId(
        `save:${target.sessionId}:${target.sessionQuestionId}:${target.recordingId}:${currentDraft.version}:${text}`,
      ),
      target,
      expectedReviewVersion: currentDraft.version,
      sourceTranscriptId: currentDraft.sourceTranscriptId,
      origin: currentDraft.origin,
      text,
    };

    dispatch({ type: "save-start" });
    try {
      const next = await client.saveReview(request);
      intentionRef.current = null;
      // O texto escrito é agora a versão guardada: a guarda de fecho lê este
      // valor sem esperar pelo próximo render.
      baselineRef.current = text;
      dispatch({
        type: "save-success",
        snapshot: next,
        recordingId: recording.id,
        savedText: text,
      });
      onSnapshot?.(next);
      // O texto pode ter mudado durante a escrita: a versão nova continua por
      // guardar e quem fecha ou grava de novo não pode avançar.
      return textRef.current === text ? "saved" : "pending";
    } catch (error) {
      dispatch({ type: "save-failure", error: toAppError(error) });
      return "failed";
    }
  }, [client, onSnapshot, stableRequestId]);

  const saveReview = useCallback(async (): Promise<SaveOutcome> => {
    if (saveInFlight.current) {
      return saveInFlight.current;
    }
    const operation = runSave();
    saveInFlight.current = operation;
    try {
      return await operation;
    } finally {
      saveInFlight.current = null;
    }
  }, [runSave]);

  const confirm = useCallback(async (): Promise<boolean> => {
    const current = stateRef.current;
    if (lockedRef.current) {
      dispatch({
        type: "confirm-failure",
        error: invalid(
          "A aplicação está a preparar o fecho. Aguarde que termine para confirmar.",
        ),
      });
      return false;
    }
    const selected = selectedAnswer(current.snapshot);
    const currentDraft = selected
      ? current.drafts[selected.recording.id]
      : undefined;
    const summary = current.snapshot?.activeSession ?? null;
    if (!selected || !currentDraft || !summary) {
      dispatch({
        type: "confirm-failure",
        error: invalid("Não existe uma resposta selecionada para confirmar."),
      });
      return false;
    }
    if (activeAnswer(current.snapshot?.current)?.confirmation) {
      return true;
    }
    if (current.confirming) {
      return false;
    }
    const text = textRef.current;
    if (text.trim() === "") {
      dispatch({
        type: "confirm-failure",
        error: invalid("O texto da revisão não pode estar vazio."),
      });
      return false;
    }

    const { question, recording } = selected;
    const target: AnswerTarget = {
      sessionId: question.sessionId,
      sessionQuestionId: current.snapshot?.current?.question.id ?? question.id,
      ...(current.snapshot?.current?.question.id !== question.id
        ? { answerId: question.id }
        : {}),
      recordingId: recording.id,
      selectionVersion: question.selectionVersion,
    };
    const request: ConfirmAnswer = {
      requestId: stableRequestId(
        `confirm:${target.sessionId}:${target.sessionQuestionId}:${target.recordingId}:${currentDraft.version}:${summary.version}:${text}`,
      ),
      target,
      expectedReviewVersion: currentDraft.version,
      expectedSessionVersion: summary.version,
      sourceTranscriptId: currentDraft.sourceTranscriptId,
      origin: currentDraft.origin,
      text,
    };

    dispatch({ type: "confirm-start" });
    try {
      const next = await client.confirmAnswer(request);
      intentionRef.current = null;
      baselineRef.current = textRef.current;
      dispatch({ type: "confirm-success", snapshot: next });
      onSnapshot?.(next);
      return true;
    } catch (error) {
      dispatch({ type: "confirm-failure", error: toAppError(error) });
      return false;
    }
  }, [client, onSnapshot, stableRequestId]);

  const automatic = latest?.status === "completed" ? latest.text : "";

  const isDirty = useCallback(() => {
    // O rascunho visível é lido pelos refs, que já refletem a escrita
    // terminada; os restantes vêm do estado, para uma edição antiga não ficar
    // inacessível.
    const selected = selectedAnswer(stateRef.current.snapshot);
    const selectedId = selected?.recording.id ?? null;
    for (const [id, entry] of Object.entries(draftsRef.current)) {
      if (entry.text === entry.baseline || id === selectedId) {
        continue;
      }
      return true;
    }
    return textRef.current !== baselineRef.current;
  }, []);

  return {
    recordingId: answer?.recording.id ?? null,
    text,
    origin: draft?.origin ?? "automatic",
    version: draft?.version ?? 0,
    savedAt: draft?.savedAt ?? null,
    dirty: Object.values(state.drafts).some(
      (entry) => entry.text !== entry.baseline,
    ),
    saving: state.saving,
    confirming: state.confirming,
    confirmed,
    automaticText: automatic,
    automaticNoSpeech: latest?.status === "no_speech",
    canEdit,
    canSave:
      canEdit &&
      !state.saving &&
      draft !== undefined &&
      text.trim() !== "" &&
      (draft.text !== draft.baseline || draft.version === 0),
    isDirty,
    error: state.error,
    setReviewText,
    saveReview,
    discardReview,
    confirm,
  };
}

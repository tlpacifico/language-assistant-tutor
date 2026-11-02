/**
 * Estado local da revisão da transcrição (tarefa 7).
 *
 * O hook guarda apenas a fotografia recebida, o texto de revisão e o estado da
 * escrita local. A captura, os trabalhos de modelo e a reprodução continuam a
 * ser propriedade do serviço Rust; a interface limita-se a refletir a
 * fotografia e a enviar intenções.
 *
 * Regras de proteção:
 * - Uma fotografia com revisão igual ou inferior à atual é ignorada.
 * - Uma fotografia nunca substitui uma edição local por guardar.
 * - Uma escrita falhada mantém o texto e a marca de alterações por guardar.
 */

import { useCallback, useEffect, useReducer, useRef } from "react";

import type {
  AppError,
  AppSnapshot,
  Attempt,
  Transcript,
} from "../../lib/contracts";
import type { PracticeClient } from "../../lib/tauri";

/** Revisão local associada a uma resposta e à transcrição que a originou. */
export interface ReviewState {
  attemptId: string | null;
  sourceId: string | null;
  text: string;
  /** Texto guardado (ou automático inicial) usado para detetar alterações. */
  baseline: string;
  hasSavedReview: boolean;
}

interface PracticeState {
  snapshot: AppSnapshot | null;
  loading: boolean;
  loadError: AppError | null;
  review: ReviewState;
  saving: boolean;
  saveError: AppError | null;
}

const EMPTY_REVIEW: ReviewState = {
  attemptId: null,
  sourceId: null,
  text: "",
  baseline: "",
  hasSavedReview: false,
};

const INITIAL_STATE: PracticeState = {
  snapshot: null,
  loading: true,
  loadError: null,
  review: EMPTY_REVIEW,
  saving: false,
  saveError: null,
};

type Action =
  | { type: "initial"; snapshot: AppSnapshot }
  | { type: "snapshot"; snapshot: AppSnapshot }
  | { type: "load-start" }
  | { type: "load-error"; error: AppError }
  | { type: "edit"; text: string }
  | { type: "save-start" }
  | {
      type: "save-success";
      snapshot: AppSnapshot;
      savedText: string;
      attemptId: string;
      sourceId: string;
    }
  | { type: "save-failure"; error: AppError }
  | { type: "discard" };

/** Converte uma rejeição desconhecida num `AppError` apresentável. */
export function toAppError(error: unknown): AppError {
  if (typeof error === "object" && error !== null) {
    const candidate = error as Partial<AppError>;
    if (
      typeof candidate.code === "string" &&
      typeof candidate.message === "string"
    ) {
      return {
        code: candidate.code,
        message: candidate.message,
        retryable: candidate.retryable ?? true,
      };
    }
  }
  return {
    code: "UNKNOWN",
    message: "Não foi possível concluir a operação.",
    retryable: true,
  };
}

/** Transcrição automática mais recente da resposta ativa. */
export function latestTranscript(active: Attempt | null): Transcript | null {
  if (!active || active.transcripts.length === 0) {
    return null;
  }
  return active.transcripts[active.transcripts.length - 1] ?? null;
}

function hasLocalEdits(review: ReviewState): boolean {
  return review.text !== review.baseline;
}

/**
 * Alinha a revisão local com a fotografia sem perder trabalho.
 *
 * Uma edição por guardar (ou uma escrita em curso) nunca é substituída. Sem
 * edição local, uma resposta diferente reinicia a revisão e uma transcrição
 * nova atualiza a cópia automática enquanto ainda não houver revisão guardada.
 */
function reconcileReview(
  review: ReviewState,
  active: Attempt | null,
  saving: boolean,
): ReviewState {
  if (!active || saving || hasLocalEdits(review)) {
    return review;
  }

  const latest = latestTranscript(active);
  const saved = active.review;
  const changedAttempt = review.attemptId !== active.id;

  if (changedAttempt) {
    const text = saved?.text ?? latest?.text ?? "";
    return {
      attemptId: active.id,
      sourceId: saved?.sourceTranscriptId ?? latest?.id ?? null,
      text,
      baseline: text,
      hasSavedReview: saved !== null,
    };
  }

  if (saved) {
    if (saved.text !== review.baseline) {
      return {
        ...review,
        sourceId: saved.sourceTranscriptId,
        text: saved.text,
        baseline: saved.text,
        hasSavedReview: true,
      };
    }
    return {
      ...review,
      sourceId: saved.sourceTranscriptId,
      hasSavedReview: true,
    };
  }

  if (!latest) {
    return review;
  }

  if (review.text === "") {
    return {
      ...review,
      sourceId: latest.id,
      text: latest.text,
      baseline: latest.text,
      hasSavedReview: false,
    };
  }

  if (!review.hasSavedReview && review.baseline !== latest.text) {
    return {
      ...review,
      sourceId: latest.id,
      text: latest.text,
      baseline: latest.text,
    };
  }

  return review.sourceId === null
    ? { ...review, sourceId: latest.id }
    : review;
}

function applySnapshot(state: PracticeState, next: AppSnapshot): PracticeState {
  // As fotografias são publicadas com revisão crescente; uma mais antiga pode
  // chegar atrasada e não pode reverter o que o utilizador já vê.
  if (state.snapshot && next.revision <= state.snapshot.revision) {
    return state;
  }
  return {
    ...state,
    snapshot: next,
    review: reconcileReview(state.review, next.attempt, state.saving),
  };
}

function reducer(state: PracticeState, action: Action): PracticeState {
  switch (action.type) {
    case "initial":
      return {
        ...applySnapshot(state, action.snapshot),
        loading: false,
        loadError: null,
      };
    case "snapshot":
      return applySnapshot(state, action.snapshot);
    case "load-start":
      return { ...state, loading: true, loadError: null };
    case "load-error":
      return state.snapshot
        ? { ...state, loading: false, loadError: null }
        : { ...state, loading: false, loadError: action.error };
    case "edit":
      return {
        ...state,
        review: { ...state.review, text: action.text },
      };
    case "save-start":
      return { ...state, saving: true, saveError: null };
    case "save-success": {
      const next = applySnapshot(state, action.snapshot);
      // Se o utilizador editou enquanto a escrita decorria, o texto novo fica
      // e volta a contar como alteração por guardar.
      const editedDuringSave = next.review.text !== action.savedText;
      return {
        ...next,
        saving: false,
        saveError: null,
        review: {
          ...next.review,
          attemptId: action.attemptId,
          sourceId: action.sourceId,
          text: editedDuringSave ? next.review.text : action.savedText,
          baseline: action.savedText,
          hasSavedReview: true,
        },
      };
    }
    case "save-failure":
      // `baseline` não muda, pelo que a revisão continua marcada como alterada.
      return { ...state, saving: false, saveError: action.error };
    case "discard":
      return {
        ...state,
        saveError: null,
        review: { ...state.review, text: state.review.baseline },
      };
  }
}

export interface UsePractice {
  snapshot: AppSnapshot | null;
  loading: boolean;
  loadError: AppError | null;
  review: ReviewState & {
    dirty: boolean;
    saving: boolean;
    error: AppError | null;
  };
  setReviewText: (text: string) => void;
  saveReview: () => Promise<boolean>;
  discardReview: () => void;
  acceptSnapshot: (snapshot: AppSnapshot) => void;
  reload: () => Promise<void>;
}

/** Liga a interface ao cliente injetado, sem conhecer comandos Tauri. */
export function usePractice(client: PracticeClient): UsePractice {
  const [state, dispatch] = useReducer(reducer, INITIAL_STATE);
  const snapshotRef = useRef<AppSnapshot | null>(null);
  const reviewRef = useRef<ReviewState>(state.review);

  snapshotRef.current = state.snapshot;
  reviewRef.current = state.review;

  useEffect(() => {
    let active = true;
    const unsubscribe = client.subscribe((next) => {
      dispatch({ type: "snapshot", snapshot: next });
    });

    void client.snapshot().then(
      (next) => {
        if (active) {
          dispatch({ type: "initial", snapshot: next });
        }
      },
      (error: unknown) => {
        if (active) {
          dispatch({ type: "load-error", error: toAppError(error) });
        }
      },
    );

    return () => {
      active = false;
      unsubscribe();
    };
  }, [client]);

  const acceptSnapshot = useCallback((next: AppSnapshot) => {
    dispatch({ type: "snapshot", snapshot: next });
  }, []);

  const reload = useCallback(async () => {
    dispatch({ type: "load-start" });
    try {
      dispatch({ type: "initial", snapshot: await client.snapshot() });
    } catch (error) {
      dispatch({ type: "load-error", error: toAppError(error) });
    }
  }, [client]);

  const setReviewText = useCallback((text: string) => {
    dispatch({ type: "edit", text });
  }, []);

  const saveReview = useCallback(async (): Promise<boolean> => {
    const active = snapshotRef.current?.attempt ?? null;
    const review = reviewRef.current;
    const latest = latestTranscript(active);
    // A revisão guardada mantém a transcrição que a originou; uma revisão nova
    // usa a transcrição automática mais recente.
    const sourceId =
      review.attemptId === active?.id && review.sourceId
        ? review.sourceId
        : (latest?.id ?? null);

    if (!active || !sourceId) {
      return false;
    }

    const savedText = review.text;
    dispatch({ type: "save-start" });
    try {
      const next = await client.saveReview(active.id, sourceId, savedText);
      dispatch({
        type: "save-success",
        snapshot: next,
        savedText,
        attemptId: active.id,
        sourceId,
      });
      return true;
    } catch (error) {
      dispatch({ type: "save-failure", error: toAppError(error) });
      return false;
    }
  }, [client]);

  const discardReview = useCallback(() => {
    dispatch({ type: "discard" });
  }, []);

  return {
    snapshot: state.snapshot,
    loading: state.loading,
    loadError: state.loadError,
    review: {
      ...state.review,
      dirty: hasLocalEdits(state.review),
      saving: state.saving,
      error: state.saveError,
    },
    setReviewText,
    saveReview,
    discardReview,
    acceptSnapshot,
    reload,
  };
}

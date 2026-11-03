/**
 * Treino posterior de uma pergunta de uma sessão concluída.
 *
 * Lista as práticas já feitas, permite abrir, comparar e criar uma nova
 * tentativa, e reformular a resposta original. Criar uma tentativa é local e
 * nunca pede feedback, reformulação ou seguimento automaticamente. Não é
 * possível começar uma prática com uma entrevista em curso nem com outra
 * prática pendente: o núcleo recusa e a interface explica porquê.
 */

import { useEffect, useRef, useState } from "react";

import { toAppError } from "../../lib/app-error";
import type { AppError } from "../../lib/contracts";
import type { InterviewClient } from "../../lib/interview-client";
import type {
  InterviewSnapshot,
  QuestionDetail,
} from "../../lib/interview-contracts";
import type {
  ComparisonRequest,
  PracticeDetail,
  PracticeStatus,
} from "../../lib/practice-contracts";
import { newRequestId } from "../../lib/request-id";
import Comparison from "./Comparison";
import RewritePanel from "./RewritePanel";
import { formatMoment } from "./shared";

const STATUS_LABELS: Record<PracticeStatus, string> = {
  preparing: "Em preparação",
  answering: "A responder",
  confirmed: "Confirmada",
  abandoned: "Abandonada",
};

export interface PostTrainingProps {
  client: InterviewClient;
  sessionId: string;
  question: QuestionDetail;
  /** Fotografia atual da aplicação, para bloquear por entrevista/prática. */
  snapshot: InterviewSnapshot | null;
  onSnapshot?: (snapshot: InterviewSnapshot) => void;
  onOpenPractice: (practiceId: string) => void;
}

export default function PostTraining({
  client,
  sessionId,
  question,
  snapshot,
  onSnapshot,
  onOpenPractice,
}: PostTrainingProps) {
  const sessionQuestionId = question.question.id;
  const [practices, setPractices] = useState<PracticeDetail[] | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [creating, setCreating] = useState(false);
  const [comparing, setComparing] = useState<ComparisonRequest | null>(null);
  const [rewriting, setRewriting] = useState(false);
  const intention = useRef<{ key: string; requestId: string } | null>(null);
  const revision = snapshot?.revision ?? 0;

  useEffect(() => {
    let active = true;
    client
      .listPractices(sessionQuestionId)
      .then((rows) => {
        if (active) {
          setPractices(rows);
        }
      })
      .catch((failure: unknown) => {
        if (active) {
          setPractices([]);
          setError(toAppError(failure));
        }
      });
    return () => {
      active = false;
    };
  }, [client, sessionQuestionId, revision]);

  const pendingPractice = snapshot?.practice?.activePractice ?? null;
  const interviewActive = snapshot?.activeSession != null;
  const confirmed = (practices ?? []).filter(
    (entry) => entry.status === "confirmed",
  );
  const lastConfirmed = confirmed[confirmed.length - 1] ?? null;
  const original = {
    type: "original" as const,
    sessionId,
    sessionQuestionId,
  };

  async function tryAgain(): Promise<void> {
    if (creating) {
      return;
    }
    const key = `${sessionQuestionId}:${lastConfirmed?.id ?? "none"}:${practices?.length ?? 0}`;
    if (intention.current?.key !== key) {
      intention.current = { key, requestId: newRequestId() };
    }
    setCreating(true);
    setError(null);
    try {
      // Criar a prática é local: não pede avaliação, sugestão nem seguimento.
      const created = await client.createPractice({
        requestId: intention.current.requestId,
        sessionId,
        sessionQuestionId,
        previousPracticeId: lastConfirmed?.id ?? null,
      });
      intention.current = null;
      onSnapshot?.(await client.snapshot());
      onOpenPractice(created.id);
    } catch (failure) {
      setError(toAppError(failure));
    } finally {
      setCreating(false);
    }
  }

  return (
    <section
      className="practice__panel"
      aria-labelledby={`post-training-${sessionQuestionId}`}
    >
      <h5 id={`post-training-${sessionQuestionId}`}>Treino posterior</h5>
      <p className="practice__hint">
        Nova tentativa desta pergunta, separada da resposta original. Nada é
        enviado à API ao criar ou abrir uma prática.
      </p>

      {practices === null ? (
        <p className="practice__hint" role="status">
          A carregar as práticas…
        </p>
      ) : practices.length === 0 ? (
        <p className="practice__hint">Ainda não existem práticas.</p>
      ) : (
        <ul className="practice__list">
          {practices.map((entry) => (
            <li key={entry.id} className="practice__item">
              <p className="practice__meta">
                Prática {entry.ordinal} · {formatMoment(entry.createdAt)} ·{" "}
                {STATUS_LABELS[entry.status]}
              </p>
              <div className="practice__actions">
                <button type="button" onClick={() => onOpenPractice(entry.id)}>
                  Abrir prática {entry.ordinal}
                </button>
                {entry.status === "confirmed" ? (
                  <button
                    type="button"
                    onClick={() =>
                      setComparing({
                        left: original,
                        right: { type: "practice", practiceId: entry.id },
                      })
                    }
                  >
                    Comparar prática {entry.ordinal} com a original
                  </button>
                ) : null}
              </div>
            </li>
          ))}
        </ul>
      )}

      <div className="practice__actions">
        {pendingPractice ? (
          <button
            type="button"
            onClick={() => onOpenPractice(pendingPractice.id)}
          >
            Retomar prática pendente
          </button>
        ) : null}
        <button
          type="button"
          disabled={creating || interviewActive || pendingPractice !== null}
          onClick={() => void tryAgain()}
        >
          Tentar novamente
        </button>
        <button type="button" onClick={() => setRewriting((open) => !open)}>
          {rewriting
            ? "Fechar reformulação"
            : "Reformular a resposta original"}
        </button>
      </div>
      {interviewActive ? (
        <p className="practice__hint">
          Termine a entrevista em curso antes de praticar.
        </p>
      ) : null}
      {pendingPractice ? (
        <p className="practice__hint">
          Já existe uma prática pendente. Confirme-a ou abandone-a antes de
          criar outra.
        </p>
      ) : null}

      {rewriting ? (
        <RewritePanel
          client={client}
          origin={original}
          snapshot={snapshot}
          onSnapshot={onSnapshot}
        />
      ) : null}

      {comparing ? (
        <Comparison
          client={client}
          request={comparing}
          onClose={() => setComparing(null)}
        />
      ) : null}

      {error ? (
        <div className="practice__notice practice__notice--error" role="alert">
          <p>{error.message}</p>
          <p className="practice__hint">Código: {error.code}.</p>
        </div>
      ) : null}
    </section>
  );
}

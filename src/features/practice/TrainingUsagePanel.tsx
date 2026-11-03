/**
 * Consumo de treino de uma sessão: entrevista original, treino posterior e
 * combinado. A leitura é local; nada aqui envia pedidos à API.
 */

import { useEffect, useState } from "react";

import { toAppError } from "../../lib/app-error";
import type { AppError } from "../../lib/contracts";
import type { InterviewClient } from "../../lib/interview-client";
import type { TrainingUsageView } from "../../lib/practice-contracts";
import { UsageGroupView } from "./shared";

export default function TrainingUsagePanel({
  client,
  sessionId,
}: {
  client: InterviewClient;
  sessionId: string;
}) {
  const [usage, setUsage] = useState<TrainingUsageView | null>(null);
  const [error, setError] = useState<AppError | null>(null);

  useEffect(() => {
    let active = true;
    setUsage(null);
    setError(null);
    client
      .loadTrainingUsage(sessionId)
      .then((next) => {
        if (active) {
          setUsage(next);
        }
      })
      .catch((failure: unknown) => {
        if (active) {
          setError(toAppError(failure));
        }
      });
    return () => {
      active = false;
    };
  }, [client, sessionId]);

  return (
    <section
      className="practice__panel"
      aria-labelledby={`training-usage-${sessionId}`}
    >
      <h4 id={`training-usage-${sessionId}`}>Consumo do treino</h4>
      <p className="practice__hint">
        O treino posterior (reformulações e avaliações de práticas) fica
        separado da entrevista original. Cada chamada de rede conta uma só vez
        no total combinado.
      </p>
      {error ? (
        <p role="alert">{error.message}</p>
      ) : usage === null ? (
        <p className="practice__hint" role="status">
          A carregar o consumo…
        </p>
      ) : (
        <>
          <UsageGroupView
            title="Consumo — entrevista original"
            group={usage.originalInterview}
          />
          <UsageGroupView title="Consumo — treino posterior" group={usage.laterPractice} />
          <UsageGroupView title="Consumo — total combinado" group={usage.combined} />
        </>
      )}
    </section>
  );
}

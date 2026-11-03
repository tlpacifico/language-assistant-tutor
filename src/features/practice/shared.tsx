/**
 * Auxiliares de apresentação partilhados pelo treino posterior.
 *
 * Apenas formatação: nenhuma regra persistente vive aqui. O consumo
 * desconhecido nunca é apresentado como zero.
 */

import type { QuestionReport } from "../../lib/ai-contracts";
import type { AppError } from "../../lib/contracts";
import type {
  ResponseOrigin,
  SessionUsage,
  UsageGroup,
} from "../../lib/practice-contracts";

/** Data e hora locais; um valor inválido é mostrado tal como veio. */
export function formatMoment(value: string): string {
  const moment = new Date(value);
  if (Number.isNaN(moment.getTime())) {
    return value;
  }
  return moment.toLocaleString("pt-PT", {
    dateStyle: "short",
    timeStyle: "short",
  });
}

/** Valor monetário em dólares com seis casas e vírgula decimal. */
export function formatUsd(value: number): string {
  return `US$${value.toFixed(6).replace(".", ",")}`;
}

export function errorKey(error: AppError): string {
  return `${error.code}:${error.message}`;
}

export function originLabel(origin: ResponseOrigin): string {
  return origin.type === "original" ? "Resposta original" : "Prática";
}

/**
 * Resumo de consumo de um conjunto de chamadas.
 *
 * Tokens `null` aparecem como «desconhecidos»; um total incompleto é rotulado
 * como parcial e nunca como o custo total.
 */
export function UsageLine({
  usage,
  networkAttemptCount,
}: {
  usage: SessionUsage;
  networkAttemptCount?: number;
}) {
  return (
    <>
      {networkAttemptCount !== undefined ? (
        <p>Chamadas de rede: {networkAttemptCount}.</p>
      ) : null}
      <p>
        Custo reportado conhecido: {formatUsd(usage.knownReportedSubtotal)}.
        Estimativa conhecida: {formatUsd(usage.knownEstimatedSubtotal)}.
      </p>
      <p>
        Chamadas com custo desconhecido: {usage.unknownAttemptCount}.{" "}
        {usage.complete
          ? "Consumo recebido para todas as chamadas registadas."
          : "Valores parciais; não representam o custo total."}
      </p>
      <p>
        Tokens de entrada: {usage.input ?? "desconhecidos"}; saída:{" "}
        {usage.output ?? "desconhecidos"}.
      </p>
    </>
  );
}

/** Grupo de consumo com o seu título e o detalhe por finalidade. */
export function UsageGroupView({
  title,
  group,
}: {
  title: string;
  group: UsageGroup;
}) {
  return (
    <section className="practice__item" aria-label={title}>
      <h5>{title}</h5>
      <UsageLine
        usage={group.totals}
        networkAttemptCount={group.networkAttemptCount}
      />
      {group.byPurpose.length > 0 ? (
        <ul className="practice__sublist">
          {group.byPurpose.map((entry) => (
            <li key={entry.purpose}>
              {entry.purpose}: {entry.networkAttemptCount} chamada(s);{" "}
              {entry.usage.complete
                ? formatUsd(entry.usage.knownReportedSubtotal)
                : `${formatUsd(entry.usage.knownReportedSubtotal)} (parcial)`}
            </li>
          ))}
        </ul>
      ) : null}
    </section>
  );
}

/**
 * Prioridades do relatório (no máximo três). Cada uma cita a observação a que
 * se refere; com menos de três, mostra-se o motivo indicado pelo avaliador.
 */
export function PrioritiesView({ report }: { report: QuestionReport }) {
  const explanations = new Map<string, string>();
  for (const entry of [
    ...report.content,
    ...report.strengths,
    ...report.gaps,
    ...report.english.necessaryCorrections,
    ...report.english.optionalStyle,
  ]) {
    explanations.set(entry.id, entry.explanation);
  }
  return (
    <section aria-label="Prioridades">
      <h4>Prioridades para a próxima tentativa</h4>
      {report.priorities.length === 0 ? (
        <p className="practice__hint">Nenhuma prioridade indicada.</p>
      ) : (
        <ol>
          {report.priorities.map((priority, index) => (
            <li key={`${priority.observationId}-${index}`}>
              <p>{priority.action}</p>
              {explanations.has(priority.observationId) ? (
                <p className="practice__hint">
                  Observação: {explanations.get(priority.observationId)}
                </p>
              ) : null}
            </li>
          ))}
        </ol>
      )}
      {report.fewerPrioritiesReason ? (
        <p className="practice__hint">
          Menos de três prioridades: {report.fewerPrioritiesReason}
        </p>
      ) : null}
    </section>
  );
}

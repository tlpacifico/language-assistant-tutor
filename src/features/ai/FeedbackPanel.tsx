import { useEffect, useState } from "react";
import type { InterviewClient } from "../../lib/interview-client";
import type {
  Evidence,
  FeedbackView,
  Observation,
  Correction,
} from "../../lib/ai-contracts";
import { newRequestId } from "../../lib/request-id";
import { toAppError } from "../../lib/app-error";
const dimensions: Record<string, string> = {
  relevance: "Relevância",
  situation: "Situação",
  individual_responsibility: "Responsabilidade individual",
  actions: "Ações",
  result: "Resultado",
  learning: "Aprendizagem",
};
const attemptStates: Record<string, string> = {
  running: "Em processamento",
  completed: "Concluído",
  failed: "Falhou",
  interrupted: "Interrompido",
  discarded: "Descartado",
};

/** O núcleo guarda AppError em JSON; não apresentar JSON/raw ilegível ao utilizador. */
function attemptError(raw: string | null): string {
  try {
    const error: unknown = JSON.parse(raw ?? "null");
    if (typeof error === "object" && error !== null &&
      "message" in error && typeof error.message === "string" && error.message.trim() &&
      "code" in error && typeof error.code === "string") {
      return `${error.message} Código: ${error.code}.`;
    }
  } catch { /* Registo legado ilegível: não expor o conteúdo bruto. */ }
  return "Não foi possível concluir o pedido. Consulte o diagnóstico local.";
}
function EvidenceView({ e }: { e: Evidence }) {
  return e.type === "quote" ? (
    <blockquote>
      {e.quote}
      <footer>
        {e.role === "follow_up"
          ? "Seguimento"
          : e.role === "attempt"
            ? "Tentativa"
            : "Resposta inicial"}{" "}
        — {e.confirmationId}
      </footer>
    </blockquote>
  ) : (
    <p>
      Não identificado no texto: {dimensions[e.dimension ?? ""] ?? e.dimension}.
      Fontes avaliadas: {e.assessedConfirmationIds?.join(", ")}
    </p>
  );
}
export function ObservationView({ o }: { o: Observation }) {
  return (
    <div>
      <p>
        <strong>
          {dimensions[o.dimension ?? ""] ?? o.dimension}{" "}
          {o.rating &&
            ({
              clear: "Claro",
              partial: "Parcial",
              not_identified: "Não identificado",
            }[o.rating] ??
              o.rating)}
        </strong>{" "}
        {o.explanation}
      </p>
      {o.evidence.map((e, i) => (
        <EvidenceView key={i} e={e} />
      ))}
    </div>
  );
}
export function CorrectionView({ o }: { o: Correction }) {
  return (
    <div>
      <p>{o.explanation}</p>
      <EvidenceView e={o.evidence} />
      <p>
        Sugestão: <q>{o.replacement}</q>
      </p>
    </div>
  );
}
export function QuestionReportView({
  r,
}: {
  r: FeedbackView["reports"][number];
}) {
  return (
    <article>
      <h4>
        Pergunta {r.position}: {r.questionText}
      </h4>
      <h5>Conteúdo e STAR</h5>
      {r.report.content.map((o) => (
        <ObservationView key={o.id} o={o} />
      ))}
      <h5>Pontos fortes</h5>
      {r.report.strengths.map((o) => (
        <ObservationView key={o.id} o={o} />
      ))}
      <h5>Lacunas</h5>
      {r.report.gaps.map((o) => (
        <ObservationView key={o.id} o={o} />
      ))}
      <h5>Correções necessárias no texto inglês</h5>
      {r.report.english.necessaryCorrections.length === 0 && (
        <p>Nenhuma correção necessária identificada pela IA.</p>
      )}
      {r.report.english.necessaryCorrections.map((o) => (
        <CorrectionView key={o.id} o={o} />
      ))}
      <h5>Alternativas opcionais de estilo</h5>
      {r.report.english.optionalStyle.map((o) => (
        <CorrectionView key={o.id} o={o} />
      ))}
      {r.report.priorities.length > 0 && (
        <>
          <h5>Prioridades de treino</h5>
          <ul>
            {r.report.priorities.map((p, i) => (
              <li key={i}>{p.action}</li>
            ))}
          </ul>
        </>
      )}
      <p>{r.report.fewerPrioritiesReason}</p>
      <details>
        <summary>Textos confirmados avaliados</summary>
        {r.sources.map((s) => (
          <div key={s.confirmationId}>
            <p>
              {s.role === "initial" ? "Resposta inicial" : "Seguimento"} —{" "}
              {s.confirmationId}
            </p>
            <pre lang="en">{s.text}</pre>
          </div>
        ))}
      </details>
    </article>
  );
}
export default function FeedbackPanel({
  client,
  sessionId,
}: {
  client: InterviewClient;
  sessionId: string;
}) {
  const [v, setV] = useState<FeedbackView | null>(null),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false),
    [pending, setPending] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    const load = () =>
      client
        .loadFeedback(sessionId)
        .then((v) => {
          if (live) setV(v);
        })
        .catch((e) => {
          if (live) setError(toAppError(e).message);
        });
    void load();
    void client.snapshot().then((s) => {
      if (live) setPending(s.pendingAiPublication ?? null);
    });
    const off = client.subscribe((s) => {
      setPending(s.pendingAiPublication ?? null);
      void load();
    });
    return () => {
      live = false;
      off();
    };
  }, [client, sessionId]);
  async function generate() {
    setBusy(true);
    setError("");
    try {
      setV(
        await client.generateFeedback({ requestId: newRequestId(), sessionId }),
      );
    } catch (e) {
      setError(toAppError(e).message);
    } finally {
      setBusy(false);
    }
  }
  if (v?.state === "local") return null;
  return (
    <section className="practice__panel">
      <h3>Feedback da IA</h3>
      <p>
        {v
          ? ({
              pending: "Pendente",
              running: "Em processamento",
              partial: "Parcial",
              failed: "Pedido falhado",
              ready: "Pronto",
            }[v.state] ?? v.state)
          : "A carregar…"}
      </p>
      {!error && v?.attempts.at(-1)?.state === "failed" && (
        <p className="practice__notice practice__notice--error" role="alert">
          {attemptError(v.attempts.at(-1)!.error)}
        </p>
      )}
      {v?.state !== "ready" && (
        <>
          <button
            disabled={busy || v?.state === "running"}
            onClick={() => void generate()}
          >
            Gerar ou repetir feedback em falta
          </button>
          <p>
            Repetir pede apenas relatórios em falta e pode gerar novo custo.
          </p>
        </>
      )}
      {pending && (
        <button
          disabled={busy}
          onClick={() => {
            setBusy(true);
            void client
              .retryAiPublication(pending)
              .then(() => client.loadFeedback(sessionId))
              .then(setV)
              .catch((e) => setError(toAppError(e).message))
              .finally(() => setBusy(false));
          }}
        >
          Guardar resultado recebido sem novo pedido
        </button>
      )}
      {error && <p role="alert">{error}</p>}
      {v && (
        <>
          <p>
            Subtotal reportado: US${v.usage.knownReportedSubtotal.toFixed(6)}.
            Subtotal estimado: US${v.usage.knownEstimatedSubtotal.toFixed(6)}.{" "}
            {v.usage.unknownAttemptCount} pedidos com custo desconhecido.{" "}
            {v.usage.complete
              ? "Consumo recebido para todos os pedidos registados."
              : "Valores parciais; não representam o custo total."}
          </p>
          <p>
            Tokens de entrada: {v.usage.input ?? "desconhecidos"}; saída:{" "}
            {v.usage.output ?? "desconhecidos"}.
          </p>
          {v.priorities.length > 0 && (
            <>
              <h4>Prioridades do próximo treino</h4>
              <ul>
                {v.priorities.map((p, i) => (
                  <li key={i}>
                    {p.priority.action} — pergunta{" "}
                    {v.reports.find((r) => r.questionId === p.questionId)
                      ?.position ?? ""}
                  </li>
                ))}
              </ul>
            </>
          )}
          <details>
            <summary>Pedidos e consumo</summary>
            {v.attempts.map((a) => (
              <article key={a.id}>
                <p>
                  {a.purpose === "follow_up" ? "Seguimento" : "Feedback"}:{" "}
                  {attemptStates[a.state] ?? a.state}. Modelo pedido: {a.requestedModel ?? "desconhecido"}
                  ; devolvido: {a.returnedModel ?? "desconhecido"}.
                </p>
                <p>
                  Entrada: {a.usage.input ?? "desconhecida"}; saída:{" "}
                  {a.usage.output ?? "desconhecida"}; custo reportado:{" "}
                  {a.usage.reported == null
                    ? "desconhecido"
                    : `US$${a.usage.reported.toFixed(6)}`}
                  ; estimativa:{" "}
                  {a.usage.estimated == null
                    ? "indisponível"
                    : `US$${a.usage.estimated.toFixed(6)}`}
                  .
                </p>
                {a.state === "failed" && <p>{attemptError(a.error)}</p>}
              </article>
            ))}
          </details>
          {v.reports.map((r) => (
            <QuestionReportView key={r.questionId} r={r} />
          ))}
        </>
      )}
    </section>
  );
}

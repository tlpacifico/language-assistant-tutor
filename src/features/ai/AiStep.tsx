import { useState } from "react";
import type { InterviewSnapshot } from "../../lib/interview-contracts";
import type { InterviewClient } from "../../lib/interview-client";
import { newRequestId } from "../../lib/request-id";
import { toAppError } from "../../lib/app-error";
export default function AiStep({
  client,
  snapshot,
  onSnapshot,
}: {
  client: InterviewClient;
  snapshot: InterviewSnapshot;
  onSnapshot: (s: InterviewSnapshot) => void;
}) {
  const [error, setError] = useState(""),
    [busy, setBusy] = useState(false);
  const q = snapshot.current,
    s = snapshot.activeSession;
  if (!q || !s || s.mode !== "ai_v3" || !q.confirmation) return null;
  const stage = q.followUpStage ?? "not_requested",
    closed =
      ["waived", "not_needed"].includes(stage) || !!q.followUp?.confirmation;
  async function run(work: () => Promise<InterviewSnapshot>) {
    setBusy(true);
    setError("");
    try {
      onSnapshot(await work());
    } catch (e) {
      setError(toAppError(e).message);
    } finally {
      setBusy(false);
    }
  }
  const command = () => ({
    requestId: newRequestId(),
    sessionId: s!.id,
    sessionQuestionId: q!.question.id,
    expectedSessionVersion: s!.version,
  });
  return (
    <section className="practice__panel">
      <h3>Seguimento IA</h3>
      <p>
        {stage === "waived"
          ? "Seguimento dispensado pelo utilizador; não foi uma decisão de suficiência da IA."
          : stage === "not_needed"
            ? "A IA recomendou avançar sem seguimento."
            : q.followUp
              ? "Seguimento contextual: responda e confirme para avançar."
              : stage === "running"
                ? "A aguardar o serviço externo…"
                : "A resposta está guardada. Peça a decisão de seguimento ou dispense se o serviço estiver indisponível."}
      </p>
      {!closed && !q.followUp && (
        <>
          <button
            disabled={busy || stage === "running"}
            onClick={() => void run(() => client.requestFollowUp(command()))}
          >
            {stage === "failed"
              ? "Repetir pedido de seguimento"
              : "Pedir seguimento"}
          </button>
          <button
            disabled={busy}
            onClick={() =>
              void run(() =>
                client.waiveFollowUp({
                  ...command(),
                  reason:
                    stage === "running"
                      ? "user_cancelled_wait"
                      : stage === "failed"
                        ? "request_failed"
                        : "unavailable",
                }),
              )
            }
          >
            Dispensar seguimento explicitamente
          </button>
        </>
      )}
      {stage === "failed" && (
        <p>
          Repetir pode gerar um novo custo se o fornecedor já processou o pedido
          anterior. Se a causa for a chave, substitua-a na Configuração IA;
          também pode dispensar o seguimento para desbloquear o avanço.
        </p>
      )}
      {snapshot.pendingAiPublication && (
        <button
          disabled={busy}
          onClick={() =>
            void run(() =>
              client.retryAiPublication(snapshot.pendingAiPublication!),
            )
          }
        >
          Guardar resultado recebido sem novo pedido
        </button>
      )}
      {error && <p role="alert">{error}</p>}
    </section>
  );
}

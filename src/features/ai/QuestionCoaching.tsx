import { useEffect, useRef, useState } from "react";
import type { FeedbackView } from "../../lib/ai-contracts";
import { toAppError } from "../../lib/app-error";
import type { InterviewClient } from "../../lib/interview-client";
import type { InterviewSnapshot } from "../../lib/interview-contracts";
import type { ResponseOrigin } from "../../lib/practice-contracts";
import { newRequestId } from "../../lib/request-id";
import RewritePanel from "../practice/RewritePanel";
import { UsageLine } from "../practice/shared";
import { useAiSettled } from "../practice/useAiSettled";
import { QuestionReportView } from "./FeedbackPanel";

interface Props {
  client: InterviewClient;
  snapshot: InterviewSnapshot;
  onSnapshot: (snapshot: InterviewSnapshot) => void;
}

/** Sem chamadas pagas ao montar: cada sequência nasce de um clique explícito. */
export default function QuestionCoaching(props: Props) {
  const { current, activeSession } = props.snapshot;
  if (!current?.confirmation || activeSession?.mode !== "ai_v3") return null;
  const closed =
    ["waived", "not_needed"].includes(current.followUpStage ?? "") ||
    (current.followUpStage === "follow_up" && !!current.followUp?.confirmation);
  if (!closed) return null;
  return (
    <Coaching key={`${activeSession.id}:${current.question.id}`} {...props} />
  );
}

function Coaching({ client, snapshot, onSnapshot }: Props) {
  const session = snapshot.activeSession!;
  const question = snapshot.current!.question;
  const origin: ResponseOrigin = {
    type: "original",
    sessionId: session.id,
    sessionQuestionId: question.id,
  };
  const [view, setView] = useState<FeedbackView | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [reload, setReload] = useState(0);
  const live = useRef(false);
  const activeRequest = useRef(false);
  const settled = useAiSettled(snapshot);
  useEffect(() => {
    live.current = true;
    return () => {
      live.current = false;
    };
  }, []);
  useEffect(() => {
    let current = true;
    void client
      .loadFeedback(session.id)
      .then((v) => {
        if (current) setView(v);
      })
      .catch((e) => {
        if (current) setError(toAppError(e).message);
      });
    return () => {
      current = false;
    };
  }, [client, session.id, settled]);
  const report = view?.reports.find((r) => r.questionId === question.id);

  async function improve() {
    if (activeRequest.current) return;
    activeRequest.current = true;
    setBusy(true);
    setError("");
    try {
      // Relê localmente para reutilizar resultados já guardados, inclusive após falha parcial.
      let result = await client.loadFeedback(session.id);
      if (!live.current) return;
      if (!result.reports.some((r) => r.questionId === question.id)) {
        result = await client.evaluateQuestion({
          requestId: newRequestId(),
          sessionId: session.id,
          sessionQuestionId: question.id,
          expectedSessionVersion: session.version,
        });
      }
      if (!live.current) return;
      setView(result);
      if (!result.reports.some((r) => r.questionId === question.id)) {
        throw {
          code: "AI_PENDING",
          message:
            "A avaliação ainda não está disponível. Aguarde ou repita apenas o pedido em falta.",
          retryable: true,
        };
      }
      const preference = await client.loadRewritePreference();
      if (!live.current) return;
      await client.requestRewrite({
        requestId: newRequestId(),
        origin,
        level: preference.level,
      });
      if (live.current) setReload((value) => value + 1);
    } catch (e) {
      if (live.current) setError(toAppError(e).message);
    } finally {
      activeRequest.current = false;
      if (live.current) setBusy(false);
    }
  }

  return (
    <section
      className="practice__panel practice__panel--wide"
      aria-label="Avaliação desta pergunta"
    >
      <h3>Avaliação e correções da IA</h3>
      <p className="practice__hint">
        A IA avalia o conteúdo e o inglês, explica as correções e propõe uma
        versão melhorada. O original fica preservado. Pode avançar sem pedir
        esta avaliação.
      </p>
      <button
        type="button"
        disabled={
          !view || busy || !!snapshot.aiJob || !!snapshot.pendingAiPublication
        }
        onClick={() => void improve()}
      >
        {busy
          ? "A preparar avaliação e sugestão…"
          : report
            ? "Sugerir versão melhorada"
            : "Avaliar e sugerir melhorias"}
      </button>
      <p className="practice__hint">
        Esta ação pede avaliação e reformulação à API configurada e pode gerar
        custo. Resultados válidos guardados são reutilizados; repetir após falha
        pode gerar novo custo.
      </p>
      {error && <p role="alert">{error}</p>}
      {report && <QuestionReportView r={report} />}
      {view && (
        <details>
          <summary>Consumo da entrevista (sem reformulações)</summary>
          <UsageLine usage={view.usage} />
        </details>
      )}
      {snapshot.pendingAiPublication && (
        <button
          type="button"
          disabled={busy}
          onClick={() => {
            setBusy(true);
            void client
              .retryAiPublication(snapshot.pendingAiPublication!)
              .then(onSnapshot)
              .catch((e) => {
                if (live.current) setError(toAppError(e).message);
              })
              .finally(() => {
                if (live.current) setBusy(false);
              });
          }}
        >
          Guardar resultado recebido sem novo pedido
        </button>
      )}
      {report && (
        <RewritePanel
          key={reload}
          client={client}
          origin={origin}
          snapshot={snapshot}
          onSnapshot={onSnapshot}
          showPendingPublication={false}
        />
      )}
    </section>
  );
}

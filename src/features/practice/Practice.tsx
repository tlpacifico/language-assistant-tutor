/**
 * Prática individual: nova tentativa de uma pergunta de uma sessão concluída.
 *
 * A prática tem dono e alvo de resposta próprios: não existe sessão fictícia,
 * não altera a posição, a contagem, o seguimento, a confirmação nem o relatório
 * originais e nunca gera seguimento. Criar, gravar e confirmar são locais;
 * reformular e avaliar são pedidos explícitos que seguem para a API. A captura,
 * a transcrição e a reprodução usam o estado único partilhado com a entrevista.
 *
 * O rascunho da revisão vem de `useInterview` (uma única revisão editável por
 * gravação) e a decisão guardar/descartar/cancelar é a mesma da entrevista.
 */

import { useCallback, useEffect, useRef, useState } from "react";

import { toAppError } from "../../lib/app-error";
import type { AppError } from "../../lib/contracts";
import type { InterviewClient } from "../../lib/interview-client";
import {
  CATEGORY_LABELS,
  type InterviewSnapshot,
  type RecordingView,
} from "../../lib/interview-contracts";
import type {
  ComparisonRequest,
  PracticeDetail,
  PracticeReportView,
  PracticeStatus,
} from "../../lib/practice-contracts";
import { newRequestId } from "../../lib/request-id";
import { CorrectionView, ObservationView } from "../ai/FeedbackPanel";
import type { InterviewReview } from "../interview/useInterview";
import { latestTranscript, practiceTargetOf } from "../interview/useInterview";
import StorySupport from "../stories/StorySupport";
import Comparison from "./Comparison";
import RewritePanel from "./RewritePanel";
import { UsageLine, errorKey, formatMoment } from "./shared";

const STATUS_LABELS: Record<PracticeStatus, string> = {
  preparing: "Em preparação",
  answering: "A responder",
  confirmed: "Confirmada",
  abandoned: "Abandonada",
};

const REPORT_STATE_LABELS: Record<PracticeReportView["state"], string> = {
  pending: "Ainda sem avaliação",
  running: "Avaliação em processamento",
  failed: "O pedido de avaliação falhou",
  ready: "Avaliação pronta",
};

function formatDuration(durationMs: number): string {
  return `${(Math.max(0, durationMs) / 1000).toFixed(1).replace(".", ",")} s`;
}

export interface PracticeProps {
  client: InterviewClient;
  snapshot: InterviewSnapshot;
  onSnapshot: (snapshot: InterviewSnapshot) => void;
  review: InterviewReview;
  practiceId: string;
  requestDecision: (
    intention: "navigate" | "record" | "close" | "switch",
    proceed: () => void | Promise<void>,
  ) => void;
  closing?: boolean;
  onRecordingPending?: (pending: boolean) => void;
  /** Volta ao histórico. */
  onBack: () => void;
  /** Abre outra prática (por exemplo, a nova tentativa). */
  onOpenPractice: (practiceId: string) => void;
}

export default function Practice({
  client,
  snapshot,
  onSnapshot,
  review,
  practiceId,
  requestDecision,
  closing = false,
  onRecordingPending,
  onBack,
  onOpenPractice,
}: PracticeProps) {
  const live =
    snapshot.practice?.activePractice?.id === practiceId
      ? snapshot.practice.activePractice
      : null;
  const [loaded, setLoaded] = useState<PracticeDetail | null>(null);
  const [loadError, setLoadError] = useState<AppError | null>(null);
  const [actionError, setActionError] = useState<AppError | null>(null);
  const [busyAction, setBusyAction] = useState<string | null>(null);
  const [confirmingAbandon, setConfirmingAbandon] = useState(false);
  const [report, setReport] = useState<PracticeReportView | null>(null);
  const [evaluating, setEvaluating] = useState(false);
  const [comparing, setComparing] = useState<ComparisonRequest | null>(null);
  const [retryingPublication, setRetryingPublication] = useState(false);
  const evaluateIntention = useRef<string | null>(null);
  const createIntention = useRef<{ key: string; requestId: string } | null>(
    null,
  );
  const abandonIntention = useRef<{ key: string; requestId: string } | null>(
    null,
  );

  // A prática pendente vem da fotografia; confirmada ou abandonada, é lida.
  useEffect(() => {
    if (live) {
      return;
    }
    let active = true;
    client
      .loadPractice(practiceId)
      .then((next) => {
        if (active) {
          setLoaded(next);
          setLoadError(null);
        }
      })
      .catch((failure: unknown) => {
        if (active) {
          setLoadError(toAppError(failure));
        }
      });
    return () => {
      active = false;
    };
  }, [client, practiceId, live === null, snapshot.revision]);

  const detail: PracticeDetail | null =
    live ?? (loaded && loaded.id === practiceId ? loaded : null);

  const loadReport = useCallback(async (): Promise<void> => {
    try {
      setReport(await client.loadPracticeReport(practiceId));
    } catch (failure) {
      setActionError(toAppError(failure));
    }
  }, [client, practiceId]);

  const confirmed = detail?.status === "confirmed";
  useEffect(() => {
    setReport(null);
    if (confirmed) {
      void loadReport();
    }
  }, [confirmed, loadReport]);

  const recordingActive = snapshot.recordingId !== null;
  const finalizing = snapshot.finalizing;
  const captureBusy = recordingActive || finalizing;
  const modelJob = snapshot.modelJob;

  const run = useCallback(
    async (
      action: () => Promise<InterviewSnapshot>,
    ): Promise<InterviewSnapshot | null> => {
      setActionError(null);
      try {
        const next = await action();
        onSnapshot(next);
        return next;
      } catch (failure) {
        setActionError(toAppError(failure));
        return null;
      }
    },
    [onSnapshot],
  );

  if (!detail) {
    return (
      <section className="practice__page" aria-labelledby="practice-title">
        <h2 id="practice-title">Prática</h2>
        {loadError ? (
          <div
            className="practice__notice practice__notice--error"
            role="alert"
          >
            <p>Não foi possível ler a prática.</p>
            <p>{loadError.message}</p>
          </div>
        ) : (
          <p className="practice__hint" role="status">
            A carregar a prática…
          </p>
        )}
        <div className="practice__actions">
          <button type="button" onClick={onBack}>
            Voltar ao histórico
          </button>
        </div>
      </section>
    );
  }

  const practice: PracticeDetail = detail;
  const pending =
    practice.status === "preparing" || practice.status === "answering";
  const owner = {
    type: "practice" as const,
    practiceId: practice.id,
    answerId: practice.answer.id,
  };
  const selectedRecording: RecordingView | null =
    practice.recordings.find(
      (take) => take.id === practice.answer.selectedRecordingId,
    ) ?? null;
  const confirmedHere = practice.confirmation;
  const canRecord =
    pending &&
    !captureBusy &&
    modelJob === null &&
    busyAction === null &&
    !review.confirming &&
    !closing &&
    snapshot.practice.pendingPreparation === null;
  const canUseAudio = !captureBusy && busyAction === null && !closing;
  const hasEvidence = practice.recordings.length > 0;
  const originOrigin = {
    type: "original" as const,
    sessionId: practice.sourceSessionId,
    sessionQuestionId: practice.sessionQuestionId,
  };

  async function startRecording(): Promise<void> {
    if (!canRecord) {
      return;
    }
    setBusyAction("record");
    setActionError(null);
    onRecordingPending?.(true);
    try {
      onSnapshot(
        await client.startPracticeRecording(
          owner,
          practice.answer.selectionVersion,
        ),
      );
    } catch (failure) {
      setActionError(toAppError(failure));
    } finally {
      onRecordingPending?.(false);
      setBusyAction(null);
    }
  }

  async function stopRecording(): Promise<void> {
    if (!recordingActive || busyAction !== null) {
      return;
    }
    setBusyAction("stop");
    setActionError(null);
    try {
      onSnapshot(await client.stopRecording());
    } catch (failure) {
      setActionError(toAppError(failure));
    } finally {
      setBusyAction(null);
    }
  }

  async function abandon(): Promise<void> {
    const key = `${practice.id}:${practice.version}`;
    if (abandonIntention.current?.key !== key) {
      abandonIntention.current = { key, requestId: newRequestId() };
    }
    const requestId = abandonIntention.current.requestId;
    setBusyAction("abandon");
    setActionError(null);
    try {
      await client.abandonPractice({
        requestId,
        practiceId: practice.id,
        expectedVersion: practice.version,
      });
      abandonIntention.current = null;
      setConfirmingAbandon(false);
      onSnapshot(await client.snapshot());
    } catch (failure) {
      setActionError(toAppError(failure));
    } finally {
      setBusyAction(null);
    }
  }

  async function evaluate(): Promise<void> {
    if (evaluating) {
      return;
    }
    // Um novo pedido depois de uma falha conta como nova intenção.
    const fresh = report?.state === "failed" || evaluateIntention.current === null;
    const requestId = fresh ? newRequestId() : evaluateIntention.current!;
    evaluateIntention.current = requestId;
    setEvaluating(true);
    setActionError(null);
    try {
      const next = await client.evaluatePractice({
        requestId,
        practiceId: practice.id,
        expectedVersion: practice.version,
      });
      setReport(next);
      if (next.state !== "failed") {
        evaluateIntention.current = null;
      }
    } catch (failure) {
      evaluateIntention.current = null;
      setActionError(toAppError(failure));
      await loadReport();
    } finally {
      setEvaluating(false);
    }
  }

  async function retryAgain(): Promise<void> {
    const key = `${practice.id}:${practice.version}`;
    if (createIntention.current?.key !== key) {
      createIntention.current = { key, requestId: newRequestId() };
    }
    setBusyAction("create");
    setActionError(null);
    try {
      // Criar uma nova tentativa é local: não gera avaliação nem sugestão.
      const created = await client.createPractice({
        requestId: createIntention.current.requestId,
        sessionId: practice.sourceSessionId,
        sessionQuestionId: practice.sessionQuestionId,
        previousPracticeId: practice.id,
      });
      createIntention.current = null;
      onSnapshot(await client.snapshot());
      onOpenPractice(created.id);
    } catch (failure) {
      setActionError(toAppError(failure));
    } finally {
      setBusyAction(null);
    }
  }

  async function savePending(id: string): Promise<void> {
    setRetryingPublication(true);
    setActionError(null);
    try {
      onSnapshot(await client.retryAiPublication(id));
      await loadReport();
    } catch (failure) {
      setActionError(toAppError(failure));
    } finally {
      setRetryingPublication(false);
    }
  }

  let reviewStatus = "Revisão por guardar";
  if (review.confirming) {
    reviewStatus = "A confirmar a resposta…";
  } else if (review.saving) {
    reviewStatus = "A guardar revisão…";
  } else if (review.dirty) {
    reviewStatus = "Alterações por guardar";
  } else if (review.version > 0) {
    reviewStatus = "Revisão guardada";
  }
  const latest = latestTranscript(selectedRecording);
  const errors: AppError[] = [];
  for (const error of [actionError, snapshot.lastError]) {
    if (error && !errors.some((seen) => errorKey(seen) === errorKey(error))) {
      errors.push(error);
    }
  }

  return (
    <section className="practice__page" aria-labelledby="practice-title">
      <h2 id="practice-title">Prática {practice.ordinal}</h2>
      <p className="practice__state" role="status">
        {STATUS_LABELS[practice.status]}
      </p>
      <p className="practice__hint">
        Nova tentativa da pergunta de uma sessão concluída, criada em{" "}
        {formatMoment(practice.createdAt)}. Não altera a sessão original, o
        seguimento nem o relatório original e não gera seguimento.
      </p>
      <p className="practice__hint">
        Categoria: {CATEGORY_LABELS[practice.question.category]}.
      </p>
      <p className="practice__question" lang="en">
        {practice.question.text}
      </p>

      <div className="practice__actions">
        <button
          type="button"
          disabled={!canUseAudio}
          onClick={() =>
            void run(() =>
              client.playAudio({
                type: "practice_question",
                practiceId: practice.id,
              }),
            )
          }
        >
          Ouvir pergunta
        </button>
        {snapshot.playback ? (
          <button
            type="button"
            onClick={() => void run(() => client.stopAudio())}
            disabled={busyAction !== null || closing}
          >
            Parar áudio
          </button>
        ) : null}
        <button type="button" onClick={onBack}>
          Voltar ao histórico
        </button>
      </div>

      {pending ? (
        <>
          <StorySupport
            client={client}
            owner={owner}
            snapshot={snapshot}
            onSnapshot={onSnapshot}
            editable={
              !captureBusy &&
              !hasEvidence &&
              modelJob === null &&
              busyAction === null
            }
          />

          <section className="practice__panel" aria-labelledby="practice-capture">
            <h3 id="practice-capture">Gravação</h3>
            {recordingActive ? (
              <p className="practice__state" role="status">
                Gravação em curso
              </p>
            ) : finalizing ? (
              <p className="practice__state" role="status">
                A finalizar e guardar a gravação…
              </p>
            ) : practice.recordings.length === 0 ? (
              <p className="practice__hint">
                Ainda não existe uma resposta gravada para esta prática.
              </p>
            ) : null}
            {modelJob ? (
              <p className="practice__state" role="status">
                {modelJob.stage === "loading"
                  ? "A carregar o modelo de transcrição…"
                  : "A transcrever a resposta…"}
              </p>
            ) : null}
            <div className="practice__actions">
              {recordingActive ? (
                <button
                  type="button"
                  onClick={() => void stopRecording()}
                  disabled={busyAction !== null || closing}
                >
                  Terminar gravação
                </button>
              ) : (
                <button
                  type="button"
                  disabled={!canRecord}
                  onClick={() =>
                    requestDecision("record", () => startRecording())
                  }
                >
                  {practice.recordings.length > 0
                    ? "Gravar novamente"
                    : "Iniciar gravação"}
                </button>
              )}
              {selectedRecording &&
              selectedRecording.state === "ready" &&
              !captureBusy &&
              modelJob === null ? (
                <button
                  type="button"
                  disabled={busyAction !== null || closing}
                  onClick={() =>
                    void run(() =>
                      client.transcribePracticeResponse(
                        practiceTargetOf(practice, selectedRecording.id),
                      ),
                    )
                  }
                >
                  {selectedRecording.transcripts.length > 0
                    ? "Transcrever novamente"
                    : "Transcrever resposta"}
                </button>
              ) : null}
            </div>
            <p className="practice__hint">
              A gravação começa e termina por sua ação. Uma nova tomada preserva
              as anteriores.
            </p>
          </section>

          {practice.recordings.length > 0 ? (
            <section className="practice__panel" aria-labelledby="practice-takes">
              <h3 id="practice-takes">Gravações desta prática</h3>
              <ul className="practice__list">
                {practice.recordings.map((take) => (
                  <li key={take.id} className="practice__item">
                    <p className="practice__meta">Tomada {take.ordinal}</p>
                    {take.capture ? (
                      <p className="practice__hint">
                        Duração: {formatDuration(take.capture.durationMs)}.
                      </p>
                    ) : null}
                    <div className="practice__actions">
                      <button
                        type="button"
                        disabled={!canUseAudio}
                        onClick={() =>
                          void run(() =>
                            client.playAudio({
                              type: "recording",
                              recordingId: take.id,
                            }),
                          )
                        }
                      >
                        Ouvir tomada {take.ordinal}
                      </button>
                      {take.state === "reserved" ||
                      take.state === "publish_pending" ? (
                        <button
                          type="button"
                          disabled={!canUseAudio}
                          onClick={() =>
                            void run(() => client.retrySaveRecording(take.id))
                          }
                        >
                          Repetir guardar gravação
                        </button>
                      ) : null}
                    </div>
                  </li>
                ))}
              </ul>
            </section>
          ) : null}

          <section
            className="practice__panel practice__panel--wide"
            aria-labelledby="practice-review"
          >
            <h3 id="practice-review">Transcrição e revisão</h3>
            {!selectedRecording ? (
              <p className="practice__hint">
                Grave uma resposta guardada para criar a transcrição automática
                e a revisão. Não é possível confirmar sem uma gravação.
              </p>
            ) : (
              <>
                <div className="practice__field">
                  <label htmlFor="practice-automatic">
                    Transcrição automática
                  </label>
                  <textarea
                    id="practice-automatic"
                    className="practice__textarea practice__textarea--raw"
                    value={review.automaticText}
                    readOnly
                    rows={4}
                    lang="en"
                    placeholder="O texto devolvido pelo motor aparece aqui, sem alterações."
                  />
                  <p className="practice__hint">
                    Versão do motor, apenas de leitura. Não é uma avaliação da
                    resposta.
                  </p>
                </div>
                {latest?.status === "no_speech" ? (
                  <p className="practice__warning">
                    O motor não reconheceu fala nesta gravação. Pode ouvir o
                    áudio, gravar outra tomada ou introduzir uma revisão fiel
                    ao áudio.
                  </p>
                ) : null}
                {review.origin === "manual_explicit" ? (
                  <p className="practice__warning">
                    Texto introduzido manualmente: não existe texto automático
                    utilizável nesta gravação.
                  </p>
                ) : null}
                <div className="practice__field">
                  <label htmlFor="practice-review-text">
                    Revisão editável
                  </label>
                  <textarea
                    id="practice-review-text"
                    className="practice__textarea"
                    value={review.text}
                    onChange={(event) =>
                      review.setReviewText(event.target.value)
                    }
                    readOnly={!review.canEdit}
                    rows={6}
                    lang="en"
                    aria-describedby="practice-review-status"
                  />
                  <p
                    className="practice__review-status"
                    id="practice-review-status"
                    role="status"
                  >
                    {reviewStatus}
                  </p>
                  <p className="practice__hint">
                    A revisão parte só da transcrição automática; o apoio de
                    história nunca a preenche. «Guardar revisão» não confirma;
                    ao confirmar, o texto fica fixo.
                  </p>
                </div>
                {review.error ? (
                  <div
                    className="practice__notice practice__notice--error"
                    role="alert"
                  >
                    <p>{review.error.message}</p>
                    <p className="practice__hint">
                      Código: {review.error.code}.
                    </p>
                  </div>
                ) : null}
                <div className="practice__actions">
                  <button
                    type="button"
                    onClick={() => void review.saveReview()}
                    disabled={!review.canSave || busyAction !== null || closing}
                  >
                    Guardar revisão
                  </button>
                  <button
                    type="button"
                    onClick={() => void review.confirm()}
                    disabled={
                      !review.canEdit ||
                      review.saving ||
                      review.confirming ||
                      busyAction !== null ||
                      closing ||
                      review.text.trim() === ""
                    }
                  >
                    Confirmar resposta
                  </button>
                </div>
              </>
            )}
          </section>

          <section className="practice__panel" aria-labelledby="practice-abandon">
            <h3 id="practice-abandon">Abandonar a prática</h3>
            {confirmingAbandon ? (
              <>
                <p className="practice__warning">
                  Abandonar conserva as gravações e as revisões para leitura
                  futura, mas a prática deixa de poder ser retomada, confirmada
                  ou avaliada. As respostas originais não são alteradas.
                </p>
                <div className="practice__actions">
                  <button
                    type="button"
                    disabled={busyAction !== null || captureBusy}
                    onClick={() =>
                      requestDecision("navigate", () => abandon())
                    }
                  >
                    Confirmar abandono
                  </button>
                  <button
                    type="button"
                    onClick={() => setConfirmingAbandon(false)}
                  >
                    Manter a prática
                  </button>
                </div>
              </>
            ) : (
              <div className="practice__actions">
                <button
                  type="button"
                  disabled={captureBusy || busyAction !== null || closing}
                  onClick={() => setConfirmingAbandon(true)}
                >
                  Abandonar prática
                </button>
              </div>
            )}
          </section>
        </>
      ) : null}

      {practice.status === "abandoned" ? (
        <section className="practice__panel" aria-label="Prática abandonada">
          <h3>Prática abandonada</h3>
          <p className="practice__hint">
            Abandonada
            {practice.abandonedAt
              ? ` em ${formatMoment(practice.abandonedAt)}`
              : ""}
            . Não pode ser retomada, confirmada nem avaliada; as gravações e as
            revisões ficam apenas para leitura.
          </p>
          {practice.recordings.map((take) => (
            <div key={take.id} className="practice__field">
              <p className="practice__meta">Tomada {take.ordinal}</p>
              {take.review ? <p lang="en">{take.review.text}</p> : null}
              <div className="practice__actions">
                <button
                  type="button"
                  disabled={!canUseAudio}
                  onClick={() =>
                    void run(() =>
                      client.playAudio({
                        type: "recording",
                        recordingId: take.id,
                      }),
                    )
                  }
                >
                  Ouvir tomada {take.ordinal}
                </button>
              </div>
            </div>
          ))}
        </section>
      ) : null}

      {practice.status === "confirmed" && confirmedHere ? (
        <>
          <section
            className="practice__panel practice__panel--wide"
            aria-labelledby="practice-confirmed"
          >
            <h3 id="practice-confirmed">Resposta confirmada da tentativa</h3>
            <p lang="en">{confirmedHere.text}</p>
            <p className="practice__hint">
              Confirmada em {formatMoment(confirmedHere.confirmedAt)}. Fica
              apenas de leitura.
            </p>
            <div className="practice__actions">
              {practice.recordings
                .filter((take) => take.id === confirmedHere.recordingId)
                .map((take) => (
                  <button
                    key={take.id}
                    type="button"
                    disabled={!canUseAudio}
                    onClick={() =>
                      void run(() =>
                        client.playAudio({
                          type: "recording",
                          recordingId: take.id,
                        }),
                      )
                    }
                  >
                    Ouvir original da tentativa
                  </button>
                ))}
            </div>
          </section>

          {snapshot.pendingAiPublication ? (
            <div className="practice__notice" role="status">
              <p>
                Existe um resultado já recebido que ainda não foi guardado.
                Guardar não faz um novo pedido à rede.
              </p>
              <div className="practice__actions">
                <button
                  type="button"
                  disabled={retryingPublication}
                  onClick={() => void savePending(snapshot.pendingAiPublication!)}
                >
                  Guardar resultado recebido sem novo pedido
                </button>
              </div>
            </div>
          ) : null}

          <section
            className="practice__panel practice__panel--wide"
            aria-labelledby="practice-report"
          >
            <h3 id="practice-report">Avaliação textual da tentativa</h3>
            <p className="practice__hint">
              Avaliação do texto confirmado desta tentativa. Não é um
              diagnóstico de pronúncia, de fluência acústica nem de confiança
              emocional.
            </p>
            <p className="practice__state" role="status">
              {report ? REPORT_STATE_LABELS[report.state] : "A carregar…"}
            </p>
            {report?.state === "ready" && report.report ? (
              <>
                <p className="practice__hint">
                  Rubrica {report.rubricVersion}; prompt {report.promptVersion};
                  esquema {report.schemaVersion}.
                </p>
                <h4>Conteúdo e STAR</h4>
                {report.report.content.map((entry) => (
                  <ObservationView key={entry.id} o={entry} />
                ))}
                <h4>Pontos fortes</h4>
                {report.report.strengths.map((entry) => (
                  <ObservationView key={entry.id} o={entry} />
                ))}
                <h4>Lacunas</h4>
                {report.report.gaps.map((entry) => (
                  <ObservationView key={entry.id} o={entry} />
                ))}
                <h4>Correções necessárias no texto inglês</h4>
                {report.report.english.necessaryCorrections.map((entry) => (
                  <CorrectionView key={entry.id} o={entry} />
                ))}
                <h4>Alternativas opcionais de estilo</h4>
                {report.report.english.optionalStyle.map((entry) => (
                  <CorrectionView key={entry.id} o={entry} />
                ))}
              </>
            ) : null}
            {report?.state === "failed" ? (
              <p className="practice__hint">
                Repetir é um novo pedido e pode gerar novo custo se o
                fornecedor já processou o anterior.
              </p>
            ) : null}
            {report?.state !== "ready" ? (
              <div className="practice__actions">
                <button
                  type="button"
                  disabled={
                    evaluating ||
                    report === null ||
                    report.state === "running"
                  }
                  onClick={() => void evaluate()}
                >
                  {report?.state === "failed"
                    ? "Repetir avaliação da tentativa"
                    : "Avaliar tentativa"}
                </button>
              </div>
            ) : null}
            {evaluating ? (
              <p role="status">A aguardar o serviço externo…</p>
            ) : null}
            <p className="practice__hint">
              A avaliação envia à API do OpenCode apenas o texto confirmado
              desta tentativa e os critérios da pergunta. Pode ter custo e não
              cria seguimento.
            </p>
            {report ? (
              <details>
                <summary>Consumo desta prática</summary>
                <UsageLine
                  usage={report.usage}
                  networkAttemptCount={report.networkAttempts.length}
                />
              </details>
            ) : null}
          </section>

          <RewritePanel
            client={client}
            origin={{ type: "practice", practiceId: practice.id }}
            snapshot={snapshot}
            onSnapshot={onSnapshot}
            showPendingPublication={false}
          />

          <section className="practice__panel" aria-labelledby="practice-next">
            <h3 id="practice-next">Continuar o treino</h3>
            <div className="practice__actions">
              <button
                type="button"
                onClick={() =>
                  setComparing({
                    left: originOrigin,
                    right: { type: "practice", practiceId: practice.id },
                  })
                }
              >
                Comparar com a resposta original
              </button>
              {practice.previousPracticeId ? (
                <button
                  type="button"
                  onClick={() =>
                    setComparing({
                      left: {
                        type: "practice",
                        practiceId: practice.previousPracticeId!,
                      },
                      right: { type: "practice", practiceId: practice.id },
                    })
                  }
                >
                  Comparar com a prática anterior
                </button>
              ) : null}
              <button
                type="button"
                disabled={
                  busyAction !== null ||
                  snapshot.activeSession !== null ||
                  closing
                }
                onClick={() => void retryAgain()}
              >
                Tentar novamente
              </button>
            </div>
            {snapshot.activeSession ? (
              <p className="practice__hint">
                Termine a entrevista em curso antes de praticar.
              </p>
            ) : null}
          </section>
          {comparing ? (
            <Comparison
              client={client}
              request={comparing}
              onClose={() => setComparing(null)}
            />
          ) : null}
        </>
      ) : null}

      {errors.length > 0 ? (
        <section
          className="practice__panel practice__panel--error"
          aria-labelledby="practice-errors"
        >
          <h3 id="practice-errors">Erros</h3>
          <ul className="practice__errors">
            {errors.map((error) => (
              <li key={errorKey(error)} role="alert">
                <p>{error.message}</p>
                <p className="practice__hint">Código: {error.code}.</p>
              </li>
            ))}
          </ul>
        </section>
      ) : null}
    </section>
  );
}

/**
 * Ecrã da entrevista guiada: pergunta, captura, revisão, confirmação e avanço.
 *
 * O núcleo Rust controla a sessão, a seleção e as versões; este ecrã apresenta
 * a fotografia e envia intenções. Não calcula progresso próprio, não repete
 * perguntas e não avalia a resposta: mostra apenas o que o núcleo guardou.
 *
 * As ações que devolvem uma fotografia (`startRecording`, `stopRecording`,
 * `advanceSession`, reprodução) entregam-na a `onSnapshot`; a revisão e a
 * confirmação vivem em `useInterview`, cujo resultado é partilhado com a
 * aplicação para a decisão de guardar/descartar e para o fecho da janela.
 */

import { useRef, useState } from "react";

import type { AppError } from "../../lib/contracts";
import { toAppError } from "../../lib/app-error";
import type { InterviewClient } from "../../lib/interview-client";
import {
  CATEGORY_LABELS,
  type InterviewSnapshot,
  type ModelJobView,
  type RecordingView,
  type RecordingState,
  type SessionDetail,
} from "../../lib/interview-contracts";
import { newRequestId } from "../../lib/request-id";
import type { InterviewReview } from "./useInterview";
import { selectedAnswer } from "./useInterview";

/** Intenção de navegação/captura/fecho resolvida pela decisão partilhada. */
export type InterviewGuardIntention = "navigate" | "record" | "close";

export interface InterviewProps {
  client: InterviewClient;
  snapshot: InterviewSnapshot;
  /** Recebe cada fotografia devolvida por uma mutação deste ecrã. */
  onSnapshot: (snapshot: InterviewSnapshot) => void;
  /** Rascunho e ações da revisão, propriedade única da aplicação. */
  review: InterviewReview;
  /** Pede a decisão guardar/descartar/cancelar antes de continuar. */
  requestDecision: (
    intention: InterviewGuardIntention,
    proceed: () => void | Promise<void>,
  ) => void;
}

const RECORDING_STATE_LABELS: Record<RecordingState, string> = {
  reserved: "Reservada",
  capturing: "Em captura",
  publish_pending: "Por guardar",
  ready: "Guardada",
  interrupted: "Interrompida",
};

/** Duração em segundos, com uma casa decimal e vírgula decimal. */
function formatDuration(durationMs: number): string {
  const seconds = Math.max(0, durationMs) / 1000;
  return `${seconds.toFixed(1).replace(".", ",")} s`;
}

/** Data e hora locais; um valor inválido é mostrado tal como veio. */
function formatMoment(value: string): string {
  const moment = new Date(value);
  if (Number.isNaN(moment.getTime())) {
    return value;
  }
  return moment.toLocaleString("pt-PT", {
    dateStyle: "short",
    timeStyle: "short",
  });
}

function jobMessage(job: ModelJobView): string {
  if (job.kind === "transcribe") {
    return job.stage === "loading"
      ? "A carregar o modelo de transcrição…"
      : "A transcrever a resposta…";
  }
  return job.stage === "loading"
    ? "A carregar o modelo de voz…"
    : "A preparar a pergunta em voz…";
}

function errorKey(error: AppError): string {
  return `${error.code}:${error.message}`;
}

/** Resumo consultável de uma sessão concluída, sem avaliação nem nota. */
function SessionSummaryPanel({ detail }: { detail: SessionDetail }) {
  const { summary } = detail;
  return (
    <section className="practice__panel" aria-labelledby="finished-title">
      <h3 id="finished-title">Sessão concluída</h3>
      <p>
        Respostas confirmadas: {summary.confirmedCount} de {summary.count}.
      </p>
      <p className="practice__hint">
        Categoria:{" "}
        {summary.category
          ? CATEGORY_LABELS[summary.category]
          : "todas as categorias"}
        .
      </p>
      {summary.completedAt ? (
        <p className="practice__hint">
          Concluída em {formatMoment(summary.completedAt)}.
        </p>
      ) : null}
      <p className="practice__hint">
        Os detalhes ficam disponíveis no histórico. Não existe avaliação, nota
        ou feedback pendente nesta fase.
      </p>
    </section>
  );
}

export default function Interview({
  client,
  snapshot,
  onSnapshot,
  review,
  requestDecision,
}: InterviewProps) {
  const [busyAction, setBusyAction] = useState<string | null>(null);
  const [actionError, setActionError] = useState<AppError | null>(null);
  const [advancing, setAdvancing] = useState(false);
  const [finishedSession, setFinishedSession] = useState<SessionDetail | null>(
    null,
  );
  const [finishedSessionId, setFinishedSessionId] = useState<string | null>(
    null,
  );
  const [summaryLoading, setSummaryLoading] = useState(false);
  const [summaryError, setSummaryError] = useState<AppError | null>(null);
  const advancingRef = useRef(false);
  const advanceIntention = useRef<{ key: string; requestId: string } | null>(
    null,
  );

  const session = snapshot.activeSession;
  const current = snapshot.current;
  const answer = selectedAnswer(snapshot);
  const question = current?.question ?? null;
  const confirmation = current?.confirmation ?? null;
  const recordings = current?.recordings ?? [];
  const recording = snapshot.recordingId !== null;
  const finalizing = snapshot.finalizing;
  const captureBusy = recording || finalizing;
  const modelJob = snapshot.modelJob;
  const last = session !== null && session.position >= session.count;

  async function run(
    action: () => Promise<InterviewSnapshot>,
  ): Promise<InterviewSnapshot | null> {
    setActionError(null);
    try {
      const next = await action();
      onSnapshot(next);
      return next;
    } catch (error) {
      setActionError(toAppError(error));
      return null;
    }
  }

  async function startRecording(): Promise<void> {
    if (!question || captureBusy || busyAction !== null) {
      return;
    }
    setBusyAction("record");
    setActionError(null);
    try {
      const next = await client.startRecording(
        question.id,
        question.selectionVersion,
      );
      onSnapshot(next);
    } catch (error) {
      setActionError(toAppError(error));
    } finally {
      setBusyAction(null);
    }
  }

  function requestRecord(): void {
    requestDecision("record", () => startRecording());
  }

  async function stopRecording(): Promise<void> {
    if (!recording || busyAction !== null) {
      return;
    }
    setBusyAction("stop");
    setActionError(null);
    try {
      onSnapshot(await client.stopRecording());
    } catch (error) {
      setActionError(toAppError(error));
    } finally {
      setBusyAction(null);
    }
  }

  async function loadSummary(sessionId: string): Promise<void> {
    setFinishedSessionId(sessionId);
    setSummaryLoading(true);
    setSummaryError(null);
    try {
      setFinishedSession(await client.loadSession(sessionId));
    } catch (error) {
      setSummaryError(toAppError(error));
    } finally {
      setSummaryLoading(false);
    }
  }

  async function advance(): Promise<void> {
    if (
      !session ||
      !question ||
      !confirmation ||
      advancingRef.current ||
      captureBusy
    ) {
      return;
    }
    // Uma intenção de avanço conserva o identificador ao repetir depois de uma
    // falha; dois cliques seguidos nunca produzem dois pedidos.
    const key = `advance:${session.id}:${question.id}:${session.version}`;
    if (advanceIntention.current?.key !== key) {
      advanceIntention.current = { key, requestId: newRequestId() };
    }
    const requestId = advanceIntention.current.requestId;
    const sessionId = session.id;
    advancingRef.current = true;
    setAdvancing(true);
    setActionError(null);
    try {
      const next = await client.advanceSession({
        requestId,
        sessionId,
        sessionQuestionId: question.id,
        expectedSessionVersion: session.version,
      });
      advanceIntention.current = null;
      onSnapshot(next);
      if (last) {
        // Depois da última pergunta a fotografia deixa de ter sessão ativa: o
        // resumo é lido pelo identificador que enviámos.
        await loadSummary(sessionId);
      }
    } catch (error) {
      setActionError(toAppError(error));
    } finally {
      advancingRef.current = false;
      setAdvancing(false);
    }
  }

  if (!session) {
    return (
      <section className="practice__page" aria-labelledby="interview-title">
        <h2 id="interview-title">Entrevista</h2>
        {finishedSession ? (
          <SessionSummaryPanel detail={finishedSession} />
        ) : summaryLoading ? (
          <p className="practice__hint" role="status">
            A carregar o resumo da sessão…
          </p>
        ) : summaryError && finishedSessionId ? (
          <div className="practice__notice practice__notice--error" role="alert">
            <p>Não foi possível ler o resumo da sessão concluída.</p>
            <p>{summaryError.message}</p>
            <p className="practice__hint">Código: {summaryError.code}.</p>
            <div className="practice__actions">
              <button
                type="button"
                onClick={() => void loadSummary(finishedSessionId)}
              >
                Tentar ler novamente
              </button>
            </div>
          </div>
        ) : (
          <p>
            Não existe uma sessão em curso. Comece ou retome uma entrevista na
            preparação.
          </p>
        )}
      </section>
    );
  }

  const canUseAudio = !captureBusy && busyAction === null;
  const canRecord =
    question !== null &&
    confirmation === null &&
    !review.confirmed &&
    !review.confirming &&
    !captureBusy &&
    modelJob === null &&
    busyAction === null;
  const canSaveOrConfirm = !captureBusy && busyAction === null;
  const playback = snapshot.playback;
  const errors: AppError[] = [];
  for (const error of [actionError, snapshot.lastError]) {
    if (error && !errors.some((seen) => errorKey(seen) === errorKey(error))) {
      errors.push(error);
    }
  }

  let reviewStatus = "Revisão por guardar";
  if (review.confirmed) {
    reviewStatus = "Resposta confirmada";
  } else if (review.confirming) {
    reviewStatus = "A confirmar a resposta…";
  } else if (review.saving) {
    reviewStatus = "A guardar revisão…";
  } else if (review.dirty) {
    reviewStatus = "Alterações por guardar";
  } else if (review.version > 0) {
    reviewStatus = "Revisão guardada";
  }

  return (
    <section className="practice__page" aria-labelledby="interview-title">
      <h2 id="interview-title">Entrevista</h2>
      <p className="practice__state">
        Pergunta {session.position} de {session.count}
      </p>
      <p className="practice__hint">
        Categoria:{" "}
        {session.category
          ? CATEGORY_LABELS[session.category]
          : "todas as categorias"}
        . Respostas confirmadas: {session.confirmedCount} de {session.count}.
      </p>

      {question ? (
        <p className="practice__question" lang="en">
          {question.text}
        </p>
      ) : (
        <p className="practice__hint" role="status">
          A carregar a pergunta da sessão…
        </p>
      )}

      <div className="practice__actions">
        <button
          type="button"
          onClick={() =>
            question &&
            void run(() =>
              client.playAudio({
                type: "question",
                sessionId: session.id,
                sessionQuestionId: question.id,
              }),
            )
          }
          disabled={!question || !canUseAudio}
        >
          Ouvir pergunta
        </button>
        {playback ? (
          <button
            type="button"
            onClick={() => void run(() => client.stopAudio())}
            disabled={busyAction !== null}
          >
            Parar áudio
          </button>
        ) : null}
      </div>

      <section className="practice__panel" aria-labelledby="capture-title">
        <h3 id="capture-title">Gravação</h3>
        {recording ? (
          <p className="practice__state" role="status">
            Gravação em curso
          </p>
        ) : finalizing ? (
          <p className="practice__state" role="status">
            A finalizar e guardar a gravação…
          </p>
        ) : recordings.length === 0 ? (
          <p className="practice__hint">
            Ainda não existe uma resposta gravada para esta pergunta.
          </p>
        ) : null}
        {modelJob ? (
          <p className="practice__state" role="status">
            {jobMessage(modelJob)}
          </p>
        ) : null}
        {playback ? (
          <p className="practice__state" role="status">
            {playback === "question"
              ? "A reproduzir a pergunta…"
              : "A reproduzir a resposta…"}
          </p>
        ) : null}
        <div className="practice__actions">
          {recording ? (
            <button
              type="button"
              onClick={() => void stopRecording()}
              disabled={busyAction !== null}
            >
              Terminar gravação
            </button>
          ) : confirmation === null ? (
            <button
              type="button"
              onClick={requestRecord}
              disabled={!canRecord}
            >
              {recordings.length > 0 ? "Gravar novamente" : "Iniciar gravação"}
            </button>
          ) : null}
        </div>
        <p className="practice__hint">
          A gravação começa e termina por sua ação. Uma nova tomada preserva as
          anteriores e não conta como outra pergunta.
        </p>
      </section>

      <section className="practice__panel" aria-labelledby="takes-title">
        <h3 id="takes-title">Gravações desta pergunta</h3>
        {recordings.length === 0 ? (
          <p className="practice__hint">
            Cada tomada tem identidade própria e fica consultável.
          </p>
        ) : (
          <ul className="practice__list">
            {recordings.map((take) => (
              <RecordingItem
                key={take.id}
                take={take}
                selected={
                  current?.question.selectedRecordingId === take.id || false
                }
                disabled={busyAction !== null || captureBusy}
                onPlay={() =>
                  void run(() =>
                    client.playAudio({
                      type: "recording",
                      recordingId: take.id,
                    }),
                  )
                }
                onRetry={() =>
                  void run(() => client.retrySaveRecording(take.id))
                }
              />
            ))}
          </ul>
        )}
      </section>

      <section
        className="practice__panel practice__panel--wide"
        aria-labelledby="review-title"
      >
        <h3 id="review-title">Transcrição e revisão</h3>
        {!answer ? (
          <p className="practice__hint">
            Grave uma resposta guardada para criar a transcrição automática e a
            revisão. Não é possível confirmar uma resposta sem uma gravação.
          </p>
        ) : (
          <>
            <div className="practice__field">
              <label htmlFor="automatic-transcript">
                Transcrição automática
              </label>
              <textarea
                id="automatic-transcript"
                className="practice__textarea practice__textarea--raw"
                value={review.automaticText}
                readOnly
                rows={5}
                lang="en"
                placeholder="O texto devolvido pelo motor aparece aqui, sem alterações."
              />
              <p className="practice__hint">
                Versão do motor, apenas de leitura. Não é uma avaliação da
                resposta.
              </p>
            </div>

            {review.automaticNoSpeech ? (
              <p className="practice__warning">
                O motor não reconheceu fala nesta gravação. Pode ouvir o áudio,
                gravar outra tomada ou introduzir uma revisão fiel ao áudio.
              </p>
            ) : null}

            {review.origin === "manual_explicit" ? (
              <p className="practice__warning">
                Texto introduzido manualmente: não existe texto automático
                utilizável nesta gravação. O resultado sem fala é preservado e
                não é um diagnóstico de pronúncia.
              </p>
            ) : null}

            <div className="practice__field">
              <label htmlFor="review-text">Revisão editável</label>
              <textarea
                id="review-text"
                className="practice__textarea"
                value={review.text}
                onChange={(event) => review.setReviewText(event.target.value)}
                readOnly={!review.canEdit}
                rows={6}
                lang="en"
                aria-describedby="review-status"
                placeholder="A revisão começa com a transcrição automática."
              />
              <p
                className="practice__review-status"
                id="review-status"
                role="status"
              >
                {reviewStatus}
              </p>
              <p className="practice__hint">
                «Guardar revisão» mantém o texto editável e não confirma a
                resposta. Ao confirmar, o texto mostrado fica fixo e apenas de
                leitura.
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

            {review.confirmed ? null : (
              <div className="practice__actions">
                <button
                  type="button"
                  onClick={() => void review.saveReview()}
                  disabled={!review.canSave || !canSaveOrConfirm}
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
                    !canSaveOrConfirm ||
                    review.text.trim() === ""
                  }
                >
                  Confirmar resposta
                </button>
              </div>
            )}

            {review.confirmed && confirmation ? (
              <p className="practice__hint">
                Resposta confirmada em {formatMoment(confirmation.confirmedAt)}.
                Fica apenas de leitura; não existe desfazer nesta fase.
              </p>
            ) : null}
          </>
        )}
      </section>

      <section className="practice__panel" aria-labelledby="progress-title">
        <h3 id="progress-title">Progresso da entrevista</h3>
        <p>
          Respostas confirmadas: {session.confirmedCount} de {session.count}.
        </p>
        <div className="practice__actions">
          <button
            type="button"
            onClick={() => void advance()}
            disabled={
              confirmation === null || advancing || captureBusy || !question
            }
          >
            {last ? "Terminar sessão" : "Próxima pergunta"}
          </button>
        </div>
        {confirmation === null ? (
          <p className="practice__hint">
            Confirme a resposta para {last ? "terminar a sessão" : "avançar"}.
          </p>
        ) : null}
      </section>

      {errors.length > 0 ? (
        <section
          className="practice__panel practice__panel--error"
          aria-labelledby="interview-errors-title"
        >
          <h3 id="interview-errors-title">Erros</h3>
          <ul className="practice__errors">
            {errors.map((error) => (
              <li key={errorKey(error)} role="alert">
                <p>{error.message}</p>
                <p className="practice__hint">
                  Código: {error.code}.
                </p>
              </li>
            ))}
          </ul>
        </section>
      ) : null}
    </section>
  );
}

/** Uma tomada guardada, com a transcrição automática que lhe pertence. */
function RecordingItem({
  take,
  selected,
  disabled,
  onPlay,
  onRetry,
}: {
  take: RecordingView;
  selected: boolean;
  disabled: boolean;
  onPlay: () => void;
  onRetry: () => void;
}) {
  const retryable = take.state === "reserved" || take.state === "publish_pending";
  return (
    <li className="practice__item">
      <p className="practice__meta">Tomada {take.ordinal}</p>
      <p>
        <span className="practice__status">
          {RECORDING_STATE_LABELS[take.state]}
        </span>{" "}
        {selected ? (
          <span className="practice__status">Selecionada</span>
        ) : null}
      </p>
      {take.capture ? (
        <p className="practice__hint">
          Duração: {formatDuration(take.capture.durationMs)}.
        </p>
      ) : null}
      {take.capture?.incomplete ? (
        <p className="practice__warning">
          Esta gravação pode estar incompleta; ouça-a antes de confiar no texto.
        </p>
      ) : null}
      {take.warnings.map((warning) => (
        <p key={errorKey(warning)} className="practice__warning">
          {warning.message}
        </p>
      ))}
      {take.transcripts.map((transcript) => (
        <div key={transcript.id} className="practice__field">
          <p className="practice__meta">Transcrição automática</p>
          {transcript.status === "no_speech" ? (
            <p className="practice__hint">Sem fala reconhecida nesta tomada.</p>
          ) : (
            <p lang="en">{transcript.text}</p>
          )}
        </div>
      ))}
      <div className="practice__actions">
        <button type="button" onClick={onPlay} disabled={disabled}>
          Ouvir tomada {take.ordinal}
        </button>
        {retryable ? (
          <button type="button" onClick={onRetry} disabled={disabled}>
            Repetir guardar gravação
          </button>
        ) : null}
      </div>
    </li>
  );
}

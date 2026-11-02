/**
 * Histórico: sessões guiadas, detalhes e treinos da fase 1.
 *
 * A página é apenas de leitura sobre o núcleo: abrir uma sessão usa
 * `loadSession` e não muda a sessão em curso nem a seleção de resposta; retomar
 * pergunta ao núcleo a etapa persistida (`snapshot`) e entrega o identificador à
 * aplicação, sem avançar nenhuma pergunta. Os treinos anteriores vêm de
 * `listLegacy`/`loadLegacy`, são lidos das pastas existentes e nunca são
 * convertidos, copiados para SQLite ou apagados.
 *
 * Cada pergunta apresenta cinco áreas distintas — pergunta fixada, gravações,
 * transcrição automática, revisão guardada e resposta confirmada — para os
 * textos nunca se confundirem. Uma sessão concluída mostra um resumo sem
 * avaliação, nota ou pedido de feedback.
 */

import { useEffect, useRef, useState } from "react";

import { toAppError } from "../../lib/app-error";
import type { AppError, RestoreResult } from "../../lib/contracts";
import type { InterviewClient } from "../../lib/interview-client";
import {
  CATEGORY_LABELS,
  type Category,
  type QuestionDetail,
  type RecordingView,
  type RecordingState,
  type SessionDetail,
  type SessionSummary,
} from "../../lib/interview-contracts";

/** Rótulos dos estados de uma sessão do histórico. */
const SESSION_STATUS_LABELS: Record<SessionSummary["status"], string> = {
  in_progress: "Em curso",
  completed: "Concluída",
};

/** Rótulos dos estados de uma gravação, iguais aos do ecrã da entrevista. */
const RECORDING_STATE_LABELS: Record<RecordingState, string> = {
  reserved: "Reservada",
  capturing: "Em captura",
  publish_pending: "Por guardar",
  ready: "Guardada",
  interrupted: "Interrompida",
};

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

/** Duração em segundos, com uma casa decimal e vírgula decimal. */
function formatDuration(durationMs: number): string {
  const seconds = Math.max(0, durationMs) / 1000;
  return `${seconds.toFixed(1).replace(".", ",")} s`;
}

function categoryLabel(category: Category | null): string {
  return category ? CATEGORY_LABELS[category] : "todas as categorias";
}

function errorKey(error: AppError): string {
  return `${error.code}:${error.message}`;
}

function questionsLabel(count: number): string {
  return count === 1 ? "1 pergunta" : `${count} perguntas`;
}

export interface HistoryProps {
  client: InterviewClient;
  /** Pede à aplicação que abra a entrevista na etapa persistida. */
  onResume: (sessionId: string) => void;
}

/** Detalhe de uma sessão, aberto sem alterar a sessão em curso. */
function SessionDetailView({
  detail,
  onPlay,
}: {
  detail: SessionDetail;
  onPlay: (recordingId: string) => void;
}) {
  const { summary } = detail;
  const completed = summary.status === "completed";
  return (
    <div className="practice__sublist">
      {completed ? (
        <section
          className="practice__panel"
          aria-labelledby={`history-summary-${summary.id}`}
        >
          <h4 id={`history-summary-${summary.id}`}>
            Resumo da sessão concluída
          </h4>
          <p>
            Respostas confirmadas: {summary.confirmedCount} de {summary.count}.
          </p>
          <p className="practice__hint">
            Categoria: {categoryLabel(summary.category)}.{" "}
            {summary.completedAt
              ? `Concluída em ${formatMoment(summary.completedAt)}.`
              : "Sessão concluída."}
          </p>
          <p className="practice__hint">
            Sessão realizada no modo local desta fase. Não existe avaliação,
            nota nem pedido de feedback; as respostas confirmadas ficam apenas
            de leitura.
          </p>
        </section>
      ) : (
        <p className="practice__hint">
          Sessão em curso na pergunta {summary.position} de {summary.count}.
          Retomar abre a etapa guardada tal como está, sem avançar perguntas.
        </p>
      )}

      {detail.questions.map((question) => (
        <QuestionDetailPanel
          key={question.question.id}
          detail={question}
          count={summary.count}
          onPlay={onPlay}
        />
      ))}
    </div>
  );
}

/** As áreas distintas de uma pergunta: textos, áudio e confirmação. */
function QuestionDetailPanel({
  detail,
  count,
  onPlay,
}: {
  detail: QuestionDetail;
  count: number;
  onPlay: (recordingId: string) => void;
}) {
  const { question, recordings, confirmation } = detail;
  const position = question.position;
  const label = `pergunta ${position} de ${count}`;
  const reviews = recordings.flatMap((recording) =>
    recording.review ? [{ recording, review: recording.review }] : [],
  );
  const hasTranscripts = recordings.some(
    (recording) => recording.transcripts.length > 0,
  );

  return (
    <section
      className="practice__item"
      aria-labelledby={`history-question-${question.id}`}
    >
      <h4 id={`history-question-${question.id}`}>
        Pergunta {position} de {count}
      </h4>
      <p className="practice__question" lang="en">
        {question.text}
      </p>
      <p className="practice__meta">
        Categoria: {categoryLabel(question.category)}. Fixada a partir da
        versão {question.sourceVersion} do banco.
      </p>

      <section aria-labelledby={`history-recordings-${question.id}`}>
        <h5 id={`history-recordings-${question.id}`}>
          Gravações da {label}
        </h5>
        {recordings.length === 0 ? (
          <p className="practice__hint">Sem gravações guardadas.</p>
        ) : (
          <ul className="practice__list">
            {recordings.map((recording) => (
              <RecordingRow
                key={recording.id}
                recording={recording}
                onPlay={() => onPlay(recording.id)}
              />
            ))}
          </ul>
        )}
      </section>

      <section aria-labelledby={`history-raw-${question.id}`}>
        <h5 id={`history-raw-${question.id}`}>
          Transcrição automática da {label}
        </h5>
        {hasTranscripts ? (
          recordings.map((recording) =>
            recording.transcripts.map((transcript) => (
              <div key={transcript.id} className="practice__field">
                <p className="practice__meta">
                  Tomada {recording.ordinal} · {formatMoment(transcript.createdAt)}
                </p>
                {transcript.status === "no_speech" ? (
                  <p className="practice__hint">
                    O motor não reconheceu fala nesta gravação.
                  </p>
                ) : (
                  <p lang="en">{transcript.text}</p>
                )}
              </div>
            )),
          )
        ) : (
          <p className="practice__hint">
            Sem transcrição automática guardada para esta pergunta.
          </p>
        )}
      </section>

      <section aria-labelledby={`history-review-${question.id}`}>
        <h5 id={`history-review-${question.id}`}>
          Revisão guardada da {label}
        </h5>
        {reviews.length === 0 ? (
          <p className="practice__hint">Sem revisão guardada.</p>
        ) : (
          reviews.map(({ recording, review }) => (
            <div key={`review-${recording.id}`} className="practice__field">
              <p className="practice__meta">
                Tomada {recording.ordinal} · versão {review.version} ·{" "}
                {review.savedAt ? formatMoment(review.savedAt) : "guardada"}
              </p>
              <p lang="en">{review.text}</p>
            </div>
          ))
        )}
      </section>

      <section aria-labelledby={`history-confirmed-${question.id}`}>
        <h5 id={`history-confirmed-${question.id}`}>
          Resposta confirmada da {label}
        </h5>
        {confirmation ? (
          <>
            <p lang="en">{confirmation.text}</p>
            <p className="practice__hint">
              Confirmada em {formatMoment(confirmation.confirmedAt)}; a partir
              da versão {confirmation.reviewVersion} da revisão e apenas de
              leitura.
            </p>
          </>
        ) : (
          <p className="practice__hint">
            Ainda não confirmada. A resposta fica de leitura depois da
            confirmação na entrevista.
          </p>
        )}
      </section>
    </section>
  );
}

/** Uma gravação guardada, com o original que pode ser ouvido. */
function RecordingRow({
  recording,
  onPlay,
}: {
  recording: RecordingView;
  onPlay: () => void;
}) {
  return (
    <li className="practice__item">
      <p className="practice__meta">Tomada {recording.ordinal}</p>
      <p>
        <span className="practice__status">
          {RECORDING_STATE_LABELS[recording.state]}
        </span>
      </p>
      {recording.capture ? (
        <p className="practice__hint">
          Duração: {formatDuration(recording.capture.durationMs)}.
          {recording.capture.incomplete
            ? " A captura pode estar incompleta; ouça antes de confiar no texto."
            : ""}
        </p>
      ) : null}
      {recording.warnings.map((warning) => (
        <p key={errorKey(warning)} className="practice__warning">
          {warning.message}
        </p>
      ))}
      {recording.originalFile ? (
        <div className="practice__actions">
          <button type="button" onClick={onPlay}>
            Ouvir original da tomada {recording.ordinal}
          </button>
        </div>
      ) : (
        <p className="practice__hint">
          Sem áudio original guardado; os textos desta tomada continuam
          disponíveis.
        </p>
      )}
    </li>
  );
}

/** Detalhes de um treino anterior: transcrição, revisão e original. */
function LegacyDetailPanel({
  result,
  loading,
  error,
  onPlay,
}: {
  result: RestoreResult | null;
  loading: boolean;
  error: AppError | null;
  onPlay: () => void;
}) {
  if (loading) {
    return (
      <p className="practice__hint" role="status">
        A ler o treino anterior…
      </p>
    );
  }
  if (error) {
    return (
      <div className="practice__notice practice__notice--error" role="alert">
        <p>Não foi possível ler o treino anterior.</p>
        <p>{error.message}</p>
        <p className="practice__hint">Código: {error.code}.</p>
      </div>
    );
  }
  const attempt = result?.attempt ?? null;
  if (!attempt) {
    return (
      <p className="practice__hint">
        Este treino não tem dados legíveis; os ficheiros foram conservados.
      </p>
    );
  }

  return (
    <div className="practice__sublist">
      <section aria-labelledby={`legacy-raw-${attempt.id}`}>
        <h5 id={`legacy-raw-${attempt.id}`}>
          Transcrição automática do treino
        </h5>
        {attempt.transcripts.length === 0 ? (
          <p className="practice__hint">Sem transcrição automática.</p>
        ) : (
          attempt.transcripts.map((transcript) => (
            <div key={transcript.id} className="practice__field">
              <p className="practice__meta">
                {formatMoment(transcript.createdAt)}
              </p>
              {transcript.status === "no_speech" ? (
                <p className="practice__hint">
                  O motor não reconheceu fala nesta gravação.
                </p>
              ) : (
                <p lang="en">{transcript.text}</p>
              )}
            </div>
          ))
        )}
      </section>

      <section aria-labelledby={`legacy-review-${attempt.id}`}>
        <h5 id={`legacy-review-${attempt.id}`}>
          Revisão guardada do treino
        </h5>
        {attempt.review ? (
          <>
            <p lang="en">{attempt.review.text}</p>
            <p className="practice__hint">
              Guardada em {formatMoment(attempt.review.savedAt)}.
            </p>
          </>
        ) : (
          <p className="practice__hint">Sem revisão guardada.</p>
        )}
      </section>

      <p className="practice__hint">
        Treino anterior, apenas de leitura: não é convertido, movido nem
        apagado.
      </p>
      <div className="practice__actions">
        <button type="button" onClick={onPlay}>
          Ouvir original do treino
        </button>
      </div>
    </div>
  );
}

/**
 * Página de histórico com as sessões guiadas e os treinos da fase 1.
 *
 * Uma sessão em curso ganha a ação «Retomar», que relê a etapa persistida do
 * núcleo e não avança nenhuma pergunta; uma sessão concluída só tem leitura.
 */
export default function History({ client, onResume }: HistoryProps) {
  const [sessions, setSessions] = useState<SessionSummary[] | null>(null);
  const [sessionsError, setSessionsError] = useState<AppError | null>(null);
  const [sessionsReload, setSessionsReload] = useState(0);
  const [openSessionId, setOpenSessionId] = useState<string | null>(null);
  const [detail, setDetail] = useState<SessionDetail | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [detailError, setDetailError] = useState<AppError | null>(null);
  const [legacy, setLegacy] = useState<RestoreResult[] | null>(null);
  const [legacyError, setLegacyError] = useState<AppError | null>(null);
  const [legacyReload, setLegacyReload] = useState(0);
  const [legacyDetailId, setLegacyDetailId] = useState<string | null>(null);
  const [legacyDetail, setLegacyDetail] = useState<RestoreResult | null>(null);
  const [legacyDetailLoading, setLegacyDetailLoading] = useState(false);
  const [legacyDetailError, setLegacyDetailError] = useState<AppError | null>(
    null,
  );
  const [resumed, setResumed] = useState<{
    sessionId: string;
    position: number;
    count: number;
    awaitingAdvance: boolean;
  } | null>(null);
  const [actionError, setActionError] = useState<AppError | null>(null);
  const openSessionRef = useRef<string | null>(null);

  useEffect(() => {
    let active = true;
    setSessionsError(null);
    setSessions(null);
    client
      .listSessions()
      .then((rows) => {
        if (active) {
          setSessions(rows);
        }
      })
      .catch((error: unknown) => {
        if (active) {
          setSessions([]);
          setSessionsError(toAppError(error));
        }
      });
    return () => {
      active = false;
    };
  }, [client, sessionsReload]);

  useEffect(() => {
    let active = true;
    setLegacyError(null);
    setLegacy(null);
    client
      .listLegacy()
      .then((rows) => {
        if (active) {
          setLegacy(rows);
        }
      })
      .catch((error: unknown) => {
        if (active) {
          setLegacy([]);
          setLegacyError(toAppError(error));
        }
      });
    return () => {
      active = false;
    };
  }, [client, legacyReload]);

  async function openSession(sessionId: string): Promise<void> {
    // Abrir detalhes é uma leitura: não muda a sessão em curso nem a seleção.
    openSessionRef.current = sessionId;
    setOpenSessionId(sessionId);
    setDetail(null);
    setDetailError(null);
    setDetailLoading(true);
    try {
      const next = await client.loadSession(sessionId);
      if (openSessionRef.current === sessionId) {
        setDetail(next);
      }
    } catch (error) {
      if (openSessionRef.current === sessionId) {
        setDetailError(toAppError(error));
      }
    } finally {
      if (openSessionRef.current === sessionId) {
        setDetailLoading(false);
      }
    }
  }

  function toggleSession(sessionId: string): void {
    if (openSessionId === sessionId) {
      openSessionRef.current = null;
      setOpenSessionId(null);
      setDetail(null);
      setDetailError(null);
      setDetailLoading(false);
      return;
    }
    void openSession(sessionId);
  }

  async function resume(session: SessionSummary): Promise<void> {
    setActionError(null);
    try {
      // A etapa guardada vem do núcleo; a retoma não avança nem confirma nada.
      const current = await client.snapshot();
      if (current.activeSession?.id !== session.id) {
        setActionError({
          code: "STALE_SOURCE",
          message:
            "Esta sessão já não está em curso. Atualize o histórico antes de retomar.",
          retryable: false,
        });
        return;
      }
      setResumed({
        sessionId: session.id,
        position: current.activeSession.position,
        count: current.activeSession.count,
        awaitingAdvance: current.current?.confirmation != null,
      });
      onResume(session.id);
    } catch (error) {
      setActionError(toAppError(error));
    }
  }

  async function openLegacy(attemptId: string): Promise<void> {
    if (legacyDetailId === attemptId) {
      setLegacyDetailId(null);
      setLegacyDetail(null);
      setLegacyDetailError(null);
      return;
    }
    setLegacyDetailId(attemptId);
    setLegacyDetail(null);
    setLegacyDetailError(null);
    setLegacyDetailLoading(true);
    try {
      setLegacyDetail(await client.loadLegacy(attemptId));
    } catch (error) {
      setLegacyDetailError(toAppError(error));
    } finally {
      setLegacyDetailLoading(false);
    }
  }

  async function playLegacy(attemptId: string): Promise<void> {
    setActionError(null);
    try {
      await client.playAudio({ type: "legacy", attemptId });
    } catch (error) {
      setActionError(toAppError(error));
    }
  }

  async function playRecording(recordingId: string): Promise<void> {
    setActionError(null);
    try {
      await client.playAudio({ type: "recording", recordingId });
    } catch (error) {
      setActionError(toAppError(error));
    }
  }

  return (
    <section className="practice__page" aria-labelledby="history-title">
      <h2 id="history-title">Histórico</h2>
      <p className="practice__hint">
        Consulte as sessões guardadas e os treinos anteriores. Abrir ou ouvir
        não altera a sessão em curso; nada é convertido, movido ou apagado.
      </p>

      {actionError ? (
        <div className="practice__notice practice__notice--error" role="alert">
          <p>{actionError.message}</p>
          <p className="practice__hint">Código: {actionError.code}.</p>
        </div>
      ) : null}

      {resumed ? (
        <p className="practice__state" role="status">
          Sessão retomada na pergunta {resumed.position} de {resumed.count}.
          {resumed.awaitingAdvance
            ? " A resposta confirmada aguarda que avance explicitamente."
            : ""}
        </p>
      ) : null}

      <section
        className="practice__panel practice__panel--wide"
        aria-labelledby="history-sessions-title"
      >
        <h3 id="history-sessions-title">Sessões</h3>
        {sessionsError ? (
          <div className="practice__notice practice__notice--error" role="alert">
            <p>Não foi possível ler o histórico de sessões.</p>
            <p>{sessionsError.message}</p>
            <p className="practice__hint">Código: {sessionsError.code}.</p>
            <div className="practice__actions">
              <button
                type="button"
                onClick={() => setSessionsReload((value) => value + 1)}
              >
                Tentar ler novamente
              </button>
            </div>
          </div>
        ) : sessions === null ? (
          <p className="practice__hint" role="status">
            A carregar as sessões guardadas…
          </p>
        ) : sessions.length === 0 ? (
          <p className="practice__hint">
            Ainda não existe nenhuma sessão guardada. Inicie uma entrevista na
            preparação.
          </p>
        ) : (
          <ul className="practice__list">
            {sessions.map((session) => {
              const open = openSessionId === session.id;
              return (
                <li key={session.id} className="practice__item">
                  <p className="practice__meta">
                    {formatMoment(session.createdAt)}
                  </p>
                  <p>
                    <span className="practice__status">
                      {SESSION_STATUS_LABELS[session.status]}
                    </span>{" "}
                    <span className="practice__hint">
                      {categoryLabel(session.category)}
                    </span>
                  </p>
                  <p>
                    {questionsLabel(session.count)} · respostas confirmadas:{" "}
                    {session.confirmedCount} de {session.count}
                  </p>
                  <p className="practice__hint">
                    Progresso: pergunta {session.position} de {session.count}.
                  </p>
                  <div className="practice__actions">
                    <button
                      type="button"
                      onClick={() => toggleSession(session.id)}
                      aria-expanded={open}
                    >
                      {open ? "Fechar sessão" : "Abrir sessão"}
                    </button>
                    {session.status === "in_progress" ? (
                      <button
                        type="button"
                        onClick={() => void resume(session)}
                      >
                        Retomar
                      </button>
                    ) : null}
                  </div>

                  {open ? (
                    detailLoading ? (
                      <p className="practice__hint" role="status">
                        A ler a sessão…
                      </p>
                    ) : detailError ? (
                      <div
                        className="practice__notice practice__notice--error"
                        role="alert"
                      >
                        <p>Não foi possível ler a sessão.</p>
                        <p>{detailError.message}</p>
                        <p className="practice__hint">
                          Código: {detailError.code}.
                        </p>
                        <div className="practice__actions">
                          <button
                            type="button"
                            onClick={() => void openSession(session.id)}
                          >
                            Tentar ler novamente
                          </button>
                        </div>
                      </div>
                    ) : detail && detail.summary.id === session.id ? (
                      <SessionDetailView
                        detail={detail}
                        onPlay={(recordingId) => void playRecording(recordingId)}
                      />
                    ) : null
                  ) : null}
                </li>
              );
            })}
          </ul>
        )}
      </section>

      <section
        className="practice__panel practice__panel--wide"
        aria-labelledby="history-legacy-title"
      >
        <h3 id="history-legacy-title">Treinos da fase 1</h3>
        <p className="practice__hint">
          Treinos guardados pela versão anterior. A consulta é apenas de
          leitura e não cria sessões nem confirmações.
        </p>
        {legacyError ? (
          <div className="practice__notice practice__notice--error" role="alert">
            <p>Não foi possível ler os treinos anteriores.</p>
            <p>{legacyError.message}</p>
            <p className="practice__hint">Código: {legacyError.code}.</p>
            <div className="practice__actions">
              <button
                type="button"
                onClick={() => setLegacyReload((value) => value + 1)}
              >
                Tentar ler novamente
              </button>
            </div>
          </div>
        ) : legacy === null ? (
          <p className="practice__hint" role="status">
            A ler os treinos anteriores…
          </p>
        ) : legacy.length === 0 ? (
          <p className="practice__hint">
            Não existem treinos anteriores nesta instalação.
          </p>
        ) : (
          <ul className="practice__list">
            {legacy.map((item, index) => {
              const attempt = item.attempt;
              const open = attempt !== null && legacyDetailId === attempt.id;
              return (
                <li
                  key={attempt?.id ?? `treino-invalido-${index}`}
                  className="practice__item"
                >
                  {attempt ? (
                    <>
                      <p className="practice__meta">
                        {formatMoment(attempt.createdAt)} ·{" "}
                        {formatDuration(attempt.capture.durationMs)}
                      </p>
                      <p>
                        <span className="practice__status">
                          Treino da fase 1
                        </span>
                      </p>
                      <p lang="en">{attempt.question}</p>
                      <p className="practice__hint">
                        {attempt.transcripts.length === 1
                          ? "1 transcrição automática"
                          : `${attempt.transcripts.length} transcrições automáticas`}{" "}
                        ·{" "}
                        {attempt.review
                          ? "revisão guardada"
                          : "sem revisão guardada"}
                        .
                      </p>
                      <div className="practice__actions">
                        <button
                          type="button"
                          onClick={() => void openLegacy(attempt.id)}
                          aria-expanded={open}
                        >
                          {open ? "Fechar detalhes" : "Abrir detalhes"}
                        </button>
                        <button
                          type="button"
                          onClick={() => void playLegacy(attempt.id)}
                        >
                          Ouvir original
                        </button>
                      </div>
                    </>
                  ) : (
                    <>
                      <p>
                        <span className="practice__status practice__status--error">
                          Treino inválido
                        </span>
                      </p>
                      <p className="practice__hint">
                        Este treino não pôde ser lido; os ficheiros foram
                        conservados.
                      </p>
                    </>
                  )}

                  {item.warnings.map((warning) => (
                    <p key={errorKey(warning)} className="practice__warning">
                      {warning.message}
                    </p>
                  ))}

                  {open && attempt ? (
                    <LegacyDetailPanel
                      result={legacyDetail}
                      loading={legacyDetailLoading}
                      error={legacyDetailError}
                      onPlay={() => void playLegacy(attempt.id)}
                    />
                  ) : null}
                </li>
              );
            })}
          </ul>
        )}
      </section>
    </section>
  );
}

import { useEffect, useRef, useState } from "react";

import { getCurrentWindow } from "@tauri-apps/api/window";

import type {
  AppError,
  AppSnapshot,
  ComponentStatus,
  ModelJob,
} from "../../lib/contracts";
import { INITIAL_QUESTION } from "../../lib/contracts";
import type { PracticeClient } from "../../lib/tauri";
import { latestTranscript, toAppError, usePractice } from "./usePractice";
import "./practice.css";

type Decision = { kind: "recording" } | { kind: "close" };

const COMPONENT_LABELS = {
  microphone: "Microfone",
  playback: "Reprodução",
  transcription: "Transcrição",
  synthesis: "Síntese",
} as const;

const STATUS_LABELS: Record<ComponentStatus, string> = {
  ready: "Disponível",
  missing: "Em falta",
  loading: "A carregar",
  error: "Com erro",
};

function formatCaptureDuration(durationMs: number): string {
  const seconds = Math.max(0, durationMs) / 1000;
  return `${seconds.toFixed(1).replace(".", ",")} s`;
}

function formatElapsed(seconds: number): string {
  if (seconds < 60) {
    return `${seconds} s`;
  }
  return `${Math.floor(seconds / 60)} min ${seconds % 60} s`;
}

function jobMessage(job: ModelJob): string {
  if (job.kind === "transcribe") {
    return job.stage === "loading"
      ? "A carregar o modelo de transcrição…"
      : "A transcrever a resposta…";
  }
  return job.stage === "loading"
    ? "A carregar o modelo de voz…"
    : "A preparar a pergunta em voz…";
}

/** Duração local da captura em curso; a duração guardada vem da fotografia. */
function useElapsedSeconds(active: boolean): number {
  const [elapsed, setElapsed] = useState(0);
  const startedAt = useRef<number | null>(null);

  useEffect(() => {
    if (!active) {
      startedAt.current = null;
      setElapsed(0);
      return;
    }

    if (startedAt.current === null) {
      startedAt.current = Date.now();
    }

    const update = () => {
      const start = startedAt.current ?? Date.now();
      setElapsed(Math.max(0, Math.floor((Date.now() - start) / 1000)));
    };

    update();
    const timer = window.setInterval(update, 1000);
    return () => window.clearInterval(timer);
  }, [active]);

  return elapsed;
}

function errorKey(error: AppError): string {
  return `${error.code}:${error.message}`;
}

/**
 * Ecrã de prática da fase 1.
 *
 * O cliente é injetado para que os testes usem um adaptador controlado, sem
 * microfone, IPC Tauri ou modelos reais.
 */
export default function Practice({ client }: { client: PracticeClient }) {
  const practice = usePractice(client);
  const snapshot: AppSnapshot | null = practice.snapshot;
  const attempt = snapshot?.attempt ?? null;
  const transcript = latestTranscript(attempt);
  const recording = snapshot?.recordingId != null;
  const job = snapshot?.modelJob ?? null;
  const playback = snapshot?.playback ?? null;

  const [decision, setDecision] = useState<Decision | null>(null);
  const [busyAction, setBusyAction] = useState<string | null>(null);
  const [actionError, setActionError] = useState<AppError | null>(null);

  const elapsedSeconds = useElapsedSeconds(recording);
  const busy = busyAction !== null;
  const saving = practice.review.saving;
  const dirty = practice.review.dirty;
  const staleReview =
    attempt !== null &&
    practice.review.attemptId !== null &&
    practice.review.attemptId !== attempt.id;

  // Reproduzir durante uma transcrição é permitido pela spec; só uma síntese em
  // curso (que vai reproduzir a pergunta) ocupa a saída.
  const synthesizing = job?.kind === "synthesize";
  const canUseAudio = !recording && !synthesizing && !busy;
  const canStartRecording = !recording && !job && !busy;
  const canStopRecording = recording && !busy;
  const canTranscribe = attempt !== null && !recording && !job && !busy;
  const canSaveReview =
    transcript !== null &&
    !staleReview &&
    !saving &&
    !busy &&
    (dirty || !practice.review.hasSavedReview);

  const question = snapshot?.question ?? INITIAL_QUESTION;

  let reviewStatus = "Revisão em falta";
  if (saving) {
    reviewStatus = "A guardar revisão…";
  } else if (staleReview) {
    reviewStatus = "Revisão de outra resposta por guardar";
  } else if (transcript) {
    if (dirty) {
      reviewStatus = "Alterações por guardar";
    } else if (practice.review.hasSavedReview) {
      reviewStatus = "Revisão guardada";
    } else {
      reviewStatus = "Revisão por guardar";
    }
  }

  const errors: AppError[] = [];
  for (const error of [
    practice.review.error,
    actionError,
    snapshot?.lastError ?? null,
  ]) {
    if (error && !errors.some((seen) => errorKey(seen) === errorKey(error))) {
      errors.push(error);
    }
  }

  const canRetryCaptureSave =
    practice.review.error === null &&
    snapshot?.lastError?.code === "STORAGE_WRITE_FAILED" &&
    !busy;

  async function run(
    name: string,
    command: () => Promise<AppSnapshot>,
  ): Promise<AppSnapshot | null> {
    setBusyAction(name);
    setActionError(null);
    try {
      const next = await command();
      practice.acceptSnapshot(next);
      return next;
    } catch (error) {
      setActionError(toAppError(error));
      return null;
    } finally {
      setBusyAction(null);
    }
  }

  async function startRecordingNow(): Promise<void> {
    const next = await run("start-recording", () => client.startRecording());
    if (next) {
      setDecision(null);
    }
  }

  function handleStartRecording(): void {
    if (dirty) {
      // Só depois de guardar, descartar ou cancelar é que a captura começa.
      setDecision({ kind: "recording" });
      return;
    }
    void startRecordingNow();
  }

  async function closeNow(): Promise<void> {
    setBusyAction("close");
    setActionError(null);
    try {
      await client.prepareClose();
    } catch (error) {
      setActionError(toAppError(error));
      setBusyAction(null);
      return;
    }

    setDecision(null);
    try {
      await getCurrentWindow().destroy();
    } catch (error) {
      setActionError(toAppError(error));
    } finally {
      setBusyAction(null);
    }
  }

  function requestClose(): void {
    if (saving) {
      setActionError({
        code: "BUSY",
        message:
          "A revisão está a ser guardada. Tente fechar novamente quando a escrita terminar.",
        retryable: true,
      });
      return;
    }
    if (dirty) {
      setDecision({ kind: "close" });
      return;
    }
    void closeNow();
  }

  const closeActionRef = useRef(requestClose);
  closeActionRef.current = requestClose;

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;

    void getCurrentWindow()
      .onCloseRequested((event) => {
        // O fecho imediato é sempre bloqueado; só `destroy` fecha a janela,
        // depois de a revisão e o `prepareClose` estarem resolvidos.
        event.preventDefault();
        closeActionRef.current();
      })
      .then((remove) => {
        if (disposed) {
          remove();
        } else {
          unlisten = remove;
        }
      })
      .catch((error: unknown) => {
        if (!disposed) {
          setActionError(toAppError(error));
        }
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  async function saveAndStart(): Promise<void> {
    const outcome = await practice.saveReview();
    // `pending` significa que a escrita teve êxito mas o texto mudou durante o
    // pedido: a versão mais recente continua por guardar e a decisão repete-se
    // em vez de a perder.
    if (outcome !== "saved") {
      return;
    }
    setDecision(null);
    await startRecordingNow();
  }

  function discardAndStart(): void {
    practice.discardReview();
    setDecision(null);
    void startRecordingNow();
  }

  async function saveAndClose(): Promise<void> {
    const outcome = await practice.saveReview();
    // Fechar só quando a versão mais recente está guardada. Se a escrita ficou
    // `pending` (ou falhou), a janela mantém-se e o texto continua editável.
    if (outcome !== "saved") {
      return;
    }
    await closeNow();
  }

  function discardAndClose(): void {
    practice.discardReview();
    void closeNow();
  }

  function cancelDecision(): void {
    if (!saving && !busy) {
      setDecision(null);
    }
  }

  return (
    <main className="practice" aria-labelledby="practice-question">
      <header className="practice__header">
        <p className="practice__eyebrow">Prática de entrevista</p>
        <h1 className="practice__question" id="practice-question" lang="en">
          {question}
        </h1>
      </header>

      {practice.loading && !snapshot ? (
        <p className="practice__hint" role="status">
          A carregar o estado da prática…
        </p>
      ) : null}

      {practice.loadError && !snapshot ? (
        <div className="practice__notice practice__notice--error" role="alert">
          <p>{practice.loadError.message}</p>
          <button
            type="button"
            onClick={() => void practice.reload()}
            disabled={practice.loading}
          >
            Tentar novamente
          </button>
        </div>
      ) : null}

      {snapshot ? (
        <>
          <section className="practice__panel" aria-labelledby="availability-title">
            <h2 id="availability-title">Disponibilidade</h2>
            <ul className="practice__availability">
              {(
                Object.keys(COMPONENT_LABELS) as Array<
                  keyof typeof COMPONENT_LABELS
                >
              ).map((key) => {
                const component = snapshot.availability[key];
                return (
                  <li key={key}>
                    <span className="practice__component">
                      {COMPONENT_LABELS[key]}
                    </span>
                    <span
                      className={`practice__status practice__status--${component.status}`}
                    >
                      {STATUS_LABELS[component.status]}
                    </span>
                    {component.message ? (
                      <span className="practice__hint">{component.message}</span>
                    ) : null}
                  </li>
                );
              })}
            </ul>
            <button
              type="button"
              onClick={() =>
                void run("refresh-availability", () => client.refresh())
              }
              disabled={busy}
            >
              Verificar disponibilidade
            </button>
          </section>

          <section className="practice__panel" aria-labelledby="capture-title">
            <h2 id="capture-title">Gravação</h2>
            {recording ? (
              <>
                <p className="practice__state" role="status">
                  Gravação em curso
                </p>
                <p>Duração da gravação: {formatElapsed(elapsedSeconds)}</p>
              </>
            ) : attempt ? (
              <>
                <p>
                  Duração da gravação:{" "}
                  {formatCaptureDuration(attempt.capture.durationMs)}
                </p>
                {attempt.capture.incomplete ? (
                  <p className="practice__warning">
                    A gravação pode estar incompleta; ouça-a antes de confiar no
                    texto.
                  </p>
                ) : null}
              </>
            ) : (
              <p className="practice__hint">
                Ainda não existe uma resposta gravada.
              </p>
            )}
            {job ? (
              <p className="practice__state" role="status">
                {jobMessage(job)}
              </p>
            ) : null}
            {playback ? (
              <p className="practice__state" role="status">
                {playback === "question"
                  ? "A reproduzir a pergunta…"
                  : "A reproduzir a resposta…"}
              </p>
            ) : null}
          </section>

          <section
            className="practice__panel practice__panel--wide"
            aria-labelledby="transcript-title"
          >
            <h2 id="transcript-title">Transcrição e revisão</h2>

            <div className="practice__field">
              <label htmlFor="automatic-transcript">
                Transcrição automática
              </label>
              <textarea
                id="automatic-transcript"
                className="practice__textarea practice__textarea--raw"
                value={transcript?.text ?? ""}
                readOnly
                rows={6}
                placeholder="O texto devolvido pelo motor aparece aqui, sem alterações."
              />
              <p className="practice__hint">
                Versão do motor, apenas de leitura. Não é uma avaliação da
                resposta.
              </p>
            </div>

            <div className="practice__field">
              <label htmlFor="review-text">Revisão da transcrição</label>
              <textarea
                id="review-text"
                className="practice__textarea"
                value={practice.review.text}
                onChange={(event) => practice.setReviewText(event.target.value)}
                disabled={!transcript}
                rows={6}
                aria-describedby="review-status"
                placeholder="A revisão começa com a transcrição automática."
              />
              <p className="practice__review-status" id="review-status" role="status">
                {reviewStatus}
              </p>
              {staleReview ? (
                <div
                  className="practice__notice practice__notice--error"
                  role="alert"
                >
                  <p>
                    A revisão visível pertence a outra resposta e não será
                    guardada nesta. Copie o texto se precisar dele antes de o
                    descartar.
                  </p>
                  <button
                    type="button"
                    onClick={() => practice.discardReview()}
                    disabled={saving || busy}
                  >
                    Descartar revisão anterior
                  </button>
                </div>
              ) : null}
              {!transcript ? (
                <p className="practice__hint">
                  Transcreva a resposta para criar uma cópia editável.
                </p>
              ) : null}
            </div>

            {transcript?.status === "no_speech" ? (
              <p className="practice__warning">
                O motor não reconheceu fala nesta gravação. Pode ouvir o áudio e
                gravar outra resposta; resultados de reconhecimento não são um
                diagnóstico de pronúncia.
              </p>
            ) : null}
          </section>

          <section className="practice__panel" aria-labelledby="actions-title">
            <h2 id="actions-title">Ações</h2>
            <div className="practice__actions">
              <button
                type="button"
                onClick={() => void run("play-question", () => client.play("question"))}
                disabled={!canUseAudio}
              >
                Ouvir pergunta
              </button>

              {recording ? (
                <button
                  type="button"
                  onClick={() => void run("stop-recording", () => client.stopRecording())}
                  disabled={!canStopRecording}
                >
                  Terminar gravação
                </button>
              ) : (
                <button
                  type="button"
                  onClick={handleStartRecording}
                  disabled={!canStartRecording}
                >
                  Iniciar gravação
                </button>
              )}

              {attempt ? (
                <button
                  type="button"
                  onClick={() => void run("play-response", () => client.play("response"))}
                  disabled={!canUseAudio}
                >
                  Ouvir resposta
                </button>
              ) : null}

              {playback ? (
                <button
                  type="button"
                  onClick={() => void run("stop-audio", () => client.stop())}
                  disabled={busy}
                >
                  Parar áudio
                </button>
              ) : null}

              {attempt ? (
                <button
                  type="button"
                  onClick={() =>
                    void run("transcribe", () => client.transcribe(attempt.id))
                  }
                  disabled={!canTranscribe}
                >
                  Transcrever resposta
                </button>
              ) : null}

              {transcript ? (
                <button
                  type="button"
                  onClick={() => void practice.saveReview()}
                  disabled={!canSaveReview}
                >
                  Guardar revisão
                </button>
              ) : null}

              <button
                type="button"
                onClick={() => void run("restart-worker", () => client.restartWorker())}
                disabled={busy}
              >
                Reiniciar áudio local
              </button>

              {canRetryCaptureSave ? (
                <button
                  type="button"
                  onClick={() =>
                    void run("retry-save-recording", () =>
                      client.retrySaveRecording(),
                    )
                  }
                  disabled={busy}
                >
                  Repetir guardar gravação
                </button>
              ) : null}
            </div>
          </section>
        </>
      ) : null}

      {errors.length > 0 ? (
        <section
          className="practice__panel practice__panel--error"
          aria-labelledby="errors-title"
        >
          <h2 id="errors-title">Erros</h2>
          <ul className="practice__errors">
            {errors.map((error) => (
              <li key={errorKey(error)} role="alert">
                <p>{error.message}</p>
                <p className="practice__hint">
                  {error.retryable
                    ? "Pode tentar novamente."
                    : "Corrija o problema antes de repetir."}{" "}
                  Código: {error.code}.
                </p>
              </li>
            ))}
          </ul>
        </section>
      ) : null}

      {decision ? (
        <section
          className="practice__decision"
          role="dialog"
          aria-modal="true"
          aria-labelledby="decision-title"
          aria-describedby="decision-description"
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.preventDefault();
              cancelDecision();
            }
          }}
        >
          <h2 id="decision-title">Revisão por guardar</h2>
          <p id="decision-description">
            Existem alterações por guardar na revisão da transcrição. Escolha o
            que fazer antes de continuar.
          </p>
          <p className="practice__review-status">Estado: {reviewStatus}</p>
          {staleReview ? (
            <p className="practice__warning">
              A revisão pertence a uma resposta anterior; nesta resposta só pode
              ser descartada.
            </p>
          ) : null}
          <div className="practice__actions">
            <button
              type="button"
              autoFocus={!staleReview}
              onClick={() =>
                void (decision.kind === "recording"
                  ? saveAndStart()
                  : saveAndClose())
              }
              disabled={saving || busy || staleReview}
            >
              {decision.kind === "recording"
                ? "Guardar e iniciar gravação"
                : "Guardar e fechar"}
            </button>
            <button
              type="button"
              autoFocus={staleReview}
              onClick={() =>
                decision.kind === "recording"
                  ? discardAndStart()
                  : discardAndClose()
              }
              disabled={saving || busy}
            >
              {decision.kind === "recording"
                ? "Descartar e iniciar gravação"
                : "Descartar e fechar"}
            </button>
            <button
              type="button"
              onClick={cancelDecision}
              disabled={saving || busy}
            >
              Cancelar
            </button>
          </div>
        </section>
      ) : null}
    </main>
  );
}

/**
 * Apoio opcional de uma história local antes de gravar.
 *
 * O apoio é privado: serve apenas de consulta do utilizador. Nunca preenche a
 * transcrição, a revisão nem a confirmação e não segue para a API. Antes da
 * primeira captura bem-sucedida o utilizador pode escolher, substituir ou
 * remover a história; depois fica fixada a cópia da versão escolhida (editar o
 * banco não a altera). O estado de bloqueio vem do núcleo.
 */

import { useCallback, useEffect, useRef, useState } from "react";

import { toAppError } from "../../lib/app-error";
import type { AppError } from "../../lib/contracts";
import type { InterviewClient } from "../../lib/interview-client";
import type { InterviewSnapshot } from "../../lib/interview-contracts";
import type {
  CaptureOwner,
  PreparationView,
  StoryInput,
  StoryView,
} from "../../lib/practice-contracts";
import { newRequestId } from "../../lib/request-id";

function StoryNotes({ story }: { story: StoryInput }) {
  const notes: Array<[string, string]> = [
    ["Situação", story.situation],
    ["Tarefa", story.task],
    ["Ações", story.actions],
    ["Resultado", story.outcome],
  ];
  return (
    <dl className="practice__notes">
      {notes
        .filter(([, value]) => value.trim() !== "")
        .map(([label, value]) => (
          <div key={label}>
            <dt>{label}</dt>
            <dd lang="en">{value}</dd>
          </div>
        ))}
    </dl>
  );
}

export interface StorySupportProps {
  client: InterviewClient;
  owner: CaptureOwner;
  snapshot: InterviewSnapshot;
  onSnapshot: (snapshot: InterviewSnapshot) => void;
  /** Interface permite alterar (sem captura, confirmação ou trabalho). */
  editable: boolean;
}

export default function StorySupport({
  client,
  owner,
  snapshot,
  onSnapshot,
  editable,
}: StorySupportProps) {
  const [preparation, setPreparation] = useState<PreparationView | null>(null);
  const [stories, setStories] = useState<StoryView[]>([]);
  const [error, setError] = useState<AppError | null>(null);
  const [busy, setBusy] = useState(false);
  const answerId = owner.answerId;
  const ownerRef = useRef(owner);
  ownerRef.current = owner;

  const load = useCallback(async (): Promise<void> => {
    try {
      const next = await client.loadPreparation(ownerRef.current);
      setPreparation(next);
    } catch (failure) {
      setError(toAppError(failure));
    }
  }, [client]);

  useEffect(() => {
    void load();
    // O apoio é relido quando a fotografia muda (arranque da captura, retoma).
  }, [load, answerId, snapshot.revision]);

  useEffect(() => {
    let active = true;
    client
      .listStories(null)
      .then((rows) => {
        if (active) {
          setStories(rows);
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
  }, [client]);

  async function select(storyId: string | null): Promise<void> {
    if (!preparation || busy) {
      return;
    }
    const story = storyId === null ? null : stories.find((s) => s.id === storyId);
    if (storyId !== null && !story) {
      return;
    }
    setBusy(true);
    setError(null);
    try {
      onSnapshot(
        await client.selectStory({
          requestId: newRequestId(),
          owner,
          expectedPreparationVersion: preparation.version,
          storyId,
          expectedStoryVersion: story ? story.version : null,
        }),
      );
      await load();
    } catch (failure) {
      setError(toAppError(failure));
      await load();
    } finally {
      setBusy(false);
    }
  }

  async function resolve(decision: "keep" | "did_not_start"): Promise<void> {
    const recordingId =
      preparation?.lockedCaptureId ?? snapshot.practice?.pendingPreparation ?? null;
    if (!recordingId) {
      return;
    }
    setBusy(true);
    setError(null);
    try {
      onSnapshot(
        await client.resolvePreparation(recordingId, newRequestId(), decision),
      );
      await load();
    } catch (failure) {
      setError(toAppError(failure));
    } finally {
      setBusy(false);
    }
  }

  if (!preparation) {
    return error ? (
      <p role="alert">{error.message}</p>
    ) : (
      <p className="practice__hint" role="status">
        A carregar o apoio…
      </p>
    );
  }

  const canChange = editable && preparation.lockState === "editable";
  const selected = stories.find((s) => s.id === preparation.selectedStoryId);

  return (
    <section
      className="practice__panel"
      aria-labelledby={`support-title-${answerId}`}
    >
      <h4 id={`support-title-${answerId}`}>Apoio de história (opcional)</h4>
      <p className="practice__hint">
        Consulta privada: o apoio não entra na transcrição, na revisão nem na
        confirmação e não é enviado à API. Fica fixado na primeira gravação.
      </p>

      {canChange ? (
        <div className="practice__field">
          <label htmlFor={`support-select-${answerId}`}>
            História de apoio
          </label>
          <select
            id={`support-select-${answerId}`}
            className="practice__select"
            value={preparation.selectedStoryId ?? ""}
            disabled={busy}
            onChange={(event) =>
              void select(event.target.value === "" ? null : event.target.value)
            }
          >
            <option value="">Sem apoio</option>
            {stories.map((story) => (
              <option key={story.id} value={story.id}>
                {story.title}
              </option>
            ))}
          </select>
          {preparation.selectedStoryId ? (
            <div className="practice__actions">
              <button
                type="button"
                disabled={busy}
                onClick={() => void select(null)}
              >
                Remover apoio
              </button>
            </div>
          ) : null}
          {selected ? (
            <section aria-label="Notas da história escolhida">
              <p className="practice__meta">
                {selected.title} — versão {selected.version}
              </p>
              <StoryNotes story={selected} />
            </section>
          ) : null}
        </div>
      ) : (
        <>
          {preparation.lockState === "editable" ? (
            <p className="practice__hint">
              O apoio só pode ser alterado enquanto não há gravação, trabalho
              ou confirmação em curso.
            </p>
          ) : null}
          {preparation.support === "story" && preparation.snapshot ? (
            <section aria-label="Apoio fixado">
              <p className="practice__state">
                Apoio fixado: {preparation.snapshot.content.title} (versão{" "}
                {preparation.snapshot.storyVersion}).
              </p>
              <StoryNotes story={preparation.snapshot.content} />
            </section>
          ) : preparation.support === "story" ? (
            <p className="practice__state">
              Apoio escolhido (será fixado na primeira gravação).
            </p>
          ) : preparation.support === "historical_none" ? (
            <p className="practice__hint">
              Sem apoio registado: resposta anterior à existência do apoio.
            </p>
          ) : (
            <p className="practice__hint">Sem apoio de história.</p>
          )}
        </>
      )}

      {preparation.lockState === "pending_start" ? (
        <p className="practice__state" role="status">
          A iniciar a gravação; o apoio fica fixado quando o microfone arrancar.
        </p>
      ) : null}

      {preparation.lockState === "start_uncertain" ? (
        <div className="practice__notice practice__notice--error" role="alert">
          <p>
            Não é certo que o microfone tenha chegado a iniciar. Decida se
            mantém o apoio ou se o liberta; nada é apagado.
          </p>
          <div className="practice__actions">
            <button
              type="button"
              disabled={busy}
              onClick={() => void resolve("keep")}
            >
              Manter este apoio e continuar
            </button>
            <button
              type="button"
              disabled={busy}
              onClick={() => void resolve("did_not_start")}
            >
              O microfone não chegou a iniciar
            </button>
          </div>
        </div>
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

/**
 * Aviso de metadados de apoio por guardar.
 *
 * A captura continua e «Terminar gravação» não é escondido: repetir guarda os
 * mesmos metadados sem consultar a versão atual da história.
 */
export function PendingPreparationNotice({
  client,
  snapshot,
  onSnapshot,
}: {
  client: InterviewClient;
  snapshot: InterviewSnapshot;
  onSnapshot: (snapshot: InterviewSnapshot) => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<AppError | null>(null);
  const pending = snapshot.practice?.pendingPreparation ?? null;
  if (!pending) {
    return null;
  }
  async function retry(recordingId: string): Promise<void> {
    setBusy(true);
    setError(null);
    try {
      onSnapshot(await client.retryPreparationPublication(recordingId));
    } catch (failure) {
      setError(toAppError(failure));
    } finally {
      setBusy(false);
    }
  }
  return (
    <div className="practice__notice practice__notice--error" role="alert">
      <p>
        Os metadados do apoio da gravação ainda não foram guardados.{" "}
        {snapshot.recordingId !== null
          ? "A gravação continua e pode terminá-la normalmente."
          : "O áudio está preservado."}
      </p>
      <div className="practice__actions">
        <button
          type="button"
          disabled={busy}
          onClick={() => void retry(pending)}
        >
          Repetir guardar apoio
        </button>
      </div>
      {error ? <p>{error.message}</p> : null}
    </div>
  );
}

/**
 * Banco local de histórias de preparação.
 *
 * Exclusivamente local nesta fase: consultar, filtrar, criar e editar uma
 * história nunca envia nada à API. As histórias são notas factuais do
 * utilizador; só o título e uma nota não vazia mais uma competência são
 * obrigatórios, e a falta de campos STAR é um rótulo informativo, não uma
 * avaliação. Não existe eliminação. Editar cria uma versão nova e não altera o
 * apoio já fixado numa gravação.
 *
 * O rascunho por guardar sobe para a aplicação pela mesma decisão partilhada
 * (guardar/descartar/cancelar) que protege o banco de perguntas.
 */

import { useEffect, useRef, useState } from "react";

import { toAppError } from "../../lib/app-error";
import type { AppError } from "../../lib/contracts";
import type { InterviewClient } from "../../lib/interview-client";
import type {
  MissingField,
  StoryInput,
  StoryView,
} from "../../lib/practice-contracts";
import { newRequestId } from "../../lib/request-id";
import type { SaveOutcome } from "../interview/useInterview";
import type { GuardSource } from "../navigation/NavigationGuard";

const FIELD_LABELS: Record<MissingField, string> = {
  situation: "Situação",
  task: "Tarefa",
  actions: "Ações",
  outcome: "Resultado",
};

interface Draft {
  id: string | null;
  version: number | null;
  title: string;
  situation: string;
  task: string;
  actions: string;
  outcome: string;
  competencies: string;
}

function emptyDraft(): Draft {
  return {
    id: null,
    version: null,
    title: "",
    situation: "",
    task: "",
    actions: "",
    outcome: "",
    competencies: "",
  };
}

function draftFrom(story: StoryView): Draft {
  return {
    id: story.id,
    version: story.version,
    title: story.title,
    situation: story.situation,
    task: story.task,
    actions: story.actions,
    outcome: story.outcome,
    competencies: story.competencies.join("\n"),
  };
}

function lines(value: string): string[] {
  return value
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0);
}

export default function StoryBank({
  client,
  reportGuard,
  onReplaceDraft,
}: {
  client: InterviewClient;
  reportGuard?: (guard: GuardSource | null) => void;
  onReplaceDraft?: (proceed: () => void) => void;
}) {
  const [filter, setFilter] = useState("");
  const [stories, setStories] = useState<StoryView[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<AppError | null>(null);
  const [reloadKey, setReloadKey] = useState(0);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [dirty, setDirty] = useState(false);
  const [problems, setProblems] = useState<string[]>([]);
  const [saveError, setSaveError] = useState<AppError | null>(null);
  const [saving, setSaving] = useState(false);
  const intention = useRef<{ key: string; requestId: string } | null>(null);

  useEffect(() => {
    let active = true;
    setLoading(true);
    setLoadError(null);
    client
      .listStories(filter.trim() === "" ? null : filter)
      .then((rows) => {
        if (active) {
          setStories(rows);
          setLoading(false);
        }
      })
      .catch((failure: unknown) => {
        if (active) {
          setStories([]);
          setLoadError(toAppError(failure));
          setLoading(false);
        }
      });
    return () => {
      active = false;
    };
  }, [client, filter, reloadKey]);

  function openDraft(next: () => Draft): void {
    setDraft(next());
    setDirty(false);
    setProblems([]);
    setSaveError(null);
    intention.current = null;
  }

  function requestDraft(next: () => Draft): void {
    if (dirty && onReplaceDraft) {
      onReplaceDraft(() => openDraft(next));
      return;
    }
    openDraft(next);
  }

  function cancel(): void {
    setDraft(null);
    setDirty(false);
    setProblems([]);
    setSaveError(null);
  }
  const discardRef = useRef(cancel);
  discardRef.current = cancel;

  function update(patch: Partial<Draft>): void {
    if (!draft) {
      return;
    }
    setDraft({ ...draft, ...patch });
    setDirty(true);
  }

  async function save(): Promise<SaveOutcome> {
    if (!draft || saving) {
      return "pending";
    }
    const competencies = lines(draft.competencies);
    const input: StoryInput = {
      title: draft.title.trim(),
      situation: draft.situation.trim(),
      task: draft.task.trim(),
      actions: draft.actions.trim(),
      outcome: draft.outcome.trim(),
      competencies,
    };
    const found: string[] = [];
    if (input.title === "") {
      found.push("Escreva um título para a história.");
    }
    if (
      [input.situation, input.task, input.actions, input.outcome].every(
        (note) => note === "",
      )
    ) {
      found.push("Escreva pelo menos uma nota (situação, tarefa, ações ou resultado).");
    }
    if (competencies.length === 0) {
      found.push("Indique pelo menos uma competência.");
    }
    setProblems(found);
    if (found.length > 0) {
      return "failed";
    }
    const intentionKey = `${draft.id ?? "new"}:${draft.version ?? 0}:${JSON.stringify(input)}`;
    if (intention.current?.key !== intentionKey) {
      intention.current = { key: intentionKey, requestId: newRequestId() };
    }
    const requestId = intention.current.requestId;
    setSaving(true);
    setSaveError(null);
    try {
      if (draft.id !== null) {
        await client.updateStory({
          requestId,
          id: draft.id,
          expectedVersion: draft.version ?? 1,
          input,
        });
      } else {
        await client.createStory({ requestId, input });
      }
      intention.current = null;
      setDraft(null);
      setDirty(false);
      setReloadKey((value) => value + 1);
      return "saved";
    } catch (failure) {
      setSaveError(toAppError(failure));
      return "failed";
    } finally {
      setSaving(false);
    }
  }
  const saveRef = useRef(save);
  saveRef.current = save;

  const guardDirty = draft !== null && dirty;
  useEffect(() => {
    if (!reportGuard) {
      return;
    }
    reportGuard({
      key: "stories",
      label: "edição das histórias",
      dirty: guardDirty,
      saving,
      error: saveError,
      save: () => saveRef.current(),
      discard: () => discardRef.current(),
    });
    return () => reportGuard(null);
  }, [reportGuard, guardDirty, saving, saveError]);

  return (
    <section className="practice__page" aria-labelledby="stories-title">
      <h2 id="stories-title">Histórias de preparação</h2>
      <p className="practice__hint">
        Notas factuais, só neste computador. Consultar, filtrar ou editar uma
        história nunca envia nada à API; só pode ser consultada como apoio
        antes de gravar e não preenche a resposta.
      </p>

      <div className="practice__field">
        <label htmlFor="story-filter">Filtrar por competência</label>
        <input
          id="story-filter"
          className="practice__input"
          type="text"
          value={filter}
          onChange={(event) => setFilter(event.target.value)}
        />
      </div>

      {loadError ? (
        <div className="practice__notice practice__notice--error" role="alert">
          <p>Não foi possível ler as histórias.</p>
          <p>{loadError.message}</p>
          <div className="practice__actions">
            <button
              type="button"
              onClick={() => setReloadKey((value) => value + 1)}
            >
              Tentar ler novamente
            </button>
          </div>
        </div>
      ) : loading ? (
        <p className="practice__hint">A carregar histórias…</p>
      ) : (
        <p className="practice__meta">
          {stories.length === 1
            ? "1 história nesta vista."
            : `${stories.length} histórias nesta vista.`}
        </p>
      )}

      {draft ? (
        <form
          className="practice__panel practice__form"
          aria-labelledby="story-editor-title"
          onSubmit={(event) => {
            event.preventDefault();
            void save();
          }}
        >
          <h3 id="story-editor-title">
            {draft.id === null ? "Nova história" : "Editar história"}
          </h3>
          {problems.length > 0 ? (
            <ul className="practice__errors" role="alert">
              {problems.map((message) => (
                <li key={message}>
                  <p className="practice__error-text">{message}</p>
                </li>
              ))}
            </ul>
          ) : null}
          {(
            [
              ["title", "Título", 1],
              ["situation", "Situação", 3],
              ["task", "Tarefa", 3],
              ["actions", "Ações", 4],
              ["outcome", "Resultado", 3],
            ] as const
          ).map(([field, label, rows]) => (
            <div className="practice__field" key={field}>
              <label htmlFor={`story-${field}`}>{label}</label>
              <textarea
                id={`story-${field}`}
                className="practice__textarea"
                rows={rows}
                lang={field === "title" ? undefined : "en"}
                value={draft[field]}
                disabled={saving}
                onChange={(event) => update({ [field]: event.target.value })}
              />
            </div>
          ))}
          <div className="practice__field">
            <label htmlFor="story-competencies">
              Competências (uma por linha)
            </label>
            <textarea
              id="story-competencies"
              className="practice__textarea"
              rows={2}
              value={draft.competencies}
              disabled={saving}
              onChange={(event) => update({ competencies: event.target.value })}
            />
          </div>
          <p className="practice__hint">
            O resultado e o STAR completo não são obrigatórios: pode guardar
            uma história parcial, incluindo resultados negativos ou
            aprendizagens.
          </p>
          <div className="practice__actions">
            <button type="submit" disabled={saving}>
              {draft.id === null ? "Guardar história" : "Guardar alterações"}
            </button>
            <button type="button" onClick={cancel} disabled={saving}>
              Cancelar
            </button>
          </div>
          {saveError ? (
            <div
              className="practice__notice practice__notice--error"
              role="alert"
            >
              <p>{saveError.message}</p>
              <p className="practice__hint">Código: {saveError.code}.</p>
            </div>
          ) : null}
        </form>
      ) : (
        <div className="practice__actions">
          <button
            type="button"
            disabled={loading}
            onClick={() => requestDraft(emptyDraft)}
          >
            Adicionar história
          </button>
        </div>
      )}

      {stories.length > 0 ? (
        <ul className="practice__list" aria-label="Histórias guardadas">
          {stories.map((story) => (
            <li key={story.id}>
              <article
                className="practice__item"
                aria-labelledby={`story-${story.id}`}
              >
                <h3 id={`story-${story.id}`}>{story.title}</h3>
                <p className="practice__meta">
                  Versão {story.version}. Competências:{" "}
                  {story.competencies.join(", ")}.
                </p>
                {story.missingFields.length > 0 ? (
                  <p className="practice__hint">
                    Campos em branco:{" "}
                    {story.missingFields
                      .map((field) => FIELD_LABELS[field])
                      .join(", ")}
                    . É apenas um rótulo, não uma avaliação.
                  </p>
                ) : (
                  <p className="practice__hint">Todos os campos STAR escritos.</p>
                )}
                <div className="practice__actions">
                  <button
                    type="button"
                    aria-label={`Editar história: ${story.title}`}
                    disabled={saving}
                    onClick={() => requestDraft(() => draftFrom(story))}
                  >
                    Editar
                  </button>
                </div>
              </article>
            </li>
          ))}
        </ul>
      ) : null}
    </section>
  );
}

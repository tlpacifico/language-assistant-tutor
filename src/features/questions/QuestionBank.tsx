import { useEffect, useRef, useState } from "react";

import type { AppError } from "../../lib/contracts";
import { toAppError } from "../../lib/app-error";
import type { InterviewClient } from "../../lib/interview-client";
import {
  CATEGORY_LABELS,
  type Category,
  type Question,
  type QuestionInput,
} from "../../lib/interview-contracts";
import type { GuardSource } from "../navigation/NavigationGuard";
import type { SaveOutcome } from "../interview/useInterview";

/** Filtro da lista: todas as categorias ou uma delas. */
type Filter = Category | "all";

/** Rascunho do formulário, com listas editadas como uma linha por item. */
interface Draft {
  id: string | null;
  version: number | null;
  text: string;
  category: Category;
  competencies: string;
  criteria: string;
}

function emptyDraft(): Draft {
  return {
    id: null,
    version: null,
    text: "",
    category: "conflict_collaboration",
    competencies: "",
    criteria: "",
  };
}

function draftFrom(question: Question): Draft {
  return {
    id: question.id,
    version: question.version,
    text: question.text,
    category: question.category,
    competencies: question.competencies.join("\n"),
    criteria: question.criteria.join("\n"),
  };
}

/** Ignora linhas vazias e espaços exteriores; não altera o conteúdo interior. */
function lines(value: string): string[] {
  return value
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0);
}

function questionsLabel(total: number): string {
  return total === 1 ? "1 pergunta nesta vista." : `${total} perguntas nesta vista.`;
}

/**
 * Banco de perguntas editável.
 *
 * Lista as perguntas do banco, filtradas por categoria, e permite criar e
 * editar. Cada edição envia a versão esperada para que uma alteração
 * concorrente seja recusada pelo núcleo em vez de sobrescrita. Eliminação,
 * importação em lote e gestão de categorias ou de um catálogo de competências
 * ficam fora deste incremento: competências e critérios editam-se na própria
 * pergunta.
 *
 * O rascunho é local, mas o facto de estar por guardar sobe para a aplicação
 * pela mesma decisão que protege a revisão: mudar de página, fechar a janela ou
 * trocar de pergunta no editor nunca descarta uma edição em silêncio.
 */
export default function QuestionBank({
  client,
  reportGuard,
  onReplaceDraft,
}: {
  client: InterviewClient;
  /** Entrega o estado do editor à decisão única da aplicação. */
  reportGuard?: (guard: GuardSource | null) => void;
  /** Pede a decisão guardar/descartar/cancelar antes de trocar de rascunho. */
  onReplaceDraft?: (proceed: () => void) => void;
}) {
  const [filter, setFilter] = useState<Filter>("all");
  const [questions, setQuestions] = useState<Question[]>([]);
  const [loadError, setLoadError] = useState<AppError | null>(null);
  const [loading, setLoading] = useState(true);
  const [reloadKey, setReloadKey] = useState(0);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [draftDirty, setDraftDirty] = useState(false);
  const [errors, setErrors] = useState<string[]>([]);
  const [saveError, setSaveError] = useState<AppError | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    let active = true;
    setLoading(true);
    setLoadError(null);
    client
      .listQuestions(filter === "all" ? null : filter)
      .then((rows) => {
        if (active) {
          setQuestions(rows);
          setLoading(false);
        }
      })
      .catch((error: unknown) => {
        if (active) {
          setQuestions([]);
          setLoadError(toAppError(error));
          setLoading(false);
        }
      });
    return () => {
      active = false;
    };
  }, [client, filter, reloadKey]);

  function beginNew(): void {
    requestDraft(() => emptyDraft());
  }

  function beginEdit(question: Question): void {
    requestDraft(() => draftFrom(question));
  }

  /** Abre o rascunho, pedindo a decisão se já houver alterações por guardar. */
  function requestDraft(next: () => Draft): void {
    if (draftDirty && onReplaceDraft) {
      onReplaceDraft(() => openDraft(next));
      return;
    }
    openDraft(next);
  }

  function openDraft(next: () => Draft): void {
    setDraft(next());
    setDraftDirty(false);
    setErrors([]);
    setSaveError(null);
  }

  function cancel(): void {
    setDraft(null);
    setDraftDirty(false);
    setErrors([]);
    setSaveError(null);
  }

  /** Descarte explícito pedido pela decisão partilhada. */
  const discardRef = useRef(cancel);
  discardRef.current = cancel;

  async function save(): Promise<SaveOutcome> {
    if (!draft || saving) {
      return "pending";
    }
    const text = draft.text.trim();
    const competencies = lines(draft.competencies);
    const criteria = lines(draft.criteria);
    const problems: string[] = [];
    if (text.length === 0) {
      problems.push("Escreva o texto da pergunta em inglês.");
    }
    if (competencies.length === 0) {
      problems.push("Indique pelo menos uma competência.");
    }
    if (criteria.length === 0) {
      problems.push("Indique pelo menos um critério.");
    }
    setErrors(problems);
    if (problems.length > 0) {
      return "failed";
    }

    const input: QuestionInput = {
      text,
      category: draft.category,
      competencies,
      criteria,
    };
    setSaving(true);
    setSaveError(null);
    try {
      if (draft.id !== null) {
        await client.updateQuestion(draft.id, draft.version ?? 1, input);
      } else {
        await client.createQuestion(input);
      }
      setDraft(null);
      setDraftDirty(false);
      setErrors([]);
      // Reler o banco confirma o que ficou guardado e evita divergir da versão.
      setReloadKey((value) => value + 1);
      return "saved";
    } catch (error) {
      // O editor mantém o texto e a versão vistos pelo utilizador.
      setSaveError(toAppError(error));
      return "failed";
    } finally {
      setSaving(false);
    }
  }

  const saveRef = useRef(save);
  saveRef.current = save;

  // A decisão partilhada recebe o estado mais recente do editor; as ações leem
  // sempre o último render, para uma escrita não usar um rascunho antigo.
  const dirty = draft !== null && draftDirty;
  useEffect(() => {
    if (!reportGuard) {
      return;
    }
    reportGuard({
      key: "bank",
      label: "edição do banco de perguntas",
      dirty,
      saving,
      error: saveError,
      save: () => saveRef.current(),
      discard: () => discardRef.current(),
    });
    return () => reportGuard(null);
  }, [reportGuard, dirty, saving, saveError]);

  return (
    <section className="practice__page" aria-labelledby="bank-title">
      <h2 id="bank-title">Banco de perguntas</h2>
      <p className="practice__hint">
        As perguntas são apresentadas em inglês; a interface e as explicações
        ficam em português.
      </p>

      <div className="practice__field">
        <label htmlFor="bank-filter">Filtrar por categoria</label>
        <select
          id="bank-filter"
          className="practice__select"
          value={filter}
          onChange={(event) => {
            const next = event.target.value;
            setFilter(next === "all" ? "all" : (next as Category));
          }}
        >
          <option value="all">Todas as categorias</option>
          {(Object.keys(CATEGORY_LABELS) as Category[]).map((value) => (
            <option key={value} value={value}>
              {CATEGORY_LABELS[value]}
            </option>
          ))}
        </select>
      </div>

      {loadError ? (
        <div className="practice__notice practice__notice--error" role="alert">
          <p>Não foi possível ler o banco de perguntas.</p>
          <p>{loadError.message}</p>
          <p className="practice__hint">Código: {loadError.code}.</p>
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
        <p className="practice__hint">A carregar perguntas…</p>
      ) : (
        <p className="practice__meta">{questionsLabel(questions.length)}</p>
      )}

      {draft ? (
        <form
          className="practice__panel practice__form"
          aria-labelledby="editor-title"
          onSubmit={(event) => {
            event.preventDefault();
            void save();
          }}
        >
          <h3 id="editor-title">
            {draft.id === null ? "Nova pergunta" : "Editar pergunta"}
          </h3>

          {errors.length > 0 ? (
            <ul className="practice__errors" role="alert">
              {errors.map((message) => (
                <li key={message}>
                  <p className="practice__error-text">{message}</p>
                </li>
              ))}
            </ul>
          ) : null}

          <div className="practice__field">
            <label htmlFor="question-text">
              Texto da pergunta (em inglês)
            </label>
            <textarea
              id="question-text"
              className="practice__textarea"
              rows={3}
              value={draft.text}
              onChange={(event) => {
                setDraft({ ...draft, text: event.target.value });
                setDraftDirty(true);
              }}
              disabled={saving}
            />
          </div>

          <div className="practice__field">
            <label htmlFor="question-category">Categoria</label>
            <select
              id="question-category"
              className="practice__select"
              value={draft.category}
              onChange={(event) => {
                setDraft({ ...draft, category: event.target.value as Category });
                setDraftDirty(true);
              }}
              disabled={saving}
            >
              {(Object.keys(CATEGORY_LABELS) as Category[]).map((value) => (
                <option key={value} value={value}>
                  {CATEGORY_LABELS[value]}
                </option>
              ))}
            </select>
          </div>

          <div className="practice__field">
            <label htmlFor="question-competencies">
              Competências (uma por linha)
            </label>
            <textarea
              id="question-competencies"
              className="practice__textarea"
              rows={2}
              value={draft.competencies}
              onChange={(event) => {
                setDraft({ ...draft, competencies: event.target.value });
                setDraftDirty(true);
              }}
              disabled={saving}
              aria-describedby="question-competencies-hint"
            />
            <p className="practice__hint" id="question-competencies-hint">
              Uma competência por linha. É exigida pelo menos uma.
            </p>
          </div>

          <div className="practice__field">
            <label htmlFor="question-criteria">
              Critérios (uma por linha)
            </label>
            <textarea
              id="question-criteria"
              className="practice__textarea"
              rows={3}
              value={draft.criteria}
              onChange={(event) => {
                setDraft({ ...draft, criteria: event.target.value });
                setDraftDirty(true);
              }}
              disabled={saving}
              aria-describedby="question-criteria-hint"
            />
            <p className="practice__hint" id="question-criteria-hint">
              Um critério por linha. É exigido pelo menos um.
            </p>
          </div>

          <div className="practice__actions">
            <button type="submit" disabled={saving}>
              {draft.id === null ? "Guardar pergunta" : "Guardar alterações"}
            </button>
            <button type="button" onClick={cancel} disabled={saving}>
              Cancelar
            </button>
          </div>

          {saveError ? (
            <div className="practice__notice practice__notice--error" role="alert">
              <p>{saveError.message}</p>
              <p className="practice__hint">Código: {saveError.code}.</p>
            </div>
          ) : null}
        </form>
      ) : (
        <div className="practice__actions">
          <button type="button" onClick={beginNew} disabled={loading}>
            Adicionar pergunta
          </button>
        </div>
      )}

      {questions.length > 0 ? (
        <ul className="practice__list" aria-label="Perguntas do banco">
          {questions.map((question) => (
            <li key={question.id}>
              <article
                className="practice__item"
                aria-labelledby={`question-${question.id}`}
              >
                <h3 id={`question-${question.id}`} lang="en">
                  {question.text}
                </h3>
                <p className="practice__meta">
                  {CATEGORY_LABELS[question.category]}
                </p>
                <p className="practice__meta">Versão {question.version}.</p>
                <div className="practice__field">
                  <p className="practice__meta">Competências</p>
                  <ul className="practice__sublist">
                    {question.competencies.map((competency, index) => (
                      <li key={`${question.id}-competency-${index}`}>
                        {competency}
                      </li>
                    ))}
                  </ul>
                </div>
                <div className="practice__field">
                  <p className="practice__meta">Critérios</p>
                  <ul className="practice__sublist">
                    {question.criteria.map((criterion, index) => (
                      <li key={`${question.id}-criterion-${index}`}>{criterion}</li>
                    ))}
                  </ul>
                </div>
                <div className="practice__actions">
                  <button
                    type="button"
                    aria-label={`Editar pergunta: ${question.text}`}
                    onClick={() => beginEdit(question)}
                    disabled={saving}
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

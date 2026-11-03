/**
 * Comparação local de duas respostas confirmadas da mesma pergunta.
 *
 * Só leitura: a página não envia nada à API, não pede avaliação e não escreve
 * texto analítico sobre melhoria. Mostra as duas origens, separa resposta
 * inicial, seguimento e tentativa, distingue transcrição automática de texto
 * confirmado e apresenta as sugestões da IA numa zona própria. Um relatório em
 * falta ou com rubrica diferente nunca aparece como melhoria comprovada.
 */

import { useEffect, useState } from "react";

import { toAppError } from "../../lib/app-error";
import type { AppError } from "../../lib/contracts";
import type { InterviewClient } from "../../lib/interview-client";
import {
  originKey,
  type ComparisonEntry,
  type ComparisonRequest,
  type ComparisonView,
  type RubricCompatibility,
} from "../../lib/practice-contracts";
import { ObservationView } from "../ai/FeedbackPanel";
import { formatMoment, originLabel } from "./shared";

const ROLE_LABELS: Record<string, string> = {
  initial: "Resposta inicial",
  follow_up: "Seguimento",
  attempt: "Tentativa de prática",
};

const REPORT_STATE_LABELS: Record<ComparisonEntry["reportState"], string> = {
  pending: "Relatório ainda não pedido para esta origem.",
  running: "Relatório em processamento.",
  failed: "O pedido do relatório falhou; não existe relatório válido.",
  ready: "Relatório disponível.",
  not_available:
    "Sem relatório para esta origem. Esta comparação não pede avaliação.",
};

const COMPATIBILITY_MESSAGES: Record<RubricCompatibility, string> = {
  same: "Os dois relatórios usam a mesma rubrica.",
  different:
    "Rubricas ou versões diferentes: as observações não são comparáveis e não indicam melhoria comprovada.",
  unknown:
    "Rubrica desconhecida ou relatório em falta: não há comparação de avaliação.",
};

function EntryView({
  entry,
  heading,
  onPlay,
}: {
  entry: ComparisonEntry;
  heading: string;
  onPlay: (recordingId: string) => void;
}) {
  const report = entry.report;
  return (
    <article className="practice__item" aria-label={heading}>
      <h4>{heading}</h4>
      <p className="practice__meta">
        Criada em {formatMoment(entry.createdAt)}
        {entry.confirmedAt
          ? `; confirmada em ${formatMoment(entry.confirmedAt)}`
          : "; ainda sem confirmação"}
        .
      </p>
      <p className="practice__hint">
        Apoio de história:{" "}
        {entry.preparation.support === "story" && entry.preparation.snapshot
          ? `«${entry.preparation.snapshot.content.title}» (versão ${entry.preparation.snapshot.storyVersion}), fixado na primeira gravação.`
          : entry.preparation.support === "historical_none"
            ? "sem registo (resposta anterior ao apoio)."
            : "nenhum."}
      </p>

      {entry.answers.map((answer) => (
        <section
          key={answer.role}
          aria-label={`${ROLE_LABELS[answer.role] ?? answer.role} — ${heading}`}
          className="practice__field"
        >
          <h5>{ROLE_LABELS[answer.role] ?? answer.role}</h5>
          {answer.recordings.map((recording) => (
            <div key={recording.id}>
              {recording.transcripts.length > 0 ? (
                <>
                  <p className="practice__meta">
                    Transcrição automática (tomada {recording.ordinal})
                  </p>
                  {recording.transcripts.map((transcript) => (
                    <p key={transcript.id} lang="en">
                      {transcript.text}
                    </p>
                  ))}
                </>
              ) : null}
              {recording.originalFile ? (
                <div className="practice__actions">
                  <button type="button" onClick={() => onPlay(recording.id)}>
                    Ouvir original da tomada {recording.ordinal} ({heading})
                  </button>
                </div>
              ) : null}
            </div>
          ))}
          <p className="practice__meta">Texto confirmado</p>
          {answer.confirmation ? (
            <p lang="en">{answer.confirmation.text}</p>
          ) : (
            <p className="practice__hint">Sem texto confirmado.</p>
          )}
        </section>
      ))}

      <section aria-label={`Relatório — ${heading}`}>
        <h5>Relatório</h5>
        {report ? (
          <>
            <p className="practice__hint">
              Rubrica {report.versions.rubricVersion}; prompt{" "}
              {report.versions.promptVersion}; esquema{" "}
              {report.versions.schemaVersion}.
            </p>
            {report.content.content.map((observation) => (
              <ObservationView key={observation.id} o={observation} />
            ))}
            {report.content.gaps.map((observation) => (
              <ObservationView key={observation.id} o={observation} />
            ))}
          </>
        ) : (
          <p className="practice__hint">
            {REPORT_STATE_LABELS[entry.reportState]}
          </p>
        )}
      </section>

      <aside aria-label={`Sugestões da IA — ${heading}`}>
        <h5>Sugestão da IA — confirme os factos</h5>
        <p className="practice__hint">
          Zona separada: a sugestão não faz parte da resposta confirmada.
        </p>
        {entry.rewrites.length === 0 ? (
          <p className="practice__hint">Sem sugestões guardadas.</p>
        ) : (
          entry.rewrites.map((rewrite) => (
            <div key={`${originKey(rewrite.origin)}-${rewrite.level}`}>
              <p className="practice__meta">Nível {rewrite.level}</p>
              {rewrite.segments.map((segment) => (
                <p key={segment.id} lang="en">
                  {segment.text}
                </p>
              ))}
            </div>
          ))
        )}
      </aside>
    </article>
  );
}

export default function Comparison({
  client,
  request,
  onClose,
}: {
  client: InterviewClient;
  request: ComparisonRequest;
  onClose?: () => void;
}) {
  const [view, setView] = useState<ComparisonView | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [playError, setPlayError] = useState<AppError | null>(null);
  const requestKey = `${originKey(request.left)}|${originKey(request.right)}`;

  useEffect(() => {
    let active = true;
    setView(null);
    setError(null);
    client
      .compareResponses(request)
      .then((next) => {
        if (active) {
          setView(next);
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
    // O pedido é identificado pelas duas origens.
  }, [client, requestKey]);

  async function play(recordingId: string): Promise<void> {
    setPlayError(null);
    try {
      await client.playAudio({ type: "recording", recordingId });
    } catch (failure) {
      setPlayError(toAppError(failure));
    }
  }

  return (
    <section
      className="practice__panel practice__panel--wide"
      aria-labelledby={`comparison-title-${requestKey}`}
    >
      <h3 id={`comparison-title-${requestKey}`}>Comparação das respostas</h3>
      <p className="practice__hint">
        Comparação local e só de leitura: mostra as duas respostas lado a lado,
        sem texto analítico sobre melhoria e sem nenhum pedido à API.
      </p>
      {onClose ? (
        <div className="practice__actions">
          <button type="button" onClick={onClose}>
            Fechar comparação
          </button>
        </div>
      ) : null}
      {error ? (
        <div className="practice__notice practice__notice--error" role="alert">
          <p>Não foi possível comparar as respostas.</p>
          <p>{error.message}</p>
        </div>
      ) : view === null ? (
        <p className="practice__hint" role="status">
          A preparar a comparação…
        </p>
      ) : (
        <>
          <p className="practice__state" role="status">
            {COMPATIBILITY_MESSAGES[view.rubricCompatibility]}
          </p>
          <div className="practice__compare">
            <EntryView
              entry={view.left}
              heading={`${originLabel(view.left.origin)} · ${formatMoment(view.left.createdAt)}`}
              onPlay={(id) => void play(id)}
            />
            <EntryView
              entry={view.right}
              heading={`${originLabel(view.right.origin)} · ${formatMoment(view.right.createdAt)}`}
              onPlay={(id) => void play(id)}
            />
          </div>
        </>
      )}
      {playError ? (
        <p role="alert" className="practice__warning">
          {playError.message}
        </p>
      ) : null}
    </section>
  );
}

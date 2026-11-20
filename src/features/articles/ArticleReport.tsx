import type {
  ArticleDimension,
  ArticlePlayerView,
  ArticleReport as Report,
  ArticleTurnView,
  DimensionStatus,
} from "../../lib/article-contracts";
import ArticleTurnControls from "./ArticleTurnControls";

const STATUS_LABELS: Record<DimensionStatus, string> = {
  clear: "Claro",
  partial: "Parcial",
  insufficient_evidence: "Evidência insuficiente",
};

const DIMENSIONS: {
  key: "coherence" | "cohesion" | "development" | "interaction";
  label: string;
  hint: string;
}[] = [
  {
    key: "coherence",
    label: "Coerência",
    hint: "Relevância, continuidade da ideia central e contradições.",
  },
  {
    key: "cohesion",
    label: "Coesão",
    hint: "Ligação entre frases, conectores e referências claras.",
  },
  {
    key: "development",
    label: "Desenvolvimento",
    hint: "Razões, exemplos e apoio às opiniões.",
  },
  {
    key: "interaction",
    label: "Interação",
    hint: "Resposta aos comentários do tutor e capacidade de aprofundar.",
  },
];

function DimensionCard({
  label,
  hint,
  dimension,
  turns,
  player,
  onPlay,
  onPause,
  onResume,
  onRestart,
}: {
  label: string;
  hint: string;
  dimension: ArticleDimension;
  turns: ArticleTurnView[];
  player: ArticlePlayerView;
  onPlay: (turnId: string) => void;
  onPause: () => void;
  onResume: () => void;
  onRestart: (turnId: string) => void;
}) {
  const headingId = `article-dimension-${label}`;
  return (
    <section className="article__dimension" aria-labelledby={headingId}>
      <h3 id={headingId}>{label}</h3>
      <p className="practice__hint">{hint}</p>
      <p>
        <strong>{STATUS_LABELS[dimension.status]}</strong>
      </p>
      <p>{dimension.summary}</p>
      {dimension.status === "insufficient_evidence" ? (
        <p className="practice__hint">
          Não há texto suficiente nas suas respostas para avaliar esta
          dimensão.
        </p>
      ) : null}
      {dimension.evidence.map((item, index) => {
        const turn = turns.find((candidate) => candidate.id === item.turnId);
        return (
          <figure key={`${item.turnId}-${index}`} className="article__evidence">
            <blockquote lang="en">{item.quote}</blockquote>
            <figcaption>{item.note}</figcaption>
            {turn ? (
              <ArticleTurnControls
                turn={turn}
                player={player}
                onPlay={onPlay}
                onPause={onPause}
                onResume={onResume}
                onRestart={onRestart}
              />
            ) : null}
          </figure>
        );
      })}
    </section>
  );
}

/**
 * Feedback final: quatro dimensões com citações das suas respostas e até três
 * melhorias. Baseia-se só no texto transcrito, não avalia pronúncia nem ritmo.
 * As citações reutilizam os mesmos controlos de reprodução da conversa.
 */
export default function ArticleReport({
  report,
  turns,
  player,
  onPlay,
  onPause,
  onResume,
  onRestart,
}: {
  report: Report;
  turns: ArticleTurnView[];
  player: ArticlePlayerView;
  onPlay: (turnId: string) => void;
  onPause: () => void;
  onResume: () => void;
  onRestart: (turnId: string) => void;
}) {
  return (
    <section className="practice__panel article__report" aria-label="Feedback">
      <h2>Feedback</h2>
      <p className="practice__hint">
        Baseado apenas no texto das suas respostas; não avalia pronúncia, ritmo
        nem confiança.
      </p>
      {DIMENSIONS.map(({ key, label, hint }) => (
        <DimensionCard
          key={key}
          label={label}
          hint={hint}
          dimension={report[key]}
          turns={turns}
          player={player}
          onPlay={onPlay}
          onPause={onPause}
          onResume={onResume}
          onRestart={onRestart}
        />
      ))}
      {report.improvements.length > 0 ? (
        <>
          <h3>Melhorias prioritárias</h3>
          <ol aria-label="Melhorias prioritárias">
            {report.improvements.slice(0, 3).map((item) => (
              <li key={item} lang="en">
                {item}
              </li>
            ))}
          </ol>
        </>
      ) : null}
    </section>
  );
}

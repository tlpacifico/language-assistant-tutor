import { MICROPHONE_PATH } from "./icon-paths";
import type { SessionSummary } from "../../lib/interview-contracts";
import { CATEGORY_LABELS } from "../../lib/interview-contracts";

export type Page =
  | "setup"
  | "bank"
  | "stories"
  | "interview"
  | "history"
  | "practice"
  | "ai";

export const PAGE_COPY: Record<
  Page,
  { label: string; title: string; description: string }
> = {
  setup: {
    label: "Preparação",
    title: "Prepare a próxima entrevista",
    description:
      "Verifique o ambiente, prepare as suas histórias e retome a sessão exatamente onde ficou.",
  },
  bank: {
    label: "Banco de perguntas",
    title: "Perguntas que valem ensaio",
    description:
      "Perguntas em inglês com competências e critérios de avaliação. Filtre por categoria para preparar o seu treino.",
  },
  stories: {
    label: "Histórias",
    title: "Histórias de preparação",
    description:
      "Notas factuais, consultadas antes de responder. Edite ou filtre por competência para encontrar o exemplo certo.",
  },
  interview: {
    label: "Entrevista",
    title: "Sessão de prática",
    description:
      "Ouça, responda com as suas palavras e reveja o que disse antes de confirmar.",
  },
  history: {
    label: "Histórico",
    title: "Sessões anteriores",
    description:
      "Reveja as suas sessões, ouça respostas e escolha o que quer praticar de novo.",
  },
  practice: {
    label: "Prática",
    title: "Dê mais clareza à sua resposta",
    description:
      "Reformule, pratique e compare sem perder a resposta original.",
  },
  ai: {
    label: "Configuração IA",
    title: "Como o tutor avalia",
    description:
      "Configure o serviço de IA, a autorização de envio e a referência de custos.",
  },
};

const ICON_PATHS: Record<Page, string> = {
  setup:
    "M4 7h9m5 0h2M4 17h2m5 0h9M18 7a2.5 2.5 0 1 1-5 0 2.5 2.5 0 0 1 5 0ZM11 17a2.5 2.5 0 1 1-5 0 2.5 2.5 0 0 1 5 0Z",
  bank: "M9 6h11M9 12h11M9 18h11M4 6h1M4 12h1M4 18h1",
  stories: "M4 6a3 3 0 0 1 3-3h13v15H7a3 3 0 0 0-3 3V6Zm0 15v-3M8 8h8M8 12h6",
  interview: MICROPHONE_PATH,
  history: "M20.5 12a8.5 8.5 0 1 1-17 0 8.5 8.5 0 0 1 17 0ZM12 7.5V12l3 2",
  practice: "m4 20 4-1 12-12-3-3L5 16l-1 4ZM14 7l3 3",
  ai: "M15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0ZM12 3v2m0 14v2M3 12h2m14 0h2M6 6l1 1m10 10 1 1M18 6l-1 1M7 17l-1 1",
};

export function TutorIcon({ name }: { name: Page }) {
  return (
    <svg
      className="tutor-icon"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.7"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d={ICON_PATHS[name]} />
    </svg>
  );
}

export default function TutorNavigation({
  page,
  session,
  onNavigate,
}: {
  page: Page;
  session: SessionSummary | null;
  onNavigate: (page: Page) => void;
}) {
  return (
    <aside className="tutor-sidebar">
      <div className="tutor-brand">
        <span className="tutor-brand__mark" aria-hidden="true">
          T
        </span>
        <div>
          <strong>Tutor de entrevistas</strong>
          <span>Entrevistas comportamentais</span>
        </div>
      </div>
      <nav aria-label="Navegação principal">
        <p className="tutor-nav-label">Navegação</p>
        <div className="tutor-nav-list">
          {(
            [
              "setup",
              "bank",
              "stories",
              "interview",
              "history",
              ...(page === "practice" ? ["practice"] : []),
              "ai",
            ] as Page[]
          ).map((id) => (
            <button
              key={id}
              type="button"
              aria-current={page === id ? "page" : undefined}
              onClick={() => onNavigate(id)}
            >
              <TutorIcon name={id} />
              {PAGE_COPY[id].label}
            </button>
          ))}
        </div>
      </nav>
      {session?.status === "in_progress" && (
        <div className="tutor-session-mini">
          <p className="tutor-eyebrow">
            <span className="tutor-dot" />
            Sessão em curso
          </p>
          <strong>
            {session.category
              ? CATEGORY_LABELS[session.category]
              : "Entrevista comportamental"}
          </strong>
          <span>
            Pergunta {session.position} de {session.count} ·{" "}
            {session.confirmedCount}{" "}
            {session.confirmedCount === 1 ? "confirmada" : "confirmadas"}
          </span>
          <button type="button" onClick={() => onNavigate("interview")}>
            Ir para a entrevista <span aria-hidden="true">→</span>
          </button>
        </div>
      )}
    </aside>
  );
}

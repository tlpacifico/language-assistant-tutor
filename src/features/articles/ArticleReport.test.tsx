import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { ArticleReport as Report } from "../../lib/article-contracts";
import { makeTurn } from "../../test/fake-article-client";
import ArticleReport from "./ArticleReport";

const dimension = (
  status: "clear" | "partial" | "insufficient_evidence",
  evidence: Report["coherence"]["evidence"] = [],
) => ({ status, summary: `Resumo ${status}.`, evidence });

const report: Report = {
  id: "rep-1",
  branchId: "branch-1",
  versionIds: ["v1"],
  coherence: dimension("clear", [
    { turnId: "turn-2", quote: "remote work helps", note: "Mantém o tema." },
  ]),
  cohesion: dimension("insufficient_evidence"),
  development: dimension("partial", [
    { turnId: "turn-9", quote: "citação sem áudio", note: "Falta exemplo." },
  ]),
  interaction: dimension("insufficient_evidence"),
  improvements: ["Use connectors.", "Add an example.", "Link ideas."],
};

const turns = [makeTurn(2, "user", "Remote work helps people focus.")];

describe("ArticleReport", () => {
  it("mostra as quatro dimensões com estado e insuficiência de evidência explícita", () => {
    render(<ArticleReport report={report} turns={turns} onPlay={() => undefined} />);
    for (const name of ["Coerência", "Coesão", "Desenvolvimento", "Interação"]) {
      expect(screen.getByRole("heading", { name })).toBeInTheDocument();
    }
    expect(screen.getAllByText("Evidência insuficiente")).toHaveLength(2);
    expect(screen.getByText("Claro")).toBeInTheDocument();
    expect(screen.getByText("Parcial")).toBeInTheDocument();
    // Sem pontuação global de proficiência.
    expect(screen.queryByText(/nível|nota global|score/i)).not.toBeInTheDocument();
  });

  it("mostra as citações e só oferece áudio quando o turno tem gravação", async () => {
    const user = userEvent.setup();
    const onPlay = vi.fn();
    render(<ArticleReport report={report} turns={turns} onPlay={onPlay} />);
    const coherence = screen.getByRole("region", { name: "Coerência" });
    expect(within(coherence).getByText(/remote work helps/)).toBeInTheDocument();
    await user.click(
      within(coherence).getByRole("button", { name: "Ouvir a resposta citada" }),
    );
    expect(onPlay).toHaveBeenCalledWith("turn-2");
    const development = screen.getByRole("region", { name: "Desenvolvimento" });
    expect(
      within(development).queryByRole("button", { name: "Ouvir a resposta citada" }),
    ).not.toBeInTheDocument();
  });

  it("limita a três melhorias e distingue-as de uma avaliação global", () => {
    render(
      <ArticleReport
        report={{ ...report, improvements: ["a", "b", "c", "d"] }}
        turns={turns}
        onPlay={() => undefined}
      />,
    );
    const list = screen.getByRole("list", { name: "Melhorias prioritárias" });
    expect(within(list).getAllByRole("listitem")).toHaveLength(3);
  });
});

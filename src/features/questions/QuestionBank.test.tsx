import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import {
  fakeInterviewClient,
  makeQuestion,
} from "../../test/fake-interview-client";

import QuestionBank from "./QuestionBank";

const CONFLICT = makeQuestion({
  id: "q-1",
  category: "conflict_collaboration",
  text: "Tell me about a time you disagreed with a colleague.",
  competencies: ["Gestão de conflitos"],
  criteria: ["Explicar a divergência, a ação própria e o desfecho."],
});

const FAILURE = makeQuestion({
  id: "q-8",
  category: "failure_learning",
  text: "Tell me about a time you made a mistake at work.",
  competencies: ["Responsabilidade"],
  criteria: ["Reconhecer o papel próprio, a reação e a aprendizagem."],
});

function renderBank() {
  const fake = fakeInterviewClient({ questions: [CONFLICT, FAILURE] });
  const view = render(<QuestionBank client={fake.client} />);
  return { ...fake, ...view };
}

describe("QuestionBank", () => {
  it("lists_the_questions_with_category_competencies_and_criteria", async () => {
    renderBank();

    expect(
      await screen.findByRole("heading", {
        name: "Tell me about a time you disagreed with a colleague.",
      }),
    ).toBeInTheDocument();
    const list = screen.getByRole("list", { name: "Perguntas do banco" });
    expect(within(list).getByText("Conflitos e colaboração")).toBeInTheDocument();
    expect(within(list).getByText("Gestão de conflitos")).toBeInTheDocument();
    expect(
      within(list).getByText(
        "Explicar a divergência, a ação própria e o desfecho.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByText("2 perguntas nesta vista.")).toBeInTheDocument();
  });

  it("filters_the_list_by_category", async () => {
    const user = userEvent.setup();
    const fake = renderBank();
    await screen.findByText("2 perguntas nesta vista.");

    await user.selectOptions(
      screen.getByLabelText("Filtrar por categoria"),
      "failure_learning",
    );

    expect(
      await screen.findByText("1 pergunta nesta vista."),
    ).toBeInTheDocument();
    expect(fake.client.listQuestions).toHaveBeenLastCalledWith(
      "failure_learning",
    );
    expect(
      screen.queryByRole("heading", {
        name: "Tell me about a time you disagreed with a colleague.",
      }),
    ).not.toBeInTheDocument();
  });

  it("bank_edit_survives_reload", async () => {
    const user = userEvent.setup();
    const bank = renderBank();
    await screen.findByText("2 perguntas nesta vista.");

    await user.click(
      screen.getByRole("button", {
        name: "Editar pergunta: Tell me about a time you disagreed with a colleague.",
      }),
    );

    const text = screen.getByLabelText("Texto da pergunta (em inglês)");
    expect(text).toHaveValue(CONFLICT.text);
    await user.clear(text);
    await user.type(
      text,
      "Describe a disagreement you resolved with a colleague.",
    );

    const competencies = screen.getByLabelText(
      "Competências (uma por linha)",
    );
    await user.clear(competencies);
    await user.type(competencies, "Comunicação{Enter}Colaboração");

    const criteria = screen.getByLabelText("Critérios (uma por linha)");
    await user.clear(criteria);
    await user.type(
      criteria,
      "Explicar a divergência.{Enter}Explicar o desfecho.",
    );

    await user.click(
      screen.getByRole("button", { name: "Guardar alterações" }),
    );

    await waitFor(() => expect(bank.updates).toHaveLength(1));
    expect(bank.updates[0]).toEqual({
      id: "q-1",
      expectedVersion: 1,
      input: {
        text: "Describe a disagreement you resolved with a colleague.",
        category: "conflict_collaboration",
        competencies: ["Comunicação", "Colaboração"],
        criteria: ["Explicar a divergência.", "Explicar o desfecho."],
      },
    });

    // Recarregar a página relê o banco guardado: a edição persiste.
    bank.unmount();
    render(<QuestionBank client={bank.client} />);
    expect(
      await screen.findByRole("heading", {
        name: "Describe a disagreement you resolved with a colleague.",
      }),
    ).toBeInTheDocument();
    expect(screen.getByText("Versão 2.")).toBeInTheDocument();
  });

  it("bank_rejects_empty_fields", async () => {
    const user = userEvent.setup();
    const fake = renderBank();
    await screen.findByText("2 perguntas nesta vista.");

    await user.click(screen.getByRole("button", { name: "Adicionar pergunta" }));
    await user.click(screen.getByRole("button", { name: "Guardar pergunta" }));

    expect(
      screen.getByText("Escreva o texto da pergunta em inglês."),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Indique pelo menos uma competência."),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Indique pelo menos um critério."),
    ).toBeInTheDocument();
    expect(fake.creates).toHaveLength(0);

    // Só espaços exteriores: continua a faltar texto, competência e critério.
    const text = screen.getByLabelText("Texto da pergunta (em inglês)");
    await user.type(text, "   ");
    await user.click(screen.getByRole("button", { name: "Guardar pergunta" }));

    expect(
      screen.getByText("Escreva o texto da pergunta em inglês."),
    ).toBeInTheDocument();
    expect(fake.creates).toHaveLength(0);
    // O texto do utilizador não é apagado pela validação.
    expect(text).toHaveValue("   ");
  });

  it("creates_a_question_with_competencies_and_criteria", async () => {
    const user = userEvent.setup();
    const fake = renderBank();
    await screen.findByText("2 perguntas nesta vista.");

    await user.click(screen.getByRole("button", { name: "Adicionar pergunta" }));

    await user.type(
      screen.getByLabelText("Texto da pergunta (em inglês)"),
      "Tell me about a time you improved a process that was causing problems.",
    );
    await user.selectOptions(
      screen.getByLabelText("Categoria"),
      "problem_solving",
    );
    await user.type(
      screen.getByLabelText("Competências (uma por linha)"),
      "Melhoria de processos",
    );
    await user.type(
      screen.getByLabelText("Critérios (uma por linha)"),
      "Distinguir o problema inicial, a mudança e o efeito observado.",
    );

    await user.click(screen.getByRole("button", { name: "Guardar pergunta" }));

    await waitFor(() => expect(fake.creates).toHaveLength(1));
    expect(fake.creates[0]).toEqual({
      text: "Tell me about a time you improved a process that was causing problems.",
      category: "problem_solving",
      competencies: ["Melhoria de processos"],
      criteria: [
        "Distinguir o problema inicial, a mudança e o efeito observado.",
      ],
    });
    expect(
      await screen.findByText("3 perguntas nesta vista."),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("heading", {
        name: "Tell me about a time you improved a process that was causing problems.",
      }),
    ).toBeInTheDocument();
  });

  it("keeps_the_editor_open_when_the_save_fails", async () => {
    const user = userEvent.setup();
    const fake = renderBank();
    await screen.findByText("2 perguntas nesta vista.");

    fake.client.updateQuestion = async () => {
      throw {
        code: "STALE_SOURCE",
        message: "A pergunta foi alterada entretanto. Recarregue o banco.",
        retryable: false,
      };
    };

    await user.click(
      screen.getByRole("button", { name: `Editar pergunta: ${FAILURE.text}` }),
    );
    const text = screen.getByLabelText("Texto da pergunta (em inglês)");
    await user.clear(text);
    await user.type(text, "Describe a time you learned from feedback.");
    await user.click(
      screen.getByRole("button", { name: "Guardar alterações" }),
    );

    expect(
      await screen.findByText(
        "A pergunta foi alterada entretanto. Recarregue o banco.",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByLabelText("Texto da pergunta (em inglês)"),
    ).toHaveValue("Describe a time you learned from feedback.");
  });
});

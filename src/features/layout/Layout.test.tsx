import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import App from "../../App";
import {
  fakeInterviewClient,
  makeQuestionDetail,
  sessionSummary,
  snapshot,
} from "../../test/fake-interview-client";
import { answeringPractice } from "../../test/practice-fixtures";

describe("navegação e recuperação do layout", () => {
  it("mantém recuperação de áudio na entrevista mesmo com navegação bloqueada", async () => {
    const user = userEvent.setup();
    const fake = fakeInterviewClient({
      initial: snapshot({
        activeSession: sessionSummary(),
        current: makeQuestionDetail(),
        recordingId: "capturing",
      }),
    });
    render(<App client={fake.client} />);
    await user.click(screen.getByRole("button", { name: "Entrevista" }));
    await screen.findByRole("heading", { name: "Entrevista" });
    await user.click(screen.getByRole("button", { name: "Preparação" }));
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Não é possível mudar de página",
    );
    await user.click(screen.getByText("Áudio e recuperação"));
    await user.click(
      screen.getByRole("button", { name: "Verificar disponibilidade" }),
    );
    await user.click(
      screen.getByRole("button", { name: "Reiniciar áudio local" }),
    );
    expect(fake.client.refreshAvailability).toHaveBeenCalledTimes(1);
    expect(fake.client.restartWorker).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("main")).toHaveAccessibleName("Sessão de prática");
  });

  it("foca o conteúdo depois de navegar, sem roubar foco no arranque", async () => {
    const user = userEvent.setup();
    render(<App client={fakeInterviewClient().client} />);
    await screen.findByRole("heading", { name: "Preparar entrevista" });
    expect(screen.getByRole("main")).not.toHaveFocus();
    await user.click(
      screen.getByRole("button", { name: "Banco de perguntas" }),
    );
    expect(screen.getByRole("main")).toHaveFocus();
  });

  it("identifica a Prática como página atual e mantém a recuperação sem perder a revisão", async () => {
    const user = userEvent.setup();
    const practice = answeringPractice();
    const fake = fakeInterviewClient({
      initial: snapshot({
        practice: {
          revision: 1,
          activePractice: practice,
          activeOwner: {
            type: "practice",
            practiceId: practice.id,
            answerId: practice.answer.id,
          },
          pendingPreparation: null,
        },
      }),
    });
    render(<App client={fake.client} />);
    await user.click(
      await screen.findByRole("button", { name: "Retomar prática pendente" }),
    );
    const editor = await screen.findByLabelText("Revisão editável");
    await user.type(editor, " My unsaved revision.");
    const nav = within(screen.getByRole("navigation"));
    expect(nav.getByRole("button", { name: "Prática" })).toHaveAttribute(
      "aria-current",
      "page",
    );
    expect(nav.getByRole("button", { name: "Histórico" })).not.toHaveAttribute(
      "aria-current",
    );
    await user.click(screen.getByText("Áudio e recuperação"));
    await user.click(
      screen.getByRole("button", { name: "Reiniciar áudio local" }),
    );
    await waitFor(() =>
      expect(fake.client.restartWorker).toHaveBeenCalledTimes(1),
    );
    expect(editor).toHaveValue("Practice raw transcript. My unsaved revision.");
  });
});

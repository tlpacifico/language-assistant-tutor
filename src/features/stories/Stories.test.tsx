import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import App from "../../App";
import type { CaptureOwner } from "../../lib/practice-contracts";
import {
  fakeInterviewClient,
  makeConfirmation,
  makePreparation,
  makeQuestionDetail,
  makeSessionQuestion,
  sessionSummary,
  makeStory,
  snapshot,
} from "../../test/fake-interview-client";
import { answeringPractice } from "../../test/practice-fixtures";
import StoryBank from "./StoryBank";
import StorySupport from "./StorySupport";

const MARKER = "ONLY_IN_STORY_742";
const OWNER: CaptureOwner = {
  type: "practice",
  practiceId: "practice-1",
  answerId: "answer-practice-1",
};

describe("StoryBank", () => {
  it("guarda uma história parcial com trim e sem enviar à API", async () => {
    const user = userEvent.setup();
    const fake = fakeInterviewClient();
    render(<StoryBank client={fake.client} />);

    await user.click(await screen.findByRole("button", { name: "Adicionar história" }));
    await user.type(screen.getByLabelText("Título"), "  Falha na migração  ");
    await user.type(screen.getByLabelText("Situação"), "  The migration failed.  ");
    await user.type(screen.getByLabelText(/Competências/), "  Falhas e aprendizagens  ");
    await user.click(screen.getByRole("button", { name: "Guardar história" }));

    await waitFor(() => expect(fake.client.createStory).toHaveBeenCalledTimes(1));
    expect(fake.client.createStory).toHaveBeenCalledWith(
      expect.objectContaining({
        input: {
          title: "Falha na migração",
          situation: "The migration failed.",
          task: "",
          actions: "",
          outcome: "",
          competencies: ["Falhas e aprendizagens"],
        },
      }),
    );
    // A história parcial mostra os campos em branco como rótulo, não como erro.
    expect(
      await screen.findByText(/Campos em branco: Tarefa, Ações, Resultado/),
    ).toBeInTheDocument();
    expect(fake.client.requestRewrite).not.toHaveBeenCalled();
    expect(fake.client.evaluatePractice).not.toHaveBeenCalled();
    expect(
      screen.queryByRole("button", { name: /Eliminar|Apagar|Remover história/ }),
    ).toBeNull();
  });

  it("recusa história sem nota ou sem competência", async () => {
    const user = userEvent.setup();
    const fake = fakeInterviewClient();
    render(<StoryBank client={fake.client} />);

    await user.click(await screen.findByRole("button", { name: "Adicionar história" }));
    await user.type(screen.getByLabelText("Título"), "Só título");
    await user.click(screen.getByRole("button", { name: "Guardar história" }));

    expect(await screen.findByText(/pelo menos uma nota/)).toBeInTheDocument();
    expect(screen.getByText("Indique pelo menos uma competência.")).toBeInTheDocument();
    expect(fake.client.createStory).not.toHaveBeenCalled();
  });

  it("filtra por competência e edita com a versão esperada", async () => {
    const user = userEvent.setup();
    const fake = fakeInterviewClient();
    fake.stories.push(
      makeStory({ id: "story-1", title: "Primeira", competencies: ["Liderança"] }),
      makeStory({ id: "story-2", title: "Segunda", competencies: ["Prazos"] }),
    );
    render(<StoryBank client={fake.client} />);

    expect(await screen.findByText("2 histórias nesta vista.")).toBeInTheDocument();
    await user.type(screen.getByLabelText("Filtrar por competência"), "liderança");
    expect(await screen.findByText("1 história nesta vista.")).toBeInTheDocument();
    expect(screen.queryByText("Segunda")).toBeNull();
    expect(fake.client.listStories).toHaveBeenLastCalledWith("liderança");

    await user.click(screen.getByRole("button", { name: /Editar história: Primeira/ }));
    await user.type(screen.getByLabelText("Título"), " 2");
    await user.click(screen.getByRole("button", { name: "Guardar alterações" }));

    await waitFor(() =>
      expect(fake.client.updateStory).toHaveBeenCalledWith(
        expect.objectContaining({ id: "story-1", expectedVersion: 1 }),
      ),
    );
  });

  it("pede a decisão antes de sair com a história por guardar", async () => {
    const user = userEvent.setup();
    const fake = fakeInterviewClient({ initial: snapshot({ revision: 1 }) });
    render(<App client={fake.client} />);

    await user.click(await screen.findByRole("button", { name: "Histórias" }));
    await user.click(await screen.findByRole("button", { name: "Adicionar história" }));
    await user.type(screen.getByLabelText("Título"), "Rascunho");
    await user.click(screen.getByRole("button", { name: "Banco de perguntas" }));

    expect(await screen.findByRole("button", { name: "Descartar e sair" })).toBeInTheDocument();
    const decision = screen.getByRole("button", { name: "Descartar e sair" })
      .parentElement as HTMLElement;
    await user.click(within(decision).getByRole("button", { name: "Cancelar" }));
    expect(screen.getByLabelText("Título")).toHaveValue("Rascunho");
  });
});

describe("StorySupport", () => {
  function renderSupport(
    fake: ReturnType<typeof fakeInterviewClient>,
    editable = true,
    snap = snapshot(),
  ) {
    render(
      <StorySupport
        client={fake.client}
        owner={OWNER}
        snapshot={snap}
        onSnapshot={() => undefined}
        editable={editable}
      />,
    );
  }

  it("escolhe, substitui e remove o apoio antes da captura com versões esperadas", async () => {
    const user = userEvent.setup();
    const fake = fakeInterviewClient();
    fake.stories.push(
      makeStory({ id: "story-1", title: "Primeira", version: 2 }),
      makeStory({ id: "story-2", title: "Segunda", version: 5 }),
    );
    renderSupport(fake);

    const select = await screen.findByLabelText("História de apoio");
    await waitFor(() => expect(within(select).getAllByRole("option")).toHaveLength(3));
    await user.selectOptions(select, "story-1");
    await waitFor(() =>
      expect(fake.client.selectStory).toHaveBeenLastCalledWith(
        expect.objectContaining({
          owner: OWNER,
          expectedPreparationVersion: 0,
          storyId: "story-1",
          expectedStoryVersion: 2,
        }),
      ),
    );

    await user.selectOptions(await screen.findByLabelText("História de apoio"), "story-2");
    await waitFor(() =>
      expect(fake.client.selectStory).toHaveBeenLastCalledWith(
        expect.objectContaining({
          expectedPreparationVersion: 1,
          storyId: "story-2",
          expectedStoryVersion: 5,
        }),
      ),
    );

    await user.click(await screen.findByRole("button", { name: "Remover apoio" }));
    await waitFor(() =>
      expect(fake.client.selectStory).toHaveBeenLastCalledWith(
        expect.objectContaining({
          expectedPreparationVersion: 2,
          storyId: null,
          expectedStoryVersion: null,
        }),
      ),
    );
    expect(fake.client.requestRewrite).not.toHaveBeenCalled();
  });

  it("fixa o snapshot depois da captura e ignora edições posteriores do banco", async () => {
    const fake = fakeInterviewClient();
    fake.preparations.set(
      OWNER.answerId,
      makePreparation({
        answerId: OWNER.answerId,
        version: 1,
        lockState: "locked",
        selectedStoryId: "story-1",
        selectedStoryVersion: 1,
        support: "story",
        snapshot: {
          storyId: "story-1",
          storyVersion: 1,
          content: {
            title: "Cópia fixada",
            situation: "Snapshot situation.",
            task: "",
            actions: "",
            outcome: "",
            competencies: ["Prazos"],
          },
        },
      }),
    );
    // O banco foi editado depois: a cópia fixada não muda.
    fake.stories.push(
      makeStory({ id: "story-1", title: "Título novo", situation: "Edited later.", version: 4 }),
    );
    renderSupport(fake, false);

    expect(await screen.findByText(/Apoio fixado: Cópia fixada \(versão 1\)/)).toBeInTheDocument();
    expect(screen.getByText("Snapshot situation.")).toBeInTheDocument();
    expect(screen.queryByText("Edited later.")).toBeNull();
    expect(screen.queryByLabelText("História de apoio")).toBeNull();
    expect(fake.client.selectStory).not.toHaveBeenCalled();
  });

  it("não permite escolher o apoio quando a interface não é editável", async () => {
    const fake = fakeInterviewClient();
    fake.stories.push(makeStory());
    renderSupport(fake, false);

    expect(await screen.findByText(/só pode ser alterado enquanto não há gravação/)).toBeInTheDocument();
    expect(screen.queryByLabelText("História de apoio")).toBeNull();
  });

  it("resolve explicitamente uma preparação com arranque incerto", async () => {
    const user = userEvent.setup();
    const fake = fakeInterviewClient();
    fake.preparations.set(
      OWNER.answerId,
      makePreparation({
        answerId: OWNER.answerId,
        lockState: "start_uncertain",
        lockedCaptureId: "rec-9",
      }),
    );
    renderSupport(fake, false);

    await user.click(
      await screen.findByRole("button", { name: "Manter este apoio e continuar" }),
    );
    expect(fake.client.resolvePreparation).toHaveBeenLastCalledWith(
      "rec-9",
      expect.any(String),
      "keep",
    );
    await user.click(
      screen.getByRole("button", { name: "O microfone não chegou a iniciar" }),
    );
    expect(fake.client.resolvePreparation).toHaveBeenLastCalledWith(
      "rec-9",
      expect.any(String),
      "did_not_start",
    );
  });
});

describe("apoio na entrevista", () => {
  function interviewSnapshot(withFollowUp: boolean) {
    const main = makeQuestionDetail({
      question: makeSessionQuestion({ id: "sq-2", selectedRecordingId: null }),
      initialAnswerId: "sq-2",
    });
    return snapshot({
      revision: 2,
      activeSession: sessionSummary({ version: 5, mode: "ai_v3" }),
      current: withFollowUp
        ? {
            ...main,
            confirmation: makeConfirmation({ sessionQuestionId: "sq-2" }),
            followUp: makeQuestionDetail({
              question: makeSessionQuestion({ id: "fu-1", selectedRecordingId: null }),
            }),
            followUpStage: "ready",
          }
        : main,
    });
  }

  it("oferece o apoio opcional antes da primeira gravação e usa o dono da entrevista", async () => {
    const user = userEvent.setup();
    const fake = fakeInterviewClient({ initial: interviewSnapshot(false) });
    fake.stories.push(makeStory({ id: "story-1", title: "Apoio A", version: 3 }));
    render(<App client={fake.client} />);

    await user.click(await screen.findByRole("button", { name: "Entrevista" }));
    expect(fake.client.loadPreparation).toHaveBeenCalledWith({
      type: "interview",
      sessionId: "session-1",
      sessionQuestionId: "sq-2",
      answerId: "sq-2",
    });
    const select = await screen.findByLabelText("História de apoio");
    await waitFor(() => expect(within(select).getAllByRole("option")).toHaveLength(2));
    await user.selectOptions(select, "story-1");

    await waitFor(() =>
      expect(fake.client.selectStory).toHaveBeenCalledWith(
        expect.objectContaining({
          owner: expect.objectContaining({ type: "interview", answerId: "sq-2" }),
          storyId: "story-1",
          expectedStoryVersion: 3,
        }),
      ),
    );
    // Escolher o apoio não grava, não confirma e não envia nada à API.
    expect(fake.client.startRecording).not.toHaveBeenCalled();
    expect(fake.client.requestFollowUp).not.toHaveBeenCalled();
  });

  it("no seguimento o apoio da pergunta principal é só de leitura", async () => {
    const user = userEvent.setup();
    const fake = fakeInterviewClient({ initial: interviewSnapshot(true) });
    fake.stories.push(makeStory());
    render(<App client={fake.client} />);

    await user.click(await screen.findByRole("button", { name: "Entrevista" }));

    expect(await screen.findByText("Apoio de história (opcional)")).toBeInTheDocument();
    expect(screen.queryByLabelText("História de apoio")).toBeNull();
  });
});

describe("apoio nunca preenche a resposta", () => {
  it(`o marcador ${MARKER} só existente na história não entra na transcrição nem na revisão`, async () => {
    const user = userEvent.setup();
    const practice = answeringPractice({
      preparation: makePreparation({
        answerId: "answer-practice-1",
        version: 1,
        lockState: "locked",
        support: "story",
        snapshot: {
          storyId: "story-1",
          storyVersion: 1,
          content: {
            title: "Com marcador",
            situation: MARKER,
            task: "",
            actions: "",
            outcome: "",
            competencies: ["Prazos"],
          },
        },
      }),
    });
    const fake = fakeInterviewClient({
      initial: snapshot({
        revision: 3,
        practice: {
          revision: 3,
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
    fake.practices.push(practice);
    fake.preparations.set(practice.answer.id, practice.preparation);
    render(<App client={fake.client} />);

    await user.click(
      await screen.findByRole("button", { name: "Retomar prática pendente" }),
    );
    const editor = await screen.findByLabelText("Revisão editável");
    // O apoio é mostrado só como consulta; a revisão e a transcrição não o têm.
    expect(await screen.findByText(MARKER)).toBeInTheDocument();
    expect((editor as HTMLTextAreaElement).value).not.toContain(MARKER);
    expect((screen.getByLabelText("Transcrição automática") as HTMLTextAreaElement).value).not.toContain(MARKER);
    await user.click(screen.getByRole("button", { name: "Guardar revisão" }));
    await waitFor(() => expect(fake.client.savePracticeReview).toHaveBeenCalled());
    expect(
      JSON.stringify(vi.mocked(fake.client.savePracticeReview).mock.calls),
    ).not.toContain(MARKER);
  });
});

import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import {
  fakeArticleClient,
  makeSnapshot,
  makeTurn,
} from "../../test/fake-article-client";
import ArticleConversation from "./ArticleConversation";

function setup(snapshot = makeSnapshot(), onBusyChange?: (busy: boolean) => void) {
  const fake = fakeArticleClient(snapshot);
  const view = render(
    <ArticleConversation
      client={fake.client}
      initial={snapshot}
      onBusyChange={onBusyChange}
    />,
  );
  return { fake, ...view };
}

const dialogue = [
  makeTurn(1, "tutor", "What is the main argument?"),
  makeTurn(2, "user", "Remote work helps people focus."),
  makeTurn(3, "tutor", "Good point.\n\nWhy does that matter?"),
];

describe("ArticleConversation", () => {
  it("mostra comentário e pergunta em sequência, sem editor nem correções entre turnos", async () => {
    setup(makeSnapshot({ turns: dialogue, session: { answers: 1 } }));
    const tutor = screen.getAllByTestId("turn-tutor")[1];
    const paragraphs = within(tutor).getAllByText(/./);
    expect(paragraphs.map((p) => p.textContent)).toEqual([
      "Good point.",
      "Why does that matter?",
    ]);
    // A resposta é texto, não um editor obrigatório.
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
    // Nenhuma correção entre turnos: só no final.
    expect(
      screen.queryByRole("button", { name: /Corrigir/ }),
    ).not.toBeInTheDocument();
  });

  it("grava e para por ação do utilizador, com um comando por clique", async () => {
    const user = userEvent.setup();
    const { fake } = setup();
    const record = screen.getByRole("button", { name: "Gravar resposta" });
    await user.dblClick(record);
    await waitFor(() =>
      expect(fake.client.startRecording).toHaveBeenCalled(),
    );
    expect(fake.client.startRecording).toHaveBeenCalledWith(
      expect.objectContaining({ sessionId: "session-1", expectedVersion: 3 }),
    );
    // Clique duplo: a mesma intenção reutiliza o requestId, por isso o núcleo
    // aplica-a uma só vez.
    const ids = new Set(
      fake.client.startRecording.mock.calls.map(
        (call) => (call as unknown as [{ requestId: string }])[0].requestId,
      ),
    );
    expect(ids.size).toBe(1);

    act(() => {
      fake.emit(makeSnapshot({ session: { state: "recording", version: 4 } }));
    });
    expect(
      screen.queryByRole("button", { name: "Gravar resposta" }),
    ).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Parar gravação" }));
    expect(fake.client.stopRecording).toHaveBeenCalledWith(
      expect.objectContaining({ expectedVersion: 4 }),
    );
  });

  it("sugere terminar à quinta resposta e bloqueia a gravação no limite", async () => {
    const { rerender, fake } = setup(
      makeSnapshot({
        session: { answers: 5, suggestFinish: true },
        turns: dialogue,
      }),
    );
    expect(screen.getByRole("status")).toHaveTextContent(
      "Já deu 5 respostas. Pode terminar para ver o feedback.",
    );
    expect(
      screen.getByRole("button", { name: "Gravar resposta" }),
    ).toBeEnabled();

    const limited = makeSnapshot({
      session: { answers: 20, suggestFinish: true, limitReached: true, version: 9 },
      turns: dialogue,
    });
    act(() => fake.emit(limited));
    rerender(
      <ArticleConversation client={fake.client} initial={limited} />,
    );
    expect(
      screen.getByRole("button", { name: "Gravar resposta" }),
    ).toBeDisabled();
    expect(screen.getByRole("status")).toHaveTextContent(
      /Limite da conversa atingido/,
    );
    expect(
      screen.getByRole("button", { name: "Terminar e ver feedback" }),
    ).toBeEnabled();
  });

  it("mostra o erro recuperável com foco e permite repetir", async () => {
    const user = userEvent.setup();
    const { fake } = setup(
      makeSnapshot({
        session: {
          state: "recoverable_error",
          error: {
            code: "AI_NETWORK",
            message: "Falha de rede; o custo pode ser desconhecido.",
            retryable: true,
          },
        },
      }),
    );
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("Falha de rede");
    await waitFor(() => expect(alert).toHaveFocus());
    await user.click(screen.getByRole("button", { name: "Repetir" }));
    expect(fake.client.retry).toHaveBeenCalledTimes(1);
  });

  it("mostra o erro do microfone indisponível sem entrar em gravação", async () => {
    const user = userEvent.setup();
    const { fake } = setup();
    fake.client.startRecording.mockRejectedValueOnce({
      code: "DEVICE_UNAVAILABLE",
      message: "Microfone indisponível.",
      retryable: true,
    });
    await user.click(screen.getByRole("button", { name: "Gravar resposta" }));
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Microfone indisponível.");
    expect(
      screen.queryByRole("button", { name: "Parar gravação" }),
    ).not.toBeInTheDocument();
  });

  it("para a voz do tutor sem perder o texto", async () => {
    const user = userEvent.setup();
    const { fake } = setup(
      makeSnapshot({
        turns: dialogue,
        audio: { playback: "playing", turnId: "turn-3" },
      }),
    );
    await user.click(screen.getByRole("button", { name: "Parar voz" }));
    expect(fake.client.stopVoice).toHaveBeenCalledTimes(1);
    expect(screen.getByText("Why does that matter?")).toBeInTheDocument();
  });

  it("oferece repetir só a voz quando falhou, mantendo o texto", async () => {
    const user = userEvent.setup();
    const { fake } = setup(
      makeSnapshot({
        turns: dialogue,
        audio: { playback: "failed", turnId: "turn-3" },
      }),
    );
    expect(screen.getByText("Why does that matter?")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Repetir voz" }));
    expect(fake.client.speakTurn).toHaveBeenCalledWith("session-1", "turn-3");
    // Nenhum comando que chame o fornecedor de texto.
    expect(fake.client.retry).not.toHaveBeenCalled();
    expect(fake.client.finish).not.toHaveBeenCalled();
  });

  it("ignora eventos duplicados e atrasados", async () => {
    const { fake } = setup(makeSnapshot());
    act(() => {
      fake.emit(
        makeSnapshot({
          session: { state: "transcribing", version: 5 },
          turns: dialogue,
        }),
      );
    });
    expect(screen.getByText("A transcrever a resposta…")).toBeInTheDocument();
    // Evento antigo (versão 4) chega depois: não volta atrás.
    act(() => {
      fake.emitLate(makeSnapshot({ session: { state: "recording", version: 4 } }));
    });
    expect(screen.getByText("A transcrever a resposta…")).toBeInTheDocument();
    // Evento duplicado: continua igual.
    act(() => {
      fake.emitLate(
        makeSnapshot({
          session: { state: "transcribing", version: 5 },
          turns: dialogue,
        }),
      );
    });
    expect(screen.getAllByText("A transcrever a resposta…")).toHaveLength(1);
    // Evento de outra conversa é ignorado.
    act(() => {
      fake.emitLate(
        makeSnapshot({ session: { id: "other", state: "ready", version: 99 } }),
      );
    });
    expect(screen.getByText("A transcrever a resposta…")).toBeInTheDocument();
  });

  it("revoga a autorização e pede-a de novo antes de continuar", async () => {
    const user = userEvent.setup();
    const { fake } = setup();
    fake.set(
      makeSnapshot({
        session: { state: "draft", authorization: null, version: 4 },
      }),
    );
    await user.click(
      screen.getByRole("button", { name: "Revogar autorização" }),
    );
    expect(fake.client.revoke).toHaveBeenCalledTimes(1);
    expect(
      await screen.findByRole("checkbox", {
        name: /autorizo o envio automático/i,
      }),
    ).not.toBeChecked();
    expect(
      screen.queryByRole("button", { name: "Gravar resposta" }),
    ).not.toBeInTheDocument();
    await user.click(
      screen.getByRole("checkbox", { name: /autorizo o envio automático/i }),
    );
    await user.click(screen.getByRole("button", { name: "Autorizar e continuar" }));
    expect(fake.client.authorize).toHaveBeenCalledWith(
      expect.objectContaining({ noticeVersion: "article-auto-v1" }),
    );
  });

  it("pede texto manual quando a transcrição falhou", async () => {
    const user = userEvent.setup();
    const { fake } = setup(
      makeSnapshot({
        session: { state: "needs_review" },
        turns: [
          makeTurn(1, "tutor", "What is the main argument?"),
          makeTurn(2, "user", "", { version: { id: "", source: null, text: "" } }),
        ],
      }),
    );
    const send = screen.getByRole("button", { name: "Enviar texto" });
    expect(send).toBeDisabled();
    await user.type(
      screen.getByLabelText("A sua resposta (a transcrição falhou)"),
      "I typed this.",
    );
    await user.click(send);
    expect(fake.client.submitText).toHaveBeenCalledWith(
      expect.objectContaining({ turnId: "turn-2", text: "I typed this." }),
    );
  });

  it("depois de concluída oferece correção com aviso de consumo e sem relatório inventado", async () => {
    const user = userEvent.setup();
    const { fake } = setup(
      makeSnapshot({
        session: { state: "completed", answers: 1 },
        turns: dialogue,
        report: null,
      }),
    );
    expect(
      screen.getByText(/Sem relatório: não há avaliação para mostrar/),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "Corrigir esta resposta" }),
    );
    expect(
      screen.getByText(/Regenerar o relatório pode consumir a API/),
    ).toBeInTheDocument();
    const area = screen.getByLabelText("Texto corrigido");
    await user.clear(area);
    await user.type(area, "Remote work helps people concentrate.");
    await user.click(screen.getByRole("button", { name: "Guardar correção" }));
    expect(fake.client.correct).toHaveBeenCalledWith(
      expect.objectContaining({
        turnId: "turn-2",
        text: "Remote work helps people concentrate.",
      }),
    );
  });

  it("comunica ocupado durante gravação e transcrição", async () => {
    const onBusy = vi.fn();
    const { fake } = setup(makeSnapshot(), onBusy);
    expect(onBusy).toHaveBeenLastCalledWith(false);
    act(() => {
      fake.emit(makeSnapshot({ session: { state: "recording", version: 4 } }));
    });
    expect(onBusy).toHaveBeenLastCalledWith(true);
    act(() => {
      fake.emit(makeSnapshot({ session: { state: "ready", version: 6 } }));
    });
    expect(onBusy).toHaveBeenLastCalledWith(false);
  });
});

import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import {
  fakeArticleClient,
  makeSnapshot,
  makeTurn,
} from "../../test/fake-article-client";
import Articles from "./Articles";

const ready = makeSnapshot({
  turns: [
    makeTurn(1, "tutor", "What is the main argument?"),
    makeTurn(2, "user", "Remote work helps people focus."),
  ],
  session: { answers: 1 },
});

describe("Articles", () => {
  it("lista as conversas e abrir não inicia envios, gravação nem som", async () => {
    const user = userEvent.setup();
    const fake = fakeArticleClient(ready);
    render(<Articles client={fake.client} />);
    expect(await screen.findByText("Remote work")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Abrir" }));
    expect(
      await screen.findByRole("button", { name: "Gravar resposta" }),
    ).toBeInTheDocument();
    for (const method of [
      "startRecording",
      "stopRecording",
      "submitText",
      "retry",
      "finish",
      "correct",
      "authorize",
      "playTurn",
      "stopVoice",
      "resume",
    ] as const) {
      expect(fake.client[method]).not.toHaveBeenCalled();
    }
  });

  it("retomar recupera o estado local sem iniciar nada", async () => {
    const user = userEvent.setup();
    const fake = fakeArticleClient(ready);
    render(<Articles client={fake.client} />);
    await user.click(await screen.findByRole("button", { name: "Retomar" }));
    await waitFor(() => expect(fake.client.resume).toHaveBeenCalledTimes(1));
    expect(fake.client.finish).not.toHaveBeenCalled();
    expect(fake.client.startRecording).not.toHaveBeenCalled();
    expect(fake.client.playTurn).not.toHaveBeenCalled();
  });

  it("pede confirmação antes de eliminar", async () => {
    const user = userEvent.setup();
    const fake = fakeArticleClient(ready);
    render(<Articles client={fake.client} />);
    await user.click(await screen.findByRole("button", { name: "Eliminar" }));
    expect(fake.client.delete).not.toHaveBeenCalled();
    await user.click(
      screen.getByRole("button", { name: "Confirmar eliminação" }),
    );
    await waitFor(() => expect(fake.client.delete).toHaveBeenCalledTimes(1));
  });

  it("comunica ocupado durante a gravação e liberta ao sair", async () => {
    const user = userEvent.setup();
    const fake = fakeArticleClient(
      makeSnapshot({ session: { state: "recording", version: 4 } }),
    );
    const onBusy = vi.fn();
    const { unmount } = render(
      <Articles client={fake.client} onBusyChange={onBusy} />,
    );
    await user.click(await screen.findByRole("button", { name: "Abrir" }));
    await waitFor(() => expect(onBusy).toHaveBeenLastCalledWith(true));
    // Não se volta à lista a meio de uma gravação.
    expect(
      screen.getByRole("button", { name: "Voltar à lista" }),
    ).toBeDisabled();
    unmount();
    expect(onBusy).toHaveBeenLastCalledWith(false);
  });

  it("mostra o erro de listagem com foco", async () => {
    const fake = fakeArticleClient();
    fake.client.list.mockRejectedValueOnce({
      code: "DATABASE_UNAVAILABLE",
      message: "A base de dados não está disponível.",
      retryable: true,
    });
    render(<Articles client={fake.client} />);
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("A base de dados não está disponível.");
    await waitFor(() => expect(alert).toHaveFocus());
  });
});

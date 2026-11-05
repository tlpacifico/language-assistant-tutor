import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import { fakeArticleClient, makeSnapshot } from "../../test/fake-article-client";
import ArticleSetup from "./ArticleSetup";

const CONSENT = /autorizo o envio automático/i;

describe("ArticleSetup", () => {
  it("não pré-seleciona a autorização e mostra o destino e o aviso", async () => {
    const fake = fakeArticleClient();
    render(<ArticleSetup client={fake.client} onStarted={() => undefined} />);
    const consent = await screen.findByRole("checkbox", { name: CONSENT });
    expect(consent).not.toBeChecked();
    expect(
      screen.getByText(/https:\/\/opencode\.ai\/inference\/go\/openai\/v1/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/O áudio permanece neste computador/),
    ).toBeInTheDocument();
  });

  it("só começa com artigo válido e autorização dada", async () => {
    const user = userEvent.setup();
    const fake = fakeArticleClient();
    const started: string[] = [];
    render(
      <ArticleSetup
        client={fake.client}
        onStarted={(s) => started.push(s.session.id)}
      />,
    );
    const start = screen.getByRole("button", { name: "Começar a conversa" });
    expect(start).toBeDisabled();

    await user.type(
      screen.getByLabelText("Texto do artigo"),
      "  Remote work helps.  ",
    );
    expect(start).toBeDisabled(); // falta a autorização
    await user.click(await screen.findByRole("checkbox", { name: CONSENT }));
    expect(start).toBeEnabled();
    await user.type(screen.getByLabelText(/Título/), "Remote work");
    await user.click(start);

    await waitFor(() => expect(fake.client.create).toHaveBeenCalledTimes(1));
    expect(fake.client.create).toHaveBeenCalledWith(
      expect.objectContaining({
        title: "Remote work",
        text: "  Remote work helps.  ",
      }),
    );
    await waitFor(() => expect(fake.client.authorize).toHaveBeenCalledTimes(1));
    expect(fake.client.authorize).toHaveBeenCalledWith(
      expect.objectContaining({
        destination: "https://opencode.ai/inference/go/openai/v1",
        noticeVersion: "article-auto-v1",
      }),
    );
    expect(started).toEqual([makeSnapshot().session.id]);
  });

  it("recusa um artigo acima do limite sem o cortar", async () => {
    const user = userEvent.setup();
    const fake = fakeArticleClient();
    render(<ArticleSetup client={fake.client} onStarted={() => undefined} />);
    const area = screen.getByLabelText("Texto do artigo");
    await user.click(area);
    await user.paste("a".repeat(12_001));
    expect(screen.getByRole("alert")).toHaveTextContent(
      /excede 12 000 caracteres/,
    );
    expect(screen.getByText("12 001 / 12 000")).toBeInTheDocument();
    await user.click(await screen.findByRole("checkbox", { name: CONSENT }));
    expect(
      screen.getByRole("button", { name: "Começar a conversa" }),
    ).toBeDisabled();
    expect((area as HTMLTextAreaElement).value).toHaveLength(12_001);
  });

  it("mostra o erro de criação, move o foco para ele e mantém o texto", async () => {
    const user = userEvent.setup();
    const fake = fakeArticleClient();
    fake.client.create.mockRejectedValueOnce({
      code: "ACTIVE_SESSION_EXISTS",
      message: "Já existe uma conversa sobre um artigo em curso.",
      retryable: false,
    });
    render(<ArticleSetup client={fake.client} onStarted={() => undefined} />);
    await user.type(screen.getByLabelText("Texto do artigo"), "Some article.");
    await user.click(await screen.findByRole("checkbox", { name: CONSENT }));
    await user.click(
      screen.getByRole("button", { name: "Começar a conversa" }),
    );
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Já existe uma conversa");
    expect(alert).toHaveFocus();
    expect(fake.client.authorize).not.toHaveBeenCalled();
    expect(screen.getByLabelText("Texto do artigo")).toHaveValue(
      "Some article.",
    );
  });
});

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import App from "../../App";
import {
  fakeInterviewClient,
  snapshot as interviewSnapshot,
} from "../../test/fake-interview-client";
import { fakeArticleClient, makeSnapshot } from "../../test/fake-article-client";

async function openArticle(state: "recording" | "ready") {
  const user = userEvent.setup();
  const articles = fakeArticleClient(
    makeSnapshot({ session: { state, version: 4 } }),
  );
  render(
    <App
      client={fakeInterviewClient({ initial: interviewSnapshot() }).client}
      articleClient={articles.client}
    />,
  );
  await user.click(await screen.findByRole("button", { name: "Artigos" }));
  await user.click(await screen.findByRole("button", { name: "Abrir" }));
  return { user, articles };
}

describe("navegação com a conversa sobre artigos", () => {
  it("o menu tem a página Artigos", async () => {
    render(
      <App
        client={fakeInterviewClient({ initial: interviewSnapshot() }).client}
        articleClient={fakeArticleClient().client}
      />,
    );
    expect(
      await screen.findByRole("button", { name: "Artigos" }),
    ).toBeInTheDocument();
  });

  it("não deixa mudar de página durante a gravação de um artigo", async () => {
    const { user } = await openArticle("recording");
    await screen.findByRole("button", { name: "Parar gravação" });
    await user.click(screen.getByRole("button", { name: "Histórico" }));
    expect(
      await screen.findByText(/Não é possível mudar de página/),
    ).toBeInTheDocument();
    // Continua na conversa, com o botão de parar acessível.
    expect(
      screen.getByRole("button", { name: "Parar gravação" }),
    ).toBeInTheDocument();
  });

  it("permite navegar quando a conversa não está ocupada", async () => {
    const { user } = await openArticle("ready");
    await screen.findByRole("button", { name: "Gravar resposta" });
    await user.click(screen.getByRole("button", { name: "Histórico" }));
    expect(
      screen.queryByText(/Não é possível mudar de página/),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Gravar resposta" }),
    ).not.toBeInTheDocument();
  });
});

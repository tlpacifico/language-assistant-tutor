import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import type { ResponseOrigin, RewriteView } from "../../lib/practice-contracts";
import {
  appError,
  emptyUsage,
  fakeInterviewClient,
  makeRewrite,
  snapshot,
} from "../../test/fake-interview-client";
import RewritePanel from "./RewritePanel";

const ORIGIN: ResponseOrigin = {
  type: "original",
  sessionId: "session-1",
  sessionQuestionId: "sq-1",
};

function setup(seed: RewriteView[] = []) {
  const fake = fakeInterviewClient();
  fake.rewrites.push(...seed);
  render(<RewritePanel client={fake.client} origin={ORIGIN} snapshot={snapshot()} />);
  return fake;
}

describe("RewritePanel", () => {
  it("começa em B1 e nunca pede sugestões sozinho", async () => {
    const fake = setup();

    expect(await screen.findByRole("radio", { name: "B1" })).toBeChecked();
    expect(screen.getByRole("radio", { name: "B2" })).not.toBeChecked();
    expect(screen.getByRole("radio", { name: "C1" })).not.toBeChecked();
    expect(screen.getByText(/não um diagnóstico CEFR/)).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Pedir sugestão B1" }),
    ).toBeEnabled();
    expect(fake.client.requestRewrite).not.toHaveBeenCalled();
  });

  it("pede a sugestão só por ação explícita e separa o conteúdo", async () => {
    const user = userEvent.setup();
    const fake = setup();

    await user.click(await screen.findByRole("button", { name: "Pedir sugestão B1" }));

    expect(fake.client.requestRewrite).toHaveBeenCalledTimes(1);
    expect(fake.client.requestRewrite).toHaveBeenCalledWith(
      expect.objectContaining({ origin: ORIGIN, level: "B1" }),
    );
    expect(
      await screen.findByText("Sugestão da IA — confirme os factos"),
    ).toBeInTheDocument();
    const text = screen.getByLabelText("Texto sugerido");
    expect(within(text).getByText("Suggested B1 sentence one.")).toBeInTheDocument();
    // As explicações e as lacunas ficam fora do texto sugerido (e da voz).
    expect(within(text).queryByText(/Corrige a concordância/)).toBeNull();
    expect(
      within(screen.getByLabelText("Alterações necessárias")).getByText(
        "Corrige a concordância do verbo.",
      ),
    ).toBeInTheDocument();
    expect(
      within(screen.getByLabelText("Alternativas opcionais de estilo")).getByText(
        "Alternativa de estilo opcional.",
      ),
    ).toBeInTheDocument();
    const gaps = screen.getByLabelText("Informação em falta");
    expect(within(gaps).getByText("What was the measurable result?")).toBeInTheDocument();
    expect(within(text).queryByText(/measurable result/)).toBeNull();
  });

  it("ouve apenas a sugestão pelo identificador da reformulação", async () => {
    const user = userEvent.setup();
    const fake = setup([makeRewrite({ origin: ORIGIN, level: "B1" })]);

    await user.click(await screen.findByRole("button", { name: "Ouvir só a sugestão" }));

    expect(fake.client.playAudio).toHaveBeenCalledWith({
      type: "rewrite",
      rewriteId: "rewrite-B1",
    });
    expect(fake.client.requestRewrite).not.toHaveBeenCalled();
  });

  it("alterna entre variantes guardadas sem pedir à API e guarda a preferência local", async () => {
    const user = userEvent.setup();
    const fake = setup([
      makeRewrite({ origin: ORIGIN, level: "B1" }),
      makeRewrite({ origin: ORIGIN, level: "B2" }),
    ]);

    expect(await screen.findByText("Suggested B1 sentence one.")).toBeInTheDocument();
    await user.click(screen.getByRole("radio", { name: /B2/ }));

    expect(screen.getByText("Suggested B2 sentence one.")).toBeInTheDocument();
    expect(screen.queryByText("Suggested B1 sentence one.")).toBeNull();
    expect(fake.client.setRewriteLevel).toHaveBeenCalledWith(
      expect.objectContaining({ level: "B2", expectedVersion: 1 }),
    );
    expect(fake.client.requestRewrite).not.toHaveBeenCalled();
  });

  it("uma resposta para outro nível não troca a seleção do utilizador", async () => {
    const user = userEvent.setup();
    const fake = fakeInterviewClient();
    let release: (view: RewriteView) => void = () => undefined;
    fake.setRewriteResponder(
      () =>
        new Promise<RewriteView>((resolve) => {
          release = resolve;
        }),
    );
    render(<RewritePanel client={fake.client} origin={ORIGIN} snapshot={snapshot()} />);

    await user.click(await screen.findByRole("button", { name: "Pedir sugestão B1" }));
    expect(await screen.findByText("A aguardar o serviço externo…")).toBeInTheDocument();
    // O utilizador muda para B2 enquanto a chamada decorre.
    await user.click(screen.getByRole("radio", { name: /B2/ }));
    expect(screen.getByRole("radio", { name: /B2/ })).toBeChecked();

    await act(async () => {
      release(makeRewrite({ origin: ORIGIN, level: "B1" }));
    });

    // Continua em B2, sem sugestão B2; a variante B1 ficou guardada.
    expect(screen.getByRole("radio", { name: /B2/ })).toBeChecked();
    expect(screen.getByText(/Ainda não existe uma sugestão B2/)).toBeInTheDocument();
    expect(screen.queryByText("Suggested B1 sentence one.")).toBeNull();
    await user.click(screen.getByRole("radio", { name: /B1/ }));
    expect(screen.getByText("Suggested B1 sentence one.")).toBeInTheDocument();
  });

  it("um resultado que chega para outra origem não aparece na nova origem", async () => {
    const user = userEvent.setup();
    const fake = fakeInterviewClient();
    let release: (view: RewriteView) => void = () => undefined;
    fake.setRewriteResponder(
      () =>
        new Promise<RewriteView>((resolve) => {
          release = resolve;
        }),
    );
    const other: ResponseOrigin = { type: "practice", practiceId: "practice-2" };
    const view = render(
      <RewritePanel client={fake.client} origin={ORIGIN} snapshot={snapshot()} />,
    );

    await user.click(await screen.findByRole("button", { name: "Pedir sugestão B1" }));
    // O utilizador abre outra prática enquanto a chamada decorre.
    view.rerender(
      <RewritePanel client={fake.client} origin={other} snapshot={snapshot()} />,
    );
    await act(async () => {
      release(makeRewrite({ origin: ORIGIN, level: "B1" }));
    });

    expect(screen.queryByText("Suggested B1 sentence one.")).toBeNull();
    expect(screen.getByText(/Ainda não existe uma sugestão B1/)).toBeInTheDocument();
    // A variante ficou guardada na origem que a pediu.
    expect(await fake.client.listRewrites(ORIGIN)).toHaveLength(1);
    expect(await fake.client.listRewrites(other)).toHaveLength(0);
  });

  it("uma falha permite repetir com novo identificador de pedido", async () => {
    const user = userEvent.setup();
    const fake = fakeInterviewClient();
    const ids: string[] = [];
    let attempt = 0;
    fake.setRewriteResponder(async (request) => {
      ids.push(request.requestId);
      attempt += 1;
      if (attempt === 1) {
        throw appError("AI_REQUEST_FAILED", "Serviço indisponível.", true);
      }
      return makeRewrite({ origin: ORIGIN, level: "B1" });
    });
    render(<RewritePanel client={fake.client} origin={ORIGIN} snapshot={snapshot()} />);

    await user.click(await screen.findByRole("button", { name: "Pedir sugestão B1" }));
    expect(await screen.findByText(/Serviço indisponível/)).toBeInTheDocument();
    expect(screen.getByText(/pode gerar novo custo/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Repetir pedido de sugestão B1" }));

    expect(await screen.findByText("Suggested B1 sentence one.")).toBeInTheDocument();
    expect(ids).toHaveLength(2);
    expect(ids[0]).not.toEqual(ids[1]);
  });

  it("guarda um resultado recebido sem novo pedido à rede", async () => {
    const user = userEvent.setup();
    const fake = fakeInterviewClient();
    render(
      <RewritePanel
        client={fake.client}
        origin={ORIGIN}
        snapshot={snapshot({ pendingAiPublication: "attempt-3" })}
      />,
    );

    await user.click(
      await screen.findByRole("button", {
        name: "Guardar resultado recebido sem novo pedido",
      }),
    );

    expect(fake.client.retryAiPublication).toHaveBeenCalledWith("attempt-3");
    expect(fake.client.requestRewrite).not.toHaveBeenCalled();
  });

  it("rotula o consumo desconhecido em vez de o mostrar como zero", async () => {
    const user = userEvent.setup();
    setup([
      makeRewrite({
        origin: ORIGIN,
        level: "B1",
        usage: emptyUsage({ input: null, output: null, complete: false, unknownAttemptCount: 1 }),
      }),
    ]);

    await user.click(await screen.findByText("Consumo desta sugestão"));

    await waitFor(() =>
      expect(screen.getByText(/Tokens de entrada: desconhecidos/)).toBeInTheDocument(),
    );
    expect(screen.getByText(/Valores parciais/)).toBeInTheDocument();
  });
  it("relê as variantes quando o trabalho de IA termina e mostra a sugestão", async () => {
    const user = userEvent.setup();
    const fake = fakeInterviewClient();
    fake.setRewriteResponder(async () =>
      makeRewrite({ id: null, state: "running", segments: [] }),
    );
    const busy = snapshot({
      revision: 2,
      aiJob: {
        attemptId: "a1",
        purpose: "rewrite",
        sessionId: "session-1",
        sessionQuestionId: "sq-1",
        stage: "running",
      },
    });
    const { rerender } = render(
      <RewritePanel client={fake.client} origin={ORIGIN} snapshot={snapshot({ revision: 1 })} />,
    );
    await user.click(await screen.findByRole("button", { name: "Pedir sugestão B1" }));
    rerender(<RewritePanel client={fake.client} origin={ORIGIN} snapshot={busy} />);
    expect(screen.queryByText("Sugestão da IA — confirme os factos")).toBeNull();

    // O Rust publica o resultado e limpa o trabalho.
    fake.rewrites.push(makeRewrite());
    rerender(
      <RewritePanel client={fake.client} origin={ORIGIN} snapshot={snapshot({ revision: 3 })} />,
    );
    expect(
      await screen.findByText("Sugestão da IA — confirme os factos"),
    ).toBeInTheDocument();
  });

  it("permite repetir depois de uma falha publicada pelo núcleo", async () => {
    const user = userEvent.setup();
    const fake = fakeInterviewClient();
    fake.setRewriteResponder(async () =>
      makeRewrite({ id: null, state: "running", segments: [] }),
    );
    const { rerender } = render(
      <RewritePanel client={fake.client} origin={ORIGIN} snapshot={snapshot({ revision: 1 })} />,
    );
    await user.click(await screen.findByRole("button", { name: "Pedir sugestão B1" }));
    expect(screen.getByRole("button", { name: "Pedir sugestão B1" })).toBeDisabled();

    fake.rewrites.push(makeRewrite({ id: "rewrite-failed", state: "failed", segments: [] }));
    rerender(
      <RewritePanel client={fake.client} origin={ORIGIN} snapshot={snapshot({ revision: 2 })} />,
    );
    expect(
      await screen.findByRole("button", { name: "Repetir pedido de sugestão B1" }),
    ).toBeEnabled();
  });

  it("mostra as citações e o papel de cada segmento", async () => {
    const fake = fakeInterviewClient();
    fake.rewrites.push(makeRewrite());
    render(<RewritePanel client={fake.client} origin={ORIGIN} snapshot={snapshot()} />);

    const origins = await screen.findByLabelText("Origem dos segmentos");
    expect(within(origins).getAllByText(/Resposta inicial/).length).toBeGreaterThan(0);
    expect(within(origins).getByText("I solved a difficult problem")).toBeInTheDocument();
    // A zona do texto sugerido (e da voz) continua sem citações.
    expect(
      within(screen.getByLabelText("Texto sugerido")).queryByText("I solved a difficult problem"),
    ).toBeNull();
  });
});

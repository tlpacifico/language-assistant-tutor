import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect, it, vi } from "vitest";

import { fakeArticleClient, makeSnapshot, makeTurn } from "../../test/fake-article-client";
import ArticleConversation from "./ArticleConversation";

const first = makeTurn(1, "tutor", "What is the main argument?");
const second = makeTurn(2, "tutor", "Why does it matter?");

function openConversation() {
  const initial = makeSnapshot({ turns: [first] });
  const fake = fakeArticleClient(initial);
  render(<ArticleConversation client={fake.client} initial={initial} />);
  return fake;
}

it("preserves reading position until the user jumps to the latest turn", async () => {
  const fake = openConversation();
  const chat = screen.getByRole("region", { name: "Mensagens da conversa" });
  Object.defineProperties(chat, {
    clientHeight: { value: 200 },
    scrollHeight: { value: 600 },
  });
  const scrollTo = vi.fn(({ top }: ScrollToOptions) => {
    chat.scrollTop = top ?? 0;
  });
  Object.defineProperty(chat, "scrollTo", { value: scrollTo });
  chat.scrollTop = 100;
  fireEvent.scroll(chat);
  act(() => fake.emit(makeSnapshot({ turns: [first, second], session: { version: 4 } })));
  expect(chat.scrollTop).toBe(100);
  expect(scrollTo).not.toHaveBeenCalled();
  await userEvent.click(screen.getByRole("button", { name: /Nova resposta/ }));
  expect(chat.scrollTop).toBe(400);
  expect(screen.queryByRole("button", { name: /Nova resposta/ })).toBeNull();
  act(() => fake.emit(makeSnapshot({
    turns: [first, second, makeTurn(3, "tutor", "Could you give an example?")],
    session: { version: 5 },
  })));
  expect(scrollTo).toHaveBeenCalledTimes(2);
});

it("focuses the heading silently on opening and offers a jump to the latest question", async () => {
  const fake = openConversation();
  expect(screen.getByRole("heading", { level: 2 })).toHaveFocus();
  await userEvent.click(screen.getByRole("link", { name: "Ir para a última pergunta" }));
  expect(screen.getByTestId("turn-tutor")).toHaveFocus();
  expect(fake.client.play).not.toHaveBeenCalled();
});

it("keeps focus on the article toggle when turns and recorded errors arrive", async () => {
  const fake = openConversation();
  const toggle = screen.getByRole("button", { name: "Ver o artigo" });
  await userEvent.click(toggle);
  const chat = screen.getByRole("region", { name: "Mensagens da conversa" });
  chat.scrollTop = 100;
  await userEvent.click(toggle);
  expect(chat.scrollTop).toBe(100);
  act(() => fake.emit(makeSnapshot({ turns: [first, second], session: { version: 4 } })));
  act(() => fake.emit(makeSnapshot({
    turns: [first, second],
    session: { version: 5, state: "recoverable_error", recovery: "retry_send",
      error: { code: "AI_NETWORK", message: "Falha de rede.", retryable: true } },
  })));
  expect(toggle).toHaveFocus();
  expect(screen.getByRole("alert")).toHaveTextContent("Falha de rede.");
});

it("keeps recording controls available with the article open and preserves action focus", async () => {
  const fake = openConversation();
  await userEvent.click(screen.getByRole("button", { name: "Ver o artigo" }));
  expect(screen.getByRole("region", { name: "Artigo" })).toBeVisible();
  const bar = screen.getByRole("region", { name: "Controlos da conversa" });
  await userEvent.click(within(bar).getByRole("button", { name: "Gravar resposta" }));
  act(() => fake.emit(makeSnapshot({ turns: [first], session: { version: 4, state: "recording" } })));
  expect(within(bar).getByRole("button", { name: "Parar e enviar" })).toHaveFocus();
  act(() => fake.emit(makeSnapshot({ turns: [first], session: { version: 5, state: "awaiting_tutor" } })));
  expect(within(bar).getByText("O tutor está a preparar a resposta…", { selector: "p" })).toHaveFocus();
  act(() => fake.emit(makeSnapshot({ turns: [first, second], session: { version: 6 } })));
  expect(within(bar).getByRole("button", { name: "Gravar resposta" })).toHaveFocus();
});

it("announces playback changes without progress chatter", async () => {
  const fake = openConversation();
  await waitFor(() => expect(fake.client.focusView).toHaveBeenCalled());
  const log = screen.getByRole("log");
  act(() => fake.emitPlayer(makeSnapshot({ audio: {
    turnId: first.id, source: "tutor_voice", state: "playing", generation: 1, sequence: 1,
  } }).audio));
  expect(log).toHaveTextContent("A reproduzir áudio.");
  const announcement = log.firstChild;
  act(() => fake.emitPlayer(makeSnapshot({ audio: {
    turnId: first.id, source: "tutor_voice", state: "playing", generation: 1, sequence: 2, positionMs: 2000,
  } }).audio));
  expect(log.firstChild).toBe(announcement);
  expect(log).toHaveTextContent(/^A reproduzir áudio\.$/);
});

it("recovers a tutor reply saved before the conversation subscriptions were installed", async () => {
  const initial = makeSnapshot({ turns: [], session: { state: "awaiting_tutor" } });
  const fake = fakeArticleClient(initial);
  fake.set(makeSnapshot({ turns: [first], session: { state: "ready", version: 4 } }));
  render(<ArticleConversation client={fake.client} initial={initial} />);
  expect(await screen.findByText(first.version.text)).toBeInTheDocument();
  expect(fake.client.play).not.toHaveBeenCalled();
});

it("keeps replay disabled until the conversation holds a live focus token", async () => {
  const initial = makeSnapshot({ turns: [first] });
  const fake = fakeArticleClient(initial);
  let completeSnapshot!: (snapshot: typeof initial) => void;
  fake.client.snapshot.mockReturnValue(new Promise((resolve) => { completeSnapshot = resolve; }));
  const { unmount } = render(<ArticleConversation client={fake.client} initial={initial} />);
  const play = screen.getByRole("button", { name: "Ouvir" });
  expect(play).toBeDisabled();
  await userEvent.click(play);
  expect(fake.client.play).not.toHaveBeenCalled();
  unmount();
  await act(async () => completeSnapshot(initial));
  expect(fake.client.focusView).not.toHaveBeenCalled();
});

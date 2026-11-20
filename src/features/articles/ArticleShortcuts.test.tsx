import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, it } from "vitest";

import { fakeArticleClient, makeSnapshot, makeTurn } from "../../test/fake-article-client";
import ArticleConversation from "./ArticleConversation";

it("ignores shortcuts in editors, dialogs, repeated events and IME composition", async () => {
  const initial = makeSnapshot();
  const fake = fakeArticleClient(initial);
  const { unmount } = render(<>
    <ArticleConversation client={fake.client} initial={initial} />
    <textarea aria-label="Editor" />
    <div aria-modal="true" role="dialog"><button>Fechar</button></div>
    <div contentEditable tabIndex={0}>Texto</div>
  </>);
  await waitFor(() => expect(fake.client.focusView).toHaveBeenCalled());
  const shortcut = { key: "Enter", ctrlKey: true };
  fireEvent.keyDown(screen.getByRole("textbox"), shortcut);
  fireEvent.keyDown(screen.getByRole("button", { name: "Fechar" }), shortcut);
  fireEvent.keyDown(screen.getByText("Texto"), shortcut);
  fireEvent.keyDown(document, { ...shortcut, repeat: true });
  fireEvent.keyDown(document, { ...shortcut, isComposing: true });
  expect(fake.client.startRecording).not.toHaveBeenCalled();
  await act(async () => { fireEvent.keyDown(document, shortcut); });
  expect(fake.client.startRecording).toHaveBeenCalledTimes(1);
  act(() => fake.emit(makeSnapshot({ session: { state: "recording", version: 4 } })));
  await act(async () => { fireEvent.keyDown(document, shortcut); });
  expect(fake.client.stopRecording).toHaveBeenCalledTimes(1);
  unmount();
  fireEvent.keyDown(document, shortcut);
  expect(fake.client.startRecording).toHaveBeenCalledTimes(1);
});

it("Alt+P scopes pause and resume to the current selection", async () => {
  const turn = makeTurn(1, "tutor", "What is the main argument?");
  const initial = makeSnapshot({ turns: [turn] });
  const fake = fakeArticleClient(initial);
  render(<ArticleConversation client={fake.client} initial={initial} />);
  await waitFor(() => expect(fake.client.focusView).toHaveBeenCalled());
  const shortcut = { key: "p", altKey: true };
  fireEvent.keyDown(document, shortcut);
  expect(fake.client.pause).not.toHaveBeenCalled();
  const selection = { turnId: turn.id, source: "tutor_voice" as const, generation: 3 };
  act(() => fake.emitPlayer(makeSnapshot({ audio: { ...selection, state: "playing", sequence: 1 } }).audio));
  await act(async () => { fireEvent.keyDown(document, shortcut); });
  expect(fake.client.pause).toHaveBeenCalledWith(initial.session.id, 3);
  act(() => fake.emitPlayer(makeSnapshot({ audio: { ...selection, state: "paused", sequence: 3 } }).audio));
  await act(async () => { fireEvent.keyDown(document, shortcut); });
  expect(fake.client.resume).toHaveBeenCalledWith(initial.session.id, 3);
});

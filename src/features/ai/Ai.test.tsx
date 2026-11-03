import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import AiSettings from "./AiSettings";
import AiStep from "./AiStep";
import FeedbackPanel from "./FeedbackPanel";
import {
  fakeInterviewClient,
  makeQuestionDetail,
  makeConfirmation,
  sessionSummary,
  snapshot,
} from "../../test/fake-interview-client";
import type { FeedbackView } from "../../lib/ai-contracts";
describe("Tutor IA", () => {
  it("limpa chave em falha e apresenta destino antes de autorizar", async () => {
    const f = fakeInterviewClient();
    vi.mocked(f.client.saveAiKey).mockRejectedValue({
      code: "AI_CREDENTIAL",
      message: "Falhou guardar",
      retryable: true,
    });
    render(<AiSettings client={f.client} />);
    const field = await screen.findByLabelText("Chave da API");
    fireEvent.change(field, { target: { value: "SECRET" } });
    fireEvent.click(screen.getByText("Guardar chave protegida"));
    await screen.findByText("Falhou guardar");
    expect(field).toHaveValue("");
    expect(
      screen.getByText("https://opencode.ai/inference/openai/v1"),
    ).toBeVisible();
  });
  it("pedido segue ação explícita e dispensa não espera pelo HTTP", async () => {
    const f = fakeInterviewClient();
    const s = snapshot({
      activeSession: sessionSummary({ mode: "ai_v3" }),
      current: makeQuestionDetail({
        confirmation: makeConfirmation(),
        followUpStage: "running",
      }),
    });
    vi.mocked(f.client.waiveFollowUp).mockResolvedValue(s);
    const changed = vi.fn();
    render(<AiStep client={f.client} snapshot={s} onSnapshot={changed} />);
    expect(f.client.requestFollowUp).not.toHaveBeenCalled();
    fireEvent.click(screen.getByText("Dispensar seguimento explicitamente"));
    await waitFor(() => expect(changed).toHaveBeenCalled());
    expect(f.client.waiveFollowUp).toHaveBeenCalledWith(
      expect.objectContaining({ reason: "user_cancelled_wait" }),
    );
  });
  it("resultado de feedback pendente pode ser guardado sem novo pedido", async () => {
    const f = fakeInterviewClient({
      initial: snapshot({ pendingAiPublication: "result-id" }),
    });
    const v: FeedbackView = {
      sessionId: "s",
      state: "partial",
      reports: [],
      priorities: [],
      usage: {
        knownReportedSubtotal: 0.01,
        knownEstimatedSubtotal: 0,
        unknownAttemptCount: 1,
        input: null,
        output: null,
        complete: false,
      },
      attempts: [],
    };
    vi.mocked(f.client.loadFeedback).mockResolvedValue(v);
    vi.mocked(f.client.retryAiPublication).mockResolvedValue(snapshot());
    render(<FeedbackPanel client={f.client} sessionId="s" />);
    fireEvent.click(
      await screen.findByText("Guardar resultado recebido sem novo pedido"),
    );
    await waitFor(() =>
      expect(f.client.retryAiPublication).toHaveBeenCalledWith("result-id"),
    );
    expect(f.client.generateFeedback).not.toHaveBeenCalled();
    expect(screen.getByText(/Valores parciais/)).toBeVisible();
    expect(screen.getByText(/Tokens de entrada: desconhecidos/)).toBeVisible();
  });
});

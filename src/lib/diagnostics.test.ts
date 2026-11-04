import { afterEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { diagnosticInvoke, installGlobalDiagnostics } from "./diagnostics";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
afterEach(() => vi.resetAllMocks());

describe("diagnóstico da fronteira IPC", () => {
  it("conserva a rejeição e regista a operação sem argumentos nem mensagem", async () => {
    const secret = { code: "AI_TIMEOUT", message: "private answer and key", retryable: true };
    vi.mocked(invoke).mockRejectedValueOnce(secret).mockResolvedValue(undefined);
    await expect(diagnosticInvoke("interview_ai_save_ai_key", { key: "secret-key" })).rejects.toBe(secret);
    const call = vi.mocked(invoke).mock.calls.find(([name]) => name === "diagnostics_record");
    expect(call).toBeDefined();
    expect(call?.[1]).toMatchObject({ event: { operation: "interview_ai_save_ai_key", code: "AI_TIMEOUT", component: "frontend" } });
    expect(JSON.stringify(call)).not.toMatch(/secret-key|private answer/);
  });

  it("não deixa uma falha no diagnóstico substituir a falha original", async () => {
    vi.mocked(invoke).mockRejectedValue("offline");
    await expect(diagnosticInvoke("interview_snapshot")).rejects.toBe("offline");
    await Promise.resolve();
  });

  it("captura erros globais sem transmitir o texto ou a stack", async () => {
    vi.mocked(invoke).mockResolvedValue(undefined);
    const remove = installGlobalDiagnostics();
    window.dispatchEvent(new ErrorEvent("error", { message: "private answer", error: new Error("secret-key") }));
    const call = vi.mocked(invoke).mock.calls.at(-1);
    expect(call?.[1]).toMatchObject({ event: { operation: "window_error", code: "UNEXPECTED" } });
    expect(JSON.stringify(call)).not.toMatch(/private answer|secret-key/);
    remove();
    const count = vi.mocked(invoke).mock.calls.length;
    window.dispatchEvent(new ErrorEvent("error"));
    expect(vi.mocked(invoke).mock.calls).toHaveLength(count);
  });
});

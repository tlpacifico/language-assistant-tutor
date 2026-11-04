import { invoke } from "@tauri-apps/api/core";

export interface DiagnosticStatus {
  build: { version: string; revision: string; dirty: boolean; builtAt: string };
  writable: boolean;
  dropped: number;
  directory: string;
  defaultDirectory: string;
}

/** Sem mensagens, argumentos ou stacks: o núcleo aplica ainda listas fechadas. */
export function reportDiagnostic(operation: string, code = "UNEXPECTED", durationMs?: number): void {
  try {
    void invoke("diagnostics_record", {
      event: { component: "frontend", operation, code, durationMs: durationMs == null ? null : Math.round(durationMs), file: null, line: null },
    }).catch(() => undefined);
  } catch { /* O diagnóstico nunca substitui a falha original. */ }
}

export async function diagnosticInvoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const start = performance.now();
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    const code = typeof error === "object" && error !== null && "code" in error && typeof error.code === "string" ? error.code : "UNKNOWN";
    reportDiagnostic(command, code, performance.now() - start);
    throw error;
  }
}

export function installGlobalDiagnostics(): () => void {
  const onError = () => reportDiagnostic("window_error");
  const onRejection = () => reportDiagnostic("unhandled_rejection");
  window.addEventListener("error", onError);
  window.addEventListener("unhandledrejection", onRejection);
  return () => {
    window.removeEventListener("error", onError);
    window.removeEventListener("unhandledrejection", onRejection);
  };
}

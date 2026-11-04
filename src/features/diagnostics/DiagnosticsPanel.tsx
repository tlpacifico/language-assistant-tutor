import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { DiagnosticStatus } from "../../lib/diagnostics";

export default function DiagnosticsPanel() {
  const [status, setStatus] = useState<DiagnosticStatus | null>(null);
  const [directory, setDirectory] = useState("");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");

  async function refresh() {
    setBusy(true);
    try {
      const next = await invoke<DiagnosticStatus>("diagnostics_status");
      setStatus(next);
      setDirectory((current) => current || next.defaultDirectory);
      setMessage("");
    } catch { setMessage("Não foi possível consultar o diagnóstico local."); }
    finally { setBusy(false); }
  }

  async function exportLogs() {
    setBusy(true);
    setMessage("");
    try {
      const path = await invoke<string>("diagnostics_export", { directory: directory.trim() || null });
      setMessage(`Diagnóstico guardado em ${path}`);
    } catch { setMessage("Não foi possível exportar. Verifique se a pasta existe e permite escrita."); }
    finally { setBusy(false); }
  }

  return (
    <details className="tutor-audio-recovery" onToggle={(event) => { if (event.currentTarget.open) void refresh(); }}>
      <summary>Diagnóstico local</summary>
      <p className="practice__hint">Os registos ficam neste computador e são preservados entre atualizações, até 30 dias e 5 MiB. A exportação contém dados técnicos, sem gravações, respostas ou chaves.</p>
      {status && <>
        <p>Versão {status.build.version} · {status.build.revision === "unknown" ? "commit desconhecido" : status.build.revision.slice(0, 7)}{status.build.dirty ? " · com alterações locais" : ""}</p>
        <p className="practice__hint">Compilação: {status.build.builtAt === "unknown" ? "desconhecida" : new Date(Number(status.build.builtAt) * 1000).toLocaleString("pt-PT")}</p>
        <p>{status.writable ? "Registo disponível" : "Não foi possível escrever os registos"} · {status.dropped} eventos não guardados nesta execução.</p>
        <p className="practice__hint" style={{ overflowWrap: "anywhere" }}>Registos: {status.directory}</p>
        <label>Pasta de destino
          <input value={directory} onChange={(event) => setDirectory(event.target.value)} disabled={busy} placeholder="C:\\…" />
        </label>
      </>}
      <div className="practice__actions">
        <button type="button" onClick={() => void refresh()} disabled={busy}>Atualizar diagnóstico</button>
        <button type="button" onClick={() => void exportLogs()} disabled={busy || !status}>Exportar diagnóstico</button>
      </div>
      {message && <p role="status" style={{ overflowWrap: "anywhere" }}>{message}</p>}
    </details>
  );
}

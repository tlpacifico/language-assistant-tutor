import { Component } from "react";
import type { ReactNode } from "react";
import { reportDiagnostic } from "../../lib/diagnostics";
import DiagnosticsPanel from "./DiagnosticsPanel";

export default class DiagnosticsBoundary extends Component<{ children: ReactNode }, { failed: boolean }> {
  state = { failed: false };
  static getDerivedStateFromError() { return { failed: true }; }
  componentDidCatch() { reportDiagnostic("render"); }
  render() {
    if (!this.state.failed) return this.props.children;
    return <main className="practice tutor-content">
      <div role="alert">
        <h1>Ocorreu uma falha na interface</h1>
        <p>Exporte o diagnóstico antes de fechar e voltar a abrir a aplicação. As alterações ainda não guardadas podem não ser recuperadas.</p>
      </div>
      <DiagnosticsPanel />
    </main>;
  }
}

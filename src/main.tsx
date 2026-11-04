import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import DiagnosticsBoundary from "./features/diagnostics/DiagnosticsBoundary";
import { installGlobalDiagnostics } from "./lib/diagnostics";

const removeDiagnostics = installGlobalDiagnostics();
if (import.meta.hot) import.meta.hot.dispose(removeDiagnostics);

const container = document.getElementById("root");

if (!container) {
  throw new Error("Elemento raiz da interface não encontrado.");
}

ReactDOM.createRoot(container).render(
  <React.StrictMode>
    <DiagnosticsBoundary><App /></DiagnosticsBoundary>
  </React.StrictMode>,
);

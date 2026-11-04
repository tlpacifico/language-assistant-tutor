import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import DiagnosticsBoundary from "./DiagnosticsBoundary";
import DiagnosticsPanel from "./DiagnosticsPanel";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
afterEach(() => vi.restoreAllMocks());

const status = { build: { version: "0.1.0", revision: "abcdef012345", dirty: true, builtAt: "1793790000" }, writable: true, dropped: 2, directory: "C:/Data/diagnostics", defaultDirectory: "C:/Exports" };

it("mostra build e perdas e exporta para o diretório indicado", async () => {
  vi.mocked(invoke).mockImplementation(async (command) => command === "diagnostics_status" ? status : "C:/Exports/tutor.json");
  render(<DiagnosticsPanel />);
  fireEvent.click(screen.getByText("Diagnóstico local"));
  await screen.findByText(/abcdef0/);
  expect(screen.getByText(/2 eventos/)).toBeInTheDocument();
  fireEvent.change(screen.getByLabelText("Pasta de destino"), { target: { value: "C:/Other" } });
  fireEvent.click(screen.getByRole("button", { name: "Exportar diagnóstico" }));
  await screen.findByText(/C:\/Exports\/tutor.json/);
  expect(invoke).toHaveBeenCalledWith("diagnostics_export", { directory: "C:/Other" });
});

it("permite diagnosticar uma falha React sem expor o erro privado", async () => {
  vi.spyOn(console, "error").mockImplementation(() => undefined);
  vi.mocked(invoke).mockImplementation(async (command) => command === "diagnostics_status" ? status : "C:/Exports/tutor.json");
  function Broken(): never { throw new Error("private answer"); }
  render(<DiagnosticsBoundary><Broken /></DiagnosticsBoundary>);
  expect(screen.getByRole("alert")).toHaveTextContent("Ocorreu uma falha na interface");
  expect(screen.queryByText(/private answer/)).not.toBeInTheDocument();
  fireEvent.click(screen.getByText("Diagnóstico local"));
  await waitFor(() => expect(screen.getByRole("button", { name: "Exportar diagnóstico" })).toBeEnabled());
  fireEvent.click(screen.getByRole("button", { name: "Exportar diagnóstico" }));
  await screen.findByText(/tutor.json/);
  const reports = vi.mocked(invoke).mock.calls.filter(([command]) => command === "diagnostics_record");
  expect(reports.length).toBeGreaterThan(0);
  expect(JSON.stringify(reports)).not.toContain("private answer");
});

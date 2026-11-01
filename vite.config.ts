import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

// A janela Tauri carrega o servidor de desenvolvimento nesta porta fixa.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.{ts,tsx}"],
    // A base da fase 1 ainda não tem testes de interface; a tarefa 7 acrescenta-os.
    passWithNoTests: true,
  },
});

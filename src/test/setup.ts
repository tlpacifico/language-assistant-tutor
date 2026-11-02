import "@testing-library/jest-dom/vitest";

import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

// O projeto não ativa os globais do Vitest, pelo que a limpeza automática do
// Testing Library não é registada. Limpar explicitamente entre testes.
afterEach(() => {
  cleanup();
});

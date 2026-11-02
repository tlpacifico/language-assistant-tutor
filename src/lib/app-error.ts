/**
 * Normalização de rejeições do cliente para a interface.
 *
 * Os comandos devolvem sempre um `AppError`; uma falha inesperada de IPC é
 * apresentada como erro repetível, sem inventar códigos de domínio.
 */

import type { AppError } from "./contracts";

/** Converte qualquer rejeição do cliente num erro estável para a interface. */
export function toAppError(error: unknown): AppError {
  if (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    "message" in error &&
    "retryable" in error
  ) {
    return error as AppError;
  }
  return {
    code: "UNKNOWN",
    message:
      "Não foi possível comunicar com o núcleo da aplicação. Tente novamente.",
    retryable: true,
  };
}

/**
 * Marca de «trabalho IA assentou».
 *
 * O núcleo devolve `running` e publica o resultado depois, limpando
 * `aiJob` numa fotografia nova. Os painéis relêem os seus dados sempre que
 * esta marca muda: enquanto um trabalho está em curso fica congelada; sem
 * trabalho acompanha a revisão da fotografia (releitura local, sem rede).
 */

import { useRef } from "react";

import type { InterviewSnapshot } from "../../lib/interview-contracts";

export function useAiSettled(
  snapshot: InterviewSnapshot | null | undefined,
): number {
  const settled = useRef(0);
  if (snapshot && !snapshot.aiJob) {
    settled.current = snapshot.revision;
  }
  return settled.current;
}

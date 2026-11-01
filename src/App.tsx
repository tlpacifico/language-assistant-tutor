import { INITIAL_QUESTION } from "./lib/contracts";

/**
 * Base da fase 1: a janela apresenta apenas a pergunta fixa.
 * A experiência completa (disponibilidade, gravação, transcrição e revisão)
 * entra na tarefa 7; não há áudio simulado nesta base.
 */
export default function App() {
  return (
    <main>
      <h1>{INITIAL_QUESTION}</h1>
    </main>
  );
}

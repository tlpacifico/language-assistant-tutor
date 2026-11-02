/**
 * Adaptador Tauri da fase 1.
 *
 * Este é o único ficheiro que conhece os nomes dos comandos e do evento
 * `practice://snapshot`; a interface usa apenas `PracticeClient`. Cada método
 * devolve a fotografia (`AppSnapshot`) ou rejeita com o `AppError` estável
 * devolvido pelo serviço (em português de Portugal).
 */

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import type { AppSnapshot, PlaybackTarget } from "./contracts";

/** Evento com a fotografia do estado, publicado depois de cada alteração. */
export const PRACTICE_SNAPSHOT_EVENT = "practice://snapshot";

/** Remoção de um listener de fotografias. */
export type Unsubscribe = () => void;

/** Acesso único da interface aos comandos Tauri. */
export interface PracticeClient {
  /** Fotografia atual, sem alterar o estado. */
  snapshot(): Promise<AppSnapshot>;
  /** Verifica microfone, saída e auxiliar. */
  refresh(): Promise<AppSnapshot>;
  /** Reserva a pasta e abre o microfone. */
  startRecording(): Promise<AppSnapshot>;
  /** Termina a captura e publica a resposta. */
  stopRecording(): Promise<AppSnapshot>;
  /** Repete a publicação de uma captura já finalizada. */
  retrySaveRecording(): Promise<AppSnapshot>;
  /** Transcreve a resposta ativa. */
  transcribe(attemptId: string): Promise<AppSnapshot>;
  /** Reproduz a pergunta ou o original da resposta ativa. */
  play(target: PlaybackTarget): Promise<AppSnapshot>;
  /** Pára a reprodução em curso. */
  stop(): Promise<AppSnapshot>;
  /** Guarda a revisão de uma transcrição; não confirma a resposta. */
  saveReview(
    attemptId: string,
    sourceId: string,
    text: string,
  ): Promise<AppSnapshot>;
  /** Reinicia o auxiliar Python. */
  restartWorker(): Promise<AppSnapshot>;
  /** Finaliza a captura, interrompe o trabalho pendente e encerra o auxiliar. */
  prepareClose(): Promise<AppSnapshot>;
  /** Recebe cada fotografia publicada; devolve a remoção do listener. */
  subscribe(listener: (snapshot: AppSnapshot) => void): Unsubscribe;
}

/** Cria o cliente sobre a aplicação Tauri em execução. */
export function createPracticeClient(): PracticeClient {
  return {
    snapshot: () => invoke<AppSnapshot>("get_snapshot"),
    refresh: () => invoke<AppSnapshot>("refresh_availability"),
    startRecording: () => invoke<AppSnapshot>("start_recording"),
    stopRecording: () => invoke<AppSnapshot>("stop_recording"),
    retrySaveRecording: () => invoke<AppSnapshot>("retry_save_recording"),
    transcribe: (attemptId) =>
      invoke<AppSnapshot>("transcribe_response", { attemptId }),
    play: (target) => invoke<AppSnapshot>("play_audio", { target }),
    stop: () => invoke<AppSnapshot>("stop_audio"),
    saveReview: (attemptId, sourceId, text) =>
      invoke<AppSnapshot>("save_review", { attemptId, sourceId, text }),
    restartWorker: () => invoke<AppSnapshot>("restart_worker"),
    prepareClose: () => invoke<AppSnapshot>("prepare_close"),
    subscribe(listener) {
      let unlisten: Unsubscribe | null = null;
      let disposed = false;
      void listen<AppSnapshot>(PRACTICE_SNAPSHOT_EVENT, (event) => {
        listener(event.payload);
      }).then((remove) => {
        // A subscrição pode terminar antes de `listen` resolver.
        if (disposed) {
          remove();
        } else {
          unlisten = remove;
        }
      });
      return () => {
        disposed = true;
        if (unlisten) {
          unlisten();
          unlisten = null;
        }
      };
    },
  };
}

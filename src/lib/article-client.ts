/**
 * Adaptador Tauri da conversa sobre artigos.
 *
 * Único ficheiro que conhece os nomes dos comandos `article_*` e do evento
 * `article://snapshot`; a interface usa apenas `ArticleClient`. Cada comando
 * devolve a fotografia persistida; trabalho externo conclui por evento.
 */

import { listen } from "@tauri-apps/api/event";

import { diagnosticInvoke as invoke } from "./diagnostics";
import type {
  ArticleCommand,
  ArticleConsent,
  ArticleCorrection,
  ArticleCreate,
  ArticleSnapshot,
  ArticleSummary,
} from "./article-contracts";

export const ARTICLE_SNAPSHOT_EVENT = "article://snapshot";

export interface ArticleClient {
  /** Destino configurado, mostrado antes de autorizar. */
  destination(): Promise<string>;
  list(): Promise<ArticleSummary[]>;
  snapshot(sessionId: string): Promise<ArticleSnapshot>;
  create(input: ArticleCreate): Promise<ArticleSnapshot>;
  authorize(consent: ArticleConsent): Promise<ArticleSnapshot>;
  revoke(command: ArticleCommand): Promise<ArticleSnapshot>;
  startRecording(command: ArticleCommand): Promise<ArticleSnapshot>;
  stopRecording(command: ArticleCommand): Promise<ArticleSnapshot>;
  submitText(correction: ArticleCorrection): Promise<ArticleSnapshot>;
  retry(command: ArticleCommand): Promise<ArticleSnapshot>;
  finish(command: ArticleCommand): Promise<ArticleSnapshot>;
  correct(correction: ArticleCorrection): Promise<ArticleSnapshot>;
  resume(command: ArticleCommand): Promise<ArticleSnapshot>;
  delete(command: ArticleCommand): Promise<void>;
  /** Ouve a gravação de uma resposta (turno do utilizador) já guardada. */
  playTurn(sessionId: string, turnId: string): Promise<void>;
  /** Interrompe a voz do tutor sem apagar texto nem estado. */
  stopVoice(): Promise<void>;
  /** Recebe cada fotografia publicada; devolve a remoção do listener. */
  subscribe(handler: (snapshot: ArticleSnapshot) => void): Promise<() => void>;
}

export function createArticleClient(): ArticleClient {
  return {
    async destination() {
      const settings = await invoke<{ baseUrl: string }>(
        "interview_ai_load_ai_settings",
      );
      return settings.baseUrl;
    },
    list: () => invoke<ArticleSummary[]>("article_list"),
    snapshot: (sessionId) =>
      invoke<ArticleSnapshot>("article_snapshot", { sessionId }),
    create: (input) => invoke<ArticleSnapshot>("article_create", { input }),
    authorize: (consent) =>
      invoke<ArticleSnapshot>("article_authorize", { consent }),
    revoke: (command) => invoke<ArticleSnapshot>("article_revoke", { command }),
    startRecording: (command) =>
      invoke<ArticleSnapshot>("article_start_recording", { command }),
    stopRecording: (command) =>
      invoke<ArticleSnapshot>("article_stop_recording", { command }),
    submitText: (correction) =>
      invoke<ArticleSnapshot>("article_submit_text", { correction }),
    retry: (command) => invoke<ArticleSnapshot>("article_retry", { command }),
    finish: (command) => invoke<ArticleSnapshot>("article_finish", { command }),
    correct: (correction) =>
      invoke<ArticleSnapshot>("article_correct", { correction }),
    resume: (command) => invoke<ArticleSnapshot>("article_resume", { command }),
    async delete(command) {
      await invoke<void>("article_delete", { command });
    },
    async playTurn(sessionId, turnId) {
      await invoke<void>("article_play_turn", { sessionId, turnId });
    },
    async stopVoice() {
      await invoke("interview_stop_audio");
    },
    async subscribe(handler) {
      return listen<ArticleSnapshot>(ARTICLE_SNAPSHOT_EVENT, (event) =>
        handler(event.payload),
      );
    },
  };
}

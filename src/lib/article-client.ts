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
  ArticlePlayerView,
  ArticlePreferences,
  ArticleRecordingView,
  ArticleSnapshot,
  ArticleSummary,
} from "./article-contracts";

export const ARTICLE_SNAPSHOT_EVENT = "article://snapshot";
export const ARTICLE_PLAYER_EVENT = "article://player";
export const ARTICLE_RECORDING_EVENT = "article://recording";

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
  /** Retoma uma conversa guardada. */
  resume(command: ArticleCommand): Promise<ArticleSnapshot>;
  /** Retoma a reprodução atual, limitada à geração indicada. */
  resume(sessionId: string, generation: number): Promise<ArticlePlayerView>;
  delete(command: ArticleCommand): Promise<void>;
  /** Preferências do leitor (reprodução automática e velocidade). */
  preferences(): Promise<ArticlePreferences>;
  /** Guarda as preferências e devolve a versão validada. */
  savePreferences(input: ArticlePreferences): Promise<ArticlePreferences>;
  /** Reproduz uma fala (tutor) ou a gravação de uma resposta, do início. */
  play(sessionId: string, turnId: string): Promise<ArticlePlayerView>;
  /** Pausa a reprodução atual, limitada à geração indicada. */
  pause(sessionId: string, generation: number): Promise<ArticlePlayerView>;
  /** Para a reprodução atual, limitada à geração indicada. */
  stop(sessionId: string, generation: number): Promise<ArticlePlayerView>;
  /** Regista a sessão e o token da vista focada. */
  focusView(sessionId: string, token: string): Promise<void>;
  /** Limpa o foco apenas quando o token coincide. */
  blurView(token: string): Promise<void>;
  /** Ouve o estado de reprodução; devolve a remoção do listener. */
  subscribePlayer(
    handler: (view: ArticlePlayerView) => void,
  ): Promise<() => void>;
  /** Ouve o estado de gravação; devolve a remoção do listener. */
  subscribeRecording(
    handler: (view: ArticleRecordingView) => void,
  ): Promise<() => void>;
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
    resume: ((
      sessionIdOrCommand: string | ArticleCommand,
      generation?: number,
    ) =>
      typeof sessionIdOrCommand === "string"
        ? invoke<ArticlePlayerView>("article_player_resume", {
            sessionId: sessionIdOrCommand,
            generation,
          })
        : invoke<ArticleSnapshot>("article_resume", {
            command: sessionIdOrCommand,
          })) as ArticleClient["resume"],
    async delete(command) {
      await invoke<void>("article_delete", { command });
    },
    preferences: () => invoke<ArticlePreferences>("article_preferences_load"),
    savePreferences: (input) =>
      invoke<ArticlePreferences>("article_preferences_save", {
        preferences: input,
      }),
    play: (sessionId, turnId) =>
      invoke<ArticlePlayerView>("article_player_play", { sessionId, turnId }),
    pause: (sessionId, generation) =>
      invoke<ArticlePlayerView>("article_player_pause", { sessionId, generation }),
    stop: (sessionId, generation) =>
      invoke<ArticlePlayerView>("article_player_stop", { sessionId, generation }),
    focusView: (sessionId, token) =>
      invoke<void>("article_view_focus", { sessionId, token }),
    blurView: (token) => invoke<void>("article_view_blur", { token }),
    subscribePlayer: (handler) =>
      listen<ArticlePlayerView>(ARTICLE_PLAYER_EVENT, (event) =>
        handler(event.payload),
      ),
    subscribeRecording: (handler) =>
      listen<ArticleRecordingView>(ARTICLE_RECORDING_EVENT, (event) =>
        handler(event.payload),
      ),
    async subscribe(handler) {
      return listen<ArticleSnapshot>(ARTICLE_SNAPSHOT_EVENT, (event) =>
        handler(event.payload),
      );
    },
  };
}

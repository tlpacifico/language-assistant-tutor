//! Orquestração da conversa sobre artigos.
//!
//! O núcleo é a autoridade: cada comando valida o estado persistido, guarda
//! antes de enviar e só depois lança trabalho externo. A gravação termina
//! apenas por `article_stop_recording`; o envio ao fornecedor acontece depois
//! de a transcrição estar guardada e com a autorização desta sessão válida.
//! Um resultado só se aplica ao pedido e à geração que o originaram.
//!
//! A reprodução é a máquina de estados [`PlayerRuntime`] (`article/player.rs`):
//! é a única dona da geração e da sequência, e é espelhada no campo `audio` da
//! fotografia e no evento `article://player`. A síntese incremental do tutor
//! escreve diretamente na entrada da cache de voz e só fica reutilizável depois
//! de o manifesto completo ser escrito.
//!
//! Captura, transcrição e pedidos HTTP deste modo têm estado próprio
//! ([`ArticleRuntime`]); não passam pelas gravações nem pelos answer slots STAR.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Weak;
use std::time::{Duration, Instant};

use serde_json::json;
use tokio::task::JoinHandle;

use super::*;
use crate::ai::{
    validate_article_opening, validate_article_reply, validate_article_report, ArticleAiTask,
    ArticleReply, ArticleTransportRequest,
};
use crate::article::{
    begin_entry, can_send_automatically, finish_entry, read_cache, remove_incomplete, voice_key,
    ArticleCommand, ArticleConsent, ArticleCorrection, ArticleCreate, ArticleError,
    ArticleMutation, ArticleOutcome, ArticlePlayerView, ArticlePreferences, ArticleRequestKind,
    ArticleSnapshot, ArticleSpeaker, ArticleState, ArticleSummary, CachedVoice, PlaybackTrigger,
    PlayerEvent, PlayerOperation, PlayerRuntime, PlayerSource, PlayerState, RecordingRuntime,
    ARTICLE_NOTICE_VERSION, ENGINE_ID,
};
use crate::article::{ChunkRejection, ChunkSequence};
use crate::audio::{PlaybackProgress, RecordingTarget};
use crate::worker::protocol::{SynthesizeStreamParams, TranscribeParams};

/// Voz fixa do tutor no modo artigo; entra na identidade da cache.
const ARTICLE_VOICE: &str = "af_heart";

/// Destinatário das fotografias publicadas em `article://snapshot`.
pub type ArticleListener = Arc<dyn Fn(&ArticleSnapshot) + Send + Sync>;

/// Destinatário do estado de reprodução publicado em `article://player`.
pub type ArticlePlayerListener = Arc<dyn Fn(&ArticlePlayerView) + Send + Sync>;

/// Captura de uma resposta em curso.
#[derive(Debug, Clone)]
pub(super) struct ArticleCapture {
    session_id: String,
    turn_id: String,
    directory: PathBuf,
}

/// Transcrição pedida ao auxiliar, ligada ao turno que a originou.
#[derive(Debug, Clone)]
pub(super) struct ArticleTranscription {
    request_id: String,
    session_id: String,
    turn_id: String,
    started: Instant,
}

/// Voz do tutor em curso: blocos aceites só pela ordem e na entrada da cache.
#[derive(Debug, Clone)]
pub(super) struct ArticleSpeech {
    request_id: String,
    /// Sessão a que pertence a síntese; fecha a entrada da cache ao concluir.
    session_id: String,
    /// Chave da cache (identidade exata de texto, voz, velocidade e motor).
    key: String,
    text: String,
    voice: String,
    speed_x10: u8,
    engine: String,
    sequence: ChunkSequence,
    /// Quantos blocos aceites já foram entregues à porta de áudio.
    played: u32,
    started: Instant,
    first_reported: bool,
}

/// Estado em memória do modo artigo.
pub(super) struct ArticleRuntime {
    capture: Option<ArticleCapture>,
    transcription: Option<ArticleTranscription>,
    speech: Option<ArticleSpeech>,
    /// Máquina de estados da reprodução; única dona de generation/sequence.
    player: PlayerRuntime,
    /// Máquina de estados da gravação; dona de capture_id/sequence.
    recording: RecordingRuntime,
    /// Identidade da reprodução atual devolvida pela porta de áudio.
    playback_id: Option<crate::audio::PlaybackId>,
    /// Foco da vista do artigo: (sessão, token). A reprodução automática só
    /// acontece na sessão focada.
    focus: Option<(String, String)>,
    tasks: Vec<JoinHandle<()>>,
    this: Option<Weak<AppService>>,
}

impl Default for ArticleRuntime {
    fn default() -> Self {
        Self {
            capture: None,
            transcription: None,
            speech: None,
            player: PlayerRuntime::idle(String::new()),
            recording: RecordingRuntime::new(String::new()),
            playback_id: None,
            focus: None,
            tasks: Vec::new(),
            this: None,
        }
    }
}

impl ArticleRuntime {
    fn busy(&self) -> bool {
        self.capture.is_some() || self.transcription.is_some()
    }
}

/// Nome de um bloco da cache, como o auxiliar o escreve.
fn chunk_file(index: u32) -> String {
    format!("chunk-{index:04}.wav")
}

/// Duração real de um WAV em milissegundos, arredondada.
fn wav_duration_ms(path: &Path) -> Result<u64, AppError> {
    let reader = hound::WavReader::open(path).map_err(|error| {
        invalid_audio(format!(
            "O áudio em «{}» não é um WAV legível: {error}.",
            path.display()
        ))
    })?;
    let spec = reader.spec();
    if spec.channels == 0 || spec.sample_rate == 0 {
        return Err(invalid_audio(
            "O áudio desta resposta tem um formato inválido.",
        ));
    }
    let frames = u64::from(reader.duration());
    let rate = u64::from(spec.sample_rate.max(1));
    Ok((frames * 1_000 + rate / 2) / rate)
}

/// Métrica de um turno: operação fechada, código estável e duração; nunca conteúdo.
fn record_metric(operation: &str, code: &str, started: Instant) {
    let mut event = crate::diagnostics::Event::new("rust", operation, code);
    event.duration_ms = Some(started.elapsed().as_millis() as u64);
    crate::diagnostics::record(event);
}

fn article_error(error: &AppError) -> ArticleError {
    ArticleError {
        code: error.code.clone(),
        message: error.message.clone(),
        retryable: error.retryable,
    }
}

fn consent_error(message: &str) -> AppError {
    AppError::new("AI_CONSENT", message, false)
}

impl AppService {
    fn lock_article(&self) -> std::sync::MutexGuard<'_, ArticleRuntime> {
        self.article.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn lock_article_listener(&self) -> std::sync::MutexGuard<'_, Option<ArticleListener>> {
        self.article_listener
            .lock()
            .unwrap_or_else(|p| p.into_inner())
    }

    fn lock_article_player_listener(
        &self,
    ) -> std::sync::MutexGuard<'_, Option<ArticlePlayerListener>> {
        self.article_player_listener
            .lock()
            .unwrap_or_else(|p| p.into_inner())
    }

    /// Substitui o transporte do modo artigo (testes e configuração).
    pub fn with_article_transport(
        mut self,
        transport: Arc<dyn crate::ai::ArticleTransport>,
    ) -> Self {
        self.article_transport = transport;
        self
    }

    /// Regista o destinatário das fotografias do modo artigo.
    pub fn subscribe_articles(&self, listener: ArticleListener) {
        *self.lock_article_listener() = Some(listener);
    }

    /// Regista o destinatário do estado de reprodução (`article://player`).
    pub fn subscribe_article_player(&self, listener: ArticlePlayerListener) {
        *self.lock_article_player_listener() = Some(listener);
    }

    /// Guarda a referência ao próprio serviço para lançar pedidos em segundo plano.
    pub(super) fn remember_self(self: &Arc<Self>) {
        self.lock_article().this = Some(Arc::downgrade(self));
    }

    /// Junta à fotografia persistida o estado (em memória) da reprodução e da
    /// gravação, quando pertencem a esta sessão.
    fn with_audio(&self, mut snapshot: ArticleSnapshot) -> ArticleSnapshot {
        let (audio, recording) = {
            let runtime = self.lock_article();
            (runtime.player.view(), runtime.recording.view())
        };
        if audio.session_id == snapshot.session.id {
            snapshot.audio = audio;
        }
        if recording.session_id == snapshot.session.id {
            snapshot.recording = recording;
        }
        snapshot
    }

    fn publish_article(&self, snapshot: &ArticleSnapshot) {
        let listener = self.lock_article_listener().clone();
        if let Some(listener) = listener {
            listener(&self.with_audio(snapshot.clone()));
        }
    }

    fn publish_player(&self, view: &ArticlePlayerView) {
        let listener = self.lock_article_player_listener().clone();
        if let Some(listener) = listener {
            listener(view);
        }
    }

    /// Aplica um acontecimento à geração atual da reprodução e publica a
    /// mudança em `article://player`. Devolve a vista atual.
    fn apply_player_event(&self, generation: u64, event: PlayerEvent) -> ArticlePlayerView {
        let (changed, view) = {
            let mut runtime = self.lock_article();
            let changed = runtime.player.apply(generation, event);
            (changed, runtime.player.view())
        };
        if changed {
            self.publish_player(&view);
        }
        view
    }

    /// Seleciona uma reprodução nova do início. Garante que a máquina pertence
    /// a esta sessão (uma sessão nova começa com uma máquina limpa).
    fn select_player(
        &self,
        session_id: &str,
        turn_id: &str,
        version_id: Option<String>,
        source: PlayerSource,
        trigger: PlaybackTrigger,
    ) -> ArticlePlayerView {
        let view = {
            let mut runtime = self.lock_article();
            if runtime.player.view().session_id != session_id {
                runtime.player = PlayerRuntime::idle(session_id.to_owned());
            }
            runtime
                .player
                .select(Some(turn_id.to_owned()), version_id, source, trigger)
        };
        self.publish_player(&view);
        view
    }

    fn current_player_view(&self) -> ArticlePlayerView {
        self.lock_article().player.view()
    }

    /// A reprodução do artigo tomou o dispositivo: o STAR/prática deixa de
    /// apontar para a identidade que perdeu a saída.
    fn clear_star_playback(&self) {
        self.commit(|state| {
            state.playback = None;
            state.playback_id = None;
        });
    }

    /// A pasta de dados que contém `articles/`: a da base de dados.
    fn articles_base(&self) -> PathBuf {
        self.database_path
            .parent()
            .map(PathBuf::from)
            .unwrap_or_default()
    }

    /// Pasta exclusiva de uma conversa sobre um artigo.
    fn session_dir(&self, session_id: &str) -> PathBuf {
        self.articles_base().join("articles").join(session_id)
    }

    /// Espera pelos pedidos em segundo plano (apenas para os testes).
    #[doc(hidden)]
    pub async fn article_settle(&self) {
        loop {
            let tasks: Vec<_> = std::mem::take(&mut self.lock_article().tasks);
            if tasks.is_empty() {
                break;
            }
            for task in tasks {
                let _ = task.await;
            }
        }
    }

    fn set_article_busy(&self) {
        let busy = self.lock_article().busy();
        self.commit(|state| state.article_busy = busy);
    }

    // -- comandos de leitura e criação ---------------------------------------

    pub async fn article_create(&self, input: ArticleCreate) -> Result<ArticleSnapshot, AppError> {
        let database = self.database()?;
        let snapshot = database_task(move || database.create_article(input)).await?;
        self.publish_article(&snapshot);
        Ok(snapshot)
    }

    /// Fotografia persistida; nunca inicia trabalho externo.
    pub async fn article_snapshot(&self, session_id: &str) -> Result<ArticleSnapshot, AppError> {
        let database = self.database()?;
        let id = session_id.to_owned();
        let snapshot = database_task(move || database.article_snapshot(&id)).await?;
        Ok(self.with_audio(snapshot))
    }

    pub async fn article_list(&self) -> Result<Vec<ArticleSummary>, AppError> {
        let database = self.database()?;
        database_task(move || database.list_articles()).await
    }

    /// Preferências do leitor de artigos; nunca inicia trabalho externo.
    pub async fn article_preferences_load(&self) -> Result<ArticlePreferences, AppError> {
        let database = self.database()?;
        database_task(move || database.article_preferences()).await
    }

    /// Guarda as preferências do leitor e devolve a versão validada.
    pub async fn article_preferences_save(
        &self,
        preferences: ArticlePreferences,
    ) -> Result<ArticlePreferences, AppError> {
        let _transition = self.transitions.lock().await;
        let database = self.database()?;
        let previous = {
            let db = database.clone();
            database_task(move || db.article_preferences()).await?
        };
        let db = database.clone();
        let input = preferences;
        let saved = database_task(move || db.put_article_preferences(&input)).await?;
        // Desligar a reprodução automática pára apenas a voz automática em
        // curso; uma repetição explícita continua.
        if previous.autoplay && !saved.autoplay {
            let trigger = self.lock_article().player.view().trigger;
            if trigger == Some(PlaybackTrigger::Autoplay) {
                self.stop_article_speech_locked().await;
            }
        }
        Ok(saved)
    }

    // -- consentimento --------------------------------------------------------

    pub async fn article_authorize(
        self: &Arc<Self>,
        consent: ArticleConsent,
    ) -> Result<ArticleSnapshot, AppError> {
        let _transition = self.transitions.lock().await;
        let database = self.database()?;
        let settings = self.load_ai_settings().await?;
        if consent.destination != settings.base_url {
            return Err(consent_error(
                "O destino mudou. Atualize a página e autorize o destino atual.",
            ));
        }
        if !settings.credential_configured {
            return Err(AppError::new(
                "AI_CREDENTIAL",
                "Configure a credencial do fornecedor antes de autorizar a conversa.",
                false,
            ));
        }
        let command = consent.command;
        let change = ArticleMutation::Authorize {
            destination: consent.destination,
            notice_version: consent.notice_version,
        };
        let snapshot = database_task(move || database.apply_article(&command, change)).await?;
        self.publish_article(&snapshot);
        self.dispatch_request_if_due(&snapshot);
        Ok(snapshot)
    }

    pub async fn article_revoke(
        &self,
        command: ArticleCommand,
    ) -> Result<ArticleSnapshot, AppError> {
        let _transition = self.transitions.lock().await;
        // A revogação invalida a reprodução e a intenção automática antes de
        // tocar na base.
        self.stop_article_speech_locked().await;
        let database = self.database()?;
        let snapshot =
            database_task(move || database.apply_article(&command, ArticleMutation::Revoke))
                .await?;
        self.publish_article(&snapshot);
        Ok(snapshot)
    }

    // -- gravação -------------------------------------------------------------

    pub async fn article_start_recording(
        &self,
        command: ArticleCommand,
    ) -> Result<ArticleSnapshot, AppError> {
        let _transition = self.transitions.lock().await;
        let database = self.database()?;
        let turn_id = uuid::Uuid::new_v4().to_string();
        let audio_path = format!(
            "articles/{}/{turn_id}/{}",
            command.session_id,
            crate::audio::ORIGINAL_FILE
        );
        let change = ArticleMutation::StartRecording {
            turn_id: turn_id.clone(),
            audio_path,
        };

        // Um comando repetido devolve o resultado guardado, sem tocar no áudio.
        let db = database.clone();
        let request = command.request_id.clone();
        if database_task(move || db.article_replayed(&request)).await? {
            let db = database.clone();
            return database_task(move || db.apply_article(&command, change)).await;
        }

        self.stop_article_speech_locked().await;
        self.ensure_audio_free_for_article()?;
        let directory = self
            .articles_base()
            .join("articles")
            .join(&command.session_id)
            .join(&turn_id);
        let created = directory.clone();
        storage_task(move || {
            fs::create_dir_all(&created).map_err(|error| {
                AppError::new(
                    error_code::STORAGE_WRITE_FAILED,
                    format!("Não foi possível preparar a pasta da gravação: {error}."),
                    true,
                )
            })
        })
        .await?;

        let audio = Arc::clone(&self.audio);
        let _ = audio_task(move || audio.stop_all_playback()).await;
        self.commit(|state| {
            state.playback = None;
            state.playback_id = None;
        });

        let db = database.clone();
        let session_id = command.session_id.clone();
        let applied = database_task(move || db.apply_article(&command, change)).await;
        let snapshot = match applied {
            Ok(snapshot) => snapshot,
            Err(error) => {
                let _ = fs::remove_dir(&directory);
                return Err(error);
            }
        };

        let target = RecordingTarget {
            id: turn_id.clone(),
            directory: directory.clone(),
        };
        let audio = Arc::clone(&self.audio);
        match audio_task(move || audio.start_capture(target)).await {
            Ok(()) => {
                self.lock_article().capture = Some(ArticleCapture {
                    session_id,
                    turn_id,
                    directory,
                });
                self.set_article_busy();
                self.publish_article(&snapshot);
                Ok(snapshot)
            }
            Err(error) => {
                let _ = fs::remove_dir(&directory);
                let db = database.clone();
                let failure = article_error(&error);
                let failed = database_task(move || {
                    db.apply_article_system(&session_id, ArticleMutation::Fail { error: failure })
                })
                .await;
                if let Ok(snapshot) = &failed {
                    self.publish_article(snapshot);
                }
                self.resample_after_device_error(&error).await;
                Err(error)
            }
        }
    }

    pub async fn article_stop_recording(
        &self,
        command: ArticleCommand,
    ) -> Result<ArticleSnapshot, AppError> {
        let _transition = self.transitions.lock().await;
        let database = self.database()?;
        let db = database.clone();
        let request = command.request_id.clone();
        let replayed = database_task(move || db.article_replayed(&request)).await?;
        let capture = self
            .lock_article()
            .capture
            .clone()
            .filter(|c| c.session_id == command.session_id);
        let (Some(capture), false) = (capture, replayed) else {
            // Repetição, ou nada a parar: o repositório devolve o resultado
            // guardado ou recusa pelo estado.
            return database_task(move || {
                database.apply_article(&command, ArticleMutation::StopRecording)
            })
            .await;
        };

        let audio = Arc::clone(&self.audio);
        let stopped = audio_task(move || audio.stop_capture()).await;
        let captured = match stopped {
            Ok(captured) => captured,
            Err(error) => {
                self.lock_article().capture = None;
                self.set_article_busy();
                let db = database.clone();
                let failure = article_error(&error);
                let session_id = capture.session_id.clone();
                let failed = database_task(move || {
                    db.apply_article_system(&session_id, ArticleMutation::Fail { error: failure })
                })
                .await;
                if let Ok(snapshot) = &failed {
                    self.publish_article(snapshot);
                }
                self.resample_after_device_error(&error).await;
                return Err(error);
            }
        };

        // O áudio finalizado já está no disco; só agora se regista o fim.
        let db = database.clone();
        let session_id = capture.session_id.clone();
        let stopped =
            database_task(move || db.apply_article(&command, ArticleMutation::StopRecording)).await;
        self.lock_article().capture = None;
        let snapshot = match stopped {
            Ok(snapshot) => snapshot,
            Err(error) => {
                // O áudio já fechou: sem esta marca a conversa ficava em `recording`
                // sem captura viva. Fica recuperável (`Retry` volta a gravar).
                self.set_article_busy();
                let db = database.clone();
                let failure = article_error(&error);
                let failed = database_task({
                    let session_id = session_id.clone();
                    move || {
                        db.apply_article_system(
                            &session_id,
                            ArticleMutation::Fail { error: failure },
                        )
                    }
                })
                .await;
                if let Ok(snapshot) = &failed {
                    self.publish_article(snapshot);
                }
                return Err(error);
            }
        };
        self.publish_article(&snapshot);

        let submitted = self.submit_article_transcription(&capture, &captured).await;
        match submitted {
            Ok(()) => {
                self.set_article_busy();
                Ok(snapshot)
            }
            Err(error) => {
                // O áudio e o turno ficam guardados; sem transcrição o utilizador
                // escreve o texto ou grava de novo.
                self.set_article_busy();
                let db = database.clone();
                let turn_id = capture.turn_id.clone();
                let snapshot = database_task(move || {
                    db.apply_article_system(
                        &session_id,
                        ArticleMutation::RecordTranscript {
                            turn_id,
                            text: None,
                        },
                    )
                })
                .await?;
                self.publish_article(&snapshot);
                self.record_error(&error);
                Ok(snapshot)
            }
        }
    }

    /// Normaliza o áudio e pede a transcrição ao auxiliar.
    async fn submit_article_transcription(
        &self,
        capture: &ArticleCapture,
        captured: &CapturedAudio,
    ) -> Result<(), AppError> {
        let Some(control) = self.worker_control().filter(|c| c.is_running()) else {
            return Err(worker_unavailable(
                "O auxiliar Python não está em execução; a resposta continua guardada.",
            ));
        };
        let generation = self.generation.load(Ordering::SeqCst);
        if generation == 0 {
            return Err(worker_unavailable(
                "O auxiliar Python não está em execução; reinicie-o antes de transcrever.",
            ));
        }
        let model_path = self.whisper_directory()?;
        let original = captured.original.clone();
        let normalized = capture.directory.join(crate::artifacts::NORMALIZED_FILE);
        let target = normalized.clone();
        storage_task(move || crate::audio::normalize_wav(&original, &target).map(|_| ())).await?;

        let request_id = uuid::Uuid::new_v4().to_string();
        let params = TranscribeParams::cpu(&normalized, &model_path);
        let duration = std::time::Duration::from_millis(captured.meta.duration_ms.max(1));
        let request = WorkerRequest::transcribe(request_id.clone(), params, duration)?;
        self.lock_article().transcription = Some(ArticleTranscription {
            request_id,
            session_id: capture.session_id.clone(),
            turn_id: capture.turn_id.clone(),
            started: Instant::now(),
        });
        // Depois de parar a voz o auxiliar pode demorar um instante a ficar livre.
        let mut attempts = 0;
        loop {
            let control = Arc::clone(&control);
            let request = request.clone();
            match worker_task(move || control.submit(request)).await {
                Ok(()) => return Ok(()),
                Err(error) if error.code == error_code::BUSY && attempts < 40 => {
                    attempts += 1;
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                Err(error) => {
                    self.lock_article().transcription = None;
                    return Err(error);
                }
            }
        }
    }

    /// A captura e o auxiliar são partilhados com o STAR: só um dono de cada vez.
    fn ensure_audio_free_for_article(&self) -> Result<(), AppError> {
        let star_busy = {
            let state = self.lock_state();
            state.recording_id.is_some() || state.finalizing || state.model_job.is_some()
        } || self.lock_pending().is_some()
            || self.lock_capture().is_some();
        if star_busy {
            return Err(AppError::busy(
                "Há uma gravação ou transcrição da entrevista em curso. Aguarde que termine.",
            ));
        }
        if self.lock_article().busy() {
            return Err(AppError::busy(
                "Já está uma gravação ou transcrição de um artigo em curso.",
            ));
        }
        Ok(())
    }

    // -- eventos do auxiliar --------------------------------------------------

    /// Encaminha uma mensagem do auxiliar se pertencer à transcrição do artigo.
    ///
    /// Devolve `true` quando a mensagem foi tratada aqui.
    pub(super) async fn route_article_worker_message(
        &self,
        generation: u64,
        message: &WorkerMessage,
    ) -> bool {
        let request_id = match message {
            WorkerMessage::Progress { request_id, .. }
            | WorkerMessage::AudioChunk { request_id, .. }
            | WorkerMessage::Completed { request_id, .. }
            | WorkerMessage::Failed { request_id, .. } => request_id,
        };
        if self
            .route_speech_message(generation, request_id, message)
            .await
        {
            return true;
        }
        let job = self.lock_article().transcription.clone();
        let Some(job) = job.filter(|job| job.request_id == *request_id) else {
            return false;
        };
        let text = match message {
            WorkerMessage::Progress { .. } | WorkerMessage::AudioChunk { .. } => return true,
            WorkerMessage::Completed {
                result: WorkerResult::Transcription(result),
                ..
            } => {
                let current =
                    generation != 0 && generation == self.generation.load(Ordering::SeqCst);
                if current
                    && result.status == TranscriptStatus::Completed
                    && !result.text.trim().is_empty()
                {
                    Some(result.text.clone())
                } else {
                    None
                }
            }
            // Falha, geração antiga ou resultado de outro tipo: sem texto.
            _ => None,
        };
        self.lock_article().transcription = None;
        self.set_article_busy();
        let code = match message {
            WorkerMessage::Failed { error, .. } => error.code.as_str(),
            _ => "OK",
        };
        record_metric("article_transcribe", code, job.started);

        let Ok(database) = self.database() else {
            return true;
        };
        let session_id = job.session_id.clone();
        let turn_id = job.turn_id.clone();
        let applied = database_task(move || {
            database.apply_article_system(
                &session_id,
                ArticleMutation::RecordTranscript { turn_id, text },
            )
        })
        .await;
        // Uma conversa eliminada, revogada ou mudada entretanto recusa o resultado.
        if let Ok(snapshot) = applied {
            self.publish_article(&snapshot);
            self.dispatch_request_if_due(&snapshot);
        }
        true
    }

    // -- pedidos ao fornecedor --------------------------------------------------

    /// Lança o pedido seguinte quando a conversa está à espera do tutor ou a terminar.
    fn dispatch_request_if_due(&self, snapshot: &ArticleSnapshot) {
        if !matches!(
            snapshot.session.state,
            ArticleState::AwaitingTutor | ArticleState::Finishing
        ) {
            return;
        }
        let session_id = snapshot.session.id.clone();
        let this = self.lock_article().this.clone().and_then(|w| w.upgrade());
        let Some(this) = this else {
            return;
        };
        let task = tokio::spawn({
            let this = Arc::clone(&this);
            async move { this.run_article_request(&session_id).await }
        });
        let mut runtime = self.lock_article();
        runtime.tasks.retain(|t| !t.is_finished());
        runtime.tasks.push(task);
    }

    async fn run_article_request(&self, session_id: &str) {
        let Ok(database) = self.database() else {
            return;
        };
        let Ok(settings) = self.load_ai_settings().await else {
            return;
        };
        let db = database.clone();
        let id = session_id.to_owned();
        let Ok(snapshot) = database_task(move || db.article_snapshot(&id)).await else {
            return;
        };
        // Sem autorização válida para o destino atual, nada sai.
        if let Err(error) =
            can_send_automatically(snapshot.session.authorization.as_ref(), &settings.base_url)
        {
            self.fail_article(&database, session_id, &error).await;
            return;
        }
        let store = self.credentials.clone();
        let key = match database_task(move || store.get()).await {
            Ok(Some(key)) => key,
            _ => {
                let error = AppError::new(
                    "AI_CREDENTIAL",
                    "A credencial do fornecedor não está disponível.",
                    false,
                );
                self.fail_article(&database, session_id, &error).await;
                return;
            }
        };

        let config = json!({
            "baseUrl": settings.base_url,
            "model": settings.model,
            "schema": crate::ai::ARTICLE_SCHEMA,
        })
        .to_string();
        let db = database.clone();
        let id = session_id.to_owned();
        let ticket = match database_task(move || db.begin_article_request(&id, &config)).await {
            Ok(Some(ticket)) => ticket,
            // Nada a pedir ou já há um pedido pendente (clique duplo): não repete.
            _ => return,
        };
        let started = Instant::now();
        let task = match ticket.kind {
            ArticleRequestKind::Opening => ArticleAiTask::Opening,
            ArticleRequestKind::Reply => ArticleAiTask::Reply,
            ArticleRequestKind::Feedback => ArticleAiTask::Feedback,
        };
        let completion = self
            .article_transport
            .complete(ArticleTransportRequest {
                settings,
                key,
                conversation_id: session_id.to_owned(),
                task,
                context: ticket.context.clone(),
            })
            .await;

        let (outcome, usage) = match completion {
            Err(error) => (ArticleOutcome::Failed(article_error(&error)), None),
            Ok(completion) => {
                let usage = serde_json::to_string(&completion.usage).ok();
                let validated = if completion.invalid {
                    Err(AppError::new(
                        "AI_INVALID_RESULT",
                        "O fornecedor devolveu uma resposta incompleta.",
                        true,
                    ))
                } else {
                    match ticket.kind {
                        ArticleRequestKind::Opening => {
                            validate_article_opening(&completion.content).map(reply_outcome)
                        }
                        ArticleRequestKind::Reply => {
                            validate_article_reply(&completion.content).map(reply_outcome)
                        }
                        ArticleRequestKind::Feedback => {
                            validate_article_report(&completion.content, &ticket.context)
                                .map(|report| ArticleOutcome::Report(Box::new(report)))
                        }
                    }
                };
                match validated {
                    Ok(outcome) => (outcome, usage),
                    Err(error) => (ArticleOutcome::Failed(article_error(&error)), usage),
                }
            }
        };

        let code = match &outcome {
            ArticleOutcome::Failed(error) => error.code.clone(),
            _ => "OK".to_owned(),
        };
        let operation = if ticket.kind == ArticleRequestKind::Feedback {
            "article_feedback"
        } else {
            "article_reply"
        };
        record_metric(operation, &code, started);
        let db = database.clone();
        let request_id = ticket.request_id.clone();
        let finished = database_task(move || {
            db.finish_article_request(&request_id, outcome, usage.as_deref())
        })
        .await;
        if let Ok(Some(snapshot)) = finished {
            self.publish_article(&snapshot);
            // Só uma intervenção nova do tutor (aplicada agora) toca sozinha.
            if ticket.kind != ArticleRequestKind::Feedback
                && snapshot.session.state == ArticleState::Ready
            {
                if let Some(turn) = snapshot
                    .turns
                    .iter()
                    .rev()
                    .find(|t| t.speaker == ArticleSpeaker::Tutor)
                {
                    let id = turn.id.clone();
                    let version_id = turn.version.id.clone();
                    let text = turn.version.text.clone();
                    self.maybe_autoplay(&snapshot.session.id, &id, &version_id, &text)
                        .await;
                }
            }
        }
    }

    // -- voz do tutor -----------------------------------------------------------

    /// Reprodução automática de um turno novo do tutor, uma única vez.
    ///
    /// Só acontece com a preferência ligada e a vista desta sessão focada;
    /// nunca depois de reabrir, retomar ou de um evento duplicado.
    async fn maybe_autoplay(&self, session_id: &str, turn_id: &str, version_id: &str, text: &str) {
        let Ok(preferences) = self.article_preferences_load().await else {
            return;
        };
        if !preferences.autoplay {
            return;
        }
        let focused = self
            .lock_article()
            .focus
            .as_ref()
            .is_some_and(|(session, _)| session == session_id);
        if !focused {
            return;
        }
        let _transition = self.transitions.lock().await;
        let version_id = (!version_id.is_empty()).then(|| version_id.to_owned());
        let _ = self
            .play_tutor_turn(
                session_id,
                turn_id,
                version_id,
                text,
                preferences.speed_x10,
                PlaybackTrigger::Autoplay,
            )
            .await;
    }

    /// Reproduz uma fala do tutor: cache completa, prefixo de uma geração em
    /// curso, ou síntese nova. O chamador detém o coordenador serial.
    async fn play_tutor_turn(
        &self,
        session_id: &str,
        turn_id: &str,
        version_id: Option<String>,
        text: &str,
        speed_x10: u8,
        trigger: PlaybackTrigger,
    ) -> Result<(), AppError> {
        if text.trim().is_empty() {
            return Err(AppError::new(
                error_code::INVALID_INPUT,
                "A fala do tutor está vazia.",
                false,
            ));
        }
        let key = voice_key(text, ARTICLE_VOICE, speed_x10, ENGINE_ID);

        // Reinício durante uma geração ativa: conserva o prefixo ordenado e
        // aceita os blocos restantes uma vez, sem regenerar.
        let reuse = self
            .lock_article()
            .speech
            .as_ref()
            .is_some_and(|speech| speech.key == key);
        if reuse {
            {
                let mut runtime = self.lock_article();
                runtime.playback_id = None;
                if let Some(speech) = runtime.speech.as_mut() {
                    speech.played = 0;
                }
            }
            self.select_player(
                session_id,
                turn_id,
                version_id,
                PlayerSource::TutorVoice,
                trigger,
            );
            self.advance_speech_playback().await?;
            return Ok(());
        }

        let dir = self.session_dir(session_id);
        let dir2 = dir.clone();
        storage_task(move || remove_incomplete(&dir2)).await?;
        let dir3 = dir.clone();
        let key3 = key.clone();
        let cached = storage_task(move || Ok(read_cache(&dir3, &key3))).await?;

        if let Some(cached) = cached {
            // Cache completa: começa já, do início.
            self.stop_article_speech_locked().await;
            self.select_player(
                session_id,
                turn_id,
                version_id,
                PlayerSource::TutorVoice,
                trigger,
            );
            self.play_cached_chunks(&cached).await?;
            return Ok(());
        }

        self.stop_article_speech_locked().await;
        self.select_player(
            session_id,
            turn_id,
            version_id,
            PlayerSource::TutorVoice,
            trigger,
        );
        self.begin_synthesis(session_id, text, &key, speed_x10)
            .await?;
        Ok(())
    }

    /// Lança a síntese incremental de uma fala do tutor, escrevendo diretamente
    /// na entrada da cache. Falhas não perdem texto: a voz fica `failed`.
    async fn begin_synthesis(
        &self,
        session_id: &str,
        text: &str,
        key: &str,
        speed_x10: u8,
    ) -> Result<(), AppError> {
        let Some(control) = self.worker_control().filter(|c| c.is_running()) else {
            self.fail_player_synthesis();
            return Err(worker_unavailable(
                "O auxiliar Python não está em execução; reinicie-o antes de sintetizar a voz.",
            ));
        };
        if self.generation.load(Ordering::SeqCst) == 0 {
            self.fail_player_synthesis();
            return Err(worker_unavailable(
                "O auxiliar Python não está em execução; reinicie-o antes de sintetizar a voz.",
            ));
        }
        let dir = self.session_dir(session_id);
        let entry = {
            let dir = dir.clone();
            let key = key.to_owned();
            storage_task(move || begin_entry(&dir, &key)).await?
        };
        let request_id = format!("voice-{}", uuid::Uuid::new_v4());
        let params = SynthesizeStreamParams {
            text: text.trim().to_owned(),
            output_dir: entry.clone(),
            voice: ARTICLE_VOICE.to_owned(),
            speed: f32::from(speed_x10) / 10.0,
        };
        let request = WorkerRequest::synthesize_stream(request_id.clone(), params)?;
        self.lock_article().speech = Some(ArticleSpeech {
            request_id: request_id.clone(),
            session_id: session_id.to_owned(),
            key: key.to_owned(),
            text: text.trim().to_owned(),
            voice: ARTICLE_VOICE.to_owned(),
            speed_x10,
            engine: ENGINE_ID.to_owned(),
            sequence: ChunkSequence::new(entry),
            played: 0,
            started: Instant::now(),
            first_reported: false,
        });
        if let Err(error) = worker_task(move || control.submit(request)).await {
            self.lock_article().speech = None;
            self.fail_player_synthesis();
            return Err(error);
        }
        Ok(())
    }

    /// Pára a voz: limpa a fila, invalida a sequência (blocos tardios são
    /// descartados) e pede ao auxiliar que pare entre blocos.
    pub(super) async fn stop_article_speech_locked(&self) {
        let (speech, playback_id, generation) = {
            let mut runtime = self.lock_article();
            let speech = runtime.speech.take();
            let playback_id = runtime.playback_id.take();
            let generation = runtime.player.generation();
            (speech, playback_id, generation)
        };
        if speech.is_some() {
            if let Some(control) = self.worker_control() {
                let _ = worker_task(move || control.cancel()).await;
            }
        }
        if let Some(id) = playback_id {
            let audio = Arc::clone(&self.audio);
            let _ = audio_task(move || audio.stop_playback(&id)).await;
        }
        self.apply_player_event(generation, PlayerEvent::Stop);
    }

    /// Entrega à porta de áudio todos os blocos aceites e ainda não tocados,
    /// começando uma reprodução nova quando a fila está vazia.
    async fn advance_speech_playback(&self) -> Result<(), AppError> {
        let (directory, mut played, next, mut id) = {
            let runtime = self.lock_article();
            let Some(speech) = runtime.speech.as_ref() else {
                return Ok(());
            };
            (
                speech.sequence.directory().to_path_buf(),
                speech.played,
                speech.sequence.accepted(),
                runtime.playback_id.clone(),
            )
        };
        if played >= next {
            return Ok(());
        }
        if id.is_none() {
            let first = directory.join(chunk_file(played));
            let audio = Arc::clone(&self.audio);
            let new_id = audio_task(move || audio.start_playback(first)).await?;
            {
                let mut runtime = self.lock_article();
                runtime.playback_id = Some(new_id.clone());
                if let Some(speech) = runtime.speech.as_mut() {
                    speech.played = played + 1;
                }
            }
            self.clear_star_playback();
            id = Some(new_id);
            played += 1;
        }
        while played < next {
            let path = directory.join(chunk_file(played));
            let id2 = id.clone().expect("identidade da voz em curso");
            let audio = Arc::clone(&self.audio);
            audio_task(move || audio.append_chunk(path, &id2)).await?;
            played += 1;
        }
        if let Some(speech) = self.lock_article().speech.as_mut() {
            speech.played = played;
        }
        Ok(())
    }

    /// Reproduz uma entrada completa da cache, do primeiro ao último bloco.
    async fn play_cached_chunks(&self, cached: &CachedVoice) -> Result<(), AppError> {
        if cached.chunks.is_empty() {
            return Err(invalid_audio("A cache de voz não tem blocos."));
        }
        let first = cached.chunks[0].path.clone();
        let audio = Arc::clone(&self.audio);
        let id = audio_task(move || audio.start_playback(first)).await?;
        self.lock_article().playback_id = Some(id.clone());
        self.clear_star_playback();
        for chunk in cached.chunks.iter().skip(1) {
            let path = chunk.path.clone();
            let id2 = id.clone();
            let audio = Arc::clone(&self.audio);
            audio_task(move || audio.append_chunk(path, &id2)).await?;
        }
        let duration = cached.total_duration_ms();
        let generation = self.lock_article().player.generation();
        self.apply_player_event(
            generation,
            PlayerEvent::Completed {
                duration_ms: duration,
            },
        );
        Ok(())
    }

    /// Reproduz a gravação original de uma resposta, à velocidade gravada.
    async fn play_user_recording(&self, session_id: &str, turn_id: &str) -> Result<(), AppError> {
        let path = self
            .session_dir(session_id)
            .join(turn_id)
            .join(crate::audio::ORIGINAL_FILE);
        let file = path.clone();
        if !storage_task(move || Ok(file.is_file())).await? {
            return Err(artifact_missing(
                "O ficheiro de áudio desta resposta não foi encontrado.",
            ));
        }
        let duration = {
            let path = path.clone();
            storage_task(move || wav_duration_ms(&path)).await?
        };
        let audio = Arc::clone(&self.audio);
        let id = audio_task(move || audio.start_playback(path)).await?;
        self.lock_article().playback_id = Some(id);
        self.clear_star_playback();
        let generation = self.lock_article().player.generation();
        self.apply_player_event(
            generation,
            PlayerEvent::Completed {
                duration_ms: duration,
            },
        );
        Ok(())
    }

    /// Conclui uma síntese incremental: escreve o manifesto e completa a
    /// reprodução, ou marca a execução incompleta sem perder o texto.
    async fn complete_speech(&self, cancelled: bool) {
        let speech = self.lock_article().speech.take();
        let Some(speech) = speech else {
            return;
        };
        let accepted = speech.sequence.accepted();
        if accepted == 0 || cancelled {
            self.fail_player_synthesis();
            return;
        }
        let directory = speech.sequence.directory().to_path_buf();
        let text = speech.text.clone();
        let voice = speech.voice.clone();
        let speed_x10 = speech.speed_x10;
        let engine = speech.engine.clone();
        let chunks: Vec<(u32, u64)> = (0..accepted).map(|index| (index, 0)).collect();
        let written = storage_task(move || {
            finish_entry(&directory, &text, &voice, speed_x10, &engine, &chunks)
        })
        .await;
        let duration = match written {
            Ok(()) => {
                let session_dir = self.session_dir(&speech.session_id);
                let key = speech.key.clone();
                storage_task(move || {
                    Ok(read_cache(&session_dir, &key).map(|entry| entry.total_duration_ms()))
                })
                .await
                .ok()
                .flatten()
            }
            Err(_) => None,
        };
        match duration {
            Some(duration) => {
                let generation = self.lock_article().player.generation();
                self.apply_player_event(
                    generation,
                    PlayerEvent::Completed {
                        duration_ms: duration,
                    },
                );
            }
            None => self.fail_player_synthesis(),
        }
    }

    /// Marca a síntese como falhada; o texto do tutor continua visível.
    fn fail_player_synthesis(&self) {
        let generation = self.lock_article().player.generation();
        self.apply_player_event(
            generation,
            PlayerEvent::Failed {
                operation: PlayerOperation::Synthesis,
                retryable: true,
            },
        );
    }

    /// Falha da síntese: cancela o auxiliar, esquece a sequência e marca a
    /// reprodução `failed` sem perder o texto.
    async fn fail_speech_after_error(&self, error: &AppError) {
        if let Some(control) = self.worker_control() {
            let _ = worker_task(move || control.cancel()).await;
        }
        self.lock_article().speech = None;
        self.record_error(error);
        self.fail_player_synthesis();
    }

    // -- controlo público da reprodução -----------------------------------------

    /// Reproduz uma fala do tutor ou a gravação de uma resposta, do início.
    pub async fn article_player_play(
        &self,
        session_id: &str,
        turn_id: &str,
    ) -> Result<ArticlePlayerView, AppError> {
        let started = Instant::now();
        let _transition = self.transitions.lock().await;
        let result = self.play_turn_locked(session_id, turn_id).await;
        let code = match &result {
            Ok(_) => "OK",
            Err(error) => error.code.as_str(),
        };
        record_metric("article_play_start", code, started);
        result
    }

    /// Seleção interna de um turno; o chamador detém o coordenador serial.
    async fn play_turn_locked(
        &self,
        session_id: &str,
        turn_id: &str,
    ) -> Result<ArticlePlayerView, AppError> {
        self.ensure_audio_free_for_article()?;
        let snapshot = self.article_snapshot(session_id).await?;
        let turn = snapshot
            .turns
            .iter()
            .find(|turn| turn.id == turn_id)
            .ok_or_else(|| stale_source("Esta fala não pertence à conversa ativa."))?;
        let version_id = (!turn.version.id.is_empty()).then(|| turn.version.id.clone());
        match turn.speaker {
            ArticleSpeaker::User if turn.audio_available => {
                self.stop_article_speech_locked().await;
                self.select_player(
                    session_id,
                    turn_id,
                    version_id,
                    PlayerSource::UserRecording,
                    PlaybackTrigger::Explicit,
                );
                self.play_user_recording(session_id, turn_id).await?;
            }
            ArticleSpeaker::User => {
                return Err(stale_source(
                    "Esta resposta não tem uma gravação para ouvir na conversa ativa.",
                ));
            }
            ArticleSpeaker::Tutor => {
                let preferences = self.article_preferences_load().await?;
                self.play_tutor_turn(
                    session_id,
                    turn_id,
                    version_id,
                    &turn.version.text,
                    preferences.speed_x10,
                    PlaybackTrigger::Explicit,
                )
                .await?;
            }
        }
        Ok(self.current_player_view())
    }

    pub async fn article_player_pause(
        &self,
        session_id: &str,
        generation: u64,
    ) -> Result<ArticlePlayerView, AppError> {
        let started = Instant::now();
        let _transition = self.transitions.lock().await;
        let (matches, id) = {
            let runtime = self.lock_article();
            let view = runtime.player.view();
            (
                view.session_id == session_id && view.generation == generation,
                runtime.playback_id.clone(),
            )
        };
        if !matches {
            record_metric("article_play_stop", "STALE_SOURCE", started);
            return Ok(self.current_player_view());
        }
        if let Some(id) = id {
            let audio = Arc::clone(&self.audio);
            if let Err(error) = audio_task(move || audio.pause_playback(&id)).await {
                self.record_error(&error);
            }
        }
        self.apply_player_event(generation, PlayerEvent::Pause);
        Ok(self.current_player_view())
    }

    pub async fn article_player_resume(
        &self,
        session_id: &str,
        generation: u64,
    ) -> Result<ArticlePlayerView, AppError> {
        let started = Instant::now();
        let _transition = self.transitions.lock().await;
        let (matches, id) = {
            let runtime = self.lock_article();
            let view = runtime.player.view();
            (
                view.session_id == session_id && view.generation == generation,
                runtime.playback_id.clone(),
            )
        };
        if !matches {
            record_metric("article_play_stop", "STALE_SOURCE", started);
            return Ok(self.current_player_view());
        }
        if let Some(id) = id {
            let audio = Arc::clone(&self.audio);
            if let Err(error) = audio_task(move || audio.resume_playback(&id)).await {
                self.record_error(&error);
            }
        }
        self.apply_player_event(generation, PlayerEvent::Resume);
        // Retomar pode reabrir a fila dos blocos aceites mas ainda não tocados.
        if let Err(error) = self.advance_speech_playback().await {
            self.record_error(&error);
            self.fail_player_synthesis();
        }
        Ok(self.current_player_view())
    }

    pub async fn article_player_stop(
        &self,
        session_id: &str,
        generation: u64,
    ) -> Result<ArticlePlayerView, AppError> {
        let started = Instant::now();
        let _transition = self.transitions.lock().await;
        let matches = {
            let runtime = self.lock_article();
            let view = runtime.player.view();
            view.session_id == session_id && view.generation == generation
        };
        if !matches {
            record_metric("article_play_stop", "STALE_SOURCE", started);
            return Ok(self.current_player_view());
        }
        self.stop_article_speech_locked().await;
        record_metric("article_play_stop", "OK", started);
        Ok(self.current_player_view())
    }

    /// Regista a sessão e o token da vista focada; uma sessão diferente ao
    /// tomar o foco pára a reprodução anterior e a sua intenção automática.
    pub async fn article_view_focus(&self, session_id: &str, token: &str) -> Result<(), AppError> {
        let _transition = self.transitions.lock().await;
        let previous = {
            let mut runtime = self.lock_article();
            let previous = runtime.focus.as_ref().map(|(session, _)| session.clone());
            runtime.focus = Some((session_id.to_owned(), token.to_owned()));
            previous
        };
        if previous
            .as_deref()
            .is_some_and(|session| session != session_id)
        {
            let player_session = self.lock_article().player.view().session_id;
            if player_session != session_id {
                self.stop_article_speech_locked().await;
            }
        }
        Ok(())
    }

    /// Limpa o foco apenas quando o token coincide, para que uma desmontagem
    /// atrasada não apague a vista que a substituiu.
    pub async fn article_view_blur(&self, token: &str) -> Result<(), AppError> {
        let _transition = self.transitions.lock().await;
        let mut runtime = self.lock_article();
        if runtime
            .focus
            .as_ref()
            .is_some_and(|(_, stored)| stored == token)
        {
            runtime.focus = None;
        }
        Ok(())
    }

    // -- eventos de reprodução ---------------------------------------------------

    /// Progresso audível reportado pela porta de áudio (≤4 Hz).
    pub(super) async fn article_playback_progress(
        &self,
        id: &crate::audio::PlaybackId,
        progress: PlaybackProgress,
    ) {
        let generation = {
            let runtime = self.lock_article();
            if runtime.playback_id.as_deref() != Some(id.as_str()) {
                return;
            }
            runtime.player.generation()
        };
        self.apply_player_event(
            generation,
            PlayerEvent::Progress {
                position_ms: progress.position_ms,
                buffered_ms: progress.buffered_ms,
            },
        );
    }

    /// Fim momentâneo da fila: `Buffering` enquanto a síntese decorre, `Ended`
    /// quando a execução está completa.
    pub(super) async fn article_playback_finished(&self, id: &crate::audio::PlaybackId) {
        let generation = {
            let mut runtime = self.lock_article();
            if runtime.playback_id.as_deref() != Some(id.as_str()) {
                return;
            }
            runtime.playback_id = None;
            runtime.player.generation()
        };
        self.apply_player_event(generation, PlayerEvent::Drained);
    }

    /// Encaminha mensagens do pedido de voz; `true` se pertencem a ele.
    async fn route_speech_message(
        &self,
        generation: u64,
        request_id: &str,
        message: &WorkerMessage,
    ) -> bool {
        let owned = self
            .lock_article()
            .speech
            .as_ref()
            .is_some_and(|speech| speech.request_id == request_id);
        if !owned {
            return false;
        }
        let current = generation != 0 && generation == self.generation.load(Ordering::SeqCst);
        match message {
            WorkerMessage::Progress { .. } => {}
            WorkerMessage::AudioChunk {
                index, audio_path, ..
            } => {
                if !current {
                    return true;
                }
                let verdict = {
                    let mut runtime = self.lock_article();
                    runtime
                        .speech
                        .as_mut()
                        .map(|speech| speech.sequence.accept(*index, audio_path))
                };
                match verdict {
                    Some(Ok(_)) => {
                        let started = {
                            let mut runtime = self.lock_article();
                            let Some(speech) = runtime.speech.as_mut() else {
                                return true;
                            };
                            if speech.first_reported {
                                None
                            } else {
                                speech.first_reported = true;
                                Some(speech.started)
                            }
                        };
                        if let Some(started) = started {
                            record_metric("article_first_audio", "OK", started);
                        }
                        // Em pausa os blocos são aceites mas não tocam; retomar
                        // entrega-os depois.
                        let paused = self.lock_article().player.view().state == PlayerState::Paused;
                        if !paused {
                            if let Err(error) = self.advance_speech_playback().await {
                                self.fail_speech_after_error(&error).await;
                            }
                        }
                    }
                    // Repetição de um bloco já entregue: inofensiva.
                    Some(Err(ChunkRejection::Duplicate)) => {}
                    Some(Err(_)) => {
                        let error = AppError::new(
                            error_code::WORKER_PROTOCOL,
                            "O auxiliar enviou um bloco de voz inválido.",
                            false,
                        );
                        self.fail_speech_after_error(&error).await;
                    }
                    None => {}
                }
            }
            WorkerMessage::Completed {
                result: WorkerResult::SynthesisStream(done),
                ..
            } => {
                if !current {
                    self.lock_article().speech = None;
                    return true;
                }
                self.complete_speech(done.cancelled).await;
            }
            WorkerMessage::Completed { .. } => {
                let error = AppError::new(
                    error_code::WORKER_PROTOCOL,
                    "O auxiliar devolveu um resultado inesperado para a síntese.",
                    false,
                );
                self.fail_speech_after_error(&error).await;
            }
            WorkerMessage::Failed { error, .. } => {
                self.fail_speech_after_error(error).await;
            }
        }
        true
    }

    async fn fail_article(&self, database: &Database, session_id: &str, error: &AppError) {
        let db = database.clone();
        let id = session_id.to_owned();
        let failure = article_error(error);
        let failed = database_task(move || {
            db.apply_article_system(&id, ArticleMutation::Fail { error: failure })
        })
        .await;
        if let Ok(snapshot) = failed {
            self.publish_article(&snapshot);
        }
    }

    // -- repetir, terminar, eliminar ---------------------------------------------

    pub async fn article_retry(
        self: &Arc<Self>,
        command: ArticleCommand,
    ) -> Result<ArticleSnapshot, AppError> {
        let _transition = self.transitions.lock().await;
        let database = self.database()?;
        let snapshot =
            database_task(move || database.apply_article(&command, ArticleMutation::Retry)).await?;
        self.publish_article(&snapshot);
        self.dispatch_request_if_due(&snapshot);
        Ok(snapshot)
    }

    /// Corrige uma resposta de uma conversa concluída num ramo novo. Não envia
    /// nada: o relatório novo só é pedido por `article_retry`.
    pub async fn article_correct(
        &self,
        correction: ArticleCorrection,
    ) -> Result<ArticleSnapshot, AppError> {
        let _transition = self.transitions.lock().await;
        let database = self.database()?;
        let change = ArticleMutation::Correct {
            turn_id: correction.turn_id,
            text: correction.text,
        };
        let command = correction.command;
        let snapshot = database_task(move || database.apply_article(&command, change)).await?;
        self.publish_article(&snapshot);
        Ok(snapshot)
    }

    /// Para a voz do tutor sem perder texto nem estado. Compatibilidade.
    pub async fn article_stop_voice(&self) -> Result<(), AppError> {
        let _transition = self.transitions.lock().await;
        self.stop_article_speech_locked().await;
        Ok(())
    }

    /// Repete a voz de uma fala do tutor, sem nova chamada ao fornecedor de
    /// texto. Compatibilidade: delega a reprodução no novo leitor.
    pub async fn article_speak_turn(
        &self,
        session_id: &str,
        turn_id: &str,
    ) -> Result<(), AppError> {
        let _transition = self.transitions.lock().await;
        self.play_turn_locked(session_id, turn_id).await.map(|_| ())
    }

    /// Ouve a gravação guardada de uma resposta. Compatibilidade: só gravações
    /// do utilizador; delega a reprodução no novo leitor.
    pub async fn article_play_turn(&self, session_id: &str, turn_id: &str) -> Result<(), AppError> {
        let _transition = self.transitions.lock().await;
        let snapshot = self.article_snapshot(session_id).await?;
        let playable = snapshot.turns.iter().any(|turn| {
            turn.id == turn_id && turn.audio_available && turn.speaker == ArticleSpeaker::User
        });
        if !playable {
            return Err(stale_source(
                "Esta resposta não tem uma gravação para ouvir na conversa ativa.",
            ));
        }
        self.play_turn_locked(session_id, turn_id).await.map(|_| ())
    }

    /// Texto escrito pelo utilizador quando a transcrição falhou ou excedeu o
    /// limite: versão distinta da automática; o envio segue como numa resposta.
    pub async fn article_submit_text(
        self: &Arc<Self>,
        correction: ArticleCorrection,
    ) -> Result<ArticleSnapshot, AppError> {
        let _transition = self.transitions.lock().await;
        let database = self.database()?;
        let change = ArticleMutation::ManualText {
            turn_id: correction.turn_id,
            text: correction.text,
        };
        let command = correction.command;
        let snapshot = database_task(move || database.apply_article(&command, change)).await?;
        self.publish_article(&snapshot);
        self.dispatch_request_if_due(&snapshot);
        Ok(snapshot)
    }

    /// Recupera o estado local depois de um reinício, sem HTTP nem envios.
    pub async fn article_resume(
        &self,
        command: ArticleCommand,
    ) -> Result<ArticleSnapshot, AppError> {
        let _transition = self.transitions.lock().await;
        let database = self.database()?;
        let live = {
            let mut runtime = self.lock_article();
            runtime.tasks.retain(|t| !t.is_finished());
            runtime
                .capture
                .as_ref()
                .is_some_and(|c| c.session_id == command.session_id)
                || runtime
                    .transcription
                    .as_ref()
                    .is_some_and(|t| t.session_id == command.session_id)
                || !runtime.tasks.is_empty()
        };
        let id = command.session_id;
        let snapshot = if live {
            database_task(move || database.article_snapshot(&id)).await?
        } else {
            database_task(move || database.recover_article(&id)).await?
        };
        self.publish_article(&snapshot);
        Ok(snapshot)
    }

    /// Limpeza das conversas eliminadas cujo apagamento de ficheiros ficou a meio.
    pub(super) async fn resume_article_cleanup(&self, database: &Database, sessions: Vec<String>) {
        for session_id in sessions {
            self.clean_deleted_article(database, &session_id).await;
        }
    }

    pub async fn article_finish(
        self: &Arc<Self>,
        command: ArticleCommand,
    ) -> Result<ArticleSnapshot, AppError> {
        let _transition = self.transitions.lock().await;
        // Terminar invalida a reprodução e a intenção automática antes da base.
        self.stop_article_speech_locked().await;
        let database = self.database()?;
        let db = database.clone();
        let session_id = command.session_id.clone();
        let mut snapshot =
            database_task(move || db.apply_article(&command, ArticleMutation::Finish)).await?;
        if snapshot.session.state == ArticleState::Finishing && snapshot.session.answers == 0 {
            // Sem respostas não há o que avaliar: conclui sem relatório fabricado.
            let db = database.clone();
            snapshot = database_task(move || {
                db.apply_article_system(&session_id, ArticleMutation::Complete)
            })
            .await?;
        }
        self.publish_article(&snapshot);
        self.dispatch_request_if_due(&snapshot);
        Ok(snapshot)
    }

    /// Eliminação lógica seguida da limpeza repetível dos ficheiros.
    pub async fn article_delete(&self, command: ArticleCommand) -> Result<(), AppError> {
        let _transition = self.transitions.lock().await;
        // Eliminar invalida a reprodução e a intenção automática antes da base.
        self.stop_article_speech_locked().await;
        let database = self.database()?;
        let session_id = command.session_id.clone();
        let db = database.clone();
        database_task(move || db.apply_article(&command, ArticleMutation::Delete)).await?;
        {
            let mut runtime = self.lock_article();
            if runtime
                .transcription
                .as_ref()
                .is_some_and(|t| t.session_id == session_id)
            {
                runtime.transcription = None;
            }
        }
        self.set_article_busy();
        self.clean_deleted_article(&database, &session_id).await;
        Ok(())
    }

    /// Fecho da aplicação: a gravação de um artigo termina com o áudio guardado
    /// e a resposta fica por rever; uma transcrição pendente deixa de ser esperada.
    pub(super) async fn close_article_audio(&self) {
        self.stop_article_speech_locked().await;
        let (capture, transcription) = {
            let mut runtime = self.lock_article();
            (runtime.capture.take(), runtime.transcription.take())
        };
        let Ok(database) = self.database() else {
            return;
        };
        if let Some(capture) = &capture {
            let audio = Arc::clone(&self.audio);
            let _ = audio_task(move || audio.stop_capture()).await;
            let db = database.clone();
            let session_id = capture.session_id.clone();
            let _ = database_task(move || {
                db.apply_article_system(&session_id, ArticleMutation::StopRecording)
            })
            .await;
        }
        let pending = capture
            .map(|c| (c.session_id, c.turn_id))
            .or_else(|| transcription.map(|t| (t.session_id, t.turn_id)));
        if let Some((session_id, turn_id)) = pending {
            let _ = database_task(move || {
                database.apply_article_system(
                    &session_id,
                    ArticleMutation::RecordTranscript {
                        turn_id,
                        text: None,
                    },
                )
            })
            .await;
        }
        self.set_article_busy();
    }

    /// Remove os ficheiros pendentes e, sem pendentes, as linhas da conversa.
    pub(super) async fn clean_deleted_article(&self, database: &Database, session_id: &str) {
        let base = self.articles_base();
        let db = database.clone();
        let id = session_id.to_owned();
        let _ = database_task(move || {
            for path in db.pending_article_artifacts(&id)? {
                let file = base.join(&path);
                match fs::remove_file(&file) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    // Não removido: fica pendente para a próxima limpeza.
                    Err(_) => continue,
                }
                db.mark_article_artifact_removed(&path)?;
                if let Some(directory) = file.parent() {
                    let _ = fs::remove_dir(directory);
                }
            }
            let _ = fs::remove_dir(base.join("articles").join(&id));
            let purged = db.purge_article_if_clean(&id)?;
            if purged {
                // Pasta exclusiva da conversa (inclui os blocos de voz temporários).
                let _ = fs::remove_dir_all(base.join("articles").join(&id));
            }
            Ok(())
        })
        .await;
    }
}

fn reply_outcome(reply: ArticleReply) -> ArticleOutcome {
    ArticleOutcome::Reply {
        comment: reply.comment,
        question: reply.question,
    }
}

#[allow(dead_code)]
const _NOTICE: &str = ARTICLE_NOTICE_VERSION;

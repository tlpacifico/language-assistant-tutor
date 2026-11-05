//! Orquestração da conversa sobre artigos.
//!
//! O núcleo é a autoridade: cada comando valida o estado persistido, guarda
//! antes de enviar e só depois lança trabalho externo. A gravação termina
//! apenas por `article_stop_recording`; o envio ao fornecedor acontece depois
//! de a transcrição estar guardada e com a autorização desta sessão válida.
//! Um resultado só se aplica ao pedido e à geração que o originaram.
//!
//! Captura, transcrição e pedidos HTTP deste modo têm estado próprio
//! ([`ArticleRuntime`]); não passam pelas gravações nem pelos answer slots STAR.

use std::fs;
use std::path::PathBuf;
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
    can_send_automatically, ArticleCommand, ArticleConsent, ArticleCorrection, ArticleCreate,
    ArticleError, ArticleMutation, ArticleOutcome, ArticleRequestKind, ArticleSnapshot,
    ArticleSpeaker, ArticleState, ArticleSummary, ARTICLE_NOTICE_VERSION,
};
use crate::article::{ArticleAudioView, ArticlePlayback, ChunkRejection, ChunkSequence};
use crate::audio::RecordingTarget;
use crate::worker::protocol::{SynthesizeStreamParams, TranscribeParams};

/// Destinatário das fotografias publicadas em `article://snapshot`.
pub type ArticleListener = Arc<dyn Fn(&ArticleSnapshot) + Send + Sync>;

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

/// Voz do tutor em curso: blocos aceites só pela ordem e na pasta do pedido.
#[derive(Debug, Clone)]
pub(super) struct ArticleSpeech {
    request_id: String,
    sequence: ChunkSequence,
    started: Instant,
    first_reported: bool,
}

/// Estado em memória do modo artigo.
#[derive(Default)]
pub(super) struct ArticleRuntime {
    capture: Option<ArticleCapture>,
    transcription: Option<ArticleTranscription>,
    speech: Option<ArticleSpeech>,
    /// Estado de reprodução da voz, independente do estado da conversa.
    playback: Option<(String, String, ArticlePlayback)>,
    /// A fila de áudio esvaziou enquanto a síntese ainda decorria.
    drained: bool,
    tasks: Vec<JoinHandle<()>>,
    this: Option<Weak<AppService>>,
}

impl ArticleRuntime {
    fn busy(&self) -> bool {
        self.capture.is_some() || self.transcription.is_some()
    }
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

    /// Guarda a referência ao próprio serviço para lançar pedidos em segundo plano.
    pub(super) fn remember_self(self: &Arc<Self>) {
        self.lock_article().this = Some(Arc::downgrade(self));
    }

    /// Junta à fotografia persistida o estado (em memória) da voz.
    fn with_audio(&self, mut snapshot: ArticleSnapshot) -> ArticleSnapshot {
        if let Some((session, turn, playback)) = self.lock_article().playback.clone() {
            if session == snapshot.session.id {
                snapshot.audio = ArticleAudioView {
                    playback,
                    turn_id: Some(turn),
                };
            }
        }
        snapshot
    }

    fn publish_article(&self, snapshot: &ArticleSnapshot) {
        let listener = self.lock_article_listener().clone();
        if let Some(listener) = listener {
            listener(&self.with_audio(snapshot.clone()));
        }
    }

    async fn republish_article(&self, session_id: &str) {
        if let Ok(snapshot) = self.article_snapshot(session_id).await {
            self.publish_article(&snapshot);
        }
    }

    fn set_playback(&self, session_id: &str, turn_id: &str, playback: Option<ArticlePlayback>) {
        self.lock_article().playback =
            playback.map(|p| (session_id.to_owned(), turn_id.to_owned(), p));
    }

    /// A pasta de dados que contém `articles/`: a da base de dados.
    fn articles_base(&self) -> PathBuf {
        self.database_path
            .parent()
            .map(PathBuf::from)
            .unwrap_or_default()
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
        let _ = audio_task(move || audio.stop_playback()).await;
        self.commit(|state| state.playback = None);

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
                    let (id, text) = (turn.id.clone(), turn.version.text.clone());
                    let _transition = self.transitions.lock().await;
                    self.start_article_speech_locked(&snapshot.session.id, &id, &text)
                        .await;
                }
            }
        }
    }

    // -- voz do tutor -----------------------------------------------------------

    /// Lança a síntese incremental de uma fala do tutor. Falhas e ocupação não
    /// perdem texto: a voz fica `failed` e pode repetir-se sem nova chamada IA.
    async fn start_article_speech_locked(&self, session_id: &str, turn_id: &str, text: &str) {
        if text.trim().is_empty() || self.ensure_audio_free_for_article().is_err() {
            return;
        }
        self.stop_article_speech_locked().await;
        let fail = |this: &Self| {
            this.set_playback(session_id, turn_id, Some(ArticlePlayback::Failed));
        };
        let Some(control) = self.worker_control().filter(|c| c.is_running()) else {
            fail(self);
            self.republish_article(session_id).await;
            return;
        };
        if self.generation.load(Ordering::SeqCst) == 0 {
            fail(self);
            self.republish_article(session_id).await;
            return;
        }
        let request_id = format!("voice-{}", uuid::Uuid::new_v4());
        let directory = self
            .articles_base()
            .join("articles")
            .join(session_id)
            .join(&request_id);
        let created = directory.clone();
        let prepared = storage_task(move || {
            fs::create_dir_all(&created).map_err(|error| {
                AppError::new(
                    error_code::STORAGE_WRITE_FAILED,
                    format!("Não foi possível preparar a pasta da voz: {error}."),
                    true,
                )
            })
        })
        .await;
        let request = prepared.and_then(|()| {
            WorkerRequest::synthesize_stream(
                request_id.clone(),
                SynthesizeStreamParams::new(text.trim(), &directory),
            )
        });
        let Ok(request) = request else {
            fail(self);
            self.republish_article(session_id).await;
            return;
        };
        self.lock_article().speech = Some(ArticleSpeech {
            request_id,
            sequence: ChunkSequence::new(directory),
            started: Instant::now(),
            first_reported: false,
        });
        self.set_playback(session_id, turn_id, Some(ArticlePlayback::Playing));
        if let Err(error) = worker_task(move || control.submit(request)).await {
            self.lock_article().speech = None;
            fail(self);
            self.record_error(&error);
        }
        self.republish_article(session_id).await;
    }

    /// Pára a voz: limpa a fila, invalida a sequência (blocos tardios são
    /// descartados) e pede ao auxiliar que pare entre blocos.
    async fn stop_article_speech_locked(&self) {
        let (speech, playing) = {
            let mut runtime = self.lock_article();
            let playing = matches!(runtime.playback, Some((_, _, ArticlePlayback::Playing)));
            let speech = runtime.speech.take();
            (speech, playing)
        };
        if speech.is_none() && !playing {
            return;
        }
        if speech.is_some() {
            if let Some(control) = self.worker_control() {
                let _ = worker_task(move || control.cancel()).await;
            }
        }
        let audio = Arc::clone(&self.audio);
        let _ = audio_task(move || audio.stop_playback()).await;
        let session = {
            let mut runtime = self.lock_article();
            let session = runtime.playback.as_ref().map(|(s, _, _)| s.clone());
            runtime.playback = None;
            session
        };
        if let Some(session) = session {
            self.republish_article(&session).await;
        }
    }

    pub async fn article_stop_voice(&self) -> Result<(), AppError> {
        let _transition = self.transitions.lock().await;
        self.stop_article_speech_locked().await;
        Ok(())
    }

    /// Repete a voz de uma fala do tutor, sem nova chamada ao fornecedor de texto.
    pub async fn article_speak_turn(
        &self,
        session_id: &str,
        turn_id: &str,
    ) -> Result<(), AppError> {
        let _transition = self.transitions.lock().await;
        let snapshot = self.article_snapshot(session_id).await?;
        let Some(turn) = snapshot
            .turns
            .iter()
            .find(|t| t.id == turn_id && t.speaker == ArticleSpeaker::Tutor)
        else {
            return Err(stale_source(
                "Esta fala do tutor não pertence à conversa ativa.",
            ));
        };
        self.ensure_audio_free_for_article()?;
        let text = turn.version.text.clone();
        self.start_article_speech_locked(session_id, turn_id, &text)
            .await;
        Ok(())
    }

    /// Fim momentâneo da fila não é fim do pedido: só passa a `idle` quando a
    /// síntese terminou e a fila esvaziou.
    pub(super) async fn article_playback_finished(&self) {
        let session = {
            let mut runtime = self.lock_article();
            if runtime.speech.is_some() {
                runtime.drained = true;
                return;
            }
            match runtime.playback.clone() {
                Some((session, _, ArticlePlayback::Playing)) => {
                    runtime.playback = None;
                    Some(session)
                }
                _ => None,
            }
        };
        if let Some(session) = session {
            self.republish_article(&session).await;
        }
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
            .is_some_and(|s| s.request_id == request_id);
        if !owned {
            return false;
        }
        let current = generation != 0 && generation == self.generation.load(Ordering::SeqCst);
        match message {
            WorkerMessage::Progress { .. } => {}
            WorkerMessage::AudioChunk {
                index, audio_path, ..
            } => {
                let verdict = {
                    let mut runtime = self.lock_article();
                    runtime
                        .speech
                        .as_mut()
                        .map(|s| s.sequence.accept(*index, audio_path))
                };
                match verdict {
                    // Repetição de um bloco já entregue: inofensiva.
                    Some(Err(ChunkRejection::Duplicate)) => {}
                    Some(Ok(path)) if current => {
                        let audio = Arc::clone(&self.audio);
                        match audio_task(move || audio.append(path)).await {
                            Ok(()) => {
                                let mut runtime = self.lock_article();
                                runtime.drained = false;
                                if let Some(speech) = runtime.speech.as_mut() {
                                    if !speech.first_reported {
                                        speech.first_reported = true;
                                        record_metric("article_first_audio", "OK", speech.started);
                                    }
                                }
                            }
                            Err(error) => self.fail_article_speech(&error).await,
                        }
                    }
                    _ => {
                        let error = AppError::new(
                            error_code::WORKER_PROTOCOL,
                            "O auxiliar enviou um bloco de voz inválido.",
                            false,
                        );
                        self.fail_article_speech(&error).await;
                    }
                }
            }
            WorkerMessage::Completed {
                result: WorkerResult::SynthesisStream(done),
                ..
            } => {
                let drained = {
                    let mut runtime = self.lock_article();
                    runtime.speech = None;
                    runtime.drained
                };
                if done.chunks == 0 && !done.cancelled {
                    self.mark_speech_failed().await;
                } else if drained {
                    // A fila já tinha esvaziado antes do resultado final.
                    self.article_playback_finished().await;
                }
            }
            WorkerMessage::Completed { .. } => {
                self.lock_article().speech = None;
                self.mark_speech_failed().await;
            }
            WorkerMessage::Failed { error, .. } => {
                self.fail_article_speech(error).await;
            }
        }
        true
    }

    /// Sem blocos, o texto continua visível e a voz pode repetir-se.
    async fn fail_article_speech(&self, error: &AppError) {
        if let Some(control) = self.worker_control() {
            let _ = worker_task(move || control.cancel()).await;
        }
        self.lock_article().speech = None;
        self.record_error(error);
        self.mark_speech_failed().await;
    }

    async fn mark_speech_failed(&self) {
        let session = {
            let mut runtime = self.lock_article();
            let Some((session, turn, _)) = runtime.playback.clone() else {
                return;
            };
            runtime.playback = Some((session.clone(), turn, ArticlePlayback::Failed));
            session
        };
        self.republish_article(&session).await;
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

    /// Ouve a gravação guardada de uma resposta. Não toca em estado nem em rede;
    /// recusa-se durante uma gravação ou transcrição.
    pub async fn article_play_turn(&self, session_id: &str, turn_id: &str) -> Result<(), AppError> {
        let _transition = self.transitions.lock().await;
        self.ensure_audio_free_for_article()?;
        let snapshot = self.article_snapshot(session_id).await?;
        let playable = snapshot
            .turns
            .iter()
            .any(|t| t.id == turn_id && t.audio_available && t.speaker == ArticleSpeaker::User);
        if !playable {
            return Err(stale_source(
                "Esta resposta não tem uma gravação para ouvir na conversa ativa.",
            ));
        }
        let path = self
            .articles_base()
            .join("articles")
            .join(session_id)
            .join(turn_id)
            .join(crate::audio::ORIGINAL_FILE);
        let file = path.clone();
        if !storage_task(move || Ok(file.is_file())).await? {
            return Err(artifact_missing(
                "O ficheiro de áudio desta resposta não foi encontrado.",
            ));
        }
        let audio = Arc::clone(&self.audio);
        audio_task(move || audio.play(path)).await
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

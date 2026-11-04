//! Rede fora do coordenador; publicação transacional e repetição sem novo custo.
use super::*;
use crate::ai::*;
use crate::db::ai::ReservedAttempt;
use std::collections::HashMap;
#[derive(Default)]
pub(super) struct AiRuntime {
    pub busy: bool,
    pub pending: HashMap<Id, (ReservedAttempt, crate::ai::Result<RemoteCompletion>)>,
}
impl AppService {
    async fn question_request_replayed(
        &self,
        c: &AiQuestionCommand,
        purpose: &'static str,
    ) -> Result<bool> {
        let db = self.database()?;
        let c = c.clone();
        database_task(move || {
            db.with_connection(|db| {
                use crate::db::ai::{
                    lookup_command_identity, CommandIdentity, CommandIdentityLocation,
                };
                if c.request_id.trim().is_empty() {
                    return Err(failure("Pedido sem identificador."));
                }
                let expected = crate::db::ai::hash(&(&c, purpose))?;
                match lookup_command_identity(db, &c.request_id)? {
                    CommandIdentity::Found {
                        location: CommandIdentityLocation::AiAttempt,
                        payload_hash,
                        ..
                    } if payload_hash == expected => Ok(true),
                    CommandIdentity::NotFound => Ok(false),
                    _ => Err(failure("Identificador reutilizado com conteúdo diferente.")),
                }
            })
        })
        .await
    }
    pub(super) async fn resolve_answer_target(
        &self,
        mut target: AnswerTarget,
    ) -> Result<AnswerTarget> {
        if let Some(id) = target.answer_id.clone() {
            let main = target.session_question_id.clone();
            let db = self.database()?;
            let saved = id.clone();
            let belongs = database_task(move || {
                db.with_connection(|c| {
                    c.query_row(
                        "SELECT EXISTS(SELECT 1
 FROM answer_slots
 WHERE id=?1
 AND session_question_id=?2)",
                        rusqlite::params![saved, main],
                        |r| r.get::<_, bool>(0),
                    )
                    .map_err(crate::db::ai::sql_error)
                })
            })
            .await?;
            if !belongs {
                return Err(failure("A resposta não pertence à pergunta indicada."));
            }
            target.session_question_id = id;
            target.answer_id = None;
        }
        Ok(target)
    }
    pub(super) fn ai_busy(&self) -> bool {
        if self.ai_closed.load(Ordering::SeqCst) {
            return true;
        }
        let r = self.ai_runtime.lock().unwrap_or_else(|p| p.into_inner());
        r.busy || !r.pending.is_empty()
    }
    pub fn with_ai(
        mut self,
        transport: Arc<dyn AiTransport>,
        credentials: Arc<dyn CredentialStore>,
    ) -> Self {
        self.ai_transport = transport;
        self.credentials = credentials;
        self
    }
    pub async fn load_ai_settings(&self) -> Result<AiSettingsView> {
        let db = self.database()?;
        let mut v = database_task(move || db.ai_settings()).await?;
        let store = self.credentials.clone();
        v.credential_configured = database_task(move || store.get().map(|v| v.is_some())).await?;
        Ok(v)
    }
    pub async fn save_ai_settings(&self, request: SaveAiSettings) -> Result<AiSettingsView> {
        let _guard = self.transitions.lock().await;
        let payload = crate::db::ai::hash(&request)?;
        let db = self.database()?;
        let saved = db.clone();
        let id = request.request_id.clone();
        let h = payload.clone();
        if database_task(move || saved.ai_receipt_matches(&id, "settings", &h)).await? {
            return self.load_ai_settings().await;
        }
        let input = validate_settings(request.input)?;
        let old = self.load_ai_settings().await?;
        if old.version != request.expected_version {
            return Err(failure("A configuração mudou; volte a carregá-la."));
        }
        let changed = old.base_url != input.base_url;
        if changed {
            let store = self.credentials.clone();
            database_task(move || store.delete()).await?;
        }
        let v = AiSettingsView {
            version: old.version + 1,
            base_url: input.base_url,
            model: input.model,
            tariff: input.tariff,
            credential_configured: if changed {
                false
            } else {
                old.credential_configured
            },
            consent: if changed { None } else { old.consent },
        };
        let db = self.database()?;
        let saved = v.clone();
        database_task(move || {
            db.put_ai_settings(
                request.expected_version,
                &saved,
                (&request.request_id, "settings", &payload),
            )
        })
        .await?;
        Ok(v)
    }
    pub async fn save_ai_key(&self, key: String) -> Result<AiSettingsView> {
        let _guard = self.transitions.lock().await;
        let key = key.trim().to_owned();
        if key.is_empty() {
            return Err(failure("Introduza uma chave não vazia."));
        }
        let destination = self.load_ai_settings().await?.base_url;
        validate_credential(&destination, &key)?;
        let store = self.credentials.clone();
        database_task(move || store.set(&key)).await?;
        self.load_ai_settings().await
    }
    pub async fn remove_ai_key(&self) -> Result<AiSettingsView> {
        let _guard = self.transitions.lock().await;
        let store = self.credentials.clone();
        database_task(move || store.delete()).await?;
        self.load_ai_settings().await
    }
    pub async fn accept_ai_consent(&self, request: AcceptAiConsent) -> Result<AiSettingsView> {
        let _guard = self.transitions.lock().await;
        let payload = crate::db::ai::hash(&request)?;
        let db = self.database()?;
        let saved = db.clone();
        let id = request.request_id.clone();
        let h = payload.clone();
        if database_task(move || saved.ai_receipt_matches(&id, "consent", &h)).await? {
            return self.load_ai_settings().await;
        }
        let mut v = self.load_ai_settings().await?;
        if request.expected_settings_version != v.version
            || request.destination != v.base_url
            || request.notice_version != "text-external-v1"
        {
            return Err(failure(
                "O destinatário ou aviso mudou. Confirme o aviso atual.",
            ));
        }
        let previous = v.version;
        v.version += 1;
        v.consent = Some(Consent {
            destination: v.base_url.clone(),
            notice_version: request.notice_version,
            accepted_at: crate::db::utc_now(),
        });
        let db = self.database()?;
        let saved = v.clone();
        database_task(move || {
            db.put_ai_settings(previous, &saved, (&request.request_id, "consent", &payload))
        })
        .await?;
        Ok(v)
    }
    pub(super) async fn ai_configuration(&self) -> Result<(AiSettingsView, String)> {
        let v = self.load_ai_settings().await?;
        if !v
            .consent
            .as_ref()
            .is_some_and(|c| c.destination == v.base_url && c.notice_version == "text-external-v1")
        {
            return Err(failure(
                "Autorize o envio de texto para o destinatário atual na configuração IA.",
            ));
        }
        let store = self.credentials.clone();
        let key = database_task(move || store.get())
            .await?
            .ok_or_else(|| failure("Configure primeiro a chave protegida."))?;
        validate_credential(&v.base_url, &key)?;
        Ok((v, key))
    }
    pub async fn request_follow_up(
        self: &Arc<Self>,
        command: AiQuestionCommand,
    ) -> Result<InterviewSnapshot> {
        let _guard = self.transitions.lock().await;
        if self
            .question_request_replayed(&command, "follow_up")
            .await?
        {
            return Ok(self.snapshot());
        }
        if self.ai_busy() {
            return Err(AppError::busy(
                "Existe um pedido IA ou resultado por guardar.",
            ));
        }
        let (settings, key) = self.ai_configuration().await?;
        let db = self.database()?;
        let reserve = db.clone();
        let a = database_task(move || reserve.reserve_ai(&command, "follow_up", &settings)).await?;
        if let Some(a) = a {
            self.ai_runtime
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .busy = true;
            self.ai_job(&a);
            let service = Arc::clone(self);
            tokio::spawn(async move {
                service.run_attempt(db, a, key).await;
                service
                    .ai_runtime
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .busy = false;
                service.commit(|s| s.ai_job = None);
            });
        }
        Ok(self.snapshot())
    }
    pub(super) fn ai_job(&self, a: &ReservedAttempt) {
        self.commit(|s| {
            s.ai_job = Some(AiJobView {
                attempt_id: a.id.clone(),
                purpose: a.purpose.clone(),
                session_id: a.session_id.clone(),
                session_question_id: a.question_id.clone(),
                stage: "requesting".into(),
            })
        });
    }
    pub(super) async fn run_attempt(&self, db: Database, a: ReservedAttempt, key: String) -> bool {
        // A admissão ao transporte é serializada com o fecho, incluindo o
        // intervalo entre reservar um pedido e iniciar a chamada. A guarda
        // termina antes de aguardar a rede.
        let response = {
            let _transition = self.transitions.lock().await;
            if self.ai_closed.load(Ordering::SeqCst) {
                return false;
            }
            self.ai_transport.complete(TransportRequest {
                settings: a.settings.clone(),
                key,
                task: a.task.clone(),
                sources: a.sources.clone(),
                conversation_id: a.practice_id.as_ref().unwrap_or(&a.session_id).clone(),
            })
        };
        let result = response.await;
        let _guard = self.transitions.lock().await;
        if self.ai_closed.load(Ordering::SeqCst) {
            return false;
        }
        self.commit(|s| {
            if let Some(j) = &mut s.ai_job {
                j.stage = "publishing".into();
            }
        });
        let saved_a = a.clone();
        let saved_result = result.clone();
        let saved_db = db.clone();
        match database_task(move || saved_db.finish_ai(&saved_a, &saved_result)).await {
            Ok(()) => {
                if let Err(e) = self.refresh_session(&db).await {
                    self.record_error(&e);
                }
                if let Err(e) = &result {
                    self.record_error(e);
                }
                result.is_ok() && self.database_report_valid(&db, &a).await
            }
            Err(e) => {
                self.ai_runtime
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .pending
                    .insert(a.id.clone(), (a.clone(), result));
                self.commit(|s| s.pending_ai_publication = Some(a.id));
                self.record_error(&e);
                false
            }
        }
    }
    async fn database_report_valid(&self, db: &Database, a: &ReservedAttempt) -> bool {
        if a.purpose != "feedback" {
            return true;
        }
        let db = db.clone();
        let q = a.question_id.clone();
        database_task(move || {
            db.with_connection(|c| {
                c.query_row(
                    "SELECT EXISTS(SELECT 1 FROM ai_reports WHERE question_id=?1)",
                    [q],
                    |r| r.get::<_, bool>(0),
                )
                .map_err(crate::db::ai::sql_error)
            })
        })
        .await
        .unwrap_or(false)
    }
    pub async fn waive_follow_up(&self, request: WaiveFollowUp) -> Result<InterviewSnapshot> {
        let _guard = self.transitions.lock().await;
        let db = self.database()?;
        let saved = db.clone();
        database_task(move || saved.waive_ai(&request)).await?;
        self.refresh_session(&db).await
    }
    pub async fn retry_ai_publication(&self, id: Id) -> Result<InterviewSnapshot> {
        let _guard = self.transitions.lock().await;
        let (a, result) = self
            .ai_runtime
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .pending
            .get(&id)
            .cloned()
            .ok_or_else(|| failure("Não existe resultado por publicar."))?;
        let db = self.database()?;
        let saved = db.clone();
        database_task(move || saved.finish_ai(&a, &result)).await?;
        self.ai_runtime
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .pending
            .remove(&id);
        self.commit(|s| s.pending_ai_publication = None);
        self.refresh_session(&db).await
    }
    pub async fn load_feedback(&self, id: Id) -> Result<FeedbackView> {
        let db = self.database()?;
        let sid = id.clone();
        let mut view = database_task(move || db.load_feedback(&id)).await?;
        if self
            .snapshot()
            .ai_job
            .as_ref()
            .is_some_and(|j| j.session_id == sid && j.purpose == "feedback")
            && view.state != "ready"
        {
            view.state = "running".into();
        }
        Ok(view)
    }
    /// Avalia uma pergunta com fontes fixas, sem bloquear transições durante a rede.
    /// O comando só termina depois de publicar; o frontend pode então pedir a reformulação.
    pub async fn evaluate_question(
        self: &Arc<Self>,
        command: AiQuestionCommand,
    ) -> Result<FeedbackView> {
        let guard = self.transitions.lock().await;
        let db = self.database()?;
        if self.question_request_replayed(&command, "feedback").await? {
            return self.load_feedback(command.session_id).await;
        }
        let saved = db.clone();
        let c = command.clone();
        database_task(move || saved.ai_sources(&c.session_id, &c.session_question_id, "feedback"))
            .await?;
        let existing = self.load_feedback(command.session_id.clone()).await?;
        if existing
            .reports
            .iter()
            .any(|r| r["questionId"] == command.session_question_id)
        {
            return Ok(existing);
        }
        if self.ai_busy() {
            return Err(AppError::busy(
                "Existe um pedido IA ou resultado por guardar.",
            ));
        }
        let (settings, key) = self.ai_configuration().await?;
        let saved = db.clone();
        let c = command.clone();
        let attempt = database_task(move || saved.reserve_ai(&c, "feedback", &settings)).await?;
        let Some(attempt) = attempt else {
            return self.load_feedback(command.session_id).await;
        };
        self.ai_runtime
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .busy = true;
        self.ai_job(&attempt);
        drop(guard);
        // A tarefa continua a guardar o resultado mesmo se o chamador deixar de o aguardar.
        let service = Arc::clone(self);
        let session_id = command.session_id;
        let work = tokio::spawn(async move {
            let valid = service.run_attempt(db, attempt, key).await;
            service
                .ai_runtime
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .busy = false;
            service.commit(|s| s.ai_job = None);
            valid
        });
        if !work.await.unwrap_or(false) {
            return Err(failure("A avaliação não ficou disponível. Consulte o erro e repita apenas o pedido em falta; pode gerar novo custo."));
        }
        self.load_feedback(session_id).await
    }

    pub async fn generate_feedback(
        self: &Arc<Self>,
        command: AiSessionCommand,
    ) -> Result<FeedbackView> {
        let _guard = self.transitions.lock().await;
        let db = self.database()?;
        let c = command.clone();
        let saved = db.clone();
        let replay = database_task(move || {
            saved.with_connection(|db| {
                let payload = crate::db::ai::hash(&c)?;
                let existing: Option<String> = {
                    use rusqlite::OptionalExtension;
                    db.query_row(
                        "SELECT payload_hash FROM ai_receipts WHERE id=?1",
                        [&c.request_id],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(crate::db::ai::sql_error)?
                };
                if let Some(h) = existing {
                    if h != payload {
                        return Err(failure("Identificador reutilizado."));
                    }
                    return Ok(true);
                }
                Ok(false)
            })
        })
        .await?;
        if replay {
            return self.load_feedback(command.session_id).await;
        }
        if self.ai_busy() {
            return Err(AppError::busy(
                "Existe um pedido IA ou resultado por publicar.",
            ));
        }
        let (settings, key) = self.ai_configuration().await?;
        let d = self.load_session(command.session_id.clone()).await?;
        if d.summary.status != crate::interview::SessionStatus::Completed
            || d.summary.mode != crate::interview::SessionMode::AiV3
        {
            return Err(failure("Termine a entrevista IA antes de avaliar."));
        }
        let view = self.load_feedback(command.session_id.clone()).await?;
        let missing: Vec<_> = d
            .questions
            .into_iter()
            .filter(|q| {
                !view
                    .reports
                    .iter()
                    .any(|r| r["questionId"] == q.question.id)
            })
            .collect();
        if missing.is_empty() {
            return Ok(view);
        }
        let c = command.clone();
        let saved = db.clone();
        database_task(move || {
            saved.with_connection(|db| {
                db.execute(
                    "INSERT INTO ai_receipts VALUES(?1,'feedback',?2,?3)",
                    rusqlite::params![c.request_id, crate::db::ai::hash(&c)?, c.session_id],
                )
                .map_err(crate::db::ai::sql_error)?;
                Ok(())
            })
        })
        .await?;
        self.ai_runtime
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .busy = true;
        self.commit(|s| {
            s.ai_job = Some(AiJobView {
                attempt_id: command.request_id.clone(),
                purpose: "feedback".into(),
                session_id: command.session_id.clone(),
                session_question_id: missing[0].question.id.clone(),
                stage: "requesting".into(),
            })
        });
        let service = Arc::clone(self);
        tokio::spawn(async move {
            for q in missing {
                let c = AiQuestionCommand {
                    request_id: format!("{}:{}", command.request_id, q.question.id),
                    session_id: command.session_id.clone(),
                    session_question_id: q.question.id,
                    expected_session_version: d.summary.version,
                };
                let saved = db.clone();
                let config = settings.clone();
                // O fecho não pode reconciliar operações e depois deixar esta
                // reserva criar uma tentativa nova como running.
                let reserved = {
                    let _transition = service.transitions.lock().await;
                    if service.ai_closed.load(Ordering::SeqCst) {
                        break;
                    }
                    let reserved =
                        database_task(move || saved.reserve_ai(&c, "feedback", &config)).await;
                    if let Ok(Some(a)) = &reserved {
                        service.ai_job(a);
                    }
                    reserved
                };
                match reserved {
                    Ok(Some(a)) => {
                        if !service.run_attempt(db.clone(), a, key.clone()).await {
                            break;
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        service.record_error(&e);
                        break;
                    }
                }
            }
            service
                .ai_runtime
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .busy = false;
            service.commit(|s| s.ai_job = None);
        });
        self.load_feedback(d.summary.id).await
    }
}

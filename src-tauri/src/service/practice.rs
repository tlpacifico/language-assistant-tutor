//! Ciclo local de prática: criar, carregar, listar, abandonar, gravar, rever,
//! confirmar e transcrever uma tentativa de treino — sem rede, sem histórias,
//! sem UI.
//!
//! Este módulo é filho de [`super`] e reutiliza o coordenador serial, a base de
//! dados, os artefactos e o auxiliar existentes: não existe um segundo serviço
//! de áudio nem um segundo worker. A captura, a transcrição e a publicação são
//! partilhadas com a entrevista e roteadas pelo dono explícito
//! ([`crate::practice::CaptureOwner`]), resolvido por
//! [`crate::db::Database::resolve_capture_owner`].
//!
//! O estado de prática é independente do trabalho IA e da entrevista: a única
//! prática pendente é espelhada em `InterviewState.active_practice` e
//! `InterviewState.active_owner`, sem tocar em `active_session` nem `current`.

use super::*;
use crate::db::{CommandIdentity, CommandIdentityLocation};
use crate::interview::{AnswerTarget, SaveReview, Version};
use crate::practice::{
    CaptureOwner, ComparisonRequest, ComparisonView, ConfirmPracticeAnswer, CreatePractice,
    CreateStory, EvaluatePractice, PracticeAnswerTarget, PracticeCommand, PracticeDetail,
    PracticeReportView, PreparationDecision, ReportState, ResolvedAnswer, ResponseOrigin,
    RewriteCommand, RewritePreference, RewriteView, SavePracticeReview, SelectStory,
    SetRewriteLevel, StoryView, TrainingUsageView, UpdateStory,
};

impl AppService {
    // -- criação, leitura e abandono ----------------------------------------

    /// Cria uma prática a partir de uma pergunta confirmada de uma sessão concluída.
    ///
    /// Idempotente por `requestId`; recusa sessão em curso, origem sem
    /// confirmação, predecessor de outra pergunta e segunda prática pendente.
    pub async fn create_practice(
        &self,
        request: CreatePractice,
    ) -> Result<PracticeDetail, AppError> {
        let _transition = self.transitions.lock().await;
        let database = self.database()?;
        let db = database.clone();
        let detail = database_task(move || db.create_practice(request)).await?;
        self.set_active_practice(Some(detail.clone()));
        Ok(detail)
    }

    /// Lê uma prática completa, sem alterar o estado observável.
    pub async fn load_practice(&self, practice_id: Id) -> Result<PracticeDetail, AppError> {
        let database = self.database()?;
        let db = database.clone();
        database_task(move || db.load_practice(&practice_id)).await
    }

    /// Lista as práticas de uma pergunta da sessão, pela ordem de criação.
    pub async fn list_practices(
        &self,
        session_question_id: Id,
    ) -> Result<Vec<PracticeDetail>, AppError> {
        let database = self.database()?;
        let db = database.clone();
        database_task(move || db.list_practices(&session_question_id)).await
    }

    /// Abandona uma prática pendente, conservando revisões e gravações.
    pub async fn abandon_practice(
        &self,
        request: PracticeCommand,
    ) -> Result<PracticeDetail, AppError> {
        let _transition = self.transitions.lock().await;
        let database = self.database()?;
        let db = database.clone();
        let detail = database_task(move || db.abandon_practice(request)).await?;
        self.set_active_practice(None);
        Ok(detail)
    }

    // -- histórias e apoio ---------------------------------------------------

    /// Lista as histórias locais, opcionalmente filtradas por competência.
    pub async fn list_stories(
        &self,
        competency: Option<String>,
    ) -> Result<Vec<StoryView>, AppError> {
        let database = self.database()?;
        let db = database.clone();
        database_task(move || db.list_stories(competency.as_deref())).await
    }

    /// Cria uma história local, idempotente por `requestId`.
    pub async fn create_story(&self, request: CreateStory) -> Result<StoryView, AppError> {
        let database = self.database()?;
        let db = database.clone();
        database_task(move || db.create_story(request)).await
    }

    /// Edita uma história local com versão esperada; `story_versions` é append-only.
    pub async fn update_story(&self, request: UpdateStory) -> Result<StoryView, AppError> {
        let database = self.database()?;
        let db = database.clone();
        database_task(move || db.update_story(request)).await
    }

    /// Lê a preparação (apoio de história) de uma resposta, sem alterar estado.
    ///
    /// Só leitura local: resolve o dono e devolve o apoio guardado, sem rede.
    pub async fn load_preparation(
        &self,
        owner: CaptureOwner,
    ) -> Result<crate::practice::PreparationView, AppError> {
        let resolved = self.resolve_practice_owner(&owner).await?;
        let database = self.database()?;
        let slot_id = resolved.slot.id;
        database_task(move || database.load_preparation(&slot_id)).await
    }

    /// Seleciona (ou remove) o apoio de história de uma resposta, antes da captura.
    pub async fn select_story(&self, request: SelectStory) -> Result<InterviewSnapshot, AppError> {
        let _transition = self.transitions.lock().await;
        let database = self.database()?;
        let db = database.clone();
        database_task(move || db.select_story(request)).await?;
        self.refresh_practice_state().await?;
        Ok(self.snapshot())
    }

    /// Resolve explicitamente uma preparação interrompida (`keep` ou `did_not_start`).
    pub async fn resolve_preparation(
        &self,
        recording_id: Id,
        request_id: Id,
        decision: PreparationDecision,
    ) -> Result<InterviewSnapshot, AppError> {
        let _transition = self.transitions.lock().await;
        let database = self.database()?;
        let db = database.clone();
        let rid = recording_id.clone();
        database_task(move || db.resolve_preparation(&rid, &request_id, decision)).await?;
        self.clear_pending_preparation();
        self.refresh_practice_state().await?;
        Ok(self.snapshot())
    }

    /// Repete a publicação local dos metadados da preparação, consolidando o
    /// mesmo snapshot sem consultar a versão atual da história.
    pub async fn retry_preparation_publication(
        &self,
        recording_id: Id,
    ) -> Result<InterviewSnapshot, AppError> {
        let _transition = self.transitions.lock().await;
        let database = self.database()?;
        let db = database.clone();
        let rid = recording_id.clone();
        let location = database_task(move || db.preparation_capture_location(&rid))
            .await?
            .ok_or_else(|| {
                stale_source("A captura de preparação indicada não existe. Atualize o estado.")
            })?;
        let (session_id, slot_id) = location;
        let artifacts = self.artifacts.clone();
        let sid = session_id.clone();
        let sqid = slot_id.clone();
        let rid = recording_id.clone();
        let target = database_task(move || artifacts.reserve(&sid, &sqid, &rid)).await?;
        let directory = target.directory.clone();
        let rid = recording_id.clone();
        storage_task(move || {
            crate::artifacts::write_preparation_marker(&directory, &rid, "started")
        })
        .await?;
        let db = database.clone();
        let rid = recording_id.clone();
        database_task(move || db.mark_preparation_started(&rid)).await?;
        self.clear_pending_preparation();
        self.refresh_practice_state().await?;
        Ok(self.snapshot())
    }

    // -- captura ------------------------------------------------------------

    /// Reserva a gravação da tentativa e só então abre o microfone.
    ///
    /// O dono tem de ser uma prática pendente e editável; a versão de seleção
    /// esperada protege contra uma interface desatualizada. Nada é escrito nas
    /// recusas.
    pub async fn start_practice_recording(
        &self,
        owner: CaptureOwner,
        expected_selection: Version,
    ) -> Result<InterviewSnapshot, AppError> {
        let _transition = self.transitions.lock().await;

        if self.lock_pending().is_some() {
            let error = storage_error(
                "A gravação anterior ainda não foi guardada. Repita o guardar antes de gravar outra resposta.",
            );
            self.record_error(&error);
            return Err(error);
        }
        {
            let state = self.lock_state();
            if state.recording_id.is_some() || state.finalizing {
                let error = AppError::busy("Já está uma gravação em curso.");
                drop(state);
                self.record_error(&error);
                return Err(error);
            }
            if state.model_job.is_some() {
                let error =
                    AppError::busy("Há uma transcrição ou síntese em curso. Aguarde que termine.");
                drop(state);
                self.record_error(&error);
                return Err(error);
            }
        }
        if self.lock_capture().is_some() {
            let error = AppError::busy(
                "A gravação interrompida ainda está a ser recuperada. Aguarde um instante e tente novamente.",
            );
            self.record_error(&error);
            return Err(error);
        }

        let database = self.database()?;
        let resolved = self.resolve_practice_owner(&owner).await?;
        let CaptureOwner::Practice {
            practice_id: _,
            answer_id,
        } = &resolved.owner
        else {
            let error = stale_source("O alvo indicado não é uma tentativa de prática.");
            self.record_error(&error);
            return Err(error);
        };
        if !resolved.writable {
            let error = practice_not_writable();
            self.record_error(&error);
            return Err(error);
        }
        if resolved.slot.selection_version != expected_selection {
            let error = stale_source(
                "A seleção da resposta mudou entretanto. Atualize a prática antes de gravar outra vez.",
            );
            self.record_error(&error);
            return Err(error);
        }
        let source_session_id = resolved.source_session_id.clone();
        let slot_id = answer_id.clone();

        let audio = Arc::clone(&self.audio);
        if let Err(error) = audio_task(move || audio.stop_playback()).await {
            self.record_error(&error);
            return Err(error);
        }
        let playing = self.lock_state().playback.is_some();
        if playing {
            self.commit(|state| state.playback = None);
        }

        // Uma preparação pendente por resolver impede nova reserva: nada é
        // escrito nem aberto até se resolver/retomar.
        let db = database.clone();
        let aid = slot_id.clone();
        if database_task(move || db.preparation_pending(&aid)).await? {
            let error = AppError::busy(
                "Há uma preparação pendente para resolver ou retomar. Resolva-a antes de gravar novamente.",
            );
            self.record_error(&error);
            return Err(error);
        }

        let db = database.clone();
        let sid = slot_id.clone();
        let reserved =
            match database_task(move || db.reserve_recording(&sid, expected_selection)).await {
                Ok(recording) => recording,
                Err(error) => {
                    self.record_error(&error);
                    return Err(error);
                }
            };
        let recording_id = reserved.id.clone();

        // Reserva o candidato imutável do apoio (ou ausência) antes do microfone.
        // Uma falha aqui não abre o microfone; um apoio já fixado deixa gravar.
        let fault = *self.lock_fault();
        let reservation = match fault {
            Some(PreparationFault::ReserveCandidate) => {
                let error = storage_error(
                    "Não foi possível guardar o apoio antes de gravar; o microfone não foi aberto.",
                );
                self.record_error(&error);
                return Err(error);
            }
            _ => {
                let db = database.clone();
                let aid = slot_id.clone();
                let rid = recording_id.clone();
                match database_task(move || db.reserve_preparation_candidate(&aid, &rid)).await {
                    Ok(reservation) => reservation,
                    Err(error) => {
                        let db = database.clone();
                        let id = recording_id.clone();
                        let _ = database_task(move || db.mark_interrupted(&id)).await;
                        self.record_error(&error);
                        return Err(error);
                    }
                }
            }
        };

        let identity = CaptureIdentity {
            recording_id: recording_id.clone(),
            session_id: source_session_id,
            session_question_id: slot_id,
            owner,
        };

        let artifacts = self.artifacts.clone();
        let s_sid = identity.session_id.clone();
        let s_sqid = identity.session_question_id.clone();
        let s_rid = recording_id.clone();
        let target = match database_task(move || artifacts.reserve(&s_sid, &s_sqid, &s_rid)).await {
            Ok(target) => target,
            Err(error) => {
                let db = database.clone();
                let id = recording_id.clone();
                let _ = database_task(move || db.mark_interrupted(&id)).await;
                let db = database.clone();
                let id = recording_id.clone();
                let _ = database_task(move || db.mark_preparation_failed(&id)).await;
                self.record_error(&error);
                return Err(error);
            }
        };
        let directory = target.directory.clone();

        let audio = Arc::clone(&self.audio);
        match audio_task(move || audio.start_capture(target)).await {
            Ok(()) => {
                *self.lock_capture() = Some(identity);
                self.commit(|state| {
                    state.recording_id = Some(recording_id.clone());
                    state.playback = None;
                    state.last_error = None;
                });
                // Marca o arranque sem parar a captura se a escrita falhar: a
                // pendência fica visível e o Terminar continua disponível.
                if let crate::db::PreparationReservation::Reserved { .. } = &reservation {
                    if let Err(error) = self
                        .finalize_preparation_start(&database, &recording_id, &directory)
                        .await
                    {
                        self.commit(|state| state.pending_preparation = Some(recording_id.clone()));
                        self.record_error(&error);
                    }
                }
                Ok(self.snapshot())
            }
            Err(error) => {
                let _ = fs::remove_dir(&directory);
                let db = database.clone();
                let id = recording_id.clone();
                let _ = database_task(move || db.mark_interrupted(&id)).await;
                let db = database.clone();
                let id = recording_id.clone();
                let _ = database_task(move || db.mark_preparation_failed(&id)).await;
                self.commit(|state| {
                    state.playback = None;
                    state.last_error = Some(error.clone());
                });
                self.resample_after_device_error(&error).await;
                Err(error)
            }
        }
    }

    // -- revisão, confirmação e transcrição ---------------------------------

    /// Guarda uma revisão da tentativa, reutilizando a revisão partilhada.
    pub async fn save_practice_review(
        &self,
        request: SavePracticeReview,
    ) -> Result<InterviewSnapshot, AppError> {
        let _transition = self.transitions.lock().await;
        let finalizing = self.lock_state().finalizing;
        if finalizing {
            let error = AppError::busy(
                "A gravação está a ser finalizada. Aguarde que termine antes de guardar a revisão.",
            );
            self.record_error(&error);
            return Err(error);
        }
        let resolved = self.resolve_practice_owner(&request.target.owner).await?;
        if !resolved.writable {
            let error = practice_not_writable();
            self.record_error(&error);
            return Err(error);
        }
        let save = SaveReview {
            request_id: request.request_id,
            target: practice_answer_target(&resolved, &request.target),
            expected_review_version: request.expected_review_version,
            source_transcript_id: request.source_transcript_id,
            origin: request.origin,
            text: request.text,
        };
        let database = self.database()?;
        let db = database.clone();
        match database_task(move || db.save_review(save)).await {
            Ok(_) => {
                self.refresh_practice_state().await?;
                Ok(self.snapshot())
            }
            Err(error) => {
                self.record_error(&error);
                Err(error)
            }
        }
    }

    /// Confirma a tentativa, tornando-a imutável, sem alterar a sessão.
    pub async fn confirm_practice_answer(
        &self,
        request: ConfirmPracticeAnswer,
    ) -> Result<PracticeDetail, AppError> {
        let _transition = self.transitions.lock().await;
        let capture_busy = {
            let state = self.lock_state();
            state.recording_id.is_some() || state.finalizing
        };
        if capture_busy {
            let error = AppError::busy("Termine a gravação antes de confirmar a resposta.");
            self.record_error(&error);
            return Err(error);
        }
        let database = self.database()?;
        let db = database.clone();
        let detail = database_task(move || db.confirm_practice_answer(request)).await?;
        self.set_active_practice(None);
        Ok(detail)
    }

    /// Transcreve a gravação selecionada da tentativa, partilhando o auxiliar.
    ///
    /// Uma falha de transcrição preserva o original e permite repetir; o retry
    /// reutiliza o mesmo ficheiro normalizado.
    pub async fn transcribe_practice_response(
        &self,
        target: PracticeAnswerTarget,
    ) -> Result<InterviewSnapshot, AppError> {
        let _transition = self.transitions.lock().await;
        {
            let state = self.lock_state();
            if state.recording_id.is_some() || state.finalizing {
                let error = AppError::busy("Termine a gravação antes de transcrever a resposta.");
                drop(state);
                self.record_error(&error);
                return Err(error);
            }
            if state.model_job.is_some() {
                let error =
                    AppError::busy("Já está um trabalho de modelo em curso. Aguarde que termine.");
                drop(state);
                self.record_error(&error);
                return Err(error);
            }
        }
        let resolved = self.resolve_practice_owner(&target.owner).await?;
        if !resolved.writable {
            let error = practice_not_writable();
            self.record_error(&error);
            return Err(error);
        }
        let database = self.database()?;
        let db = database.clone();
        let slot_id = resolved.slot.id.clone();
        let recording_id = target.recording_id.clone();
        let selection_version = target.selection_version;
        let recording = match database_task(move || {
            db.practice_selected_recording(&slot_id, &recording_id, selection_version)
        })
        .await
        {
            Ok(recording) => recording,
            Err(error) => {
                self.record_error(&error);
                return Err(error);
            }
        };
        let answer_target = practice_answer_target(&resolved, &target);
        self.dispatch_transcription(answer_target, recording).await
    }

    // -- auxiliares ---------------------------------------------------------

    /// Resolve o dono de uma prática no alvo concreto, sem rede nem escrita.
    async fn resolve_practice_owner(
        &self,
        owner: &CaptureOwner,
    ) -> Result<ResolvedAnswer, AppError> {
        let database = self.database()?;
        let db = database.clone();
        let resolved = owner.clone();
        database_task(move || db.resolve_capture_owner(&resolved)).await
    }

    /// Espelha a prática pendente (ou a sua ausência) na fotografia.
    fn set_active_practice(&self, detail: Option<PracticeDetail>) {
        let owner = detail.as_ref().map(|practice| CaptureOwner::Practice {
            practice_id: practice.id.clone(),
            answer_id: practice.answer.id.clone(),
        });
        self.commit(|state| {
            state.active_practice = detail;
            state.active_owner = owner;
        });
    }

    /// Relê a prática pendente da base e atualiza a fotografia.
    async fn refresh_practice_state(&self) -> Result<(), AppError> {
        let database = self.database()?;
        let db = database.clone();
        let pending = database_task(move || db.pending_practice()).await?;
        self.set_active_practice(pending);
        Ok(())
    }

    /// Limpa a pendência de metadados de preparação da fotografia.
    fn clear_pending_preparation(&self) {
        self.commit(|state| state.pending_preparation = None);
    }

    /// Marca o arranque da captura (marcador de ficheiro + SQLite), sem parar a
    /// captura se a escrita falhar: devolve o erro para o chamador expor a pendência.
    async fn finalize_preparation_start(
        &self,
        database: &Database,
        recording_id: &str,
        directory: &Path,
    ) -> Result<(), AppError> {
        let rid = recording_id.to_owned();
        let dir = directory.to_owned();
        storage_task(move || crate::artifacts::write_preparation_marker(&dir, &rid, "started"))
            .await?;
        if *self.lock_fault() == Some(PreparationFault::MarkStarted) {
            return Err(storage_error(
                "Não foi possível guardar o arranque do apoio; a gravação continua ativa e pode ser terminada.",
            ));
        }
        let db = database.clone();
        let rid = recording_id.to_owned();
        database_task(move || db.mark_preparation_started(&rid)).await
    }

    // -- reformulação, avaliação e preferência de nível ---------------------

    /// Recusa um `requestId` já usado noutro caminho (prática, configuração,
    /// consentimento ou comando da entrevista), antes de qualquer reutilização.
    async fn refuse_colliding_request(&self, request_id: &str) -> Result<(), AppError> {
        let database = self.database()?;
        let db = database.clone();
        let rid = request_id.to_owned();
        let identity = database_task(move || db.lookup_command_identity(&rid)).await?;
        match identity {
            CommandIdentity::Conflict
            | CommandIdentity::Found {
                location:
                    CommandIdentityLocation::AiReceipt | CommandIdentityLocation::CommandReceipt,
                ..
            } => Err(AppError::new(
                error_code::STALE_SOURCE,
                "Este pedido já foi usado com outro conteúdo. Repita a operação a partir do estado atual.",
                false,
            )),
            _ => Ok(()),
        }
    }

    /// Pede (ou reutiliza) uma reformulação para uma origem e um nível.
    ///
    /// A variante válida existente é devolvida sem rede; uma reserva nova ocupa
    /// o runtime de IA único e publica o resultado imutável quando chegar.
    pub async fn request_rewrite(
        self: &Arc<Self>,
        command: RewriteCommand,
    ) -> Result<RewriteView, AppError> {
        let _transition = self.transitions.lock().await;
        let database = self.database()?;
        self.refuse_colliding_request(&command.request_id).await?;

        let origin = command.origin.clone();
        let level = command.level;
        let existing = {
            let db = database.clone();
            let origin = origin.clone();
            database_task(move || db.list_rewrites(&origin)).await?
        };
        if let Some(view) = existing
            .iter()
            .find(|view| view.level == level && view.state == ReportState::Ready)
        {
            return Ok(view.clone());
        }
        if self.ai_busy() {
            return Err(AppError::busy(
                "Existe um pedido IA ou resultado por guardar.",
            ));
        }
        let (settings, key) = self.ai_configuration().await?;
        let reserve_db = database.clone();
        let reserved = command.clone();
        let config = settings.clone();
        let attempt = database_task(move || reserve_db.request_rewrite(&reserved, &config)).await?;
        if let Some(attempt) = attempt {
            self.ai_runtime
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .busy = true;
            self.ai_job(&attempt);
            let service = Arc::clone(self);
            let run_db = database.clone();
            tokio::spawn(async move {
                service.run_attempt(run_db, attempt, key).await;
                service
                    .ai_runtime
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .busy = false;
                service.commit(|s| s.ai_job = None);
            });
        }
        let db = database.clone();
        let origin = origin.clone();
        let list = database_task(move || db.list_rewrites(&origin)).await?;
        list.into_iter()
            .find(|view| view.level == level)
            .ok_or_else(|| {
                AppError::new(
                    error_code::STALE_SOURCE,
                    "A reformulação pedida não está disponível. Repita a operação.",
                    false,
                )
            })
    }

    /// Lista as reformulações de uma origem, uma por nível já pedido.
    pub async fn list_rewrites(
        &self,
        origin: ResponseOrigin,
    ) -> Result<Vec<RewriteView>, AppError> {
        let database = self.database()?;
        let db = database.clone();
        database_task(move || db.list_rewrites(&origin)).await
    }

    /// Pede (ou reutiliza) a avaliação de uma prática confirmada.
    ///
    /// O relatório válido existente é devolvido sem rede; uma reserva nova ocupa
    /// o runtime de IA único e publica o relatório imutável quando chegar.
    pub async fn evaluate_practice(
        self: &Arc<Self>,
        command: EvaluatePractice,
    ) -> Result<PracticeReportView, AppError> {
        let _transition = self.transitions.lock().await;
        let database = self.database()?;
        self.refuse_colliding_request(&command.request_id).await?;
        let practice_id = command.practice_id.clone();

        let existing = {
            let db = database.clone();
            let pid = practice_id.clone();
            database_task(move || db.load_practice_report(&pid)).await?
        };
        if existing.state == ReportState::Ready {
            return Ok(existing);
        }
        if self.ai_busy() {
            return Err(AppError::busy(
                "Existe um pedido IA ou resultado por guardar.",
            ));
        }
        let (settings, key) = self.ai_configuration().await?;
        let reserve_db = database.clone();
        let reserved = command.clone();
        let config = settings.clone();
        let attempt =
            database_task(move || reserve_db.evaluate_practice(&reserved, &config)).await?;
        if let Some(attempt) = attempt {
            self.ai_runtime
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .busy = true;
            self.ai_job(&attempt);
            let service = Arc::clone(self);
            let run_db = database.clone();
            tokio::spawn(async move {
                service.run_attempt(run_db, attempt, key).await;
                service
                    .ai_runtime
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .busy = false;
                service.commit(|s| s.ai_job = None);
            });
        }
        let db = database.clone();
        let pid = practice_id.clone();
        database_task(move || db.load_practice_report(&pid)).await
    }

    /// Relatório de avaliação de uma prática, com tentativas e consumo.
    pub async fn load_practice_report(
        &self,
        practice_id: Id,
    ) -> Result<PracticeReportView, AppError> {
        let database = self.database()?;
        let db = database.clone();
        database_task(move || db.load_practice_report(&practice_id)).await
    }

    /// Compara localmente duas respostas confirmadas da mesma pergunta.
    ///
    /// Só lê dados guardados: nunca cria avaliações nem chama a API. Os avisos
    /// de ficheiros em falta das gravações dependem do disco e são acrescentados
    /// à cópia devolvida.
    pub async fn compare_responses(
        &self,
        request: ComparisonRequest,
    ) -> Result<ComparisonView, AppError> {
        let database = self.database()?;
        let db = database.clone();
        let mut view = database_task(move || db.compare_responses(&request)).await?;
        let artifacts = self.artifacts.clone();
        storage_task(move || {
            for entry in [&mut view.left, &mut view.right] {
                for answer in &mut entry.answers {
                    for recording in &mut answer.recordings {
                        recording.warnings = artifacts.warnings(recording);
                    }
                }
            }
            Ok(view)
        })
        .await
    }

    /// Consumo de treino de uma sessão, separado entre entrevista original e
    /// prática posterior. Só lê dados guardados.
    pub async fn load_training_usage(&self, session_id: Id) -> Result<TrainingUsageView, AppError> {
        let database = self.database()?;
        let db = database.clone();
        database_task(move || db.load_training_usage(&session_id)).await
    }

    /// Preferência local de nível, com B1 por omissão.
    pub async fn load_rewrite_preference(&self) -> Result<RewritePreference, AppError> {
        let database = self.database()?;
        let db = database.clone();
        database_task(move || db.load_rewrite_preference()).await
    }

    /// Altera a preferência local de nível; não altera reformulações anteriores.
    pub async fn set_rewrite_level(
        &self,
        request: SetRewriteLevel,
    ) -> Result<RewritePreference, AppError> {
        let _transition = self.transitions.lock().await;
        let database = self.database()?;
        let db = database.clone();
        database_task(move || db.set_rewrite_level(request)).await
    }
}

/// Alvo de revisão/confirmação/transcrição traduzido do dono resolvido.
fn practice_answer_target(
    resolved: &ResolvedAnswer,
    target: &PracticeAnswerTarget,
) -> AnswerTarget {
    AnswerTarget {
        answer_id: None,
        session_id: resolved.source_session_id.clone(),
        session_question_id: resolved.slot.id.clone(),
        recording_id: target.recording_id.clone(),
        selection_version: target.selection_version,
    }
}

fn practice_not_writable() -> AppError {
    AppError::new(
        error_code::STALE_SOURCE,
        "Esta prática já não aceita alterações; nada foi aplicado.",
        false,
    )
}

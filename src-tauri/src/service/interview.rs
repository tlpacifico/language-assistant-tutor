//! Métodos de sessão do serviço: captura, revisão, confirmação, avanço e áudio.
//!
//! Este módulo é filho de [`super`] e usa o mesmo coordenador serial e a mesma
//! fotografia: não existe um segundo serviço com acesso independente ao
//! microfone. Os métodos públicos adquirem o coordenador uma vez e chamam
//! auxiliares privados que nunca o readquirem. Nenhuma guarda do estado é
//! mantida durante um `await` nem durante um auxiliar que volte a adquiri-la.
//!
//! A base de dados é a autoridade do progresso: depois de cada commit, a sessão
//! ativa é relida e só então publicada. Uma leitura de histórico
//! ([`AppService::load_session`], [`AppService::list_sessions`]) nunca relê a
//! sessão ativa nem toca no trabalho de modelo pendente.

use super::*;
use crate::interview::{
    AdvanceSession, AudioSelection, Category, ConfirmAnswer, Question, QuestionInput,
    RecordingState, SessionDetail, SessionQuestion, SessionStatus, StartSession, Version,
};
use crate::worker::protocol::TranscribeParams;

impl AppService {
    // -- sessões ------------------------------------------------------------

    /// Inicia a entrevista numa transação e apresenta a primeira pergunta.
    ///
    /// Um pedido repetido devolve a sessão original; uma sessão em curso impede
    /// outra (`ACTIVE_SESSION_EXISTS`); a insuficiência de perguntas é explícita
    /// e nunca reduz o total em silêncio.
    pub async fn start_session(
        &self,
        request: StartSession,
    ) -> Result<InterviewSnapshot, AppError> {
        let _transition = self.transitions.lock().await;
        let database = self.database()?;
        let db = database.clone();
        match database_task(move || db.start_session(request)).await {
            Ok(_) => self.refresh_session(&database).await,
            Err(error) => {
                self.record_error(&error);
                Err(error)
            }
        }
    }

    /// Guarda uma versão nova da revisão, sem confirmar nem avançar.
    ///
    /// Editar durante uma captura é permitido (como na fase 1); durante a
    /// finalização a escrita é recusada para não competir com a publicação.
    pub async fn save_review(&self, request: SaveReview) -> Result<InterviewSnapshot, AppError> {
        let _transition = self.transitions.lock().await;
        let finalizing = self.lock_state().finalizing;
        if finalizing {
            let error = AppError::busy(
                "A gravação está a ser finalizada. Aguarde que termine antes de guardar a revisão.",
            );
            self.record_error(&error);
            return Err(error);
        }
        let database = self.database()?;
        let db = database.clone();
        match database_task(move || db.save_review(request)).await {
            Ok(_) => self.refresh_session(&database).await,
            Err(error) => {
                self.record_error(&error);
                Err(error)
            }
        }
    }

    /// Confirma exatamente a versão de texto que o utilizador viu.
    ///
    /// Recusa durante a captura e a finalização: confirmar ou avançar a meio de
    /// uma gravação moveria a sessão e deixaria a nova tomada publicada mas sem
    /// seleção nem transcrição.
    pub async fn confirm_answer(
        &self,
        request: ConfirmAnswer,
    ) -> Result<InterviewSnapshot, AppError> {
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
        match database_task(move || db.confirm_answer(request)).await {
            Ok(_) => self.refresh_session(&database).await,
            Err(error) => {
                self.record_error(&error);
                Err(error)
            }
        }
    }

    /// Avança para a pergunta seguinte ou termina a sessão, de forma explícita.
    ///
    /// Recusa durante a captura e a finalização: a sessão e a página não podem
    /// mudar enquanto o microfone está aberto.
    ///
    /// A sessão concluída deixa de ser a ativa: a fotografia passa a `None` e a
    /// interface consulta o resumo por [`AppService::load_session`].
    pub async fn advance_session(
        &self,
        request: AdvanceSession,
    ) -> Result<InterviewSnapshot, AppError> {
        let _transition = self.transitions.lock().await;
        let capture_busy = {
            let state = self.lock_state();
            state.recording_id.is_some() || state.finalizing
        };
        if capture_busy {
            let error = AppError::busy("Termine a gravação antes de avançar a entrevista.");
            self.record_error(&error);
            return Err(error);
        }
        let database = self.database()?;
        let db = database.clone();
        match database_task(move || db.advance_session(request)).await {
            Ok(_) => self.refresh_session(&database).await,
            Err(error) => {
                self.record_error(&error);
                Err(error)
            }
        }
    }

    // -- leituras -----------------------------------------------------------

    /// Banco de perguntas, opcionalmente filtrado por categoria.
    pub async fn list_questions(
        &self,
        category: Option<Category>,
    ) -> Result<Vec<Question>, AppError> {
        let database = self.database()?;
        database_task(move || database.list_questions(category)).await
    }

    /// Cria uma pergunta editável.
    pub async fn create_question(&self, input: QuestionInput) -> Result<Question, AppError> {
        let database = self.database()?;
        database_task(move || database.create_question(input)).await
    }

    /// Edita uma pergunta com controlo otimista de versão.
    pub async fn update_question(
        &self,
        id: Id,
        expected_version: Version,
        input: QuestionInput,
    ) -> Result<Question, AppError> {
        let database = self.database()?;
        database_task(move || database.update_question(&id, expected_version, input)).await
    }

    /// Sessões para o histórico, sem alterar o estado observável.
    pub async fn list_sessions(&self) -> Result<Vec<SessionSummary>, AppError> {
        let database = self.database()?;
        database_task(move || database.list_sessions()).await
    }

    /// Sessão completa para consulta ou retoma.
    ///
    /// É uma leitura pura: não muda a pergunta corrente nem a identidade do
    /// trabalho de modelo em curso.
    pub async fn load_session(&self, session_id: Id) -> Result<SessionDetail, AppError> {
        let database = self.database()?;
        let artifacts = self.artifacts.clone();
        database_task(move || {
            let mut detail = database.load_session(&session_id)?;
            artifacts.annotate(&mut detail);
            Ok(detail)
        })
        .await
    }

    /// Lista os treinos da fase 1 a partir das pastas, apenas de leitura.
    ///
    /// Não depende de `last-attempt.json` nem copia nada para SQLite; uma pasta
    /// inválida fica visível com os seus avisos sem esconder as restantes.
    pub async fn list_legacy(&self) -> Result<Vec<RestoreResult>, AppError> {
        let store = LegacyStore::new(self.legacy_root.clone());
        storage_task(move || store.list()).await
    }

    /// Lê um treino da fase 1 pelo identificador, sem alterar o formato antigo.
    pub async fn load_legacy(&self, attempt_id: Id) -> Result<RestoreResult, AppError> {
        let store = LegacyStore::new(self.legacy_root.clone());
        storage_task(move || store.load(&attempt_id)).await
    }

    // -- captura ------------------------------------------------------------

    /// Reserva a gravação na base e a pasta, e só então abre o microfone.
    ///
    /// Só a pergunta atual da entrevista em curso, ainda não confirmada, pode
    /// ser gravada. A versão de seleção esperada protege contra uma interface
    /// desatualizada. Nada é escrito nas recusas.
    pub async fn start_recording(
        &self,
        session_question_id: Id,
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
        // Uma paragem que falhou pode ainda entregar o áudio recuperável como
        // `CaptureInterrupted`; gravar por cima perdia esse parcial sem aviso.
        let recovering = self.lock_capture().is_some();
        if recovering {
            let error = AppError::busy(
                "A gravação interrompida ainda está a ser recuperada. Aguarde um instante e tente novamente.",
            );
            self.record_error(&error);
            return Err(error);
        }

        let database = self.database()?;
        let detail = match self.load_active_detail(&database).await {
            Ok(detail) => detail,
            Err(error) => {
                self.record_error(&error);
                return Err(error);
            }
        };
        let Some(detail) = detail else {
            let error = stale_source(
                "Não há nenhuma entrevista em curso. Inicie ou retome uma entrevista antes de gravar.",
            );
            self.record_error(&error);
            return Err(error);
        };
        let Some(question) = detail
            .questions
            .iter()
            .find(|question| question.question.id == session_question_id)
        else {
            let error = stale_source(
                "A pergunta indicada não pertence à entrevista em curso. Atualize a página.",
            );
            self.record_error(&error);
            return Err(error);
        };
        let confirmed = question.confirmation.is_some();
        let position = question.question.position;
        let selection_version = question.question.selection_version;
        let session_id = detail.summary.id.clone();

        if confirmed {
            let error = answer_confirmed();
            self.record_error(&error);
            return Err(error);
        }
        if position != detail.summary.position {
            let error = stale_source(
                "A pergunta indicada já não é a pergunta atual da entrevista. Atualize a página antes de gravar.",
            );
            self.record_error(&error);
            return Err(error);
        }
        if selection_version != expected_selection {
            let error = stale_source(
                "A seleção da resposta mudou entretanto. Atualize a entrevista antes de gravar outra vez.",
            );
            self.record_error(&error);
            return Err(error);
        }

        let audio = Arc::clone(&self.audio);
        if let Err(error) = audio_task(move || audio.stop_playback()).await {
            self.record_error(&error);
            return Err(error);
        }
        // A reprodução terminou antes de abrir a entrada; o estado acompanha a
        // paragem mesmo que a reserva falhe de seguida.
        let playing = self.lock_state().playback.is_some();
        if playing {
            self.commit(|state| state.playback = None);
        }

        let db = database.clone();
        let id = session_question_id.clone();
        let reserved =
            match database_task(move || db.reserve_recording(&id, expected_selection)).await {
                Ok(recording) => recording,
                Err(error) => {
                    self.record_error(&error);
                    return Err(error);
                }
            };
        let recording_id = reserved.id.clone();
        let identity = CaptureIdentity {
            recording_id: recording_id.clone(),
            session_id,
            session_question_id,
        };

        let artifacts = self.artifacts.clone();
        let sid = identity.session_id.clone();
        let sqid = identity.session_question_id.clone();
        let rid = recording_id.clone();
        let target = match database_task(move || artifacts.reserve(&sid, &sqid, &rid)).await {
            Ok(target) => target,
            Err(error) => {
                let db = database.clone();
                let id = recording_id.clone();
                let _ = database_task(move || db.mark_interrupted(&id)).await;
                self.record_error(&error);
                return Err(error);
            }
        };
        let directory = target.directory.clone();

        let audio = Arc::clone(&self.audio);
        match audio_task(move || audio.start_capture(target)).await {
            Ok(()) => {
                *self.lock_capture() = Some(identity);
                Ok(self.commit(|state| {
                    state.recording_id = Some(recording_id);
                    state.playback = None;
                    state.last_error = None;
                }))
            }
            Err(error) => {
                // A reserva vazia não fica a poluir a pasta de artefactos.
                let _ = fs::remove_dir(&directory);
                let db = database.clone();
                let id = recording_id.clone();
                let _ = database_task(move || db.mark_interrupted(&id)).await;
                self.commit(|state| {
                    state.playback = None;
                    state.last_error = Some(error.clone());
                });
                // Um microfone que desapareceu tem de aparecer no painel de
                // disponibilidade, que é a instrução do que preparar.
                self.resample_after_device_error(&error).await;
                Err(error)
            }
        }
    }

    /// Termina a captura, publica o original e pede a transcrição automática.
    pub async fn stop_recording(&self) -> Result<InterviewSnapshot, AppError> {
        let _transition = self.transitions.lock().await;
        let capturing = self.lock_state().recording_id.is_some();
        if !capturing {
            let error = not_capturing();
            self.record_error(&error);
            return Err(error);
        }
        self.commit(|state| state.finalizing = true);

        let audio = Arc::clone(&self.audio);
        match audio_task(move || audio.stop_capture()).await {
            Ok(capture) => self.publish_capture(capture, true).await,
            Err(error) => {
                // A thread de áudio pode recuperar o parcial e entregá-lo como
                // `CaptureInterrupted`; a resposta anterior mantém-se ativa até lá.
                let identity = self.lock_capture().clone();
                self.commit(|state| {
                    state.recording_id = None;
                    state.finalizing = false;
                    state.last_error = Some(error.clone());
                });
                self.resample_after_device_error(&error).await;
                if error.code == error_code::AUDIO_TIMEOUT {
                    // Um pedido que expirou não produz evento de interrupção:
                    // a recuperação tem de acontecer aqui, senão a captura
                    // bloqueava gravar e fechar para sempre.
                    if let Some(identity) = identity {
                        self.recover_timed_out_capture(&identity, error.clone())
                            .await;
                    }
                }
                Err(error)
            }
        }
    }

    /// Repete a publicação de uma captura já finalizada, sem voltar a gravar.
    pub async fn retry_save_recording(
        &self,
        recording_id: Id,
    ) -> Result<InterviewSnapshot, AppError> {
        let _transition = self.transitions.lock().await;
        let pending = self.lock_pending().take();
        let Some(pending) = pending else {
            let error = artifact_missing("Não há nenhuma gravação à espera de publicação.");
            self.record_error(&error);
            return Err(error);
        };
        if pending.identity.recording_id != recording_id {
            let error = stale_source(
                "A gravação indicada não é a que aguarda publicação. Atualize a página.",
            );
            *self.lock_pending() = Some(pending);
            self.record_error(&error);
            return Err(error);
        }
        self.publish_capture_as(pending.capture, pending.identity, true)
            .await
    }

    /// Publica uma captura cuja identidade está registada como em curso.
    pub(super) async fn publish_capture(
        &self,
        capture: CapturedAudio,
        transcribe: bool,
    ) -> Result<InterviewSnapshot, AppError> {
        let current = self.lock_capture().clone();
        let Some(identity) = current else {
            let error = stale_source("A captura terminada já não pertence à gravação em curso.");
            self.record_error(&error);
            return Err(error);
        };
        self.publish_capture_as(capture, identity, transcribe).await
    }

    /// Publica a captura, regista o derivado e, quando pedido, transcreve.
    ///
    /// Uma falha de publicação conserva o áudio finalizado e a identidade para
    /// [`AppService::retry_save_recording`], sem nova captura; nunca indica que
    /// tudo ficou guardado.
    pub(super) async fn publish_capture_as(
        &self,
        capture: CapturedAudio,
        identity: CaptureIdentity,
        transcribe: bool,
    ) -> Result<InterviewSnapshot, AppError> {
        if identity.recording_id != capture.id {
            let error = stale_source("A captura não corresponde à gravação reservada.");
            self.record_error(&error);
            return Err(error);
        }
        let database = self.database()?;
        *self.lock_capture() = None;
        self.commit(|state| state.finalizing = true);

        let publishing = capture.clone();
        let db = database.clone();
        let published = database_task(move || db.publish_recording(publishing)).await;
        if let Err(error) = published {
            // O áudio finalizado fica disponível para repetir a publicação e a
            // reserva continua visível com a ação de repetir, em qualquer página.
            let pending_id = identity.recording_id.clone();
            *self.lock_pending() = Some(PendingPublication { capture, identity });
            self.commit(|state| {
                state.recording_id = None;
                state.finalizing = false;
                state.pending_recording_id = Some(pending_id);
                state.last_error = Some(error.clone());
            });
            // Sem reler a sessão, a fotografia continuava a mostrar o detalhe
            // anterior à captura e a reserva não aparecia em `current`. A
            // releitura limpa o erro, por isso ele é reposto depois.
            match self.refresh_session(&database).await {
                Ok(_) => {
                    self.commit(|state| state.last_error = Some(error.clone()));
                }
                Err(refresh) => {
                    self.record_error(&refresh);
                }
            }
            return Err(error);
        }

        *self.lock_pending() = None;
        *self.lock_capture() = None;

        // O derivado é opcional: sem ele a resposta continua guardada e pode
        // ser transcrita mais tarde. A falha fica visível e não apaga o áudio.
        let normalized = self.normalize_published(&database, &identity).await;

        self.commit(|state| {
            state.recording_id = None;
            state.pending_recording_id = None;
            state.finalizing = false;
            state.playback = None;
            state.last_error = None;
        });
        if let Err(error) = self.refresh_session(&database).await {
            self.record_error(&error);
        }
        if let Err(error) = normalized {
            self.record_error(&error);
        }

        if transcribe && self.transcriber_ready() {
            if let Some(target) = self.automatic_target() {
                // A resposta já está publicada: uma falha do auxiliar fica
                // visível e o áudio conserva-se para repetir.
                let _ = self.submit_transcription_locked(target).await;
            }
        }

        Ok(self.snapshot())
    }

    /// Alvo da transcrição automática: a gravação selecionada de uma pergunta
    /// publicada e ainda não confirmada.
    fn automatic_target(&self) -> Option<AnswerTarget> {
        let current = self.lock_state().current.clone()?;
        if current.confirmation.is_some() {
            return None;
        }
        let selected = current.question.selected_recording_id.clone()?;
        let recording = current
            .recordings
            .iter()
            .find(|recording| recording.id == selected)?;
        if recording.state != RecordingState::Ready {
            return None;
        }
        Some(AnswerTarget {
            session_id: current.question.session_id,
            session_question_id: current.question.id,
            recording_id: recording.id.clone(),
            selection_version: current.question.selection_version,
        })
    }

    /// Produz e regista o derivado de transcrição de uma gravação publicada.
    async fn normalize_published(
        &self,
        database: &Database,
        identity: &CaptureIdentity,
    ) -> Result<(), AppError> {
        let artifacts = self.artifacts.clone();
        let database = database.clone();
        let session_id = identity.session_id.clone();
        let session_question_id = identity.session_question_id.clone();
        let recording_id = identity.recording_id.clone();
        database_task(move || {
            let target = artifacts.reserve(&session_id, &session_question_id, &recording_id)?;
            artifacts.normalize(&target)?;
            let reference = crate::artifacts::normalized_reference(
                &session_id,
                &session_question_id,
                &recording_id,
            )?;
            database.set_normalized(&recording_id, &reference)
        })
        .await
    }

    /// Recupera uma captura cuja paragem excedeu o prazo.
    ///
    /// A thread de áudio pode ter finalizado `original.wav` depois de o pedido
    /// expirar. Se existir um WAV legível, é publicado como recuperável e
    /// marcado `incomplete`; sem ficheiro, a reserva é marcada interrompida. Em
    /// ambos os casos a captura deixa de estar em curso, para não bloquear
    /// gravar e fechar à espera de um evento que já não chega.
    pub(super) async fn recover_timed_out_capture(
        &self,
        identity: &CaptureIdentity,
        error: AppError,
    ) {
        *self.lock_capture() = None;
        let artifacts = self.artifacts.clone();
        let session_id = identity.session_id.clone();
        let session_question_id = identity.session_question_id.clone();
        let recording_id = identity.recording_id.clone();
        let reserved_id = recording_id.clone();
        let target = match database_task(move || {
            artifacts.reserve(&session_id, &session_question_id, &reserved_id)
        })
        .await
        {
            Ok(target) => target,
            Err(storage_error) => {
                self.record_error(&storage_error);
                return;
            }
        };
        let original = target.directory.join(ORIGINAL_FILE);
        let directory = target.directory.clone();
        let id = recording_id.clone();
        match storage_task(move || read_recovered_capture(&id, &original)).await {
            Ok(Some(capture)) => {
                // A publicação limpa o erro; o timeout continua a ser o que
                // explica porque é que a resposta foi recuperada.
                if self
                    .publish_capture_as(capture, identity.clone(), false)
                    .await
                    .is_ok()
                {
                    self.record_error(&error);
                }
            }
            Ok(None) => {
                // Nunca foi finalizado áudio: a pasta reservada vazia não fica.
                let _ = fs::remove_dir(&directory);
                if let Ok(database) = self.database() {
                    let id = recording_id.clone();
                    let _ = database_task(move || database.mark_interrupted(&id)).await;
                }
                self.record_error(&error);
            }
            Err(storage_error) => {
                self.record_error(&storage_error);
            }
        }
    }

    // -- transcrição --------------------------------------------------------

    /// Transcreve uma resposta publicada e ainda não confirmada.
    ///
    /// Cada resultado fica ligado ao alvo que o originou; um resultado cujo alvo
    /// foi entretanto confirmado ou substituído termina como obsoleto.
    pub async fn transcribe_response(
        &self,
        target: AnswerTarget,
    ) -> Result<InterviewSnapshot, AppError> {
        let _transition = self.transitions.lock().await;
        self.submit_transcription_locked(target).await
    }

    /// Valida o alvo e submete a transcrição ao auxiliar.
    ///
    /// O chamador tem de deter o coordenador serial. O áudio é preparado fora
    /// da base e nenhum mutex atravessa os `await`.
    async fn submit_transcription_locked(
        &self,
        target: AnswerTarget,
    ) -> Result<InterviewSnapshot, AppError> {
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

        let database = self.database()?;
        let db = database.clone();
        let session_id = target.session_id.clone();
        let detail = match database_task(move || db.load_session(&session_id)).await {
            Ok(detail) => detail,
            Err(error) => {
                self.record_error(&error);
                return Err(error);
            }
        };
        if detail.summary.status != SessionStatus::InProgress {
            let error =
                stale_source("Esta entrevista já terminou; a resposta não é transcrita outra vez.");
            self.record_error(&error);
            return Err(error);
        }
        let Some(question) = detail
            .questions
            .iter()
            .find(|question| question.question.id == target.session_question_id)
        else {
            let error = stale_source(
                "A pergunta da transcrição não pertence a esta entrevista. Atualize a página.",
            );
            self.record_error(&error);
            return Err(error);
        };
        if question.confirmation.is_some() {
            let error = answer_confirmed();
            self.record_error(&error);
            return Err(error);
        }
        if question.question.position != detail.summary.position {
            let error = stale_source(
                "A transcrição indicada já não é a pergunta atual da entrevista e não é iniciada.",
            );
            self.record_error(&error);
            return Err(error);
        }
        if question.question.selection_version != target.selection_version
            || question.question.selected_recording_id.as_deref()
                != Some(target.recording_id.as_str())
        {
            let error = stale_source(
                "A seleção da resposta mudou entretanto; o pedido de transcrição não foi aplicado.",
            );
            self.record_error(&error);
            return Err(error);
        }
        let Some(recording) = question
            .recordings
            .iter()
            .find(|recording| recording.id == target.recording_id)
        else {
            let error = stale_source("A gravação indicada não pertence a esta pergunta da sessão.");
            self.record_error(&error);
            return Err(error);
        };
        if recording.state != RecordingState::Ready {
            let error = AppError::new(
                error_code::ANSWER_REQUIRED,
                "Esta pergunta ainda não tem uma gravação finalizada selecionada. Grave a resposta \
                 antes de a transcrever.",
                false,
            );
            self.record_error(&error);
            return Err(error);
        }
        let normalized_file = recording.normalized_file.clone();
        let original_file = recording.original_file.clone();
        let duration_ms = recording
            .capture
            .as_ref()
            .map(|meta| meta.duration_ms)
            .unwrap_or(0)
            .max(1);

        let Some(control) = self.worker_control() else {
            let error = worker_unavailable(
                "O auxiliar Python não está configurado; a resposta continua guardada.",
            );
            self.record_error(&error);
            return Err(error);
        };
        if !control.is_running() {
            let error = worker_unavailable(
                "O auxiliar Python não está em execução; reinicie-o antes de transcrever.",
            );
            self.record_error(&error);
            return Err(error);
        }
        let generation = self.generation.load(Ordering::SeqCst);
        if generation == 0 {
            let error = worker_unavailable(
                "O auxiliar Python não está em execução; reinicie-o antes de transcrever.",
            );
            self.record_error(&error);
            return Err(error);
        }
        let model_path = match self.whisper_directory() {
            Ok(path) => path,
            Err(error) => {
                self.record_error(&error);
                return Err(error);
            }
        };
        let audio_path = match self
            .transcription_audio(&database, &target, normalized_file, original_file)
            .await
        {
            Ok(path) => path,
            Err(error) => {
                self.record_error(&error);
                return Err(error);
            }
        };

        let request_id = uuid::Uuid::new_v4().to_string();
        let params = TranscribeParams::cpu(&audio_path, &model_path);
        let duration = std::time::Duration::from_millis(duration_ms);
        let request = match WorkerRequest::transcribe(request_id.clone(), params, duration) {
            Ok(request) => request,
            Err(error) => {
                self.record_error(&error);
                return Err(error);
            }
        };
        let job = ModelJobView {
            id: request_id.clone(),
            kind: OperationKind::Transcribe,
            stage: JobStage::Loading,
            target: OperationTarget {
                session_id: target.session_id.clone(),
                session_question_id: target.session_question_id.clone(),
                recording_id: Some(target.recording_id.clone()),
                selection_version: target.selection_version,
                operation_id: request_id,
                worker_generation: generation,
            },
        };
        // A operação é registada antes de o pedido sair: um encerramento
        // abrupto deixa uma linha `running` que a reconciliação recupera.
        let operation = job.target.clone();
        let operation_id = operation.operation_id.clone();
        let db = database.clone();
        let configuration = transcription_configuration();
        if let Err(error) = database_task(move || {
            db.begin_operation(OperationKind::Transcribe, &operation, Some(&configuration))
        })
        .await
        {
            self.record_error(&error);
            return Err(error);
        }
        if let Err(error) = self.commit_try(|state| state.begin_model_job(job)) {
            self.close_operation(&database, &operation_id, None).await;
            self.record_error(&error);
            return Err(error);
        }

        match worker_task(move || control.submit(request)).await {
            Ok(()) => Ok(self.snapshot()),
            Err(error) => {
                self.close_operation(&database, &operation_id, Some(&error))
                    .await;
                self.commit(|state| {
                    state.finish_model_job();
                    state.last_error = Some(error.clone());
                });
                Err(error)
            }
        }
    }

    /// Caminho absoluto do áudio a transcrever, criando o derivado se preciso.
    ///
    /// Reutiliza o derivado já registado; se este desapareceu, reconstrói-o a
    /// partir do original, sem alterar nenhum ficheiro original.
    async fn transcription_audio(
        &self,
        database: &Database,
        target: &AnswerTarget,
        normalized_file: Option<String>,
        original_file: Option<String>,
    ) -> Result<PathBuf, AppError> {
        let artifacts = self.artifacts.clone();
        let database = database.clone();
        let session_id = target.session_id.clone();
        let session_question_id = target.session_question_id.clone();
        let recording_id = target.recording_id.clone();
        database_task(move || {
            if let Some(reference) = normalized_file {
                if let Ok(path) = artifacts.resolve(&reference) {
                    if path.is_file() {
                        return Ok(path);
                    }
                }
            }
            let Some(reference) = original_file else {
                return Err(artifact_missing(
                    "A gravação não tem áudio original guardado; grave novamente.",
                ));
            };
            let original = artifacts.resolve(&reference)?;
            if !original.is_file() {
                return Err(artifact_missing(
                    "O áudio original da resposta não foi encontrado; grave novamente.",
                ));
            }
            let recording_target =
                artifacts.reserve(&session_id, &session_question_id, &recording_id)?;
            artifacts.normalize(&recording_target)?;
            let reference = crate::artifacts::normalized_reference(
                &session_id,
                &session_question_id,
                &recording_id,
            )?;
            database.set_normalized(&recording_id, &reference)?;
            let path = artifacts.resolve(&reference)?;
            if !path.is_file() {
                return Err(artifact_missing(
                    "O áudio normalizado não foi escrito; a resposta continua guardada.",
                ));
            }
            Ok(path)
        })
        .await
    }

    // -- reprodução ---------------------------------------------------------

    /// Reproduz uma pergunta de sessão, uma gravação guardada ou um treino antigo.
    ///
    /// Só a captura e uma síntese em curso impedem reproduzir. Ouvir conteúdo
    /// guardado é permitido durante uma transcrição e não muda o alvo do
    /// trabalho nem inicia outro.
    pub async fn play_audio(&self, target: AudioSelection) -> Result<InterviewSnapshot, AppError> {
        let _transition = self.transitions.lock().await;
        {
            let state = self.lock_state();
            if state.recording_id.is_some() || state.finalizing {
                let error = AppError::busy("Termine a gravação antes de reproduzir áudio.");
                drop(state);
                self.record_error(&error);
                return Err(error);
            }
            if let Some(job) = &state.model_job {
                if job.kind == OperationKind::Synthesize {
                    let error = AppError::busy(
                        "A voz da pergunta está a ser preparada. Aguarde que termine antes de reproduzir áudio.",
                    );
                    drop(state);
                    self.record_error(&error);
                    return Err(error);
                }
            }
        }

        match target {
            AudioSelection::Question {
                session_id,
                session_question_id,
            } => self.play_question(session_id, session_question_id).await,
            AudioSelection::Recording { recording_id } => self.play_recording(recording_id).await,
            AudioSelection::Legacy { attempt_id } => self.play_legacy(attempt_id).await,
        }
    }

    /// Reproduz o texto fixado de uma pergunta, usando ou criando a cache de voz.
    async fn play_question(
        &self,
        session_id: Id,
        session_question_id: Id,
    ) -> Result<InterviewSnapshot, AppError> {
        let database = self.database()?;
        let question = self
            .find_session_question(&database, &session_id, &session_question_id)
            .await?;
        let cache = self.question_cache_path(&question.text);
        if cache.is_file() {
            let candidate = cache.clone();
            match storage_task(move || validate_synthesis_wav(&candidate)).await {
                Ok(()) => return self.play_path(PlaybackTarget::Question, cache).await,
                // Um derivado inválido nunca é reproduzido: volta a sintetizar-se.
                Err(_) => {
                    let _ = fs::remove_file(&cache);
                }
            }
        }

        let synthesis = self.lock_state().availability.synthesis.clone();
        if synthesis.status != ComponentStatus::Ready {
            let error = model_missing(synthesis.message.unwrap_or_else(|| {
                "A síntese de voz não está disponível nesta instalação.".to_owned()
            }));
            self.record_error(&error);
            return Err(error);
        }
        let Some(control) = self.worker_control() else {
            let error = worker_unavailable(
                "O auxiliar Python não está configurado; a pergunta não pode ser sintetizada.",
            );
            self.record_error(&error);
            return Err(error);
        };
        if !control.is_running() {
            let error = worker_unavailable(
                "O auxiliar Python não está em execução; reinicie-o antes de ouvir a pergunta.",
            );
            self.record_error(&error);
            return Err(error);
        }
        let generation = self.generation.load(Ordering::SeqCst);
        if generation == 0 {
            let error = worker_unavailable(
                "O auxiliar Python não está em execução; reinicie-o antes de ouvir a pergunta.",
            );
            self.record_error(&error);
            return Err(error);
        }

        if let Some(parent) = cache.parent() {
            let directory = parent.to_path_buf();
            if let Err(error) = storage_task(move || {
                fs::create_dir_all(&directory).map_err(|error| {
                    storage_error(format!(
                        "Não foi possível preparar a cache da pergunta: {error}."
                    ))
                })
            })
            .await
            {
                self.record_error(&error);
                return Err(error);
            }
        }

        let request_id = uuid::Uuid::new_v4().to_string();
        let params = SynthesizeParams::new(question.text, cache.clone());
        let request = match WorkerRequest::synthesize(request_id.clone(), params) {
            Ok(request) => request,
            Err(error) => {
                self.record_error(&error);
                return Err(error);
            }
        };
        let job = ModelJobView {
            id: request_id.clone(),
            kind: OperationKind::Synthesize,
            stage: JobStage::Loading,
            target: OperationTarget {
                session_id: question.session_id,
                session_question_id: question.id,
                recording_id: None,
                selection_version: question.selection_version,
                operation_id: request_id.clone(),
                worker_generation: generation,
            },
        };
        // A síntese também é uma operação registada: identidade, alvo, geração,
        // estado e tempos ficam na base, não só na memória.
        let operation = job.target.clone();
        let operation_id = operation.operation_id.clone();
        let db = database.clone();
        if let Err(error) =
            database_task(move || db.begin_operation(OperationKind::Synthesize, &operation, None))
                .await
        {
            self.record_error(&error);
            return Err(error);
        }
        if let Err(error) = self.commit_try(|state| state.begin_model_job(job)) {
            self.close_operation(&database, &operation_id, None).await;
            self.record_error(&error);
            return Err(error);
        }
        *self.lock_synthesis() = Some((request_id, cache));

        match worker_task(move || control.submit(request)).await {
            Ok(()) => Ok(self.snapshot()),
            Err(error) => {
                *self.lock_synthesis() = None;
                self.close_operation(&database, &operation_id, Some(&error))
                    .await;
                self.commit(|state| {
                    state.finish_model_job();
                    state.last_error = Some(error.clone());
                });
                Err(error)
            }
        }
    }

    /// Localiza uma pergunta de sessão pela identidade, sem alterar o estado.
    async fn find_session_question(
        &self,
        database: &Database,
        session_id: &str,
        session_question_id: &str,
    ) -> Result<SessionQuestion, AppError> {
        // A pergunta atual já está na fotografia; só o histórico obriga a ler a
        // base. A guarda do estado cai antes de qualquer `await`.
        let current = self
            .lock_state()
            .current
            .as_ref()
            .map(|detail| detail.question.clone());
        if let Some(question) = current {
            if question.session_id == session_id && question.id == session_question_id {
                return Ok(question);
            }
        }

        let database = database.clone();
        let session_id = session_id.to_owned();
        let session_question_id = session_question_id.to_owned();
        let detail = match database_task(move || database.load_session(&session_id)).await {
            Ok(detail) => detail,
            Err(error) => {
                self.record_error(&error);
                return Err(error);
            }
        };
        detail
            .questions
            .into_iter()
            .find(|detail| detail.question.id == session_question_id)
            .map(|detail| detail.question)
            .ok_or_else(|| {
                stale_source(
                    "A pergunta indicada não pertence a esta entrevista. Atualize a página e tente novamente.",
                )
            })
    }

    /// Reproduz o original de uma gravação já guardada.
    async fn play_recording(&self, recording_id: Id) -> Result<InterviewSnapshot, AppError> {
        let from_state = {
            let current = self.lock_state().current.clone();
            current.and_then(|detail| {
                detail
                    .recordings
                    .into_iter()
                    .find(|recording| recording.id == recording_id)
            })
        };
        let reference = match from_state {
            Some(recording) => {
                if recording.state != RecordingState::Ready {
                    let error = artifact_missing(
                        "Esta gravação ainda não tem um áudio original finalizado.",
                    );
                    self.record_error(&error);
                    return Err(error);
                }
                recording.original_file
            }
            None => {
                let database = self.database()?;
                let db = database.clone();
                let id = recording_id.clone();
                let (state, original_file) =
                    match database_task(move || db.recording_audio(&id)).await {
                        Ok(audio) => audio,
                        Err(error) => {
                            self.record_error(&error);
                            return Err(error);
                        }
                    };
                if state != RecordingState::Ready {
                    let error = artifact_missing(
                        "Esta gravação ainda não tem um áudio original finalizado.",
                    );
                    self.record_error(&error);
                    return Err(error);
                }
                original_file
            }
        };
        let Some(reference) = reference else {
            let error = artifact_missing(
                "Esta gravação não tem uma referência ao áudio original guardada.",
            );
            self.record_error(&error);
            return Err(error);
        };

        let artifacts = self.artifacts.clone();
        let path = match storage_task(move || artifacts.resolve(&reference)).await {
            Ok(path) => path,
            Err(error) => {
                self.record_error(&error);
                return Err(error);
            }
        };
        if !path.is_file() {
            let error = artifact_missing(format!(
                "O áudio original da gravação não foi encontrado em «{}»; os textos foram conservados.",
                path.display()
            ));
            self.record_error(&error);
            return Err(error);
        }
        self.play_path(PlaybackTarget::Response, path).await
    }

    /// Reproduz o original de um treino da fase 1, apenas em leitura.
    async fn play_legacy(&self, attempt_id: Id) -> Result<InterviewSnapshot, AppError> {
        let store = LegacyStore::new(self.legacy_root.clone());
        let path = match storage_task(move || store.resolve_original(&attempt_id)).await {
            Ok(path) => path,
            Err(error) => {
                self.record_error(&error);
                return Err(error);
            }
        };
        self.play_path(PlaybackTarget::Response, path).await
    }
}

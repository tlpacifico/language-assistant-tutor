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
use crate::interview::{AnswerTarget, SaveReview, Version};
use crate::practice::{
    CaptureOwner, ConfirmPracticeAnswer, CreatePractice, PracticeAnswerTarget, PracticeCommand,
    PracticeDetail, ResolvedAnswer, SavePracticeReview,
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
                let _ = fs::remove_dir(&directory);
                let db = database.clone();
                let id = recording_id.clone();
                let _ = database_task(move || db.mark_interrupted(&id)).await;
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

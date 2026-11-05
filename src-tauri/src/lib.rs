//! Base da aplicação desktop: entrevista guiada sobre o áudio local.
//!
//! `domain` reúne os contratos partilhados, `audio` os tipos da captura e da
//! reprodução, `storage` os artefactos JSON da fase 1 (apenas leitura na
//! aplicação nova), `legacy` a consulta dos treinos anteriores, `worker` a
//! supervisão do auxiliar Python, `interview` os DTOs e a validação das
//! entrevistas guiadas, `db` a ligação SQLite recuperável, `artifacts` os
//! ficheiros de áudio geridos, `service` a coordenação do fluxo e `commands` os
//! comandos Tauri.
//!
//! O arranque liga os adaptadores reais: a pasta de dados é `app_data_dir()`,
//! com `tutor.sqlite3`, `sessions/` (artefactos de áudio) e `phase1/`
//! (configuração, cache e treinos antigos); o auxiliar é configurado por
//! `audio.json` em `app_config_dir()` (ou `LANGUAGE_TUTOR_CONFIG`) e o manifesto
//! de modelos é lido do mesmo ficheiro. A base de dados abre em segundo plano e
//! uma falha é recuperável sem fechar a janela. Sem Python ou modelos, a
//! captura, a reprodução e a revisão continuam a funcionar.

pub mod ai;
pub mod article;
mod article_commands;
pub mod artifacts;
pub mod audio;
pub mod commands;
pub mod db;
pub mod diagnostics;
mod diagnostics_commands;
pub mod domain;
pub mod interview;
pub mod legacy;
pub mod practice;
pub mod service;
pub mod storage;
pub mod worker;

use std::sync::Arc;

use tauri::{Emitter, Manager};

use crate::artifacts::ArtifactStore;
use crate::audio::{AudioEvent, NativeAudio};
use crate::domain::AppError;
use crate::interview::InterviewSnapshot;
use crate::service::{AppService, LaunchedWorker, ServicePaths, WorkerLauncher};
use crate::worker::{WorkerClient, WorkerConfig};

/// Evento com a fotografia da entrevista, publicado depois de cada alteração.
pub const INTERVIEW_SNAPSHOT_EVENT: &str = "interview://snapshot";

/// Evento com a fotografia persistida da conversa sobre um artigo.
pub const ARTICLE_SNAPSHOT_EVENT: &str = "article://snapshot";

/// Arranque real do auxiliar a partir da configuração pessoal.
///
/// A configuração é lida uma vez no arranque; corrigir `audio.json` exige
/// reiniciar a aplicação. `launch` bloqueia no handshake e é chamado pelo
/// serviço dentro de um fio bloqueante, nunca numa thread do runtime.
struct AudioWorkerLauncher {
    config: Result<WorkerConfig, AppError>,
}

impl WorkerLauncher for AudioWorkerLauncher {
    fn launch(&self) -> Result<LaunchedWorker, AppError> {
        let config = self.config.clone()?;
        let client = WorkerClient::start(config)?;
        let events = client.take_events();
        Ok(LaunchedWorker {
            control: Arc::new(client),
            events,
        })
    }
}

/// Arranca a janela Tauri.
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let diagnostics = diagnostics::Diagnostics::open(
                data_dir.join("diagnostics"),
                diagnostics::BuildInfo::current(),
            );
            diagnostics::install(diagnostics.clone());
            app.manage(diagnostics_commands::DiagnosticState {
                diagnostics,
                default_directory: app
                    .path()
                    .download_dir()
                    .unwrap_or_else(|_| data_dir.clone()),
            });
            std::fs::create_dir_all(&data_dir)?;
            let legacy_root = data_dir.join("phase1");
            std::fs::create_dir_all(&legacy_root)?;
            let config_dir = app.path().app_config_dir()?;

            let config = WorkerConfig::load(&config_dir, &legacy_root);
            let models_manifest = config
                .as_ref()
                .ok()
                .map(|config| config.models_manifest.clone());
            let launcher = Arc::new(AudioWorkerLauncher { config });

            // A thread de áudio entrega eventos por canal; o serviço é o único
            // consumidor e aplica-os na coordenação serial.
            let (audio_events, mut audio_receiver) = tokio::sync::mpsc::unbounded_channel();
            let audio = NativeAudio::new(move |event: AudioEvent| {
                let _ = audio_events.send(event);
            })
            .map_err(|error| {
                format!(
                    "Não foi possível abrir a thread de áudio: {}",
                    error.message
                )
            })?;

            let service = Arc::new(AppService::new(
                ServicePaths {
                    database: data_dir.join("tutor.sqlite3"),
                    legacy: legacy_root,
                },
                ArtifactStore::new(data_dir.join("sessions")),
                Arc::new(audio),
                launcher,
                models_manifest,
            ));

            let handle = app.handle().clone();
            service.subscribe(Arc::new(move |snapshot: &InterviewSnapshot| {
                if let Err(error) = handle.emit(INTERVIEW_SNAPSHOT_EVENT, snapshot.clone()) {
                    diagnostics::record(diagnostics::Event::new(
                        "rust",
                        "snapshot_publish",
                        "UNEXPECTED",
                    ));
                    eprintln!("Não foi possível publicar a fotografia da entrevista: {error}");
                }
            }));

            let article_handle = app.handle().clone();
            service.subscribe_articles(Arc::new(move |snapshot: &article::ArticleSnapshot| {
                if article_handle
                    .emit(ARTICLE_SNAPSHOT_EVENT, snapshot.clone())
                    .is_err()
                {
                    diagnostics::record(diagnostics::Event::new(
                        "rust",
                        "snapshot_publish",
                        "UNEXPECTED",
                    ));
                }
            }));

            let audio_service = Arc::clone(&service);
            tauri::async_runtime::spawn(async move {
                while let Some(event) = audio_receiver.recv().await {
                    audio_service.on_audio_event(event).await;
                }
            });

            // O auxiliar e a base arrancam em segundo plano: a janela aparece e
            // a disponibilidade fica visível mesmo sem Python, modelos ou base.
            let starting = Arc::clone(&service);
            tauri::async_runtime::spawn(async move {
                starting.start().await;
            });

            app.manage(service);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            diagnostics_commands::diagnostics_status,
            diagnostics_commands::diagnostics_record,
            diagnostics_commands::diagnostics_export,
            commands::interview_ai_load_ai_settings,
            commands::interview_ai_save_ai_settings,
            commands::interview_ai_save_ai_key,
            commands::interview_ai_remove_ai_key,
            commands::interview_ai_accept_ai_consent,
            commands::interview_ai_request_follow_up,
            commands::interview_ai_waive_follow_up,
            commands::interview_ai_generate_feedback,
            commands::interview_ai_evaluate_question,
            commands::interview_ai_load_feedback,
            commands::interview_ai_retry_ai_publication,
            article_commands::article_authorize,
            article_commands::article_correct,
            article_commands::article_create,
            article_commands::article_delete,
            article_commands::article_finish,
            article_commands::article_list,
            article_commands::article_retry,
            article_commands::article_resume,
            article_commands::article_revoke,
            article_commands::article_snapshot,
            article_commands::article_start_recording,
            article_commands::article_stop_recording,
            commands::interview_snapshot,
            commands::interview_list_questions,
            commands::interview_create_question,
            commands::interview_update_question,
            commands::interview_start_session,
            commands::interview_list_sessions,
            commands::interview_load_session,
            commands::interview_list_legacy,
            commands::interview_load_legacy,
            commands::interview_start_recording,
            commands::interview_stop_recording,
            commands::interview_retry_save_recording,
            commands::interview_transcribe_response,
            commands::interview_save_review,
            commands::interview_confirm_answer,
            commands::interview_advance_session,
            commands::interview_play_audio,
            commands::interview_stop_audio,
            commands::interview_refresh_availability,
            commands::interview_restart_worker,
            commands::interview_prepare_close,
            commands::interview_retry_database,
            commands::interview_preload_models,
            commands::interview_create_practice,
            commands::interview_load_practice,
            commands::interview_list_practices,
            commands::interview_abandon_practice,
            commands::interview_start_practice_recording,
            commands::interview_save_practice_review,
            commands::interview_confirm_practice_answer,
            commands::interview_transcribe_practice_response,
            commands::interview_list_stories,
            commands::interview_create_story,
            commands::interview_update_story,
            commands::interview_load_preparation,
            commands::interview_select_story,
            commands::interview_resolve_preparation,
            commands::interview_retry_preparation_publication,
            commands::interview_request_rewrite,
            commands::interview_list_rewrites,
            commands::interview_evaluate_practice,
            commands::interview_load_practice_report,
            commands::interview_load_rewrite_preference,
            commands::interview_set_rewrite_level,
            commands::interview_compare_responses,
            commands::interview_load_training_usage,
        ])
        .run(tauri::generate_context!())
        .expect("erro ao arrancar a aplicação Tauri");
}

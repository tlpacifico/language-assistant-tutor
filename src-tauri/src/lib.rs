//! Base da aplicação desktop da fase 1, com os contratos e o esquema da fase 2.
//!
//! `domain` reúne os contratos e as regras independentes de hardware, `audio`
//! os tipos partilhados com a captura e a reprodução, `storage` os artefactos
//! locais com JSON atómico e recuperação, `worker` a supervisão do auxiliar
//! Python, `service` a coordenação do fluxo e `commands` os comandos Tauri.
//! `interview` reúne os DTOs e a validação das entrevistas guiadas, `db` a
//! ligação SQLite recuperável com as migrações e `artifacts` os ficheiros de
//! áudio geridos, com a reconciliação do arranque; nenhum deles está ainda
//! ligado ao serviço da fase 1.
//!
//! O arranque liga os adaptadores reais: a pasta de dados é
//! `app_data_dir()/phase1`, o auxiliar é configurado por `audio.json` em
//! `app_config_dir()` (ou `LANGUAGE_TUTOR_CONFIG`) e o manifesto de modelos é
//! lido do mesmo ficheiro. Sem Python ou modelos, a captura, a reprodução e a
//! revisão continuam a funcionar; a disponibilidade indica o componente em
//! falta.

pub mod artifacts;
pub mod audio;
pub mod commands;
pub mod db;
pub mod domain;
pub mod interview;
pub mod service;
pub mod storage;
pub mod worker;

use std::sync::Arc;

use tauri::{Emitter, Manager};

use crate::audio::{AudioEvent, NativeAudio};
use crate::domain::{AppError, AppSnapshot};
use crate::service::{AppService, LaunchedWorker, WorkerLauncher};
use crate::storage::AttemptStore;
use crate::worker::{WorkerClient, WorkerConfig};

/// Evento com a fotografia do estado, publicado depois de cada alteração.
pub const PRACTICE_SNAPSHOT_EVENT: &str = "practice://snapshot";

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
            let data_dir = app.path().app_data_dir()?.join("phase1");
            std::fs::create_dir_all(&data_dir)?;
            let config_dir = app.path().app_config_dir()?;

            let config = WorkerConfig::load(&config_dir, &data_dir);
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
                AttemptStore::new(data_dir.clone()),
                Arc::new(audio),
                data_dir,
                launcher,
                models_manifest,
            ));

            let handle = app.handle().clone();
            service.subscribe(Arc::new(move |snapshot: &AppSnapshot| {
                if let Err(error) = handle.emit(PRACTICE_SNAPSHOT_EVENT, snapshot.clone()) {
                    eprintln!("Não foi possível publicar a fotografia: {error}");
                }
            }));

            let audio_service = Arc::clone(&service);
            tauri::async_runtime::spawn(async move {
                while let Some(event) = audio_receiver.recv().await {
                    audio_service.on_audio_event(event).await;
                }
            });

            // O auxiliar arranca em segundo plano: a janela aparece e a
            // captura fica disponível mesmo sem Python ou modelos.
            let starting = Arc::clone(&service);
            tauri::async_runtime::spawn(async move {
                starting.start_worker().await;
            });

            app.manage(service);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::refresh_availability,
            commands::start_recording,
            commands::stop_recording,
            commands::retry_save_recording,
            commands::transcribe_response,
            commands::play_audio,
            commands::stop_audio,
            commands::save_review,
            commands::restart_worker,
            commands::prepare_close,
        ])
        .run(tauri::generate_context!())
        .expect("erro ao arrancar a aplicação Tauri");
}

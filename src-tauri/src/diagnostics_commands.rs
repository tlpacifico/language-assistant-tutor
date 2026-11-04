//! IPC dedicado: não aceita mensagens livres nem parâmetros de entrevistas.
use crate::diagnostics::{Diagnostics, Event, Status};
use std::path::PathBuf;
use tauri::State;

pub struct DiagnosticState {
    pub diagnostics: Diagnostics,
    pub default_directory: PathBuf,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticView {
    #[serde(flatten)]
    status: Status,
    default_directory: PathBuf,
}

#[tauri::command]
pub async fn diagnostics_status(
    state: State<'_, DiagnosticState>,
) -> Result<DiagnosticView, String> {
    let diagnostics = state.diagnostics.clone();
    let default_directory = state.default_directory.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _ = diagnostics.flush();
        DiagnosticView {
            status: diagnostics.status(),
            default_directory,
        }
    })
    .await
    .map_err(|_| "Não foi possível consultar o diagnóstico.".into())
}

#[tauri::command]
pub fn diagnostics_record(event: Event, state: State<'_, DiagnosticState>) -> String {
    state.diagnostics.record(event)
}

#[tauri::command]
pub async fn diagnostics_export(
    directory: Option<String>,
    state: State<'_, DiagnosticState>,
) -> Result<PathBuf, String> {
    let directory = directory
        .map(PathBuf::from)
        .unwrap_or_else(|| state.default_directory.clone());
    let diagnostics = state.diagnostics.clone();
    tauri::async_runtime::spawn_blocking(move || diagnostics.export(&directory))
        .await
        .map_err(|_| "Não foi possível exportar o diagnóstico.".to_owned())?
        .map_err(|_| {
            "Não foi possível exportar. Verifique a pasta e as permissões de escrita.".into()
        })
}

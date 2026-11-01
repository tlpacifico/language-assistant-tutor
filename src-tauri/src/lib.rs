//! Base da aplicação desktop da fase 1.
//!
//! `domain` reúne os contratos e as regras independentes de hardware,
//! `audio` os tipos partilhados com a captura e `storage` os artefactos locais
//! com JSON atómico e recuperação.

pub mod audio;
pub mod domain;
pub mod storage;
pub mod worker;

/// Arranca a janela Tauri.
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("erro ao arrancar a aplicação Tauri");
}

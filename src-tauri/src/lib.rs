//! Base da aplicação desktop da fase 1.
//!
//! `domain` reúne os contratos e as regras independentes de hardware; a
//! interface inicial apresenta apenas a pergunta fixa.

pub mod domain;

/// Arranca a janela Tauri.
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("erro ao arrancar a aplicação Tauri");
}

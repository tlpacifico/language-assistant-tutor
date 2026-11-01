// Impede a janela de consola nas versões de lançamento no Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    language_assistant_tutor::run();
}

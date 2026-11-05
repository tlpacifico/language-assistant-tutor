//! Reprodução de saída (Rodio).
//!
//! A saída é aberta na primeira reprodução e mantida na thread de áudio. O
//! ficheiro é validado e descodificado antes de tocar no dispositivo, para que
//! um caminho inválido devolva `ARTIFACT_MISSING`/`INVALID_AUDIO` sem abrir
//! hardware.

use std::fs::File;
use std::path::Path;

use cpal::traits::HostTrait;
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player};

use crate::audio::device_unavailable;
use crate::domain::{error_code, AppError, ComponentState, ComponentStatus};

/// Saída de áudio, usada apenas pela thread de áudio.
pub struct Playback {
    output: Option<MixerDeviceSink>,
    player: Option<Player>,
}

impl Default for Playback {
    fn default() -> Self {
        Self::new()
    }
}

impl Playback {
    pub fn new() -> Self {
        Self {
            output: None,
            player: None,
        }
    }

    /// Verifica se existe uma saída de áudio predefinida.
    pub fn availability(&self) -> ComponentState {
        let host = cpal::default_host();
        match host.default_output_device() {
            Some(_) => ComponentState::ready(),
            None => ComponentState {
                status: ComponentStatus::Missing,
                message: Some("Não foi encontrada nenhuma saída de áudio.".to_owned()),
            },
        }
    }

    /// Abre e descodifica um WAV, sem tocar no dispositivo.
    fn decode(path: &Path) -> Result<Decoder<File>, AppError> {
        if !path.is_file() {
            return Err(AppError::new(
                error_code::ARTIFACT_MISSING,
                format!(
                    "O áudio a reproduzir não foi encontrado em {}.",
                    path.display()
                ),
                false,
            ));
        }
        let file = File::open(path).map_err(|error| {
            AppError::new(
                error_code::ARTIFACT_MISSING,
                format!(
                    "Não foi possível abrir o áudio a reproduzir em {}: {error}.",
                    path.display()
                ),
                false,
            )
        })?;
        Decoder::new(file).map_err(|error| {
            AppError::new(
                error_code::INVALID_AUDIO,
                format!("O ficheiro de áudio está ilegível e não pode ser reproduzido: {error}."),
                false,
            )
        })
    }

    /// Acrescenta um WAV à fila do mesmo player (cria-o se não houver); um bloco
    /// novo nunca interrompe o que já toca.
    pub fn append(&mut self, path: &Path) -> Result<(), AppError> {
        let decoder = Self::decode(path)?;
        if self.player.is_none() {
            let output = self.output()?;
            self.player = Some(Player::connect_new(output.mixer()));
        }
        if let Some(player) = &self.player {
            player.append(decoder);
        }
        Ok(())
    }

    /// Descodifica e reproduz um WAV já pertencente à aplicação.
    pub fn play(&mut self, path: &Path) -> Result<(), AppError> {
        let decoder = Self::decode(path)?;
        // Uma reprodução nova substitui a anterior em vez de as misturar.
        self.stop()?;
        let output = self.output()?;
        let player = Player::connect_new(output.mixer());
        player.append(decoder);
        self.player = Some(player);
        Ok(())
    }

    /// Pára a reprodução em curso, se existir.
    pub fn stop(&mut self) -> Result<(), AppError> {
        if let Some(player) = self.player.take() {
            player.stop();
        }
        Ok(())
    }

    /// `true` quando o ficheiro chegou ao fim.
    pub fn finished(&self) -> bool {
        self.player.as_ref().is_some_and(|player| player.empty())
    }

    /// Abre a saída predefinida na primeira utilização e reutiliza-a depois.
    fn output(&mut self) -> Result<&MixerDeviceSink, AppError> {
        if self.output.is_none() {
            let output = DeviceSinkBuilder::open_default_sink().map_err(|error| {
                device_unavailable(format!("Não foi possível abrir a saída de áudio: {error}."))
            })?;
            self.output = Some(output);
        }
        self.output
            .as_ref()
            .ok_or_else(|| device_unavailable("A saída de áudio não está disponível."))
    }
}

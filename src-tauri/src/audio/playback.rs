//! Reprodução de saída (Rodio).
//!
//! A saída é aberta na primeira reprodução e mantida na thread de áudio. O
//! ficheiro é validado e descodificado antes de tocar no dispositivo, para que
//! um caminho inválido devolva `ARTIFACT_MISSING`/`INVALID_AUDIO` sem abrir
//! hardware.

use std::fs::File;
use std::path::Path;

use cpal::traits::HostTrait;
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source};

use crate::audio::{device_unavailable, PlaybackProgress};
use crate::domain::{error_code, AppError, ComponentState, ComponentStatus};

/// Saída de áudio, usada apenas pela thread de áudio.
pub struct Playback {
    output: Option<MixerDeviceSink>,
    player: Option<Player>,
    /// `true` enquanto a reprodução está em pausa.
    paused: bool,
    /// Duração (ms) de cada descodificador aceite, na ordem de entrada.
    durations: Vec<u64>,
    /// Duração total (ms) já descodificada e aceite na fila.
    buffered_ms: u64,
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
            paused: false,
            durations: Vec::new(),
            buffered_ms: 0,
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

    /// Duração total (ms) de um descodificador, quando conhecida.
    fn duration_ms(decoder: &Decoder<File>) -> u64 {
        decoder
            .total_duration()
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0)
    }

    /// Acrescenta um WAV à fila do mesmo player (cria-o se não houver); um bloco
    /// novo nunca interrompe o que já toca.
    pub fn append(&mut self, path: &Path) -> Result<(), AppError> {
        let decoder = Self::decode(path)?;
        let duration_ms = Self::duration_ms(&decoder);
        if self.player.is_none() {
            let output = self.output()?;
            self.player = Some(Player::connect_new(output.mixer()));
        }
        if let Some(player) = &self.player {
            player.append(decoder);
        }
        self.durations.push(duration_ms);
        self.buffered_ms = self.buffered_ms.saturating_add(duration_ms);
        Ok(())
    }

    /// Descodifica e reproduz um WAV já pertencente à aplicação.
    pub fn play(&mut self, path: &Path) -> Result<(), AppError> {
        let decoder = Self::decode(path)?;
        let duration_ms = Self::duration_ms(&decoder);
        // Uma reprodução nova substitui a anterior em vez de as misturar.
        self.stop()?;
        let output = self.output()?;
        let player = Player::connect_new(output.mixer());
        player.append(decoder);
        self.player = Some(player);
        self.durations = vec![duration_ms];
        self.buffered_ms = duration_ms;
        Ok(())
    }

    /// Pára a reprodução em curso, se existir.
    pub fn stop(&mut self) -> Result<(), AppError> {
        if let Some(player) = self.player.take() {
            player.stop();
        }
        self.paused = false;
        self.durations.clear();
        self.buffered_ms = 0;
        Ok(())
    }

    /// Pausa a reprodução sem limpar a fila.
    pub fn pause(&mut self) -> Result<(), AppError> {
        if let Some(player) = &self.player {
            player.pause();
        }
        self.paused = true;
        Ok(())
    }

    /// Retoma a reprodução a partir da posição em que ficou.
    pub fn resume(&mut self) -> Result<(), AppError> {
        if let Some(player) = &self.player {
            player.play();
        }
        self.paused = false;
        Ok(())
    }

    /// `true` quando o ficheiro chegou ao fim.
    pub fn finished(&self) -> bool {
        self.player.as_ref().is_some_and(|player| player.empty())
    }

    /// Número de descodificadores já concluídos.
    fn finished_count(&self) -> usize {
        let remaining = self.player.as_ref().map_or(0, |player| player.len());
        self.durations.len().saturating_sub(remaining)
    }

    /// Duração (ms) dos descodificadores já concluídos.
    fn finished_ms(&self) -> u64 {
        self.durations.iter().take(self.finished_count()).sum()
    }

    /// Posição atual da reprodução: o concluído mais a posição na fonte atual.
    fn position(&self) -> u64 {
        let current = self
            .player
            .as_ref()
            .map_or(0, |player| player.get_pos().as_millis() as u64);
        self.finished_ms().saturating_add(current)
    }

    /// Duração total (ms) descodificada e aceite na fila.
    fn buffered(&self) -> u64 {
        self.buffered_ms
    }

    /// Progresso atual da reprodução, quando existe um player.
    pub fn progress(&self) -> Option<PlaybackProgress> {
        self.player.as_ref()?;
        Some(PlaybackProgress {
            position_ms: self.position(),
            buffered_ms: self.buffered(),
            paused: self.paused,
        })
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

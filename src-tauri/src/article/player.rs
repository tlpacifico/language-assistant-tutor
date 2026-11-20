//! Máquinas de estado puras da reprodução e da gravação do modo artigo.
//!
//! `PlayerRuntime` é o único dono da geração e da sequência de reprodução: uma
//! seleção nova aumenta a geração e cada acontecimento aceite aumenta a
//! sequência. `RecordingRuntime` guarda a identidade da captura e só aceita
//! acontecimentos da captura atual, para que uma captura antiga nunca regrida a
//! mais recente. As vistas (`ArticlePlayerView`, `ArticleRecordingView`) são o
//! contrato público espelhado em `src/lib/article-contracts.ts`.

use serde::{Deserialize, Serialize};

/// Estados da reprodução. `Preparing` só passa a `Playing` quando a porta
/// reporta posição audível; uma fila esvaziada com síntese em curso é
/// `Buffering`, nunca `Ended`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayerState {
    Idle,
    Preparing,
    Playing,
    Paused,
    Buffering,
    Ended,
    Failed,
}

/// Origem do áudio de uma reprodução.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayerSource {
    TutorVoice,
    UserRecording,
}

/// Porque começou a reprodução: automática ou pedida explicitamente.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackTrigger {
    Autoplay,
    Explicit,
}

/// Operação que falhou; alimenta o rótulo de recuperação, nunca a mensagem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayerOperation {
    Synthesis,
    Output,
}

/// Falha tipada da reprodução, para escolher a repetição certa.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerFailure {
    pub operation: PlayerOperation,
    pub retryable: bool,
}

/// Resumo do estado de reprodução; é o campo `audio` da fotografia e o corpo do
/// evento `article://player`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArticlePlayerView {
    pub session_id: String,
    pub turn_id: Option<String>,
    pub version_id: Option<String>,
    pub generation: u64,
    pub sequence: u64,
    pub state: PlayerState,
    pub position_ms: u64,
    /// Só `Some` para uma gravação completa; enquanto gera fica `None`.
    pub duration_ms: Option<u64>,
    /// Duração preparada; rotulada como tal, nunca um total final.
    pub buffered_ms: Option<u64>,
    pub source: Option<PlayerSource>,
    pub trigger: Option<PlaybackTrigger>,
    pub failure: Option<PlayerFailure>,
}

impl ArticlePlayerView {
    /// Vista `Idle` sem seleção, usada nas fotografias persistidas.
    pub fn idle(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            turn_id: None,
            version_id: None,
            generation: 0,
            sequence: 0,
            state: PlayerState::Idle,
            position_ms: 0,
            duration_ms: None,
            buffered_ms: None,
            source: None,
            trigger: None,
            failure: None,
        }
    }
}

/// Estados da gravação de uma resposta; `Active` só depois de a captura ser
/// reconhecida pela porta de áudio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingState {
    Inactive,
    Active,
    Ended,
}

/// Resumo do estado de gravação; é o campo `recording` da fotografia e o corpo
/// do evento `article://recording`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArticleRecordingView {
    pub session_id: String,
    pub turn_id: String,
    /// Identidade da captura; muda a cada reconhecimento.
    pub capture_id: String,
    pub sequence: u64,
    pub state: RecordingState,
    pub elapsed_ms: u64,
    /// Nível normalizado (0.0..=1.0) das amostras capturadas; nunca persistido.
    pub level: Option<f32>,
    pub available: bool,
}

impl ArticleRecordingView {
    /// Vista `Inactive` sem captura: `turn_id`/`capture_id` vazios.
    pub fn inactive(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            turn_id: String::new(),
            capture_id: String::new(),
            sequence: 0,
            state: RecordingState::Inactive,
            elapsed_ms: 0,
            level: None,
            available: true,
        }
    }
}

/// Acontecimento da reprodução entregue pela orquestração ao núcleo puro.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerEvent {
    /// Progresso audível reportado pela porta; `position_ms == 0` é só um bloco
    /// na fila, não prova audível.
    Progress {
        position_ms: u64,
        buffered_ms: u64,
    },
    /// A fila esvaziou; sem síntese em curso é `Ended`, senão `Buffering`.
    Drained,
    /// Síntese concluída: a duração total passa a ser conhecida.
    Completed {
        duration_ms: u64,
    },
    Pause,
    Resume,
    Stop,
    Failed {
        operation: PlayerOperation,
        retryable: bool,
    },
}

/// Máquina de estados pura da reprodução. Única dona de `generation`/`sequence`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerRuntime {
    view: ArticlePlayerView,
    /// Há áudio na fila do dispositivo (uma reprodução está ativa). Distingue
    /// um `Buffering` com a fila vazia (fim) de um `Buffering` com áudio ainda
    /// na fila depois de a geração retomar.
    queued: bool,
}

impl PlayerRuntime {
    /// Reprodução `Idle` para uma sessão, sem seleção.
    pub fn idle(session_id: impl Into<String>) -> Self {
        Self {
            view: ArticlePlayerView::idle(session_id),
            queued: false,
        }
    }

    pub fn view(&self) -> ArticlePlayerView {
        self.view.clone()
    }

    pub fn generation(&self) -> u64 {
        self.view.generation
    }

    pub fn sequence(&self) -> u64 {
        self.view.sequence
    }

    /// Marca que há áudio na fila do dispositivo (uma reprodução foi iniciada
    /// ou retomada); só um `Drained` volta a esvaziar a marca.
    pub fn note_queued(&mut self) {
        self.queued = true;
    }

    /// Invalida a reprodução atual: aumenta a geração e regressa a `Idle` sem
    /// seleção, para que os controlos da geração anterior sejam rejeitados.
    pub fn invalidate(&mut self) -> ArticlePlayerView {
        self.view.generation += 1;
        self.view.sequence += 1;
        self.view.state = PlayerState::Idle;
        self.view.turn_id = None;
        self.view.version_id = None;
        self.view.position_ms = 0;
        self.view.duration_ms = None;
        self.view.buffered_ms = None;
        self.view.source = None;
        self.view.trigger = None;
        self.view.failure = None;
        self.queued = false;
        self.view()
    }

    /// Seleciona uma nova gravação do início: aumenta a geração, volta a
    /// `Preparing` e limpa a posição, a duração e a falha anteriores.
    pub fn select(
        &mut self,
        turn_id: Option<String>,
        version_id: Option<String>,
        source: PlayerSource,
        trigger: PlaybackTrigger,
    ) -> ArticlePlayerView {
        self.view.generation += 1;
        self.view.sequence += 1;
        self.view.turn_id = turn_id;
        self.view.version_id = version_id;
        self.view.state = PlayerState::Preparing;
        self.view.position_ms = 0;
        self.view.duration_ms = None;
        self.view.buffered_ms = None;
        self.view.source = Some(source);
        self.view.trigger = Some(trigger);
        self.view.failure = None;
        self.queued = false;
        self.view()
    }

    /// Aplica um acontecimento à geração indicada. Rejeita uma geração que não
    /// seja a atual e só avança a sequência numa mudança aceite.
    pub fn apply(&mut self, generation: u64, event: PlayerEvent) -> bool {
        if generation != self.view.generation {
            return false;
        }
        if self.step(event) {
            self.view.sequence += 1;
            true
        } else {
            false
        }
    }

    fn step(&mut self, event: PlayerEvent) -> bool {
        use PlayerState as S;
        match event {
            PlayerEvent::Progress {
                position_ms,
                buffered_ms,
            } => {
                let buffered = self.note_buffered(buffered_ms);
                match self.view.state {
                    S::Preparing => {
                        if position_ms > 0 {
                            self.queued = true;
                            self.view.position_ms = position_ms;
                            self.view.state = S::Playing;
                            true
                        } else {
                            buffered
                        }
                    }
                    S::Buffering => {
                        if position_ms > self.view.position_ms {
                            self.queued = true;
                            self.view.position_ms = position_ms;
                            self.view.state = S::Playing;
                            true
                        } else {
                            buffered
                        }
                    }
                    S::Playing => {
                        if position_ms > self.view.position_ms {
                            self.view.position_ms = position_ms;
                            true
                        } else {
                            buffered
                        }
                    }
                    _ => buffered,
                }
            }
            PlayerEvent::Drained => {
                self.queued = false;
                match self.view.state {
                    S::Preparing | S::Playing | S::Buffering => {
                        self.view.position_ms = self
                            .view
                            .duration_ms
                            .or(self.view.buffered_ms)
                            .unwrap_or(self.view.position_ms);
                        self.view.state = if self.view.duration_ms.is_some() {
                            S::Ended
                        } else {
                            S::Buffering
                        };
                        true
                    }
                    _ => false,
                }
            }
            PlayerEvent::Completed { duration_ms } => {
                let mut changed = false;
                if self.view.duration_ms != Some(duration_ms) {
                    self.view.duration_ms = Some(duration_ms);
                    changed = true;
                }
                if self.view.buffered_ms != Some(duration_ms) {
                    self.view.buffered_ms = Some(duration_ms);
                    changed = true;
                }
                // A síntese terminou: só conclui quando a fila está vazia. Com
                // áudio ainda na fila (geração retomada depois de esvaziar) o
                // fim só chega pelo `Drained` da reprodução atual.
                if self.view.state == S::Buffering && !self.queued {
                    self.view.state = S::Ended;
                    changed = true;
                }
                changed
            }
            PlayerEvent::Pause => match self.view.state {
                S::Preparing | S::Playing | S::Buffering => {
                    self.view.state = S::Paused;
                    true
                }
                _ => false,
            },
            PlayerEvent::Resume => match self.view.state {
                S::Paused => {
                    let queued = self.queued
                        && self
                            .view
                            .buffered_ms
                            .is_some_and(|b| b > self.view.position_ms);
                    self.view.state = if queued { S::Playing } else { S::Buffering };
                    true
                }
                _ => false,
            },
            PlayerEvent::Stop => {
                if self.view.state == S::Idle {
                    return false;
                }
                self.queued = false;
                self.view.state = S::Idle;
                self.view.turn_id = None;
                self.view.version_id = None;
                self.view.position_ms = 0;
                self.view.duration_ms = None;
                self.view.buffered_ms = None;
                self.view.source = None;
                self.view.trigger = None;
                self.view.failure = None;
                true
            }
            PlayerEvent::Failed {
                operation,
                retryable,
            } => {
                let failure = PlayerFailure {
                    operation,
                    retryable,
                };
                if self.view.state == S::Failed && self.view.failure.as_ref() == Some(&failure) {
                    return false;
                }
                self.view.failure = Some(failure);
                self.view.state = S::Failed;
                true
            }
        }
    }

    /// Regista a duração preparada, só se crescer; zero não conta.
    fn note_buffered(&mut self, buffered_ms: u64) -> bool {
        if buffered_ms == 0 {
            return false;
        }
        match self.view.buffered_ms {
            Some(current) if current >= buffered_ms => false,
            _ => {
                self.view.buffered_ms = Some(buffered_ms);
                true
            }
        }
    }
}

/// Máquina de estados pura da gravação. Dona de `capture_id`/`sequence`; uma
/// captura antiga nunca substitui a atual.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordingRuntime {
    view: ArticleRecordingView,
}

impl RecordingRuntime {
    /// Gravação `Inactive` para uma sessão, sem captura.
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            view: ArticleRecordingView::inactive(session_id),
        }
    }

    pub fn view(&self) -> ArticleRecordingView {
        self.view.clone()
    }

    pub fn sequence(&self) -> u64 {
        self.view.sequence
    }

    pub fn capture_id(&self) -> &str {
        &self.view.capture_id
    }

    /// Captura reconhecida pela porta de áudio: passa a `Active`. Só aceita a
    /// partir de `Inactive`; um `start` tardio ou duplicado (com a identidade
    /// igual ou diferente) deixa a vista e a sequência intactas.
    pub fn start(
        &mut self,
        turn_id: impl Into<String>,
        capture_id: impl Into<String>,
        available: bool,
    ) -> ArticleRecordingView {
        if self.view.state != RecordingState::Inactive {
            return self.view();
        }
        self.view.turn_id = turn_id.into();
        self.view.capture_id = capture_id.into();
        self.view.state = RecordingState::Active;
        self.view.elapsed_ms = 0;
        self.view.level = None;
        self.view.available = available;
        self.view.sequence += 1;
        self.view()
    }

    /// Progresso da captura; só a identidade atual e só enquanto `Active`.
    pub fn progress(&mut self, capture_id: &str, elapsed_ms: u64, level: Option<f32>) -> bool {
        if self.view.state != RecordingState::Active || capture_id != self.view.capture_id {
            return false;
        }
        self.view.elapsed_ms = elapsed_ms;
        self.view.level = level;
        self.view.sequence += 1;
        true
    }

    /// Fim da captura: `Ended` com o tempo final e sem nível; só a identidade
    /// atual e só enquanto `Active`.
    pub fn end(&mut self, capture_id: &str, elapsed_ms: u64) -> bool {
        if self.view.state != RecordingState::Active || capture_id != self.view.capture_id {
            return false;
        }
        self.view.state = RecordingState::Ended;
        self.view.elapsed_ms = elapsed_ms;
        self.view.level = None;
        self.view.sequence += 1;
        true
    }

    /// Regressa a `Inactive` depois de o turno ser consumido/apagado ou no
    /// reinício; limpa a identidade da captura.
    pub fn inactive(&mut self) -> ArticleRecordingView {
        self.view.turn_id.clear();
        self.view.capture_id.clear();
        self.view.state = RecordingState::Inactive;
        self.view.elapsed_ms = 0;
        self.view.level = None;
        self.view.available = true;
        self.view.sequence += 1;
        self.view()
    }
}

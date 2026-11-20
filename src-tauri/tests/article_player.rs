//! Máquina de estados pura da reprodução e da gravação do modo artigo.

use language_assistant_tutor::article::{
    ArticlePlayerView, ArticleRecordingView, PlaybackTrigger, PlayerEvent, PlayerOperation,
    PlayerRuntime, PlayerSource, PlayerState, RecordingRuntime, RecordingState,
};

fn select(player: &mut PlayerRuntime, turn: &str, version: &str) {
    player.select(
        Some(turn.into()),
        Some(version.into()),
        PlayerSource::TutorVoice,
        PlaybackTrigger::Autoplay,
    );
}

#[test]
fn preparing_only_becomes_playing_after_playback_progress() {
    let mut p = PlayerRuntime::idle("session-1");
    select(&mut p, "turn-3", "version-3");
    assert_eq!(p.view().state, PlayerState::Preparing);
    // Um bloco na fila não é prova audível: continua a preparar.
    assert!(p.apply(
        1,
        PlayerEvent::Progress {
            position_ms: 0,
            buffered_ms: 1200,
        }
    ));
    assert_eq!(p.view().state, PlayerState::Preparing);
    assert_eq!(p.view().buffered_ms, Some(1200));
    // O primeiro progresso audível passa a reproduzir.
    assert!(p.apply(
        1,
        PlayerEvent::Progress {
            position_ms: 300,
            buffered_ms: 1200,
        }
    ));
    assert_eq!(p.view().state, PlayerState::Playing);
    assert_eq!(p.view().position_ms, 300);
}

#[test]
fn empty_queue_while_generating_is_buffering_not_ended() {
    let mut p = PlayerRuntime::idle("session-1");
    select(&mut p, "turn-3", "version-3");
    p.apply(
        1,
        PlayerEvent::Progress {
            position_ms: 200,
            buffered_ms: 2000,
        },
    );
    assert_eq!(p.view().state, PlayerState::Playing);
    // A fila esvazia com síntese em curso: buffering, não ended.
    assert!(p.apply(1, PlayerEvent::Drained));
    assert_eq!(p.view().state, PlayerState::Buffering);
    // O progresso seguinte, depois de um bloco, volta a reproduzir.
    assert!(p.apply(
        1,
        PlayerEvent::Progress {
            position_ms: 2200,
            buffered_ms: 3000,
        }
    ));
    assert_eq!(p.view().state, PlayerState::Playing);
}

#[test]
fn pause_during_generation_stays_paused_and_keeps_position() {
    let mut p = PlayerRuntime::idle("session-1");
    select(&mut p, "turn-3", "version-3");
    p.apply(
        1,
        PlayerEvent::Progress {
            position_ms: 500,
            buffered_ms: 1000,
        },
    );
    p.apply(1, PlayerEvent::Pause);
    assert_eq!(p.view().state, PlayerState::Paused);
    assert_eq!(p.view().position_ms, 500);
    // A síntese a terminar em pausa fica em pausa e mantém a posição.
    assert!(p.apply(1, PlayerEvent::Completed { duration_ms: 2400 }));
    assert_eq!(p.view().state, PlayerState::Paused);
    assert_eq!(p.view().position_ms, 500);
    assert_eq!(p.view().duration_ms, Some(2400));
    // Retomar com áudio na fila volta a reproduzir sem perder a posição.
    assert!(p.apply(1, PlayerEvent::Resume));
    assert_eq!(p.view().state, PlayerState::Playing);
    assert_eq!(p.view().position_ms, 500);
}

#[test]
fn synthesis_without_chunks_fails_but_keeps_text() {
    let mut p = PlayerRuntime::idle("session-1");
    select(&mut p, "turn-3", "version-3");
    assert!(p.apply(
        1,
        PlayerEvent::Failed {
            operation: PlayerOperation::Synthesis,
            retryable: true,
        }
    ));
    assert_eq!(p.view().state, PlayerState::Failed);
    // O texto (turno e versão) continua visível para repetir só a voz.
    assert_eq!(p.view().turn_id.as_deref(), Some("turn-3"));
    assert_eq!(p.view().version_id.as_deref(), Some("version-3"));
    assert_eq!(
        p.view().failure.as_ref().map(|f| f.operation),
        Some(PlayerOperation::Synthesis)
    );
}

#[test]
fn stop_returns_to_idle_without_a_selection() {
    let mut p = PlayerRuntime::idle("session-1");
    select(&mut p, "turn-3", "version-3");
    p.apply(
        1,
        PlayerEvent::Progress {
            position_ms: 300,
            buffered_ms: 1200,
        },
    );
    assert_eq!(p.view().state, PlayerState::Playing);
    assert!(p.apply(1, PlayerEvent::Stop));
    assert_eq!(p.view().state, PlayerState::Idle);
    assert_eq!(p.view().turn_id, None);
    assert_eq!(p.view().version_id, None);
    assert_eq!(p.view().source, None);
    assert_eq!(p.view().position_ms, 0);
}

#[test]
fn stale_generation_is_ignored_and_sequence_is_monotonic() {
    let mut p = PlayerRuntime::idle("session-1");
    select(&mut p, "turn-3", "version-3");
    assert_eq!(p.generation(), 1);
    assert_eq!(p.sequence(), 1);
    // Uma geração antiga é recusada e não avança a sequência.
    assert!(!p.apply(
        0,
        PlayerEvent::Progress {
            position_ms: 300,
            buffered_ms: 1200,
        }
    ));
    assert_eq!(p.sequence(), 1);
    // A geração atual avança a sequência em cada mudança aceite.
    let mut last = p.sequence();
    assert!(p.apply(
        1,
        PlayerEvent::Progress {
            position_ms: 0,
            buffered_ms: 1200,
        }
    ));
    assert!(p.sequence() > last);
    last = p.sequence();
    assert!(p.apply(
        1,
        PlayerEvent::Progress {
            position_ms: 300,
            buffered_ms: 1200,
        }
    ));
    assert!(p.sequence() > last);
    // Uma nova seleção muda a geração; a anterior deixa de ser aceite.
    let generation = p.generation();
    p.select(
        Some("turn-4".into()),
        Some("version-4".into()),
        PlayerSource::TutorVoice,
        PlaybackTrigger::Explicit,
    );
    assert!(p.generation() > generation);
    assert!(!p.apply(generation, PlayerEvent::Pause));
}

#[test]
fn a_finished_recording_ignores_late_active_events() {
    let mut r = RecordingRuntime::new("session-1");
    r.start("turn-1", "capture-1", true);
    assert_eq!(r.view().state, RecordingState::Active);
    assert_eq!(r.view().capture_id, "capture-1");
    assert!(r.end("capture-1", 1200));
    assert_eq!(r.view().state, RecordingState::Ended);
    assert_eq!(r.view().elapsed_ms, 1200);
    assert_eq!(r.view().level, None);
    let sequence = r.sequence();
    // Um "Active" tardio da mesma captura já terminada é ignorado.
    r.start("turn-1", "capture-1", true);
    assert_eq!(r.view().state, RecordingState::Ended);
    assert_eq!(r.view().elapsed_ms, 1200);
    assert_eq!(r.sequence(), sequence);
    // Progresso e fim tardios de outra identidade também são ignorados.
    assert!(!r.progress("capture-0", 999, Some(0.5)));
    assert!(!r.end("capture-0", 999));
    assert_eq!(r.view().state, RecordingState::Ended);
}

#[test]
fn player_and_recording_views_serialize_with_camel_case() {
    let player = ArticlePlayerView {
        session_id: "session-1".into(),
        turn_id: Some("turn-3".into()),
        version_id: Some("version-3".into()),
        generation: 2,
        sequence: 7,
        state: PlayerState::Buffering,
        position_ms: 1500,
        duration_ms: None,
        buffered_ms: Some(2200),
        source: Some(PlayerSource::TutorVoice),
        trigger: Some(PlaybackTrigger::Autoplay),
        failure: None,
    };
    let json = serde_json::to_value(&player).unwrap();
    assert_eq!(json["sessionId"], "session-1");
    assert_eq!(json["turnId"], "turn-3");
    assert_eq!(json["versionId"], "version-3");
    assert_eq!(json["generation"], 2);
    assert_eq!(json["sequence"], 7);
    assert_eq!(json["state"], "buffering");
    assert_eq!(json["positionMs"], 1500);
    assert!(json["durationMs"].is_null());
    assert_eq!(json["bufferedMs"], 2200);
    assert_eq!(json["source"], "tutor_voice");
    assert_eq!(json["trigger"], "autoplay");
    assert!(json["failure"].is_null());
    let round: ArticlePlayerView = serde_json::from_value(json).unwrap();
    assert_eq!(round, player);

    let recording = ArticleRecordingView {
        session_id: "session-1".into(),
        turn_id: "turn-1".into(),
        capture_id: "capture-9".into(),
        sequence: 4,
        state: RecordingState::Active,
        elapsed_ms: 800,
        level: Some(0.5),
        available: true,
    };
    let json = serde_json::to_value(&recording).unwrap();
    assert_eq!(json["sessionId"], "session-1");
    assert_eq!(json["turnId"], "turn-1");
    assert_eq!(json["captureId"], "capture-9");
    assert_eq!(json["sequence"], 4);
    assert_eq!(json["state"], "active");
    assert_eq!(json["elapsedMs"], 800);
    assert_eq!(json["level"], 0.5);
    assert_eq!(json["available"], true);
    let round: ArticleRecordingView = serde_json::from_value(json).unwrap();
    assert_eq!(round, recording);
}

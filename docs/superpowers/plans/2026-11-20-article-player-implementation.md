# Article player and chat controls — R1 implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task by task. Steps use checkboxes (`- [ ]`) for tracking.

**Goal:** make the article conversation a predictable listen/respond loop: a persistent bar, real pause/resume with honest progress, per-message playback, a «Responder agora» handoff, recording feedback, transcript placeholders, scroll that respects the reader, a local article panel, saved preferences, keyboard access and operation-specific recovery.

**Architecture:** the Rust core keeps ownership of capture and playback; the audio port gains a unique playback identity in every mode, pause/resume and position, and the article runtime becomes a player state machine with generation and sequence. Completed tutor audio is cached by text/voice/speed/engine identity, so replay and reopen never call the text provider. React renders snapshots plus two scoped event streams (`article://player`, `article://recording`) and never turns `preparing` into `playing` on its own.

**Tech Stack:** React 19/TypeScript, Tauri 2/Rust, rusqlite (schema v6), CPAL/Rodio 0.23, Python helper with Kokoro/faster-whisper, OpenCode Go. No new dependency.

**Spec:** [Article conversation player — R1 design](../specs/2026-11-20-article-player-design.md). Roadmap: [R1](../../article-conversation/roadmap.md#r1--conversation-controls-and-chat).

**Status:** proposed plan, not executed. Writing it does not start implementation, accept the article mode, measure performance or start phase 5. Execution method chosen on 2026-11-20: **subagent-driven** (one subagent per task with a fresh review), to be used only after the user confirms the plan and authorizes the start.

## Global constraints

- Interface, labels, errors and hints in European Portuguese; tutor text, article text and playback content in English.
- No new dependency and no new audio engine; keep Tauri identity, `app_data_dir()/diagnostics` and the existing helper protocol.
- The core is authoritative for capture, playback ownership, transitions, cache identity and preferences; the frontend never infers a player transition.
- Persisted session versions stay separate from runtime events: progress and ticks never write SQLite and never increment `article_sessions.version`.
- Recording starts and stops only by explicit user action; only text goes to the provider, after per-session authorization.
- Keep audio, transcripts, article text, prompts, credentials and meter values out of diagnostics; new operations/codes update the closed lists.
- Every runtime event carries session, turn/version identity, playback generation and a monotonically increasing sequence; older events are ignored.
- No draggable seek, no automatic speech-end detection, no simultaneous record/play, no live transcription, no pronunciation/fluency/confidence claims.
- Do not mark R1 accepted, phases 1–4 accepted, or start phase 5. Manual acceptance (R1-01…R1-16) stays pending.
- No paid inference in automated tests; cache/synthesis reuse is tested with local fixtures only.

## Review focus

1. «Responder agora» while synthesis is still producing chunks: no chunk may reach the speaker after capture starts, and the discarded work must not leave a playable queue (tasks 6 and 7).
2. Pause with an empty buffer while the helper is still synthesizing: the item must stay `paused`, keep its position and never be reported as `ended` (tasks 4 and 6).
3. A late `PlaybackFinished`/`PlaybackProgress` from a previous item, session or STAR playback arriving after a new item started: every playback has a unique id in every mode, and only the current one may be stopped or moved (tasks 2, 4, 6 and 11).
4. Missing, corrupt, partial or extra-speed cache entry: explicit retry, never a text-provider request and never partial audio presented as complete (tasks 5 and 6).
5. Autoplay disabled/enabled while a tutor turn lands and while navigating away: no retroactive sound, no orphan autoplay intent, explicit replay unaffected (tasks 6, 8 and 9).

## Technical decisions (resolve the spec's open points)

### Player contract and events

`ArticlePlayerView` replaces the three-value `ArticleAudioView`. It is both the `audio` field of `ArticleSnapshot` (last state summary) and the payload of `article://player`:

```rust
pub enum PlayerState { Idle, Preparing, Playing, Paused, Buffering, Ended, Failed }
pub enum PlayerSource { TutorVoice, UserRecording }
pub enum PlaybackTrigger { Autoplay, Explicit }

pub struct PlayerFailure { pub operation: PlayerOperation, pub retryable: bool }
pub enum PlayerOperation { Synthesis, Output }

pub struct ArticlePlayerView {
    pub session_id: String,
    pub turn_id: Option<String>,
    pub version_id: Option<String>,
    pub generation: u64,
    pub sequence: u64,
    pub state: PlayerState,
    pub position_ms: u64,
    pub duration_ms: Option<u64>, // Some only for a complete rendition
    pub buffered_ms: Option<u64>, // labelled "preparado", never a final total
    pub source: Option<PlayerSource>,
    pub trigger: Option<PlaybackTrigger>,
    pub failure: Option<PlayerFailure>,
}

pub struct ArticleRecordingView {
    pub session_id: String,
    pub turn_id: String,
    /// Identidade desta captura; muda a cada `start_capture`.
    pub capture_id: String,
    pub sequence: u64,
    pub state: RecordingState, // Inactive | Active | Ended
    pub elapsed_ms: u64,
    pub level: Option<f32>, // 0.0..=1.0 from captured samples; never persisted
    pub available: bool,
}
```

`ArticleSnapshot` gains `recording: ArticleRecordingView`, so reopening or resubscribing recovers the current indicator instead of relying on a remembered event. `Ended`/`Inactive` hide the indicator; a late `Active` from an older `capture_id` is ignored.

`duration_ms = Some(buffered_ms)` only when the rendition is complete (synthesis result accepted, or a user recording). While generating, `duration_ms = None` and the UI shows elapsed plus «A preparar o restante áudio…».

### Playback identity across every mode

Every playback gets an identity generated by the audio port, not by the caller:

```rust
pub type PlaybackId = String; // opaque, unique per process

fn start_playback(&self, path: PathBuf) -> Result<PlaybackId, AppError>; // always replaces
fn append_chunk(&self, path: PathBuf, id: &PlaybackId) -> Result<(), AppError>; // same rendition only
fn pause_playback(&self, id: &PlaybackId) -> Result<(), AppError>;
fn resume_playback(&self, id: &PlaybackId) -> Result<(), AppError>;
fn stop_playback(&self, id: &PlaybackId) -> Result<(), AppError>;
fn stop_all_playback(&self) -> Result<(), AppError>; // shutdown/close only
```

`start_playback` is unconditional, so selecting a new message or a STAR answer always replaces the previous item. Only `append`, `pause`, `resume` and `stop` are scoped: an id that is not current is ignored without touching the device. `PlaybackProgress` and `PlaybackFinished` always carry the id; STAR and practice store the id returned by their `start_playback` in `state.playback_id` and clear it only for a matching finish. There is no `owner: None` path, so an old event is never indistinguishable from the current one in any mode.

### Typed recovery operation

`ArticleSessionView` gains `recovery: Option<ArticleRecovery>` derived from the persisted `failed_from`, never from the free-form message:

```rust
pub enum ArticleRecovery { RetrySend, RetryFeedback, ReRecord }
```

`AwaitingTutor → RetrySend`, `Finishing | Completed → RetryFeedback`, `Recording → ReRecord`. The field is part of the persisted snapshot, so the correct label survives reopening the application; the frontend never guesses it from `code` or `message`.

### Audio APIs verified against installed versions

Rodio 0.23.0 exposes `Player::{append, pause, play, is_paused, stop, empty, len, get_pos}` and `Decoder::total_duration` (symphonia WAV backend, enabled by rodio's default `wav` feature, reads the standard PCM16 header written by the helper and captures). `get_pos()` is per appended source, so the audio thread tracks queued chunk durations and reports `position = sum(finished chunks) + get_pos()`; paused time is excluded because only played samples advance it. If a file has no frame count, `total_duration` is `None` and the UI keeps the honest unknown-duration state. CPAL 0.18 gives no level API: the capture callback computes the peak absolute sample per block into an `AtomicU16` (0–1000), read by the audio thread on its polling loop. No `try_seek` is used in R1.

### Voice cache

Directory `articles/<sessionId>/voice-cache/<key>/` with `manifest.json`; the key is the SHA-256 (existing `sha2`) of `text` + voice + `speedX10` + engine id `kokoro-82m-v1`. `manifest.json` is written with the existing `atomicwrites` only when the helper reports a non-cancelled completion with at least one chunk; an entry without a valid complete manifest is invalid, removed and regenerated. Chunk names stay `chunk-NNNN.wav`. Session deletion already removes `articles/<id>` after purge, so it also removes the cache; no arbitrary path crosses the public contract.

### Preferences and schema

SQLite migration `006` adds `article_preferences` (single row) with `autoplay INTEGER` and `speed_x10 INTEGER CHECK (speed_x10 IN (8,10,12))`. Defaults: autoplay `1`, speed `10` (1.0×). Older databases migrate with these defaults and older sessions open with no playback and no inference. Speeds are integers in the contract (`8|10|12`) to avoid float equality problems.

### Test placement

- Rust port/device behaviour: `src-tauri/tests/audio.rs` (controlled adapter, no hardware).
- Player transitions: new `src-tauri/tests/article_player.rs`.
- Cache and stream protocol: `src-tauri/tests/article_voice.rs`.
- Service orchestration: `src-tauri/tests/article_service.rs`.
- Schema: `src-tauri/tests/article_migration.rs` and `article_repository.rs`.
- React: `src/features/articles/*.test.tsx`, hook test `useArticlePlayback.test.tsx`, fake client in `src/test/fake-article-client.ts`.
- End commands: `.\scripts\verify.ps1` (8 checks) and, if packaging is touched, the documented `--no-bundle` build.

## File map

- `src-tauri/src/db/migrations/006_article_preferences.sql` (new), `db/mod.rs`, `db/articles.rs` (or new `db/preferences.rs`): schema and repository.
- `src-tauri/src/audio/playback.rs`, `audio/capture.rs`, `audio/mod.rs`: playback identity, pause/resume, progress, peak level, capture progress.
- `src-tauri/src/article/player.rs` (new), `article/cache.rs` (new), `article/types.rs`, `article/mod.rs`: contracts and pure logic.
- `src-tauri/src/service/article.rs`, `article_commands.rs`, `service.rs`, `lib.rs`, `diagnostics.rs`: orchestration, events, commands, closed lists.
- `src/lib/article-contracts.ts`, `src/lib/article-client.ts`: mirror and adapter.
- `src/features/articles/{ArticleConversation,ArticleReport,Articles}.tsx`, new `ArticlePlayerBar.tsx`, `ArticleTurnControls.tsx`, `ArticlePreferences.tsx`, `useArticlePlayback.ts`, `useFollowTail.ts`, `useArticleShortcuts.ts`, `player.ts`, `articles.css`.
- `src/test/fake-article-client.ts`, `contracts/article-v1.examples.json`.
- Documentation: `docs/article-conversation/{roadmap,validation,setup,performance}.md`, `docs/diagnostics/validation.md`, spec status line.

## Task 1 — schema v6: preferences and typed recovery

**Files:** create `src-tauri/src/db/migrations/006_article_preferences.sql`; modify `src-tauri/src/db/mod.rs`, `src-tauri/src/db/articles.rs`, `src-tauri/src/article/types.rs`, `src-tauri/src/service/article.rs`, `src-tauri/src/article_commands.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/diagnostics.rs`, `src/lib/article-contracts.ts`, `src/lib/article-client.ts`, `contracts/article-v1.examples.json`; tests `src-tauri/tests/article_migration.rs`, `src-tauri/tests/article_repository.rs`, `src-tauri/tests/database.rs`, `src-tauri/tests/phase4_migration.rs`.

**Interfaces:**
- Consumes: existing `Database::open_with_migrations`, `MIGRATION_V1..V5`, `DATABASE_VERSION`, `failed_from`.
- Produces: `ArticlePreferences { autoplay: bool, speed_x10: u8 }`; `Database::article_preferences() -> Result<ArticlePreferences>`; `Database::put_article_preferences(&ArticlePreferences) -> Result<ArticlePreferences>`; `ArticleRecovery { RetrySend, RetryFeedback, ReRecord }` and `ArticleSessionView.recovery: Option<ArticleRecovery>`; commands `article_preferences_load` / `article_preferences_save`; TS `ArticlePreferences { autoplay: boolean; speedX10: 8 | 10 | 12 }`, `ArticleRecovery = "retry_send" | "retry_feedback" | "re_record"` and `ArticleClient.preferences()` / `savePreferences(input)`.

- [ ] Write the migration test `preferences_default_to_autoplay_at_normal_speed_and_keep_star_data`: apply v1–v5 with a STAR session, open with all migrations, assert `user_version = 6`, one preferences row `(autoplay = 1, speed_x10 = 10)`, the STAR dump unchanged and every article table still present. Run `cargo test --manifest-path src-tauri/Cargo.toml --test article_migration` and confirm it fails on the unknown column.
- [ ] Add `MIGRATION_V6`, bump `DATABASE_VERSION` to `6`, add `v6: &str` to `open_with_migrations` (and pass it from `open_with_schema`), then update **all** call sites so the workspace compiles: the two calls in `article_migration.rs`, the call in `phase4_migration.rs` and any test helper. Change `apply_upto` to iterate `[MIGRATION_V1, …, MIGRATION_V6]` so versions 1–5 can be seeded. Rerun the migration test.
- [ ] Update the version assertions that would otherwise lock v5: `database.rs` (`assert_eq!(report.0, 5, …)` becomes `6`) and `article_migration.rs` (`every_previous_version_migrates_to_v5…` becomes `…to_v6…`, loop `1..=5`, v0 case, plus `assert_eq!(DATABASE_VERSION, 6)`). Add `invalid_v6_migration_rolls_back_to_v5_keeping_star_and_article_data`: seed v5 with the STAR dump and one `article_sessions` row, inject a broken v6, assert `user_version = 5`, the STAR dump and the article row unchanged, and no `article_preferences` table. Run the whole `cargo test --manifest-path src-tauri/Cargo.toml --test database --test phase4_migration --test article_migration`.
- [ ] Add `article_preferences` / `put_article_preferences` with `validate_article_preferences` (speed only `8|10|12`) and repository tests `preferences_are_read_back_and_reject_an_unknown_speed`. Project `recovery` from the persisted `failed_from` (`AwaitingTutor → retry_send`, `Finishing | Completed → retry_feedback`, `Recording → re_record`, no error → `null`) and test `a_failed_send_exposes_retry_send_after_reopen`, `a_failed_feedback_exposes_retry_feedback`, `a_capture_failure_exposes_re_record` and `no_error_means_no_recovery_operation`. Run `cargo test --manifest-path src-tauri/Cargo.toml --test article_repository`.
- [ ] Add both Tauri commands returning the validated view, register them in `lib.rs`, add them to `COMMANDS` in `diagnostics.rs` and to the `docs/diagnostics/validation.md` command list. Run `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`.
- [ ] Mirror the types and methods in `src/lib/article-contracts.ts` / `article-client.ts`, add `preferences`, `preferencesSaved`, `retrySendRecovery` and `retryFeedbackRecovery` examples to `contracts/article-v1.examples.json`, extend `fakeArticleClient` with `preferences`/`savePreferences`, and run `npm.cmd run typecheck` plus `npm.cmd test`.

```sql
CREATE TABLE article_preferences (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    autoplay INTEGER NOT NULL CHECK (autoplay IN (0, 1)),
    speed_x10 INTEGER NOT NULL CHECK (speed_x10 IN (8, 10, 12)),
    updated_at TEXT NOT NULL
);
INSERT INTO article_preferences (id, autoplay, speed_x10, updated_at)
VALUES (1, 1, 10, '2026-11-20T00:00:00Z');
```

## Task 2 — audio port: unique playback identity, pause/resume and position

**Files:** modify `src-tauri/src/audio/playback.rs`, `src-tauri/src/audio/mod.rs`, `src-tauri/src/service.rs`; test `src-tauri/tests/audio.rs`.

**Interfaces:**
- Consumes: `AudioPort`, `AudioBackend`, `AudioEvent`, `Playback`, `audio_loop`, `AppService::on_audio_event`, `state.playback`.
- Produces: `PlaybackId = String`; `PlaybackProgress { position_ms: u64, buffered_ms: u64, paused: bool }`; `AudioEvent::PlaybackProgress { id: PlaybackId, progress: PlaybackProgress }`; `AudioEvent::PlaybackFinished { id: PlaybackId }`; `AudioPort::{start_playback, append_chunk, pause_playback, resume_playback, stop_playback, stop_all_playback, playback_progress}`; matching `AudioBackend::{pause_playback, resume_playback, playback_progress}`.

- [ ] Write tests `a_new_selection_replaces_the_previous_playback_with_a_new_id`, `stale_id_cannot_pause_or_stop_a_replacement`, `append_from_a_previous_rendition_is_ignored`, `pause_and_resume_keep_the_queue_and_position`, `finished_event_carries_the_unique_id` and `progress_is_monotonic_across_chunks`; run `cargo test --manifest-path src-tauri/Cargo.toml --test audio` and confirm they fail on missing APIs.
- [ ] Extend `Playback` with `paused`, `pause`, `resume`, `position` and `buffered`; accumulate `Decoder::total_duration()` per accepted append and compute `position = finished_ms + get_pos()`. Keep the existing `decode`-before-hardware guarantee.
- [ ] Add the id to `audio_loop`: `start_playback` generates a fresh `PlaybackId`, always stops the previous player and returns the id; `append_chunk`, pause, resume and stop compare the id with the current one and ignore a mismatch without touching the device. `stop_all_playback` is only for shutdown/close. Emit `PlaybackProgress` at most every 250 ms while a player exists and `PlaybackFinished { id }` when the queue empties (the service decides whether that means `Buffering` or `Ended`), never while paused.
- [ ] Implement the new trait methods in `NativeAudio`, `NativeDevices` and the test `FakeAudio`; delete the unowned `play`/`append`/`stop_playback` path so no mode can play without an id. In `service.rs`, store the returned id in `state.playback_id`, pass it to pause/resume/stop, and clear it only when a `PlaybackFinished` carries a matching id. Update every existing `AudioEvent::PlaybackFinished` constructor in the Rust tests to supply an id.
- [ ] Rerun the audio test and `cargo test --manifest-path src-tauri/Cargo.toml --lib`, then commit the port change before the service work.

```rust
pub type PlaybackId = String;

fn start_playback(&self, path: PathBuf) -> Result<PlaybackId, AppError>;
fn append_chunk(&self, path: PathBuf, id: &PlaybackId) -> Result<(), AppError>;
fn pause_playback(&self, id: &PlaybackId) -> Result<(), AppError>;
fn resume_playback(&self, id: &PlaybackId) -> Result<(), AppError>;
fn stop_playback(&self, id: &PlaybackId) -> Result<(), AppError>;
fn stop_all_playback(&self) -> Result<(), AppError>;
fn playback_progress(&self) -> Option<PlaybackProgress> { None }
```

## Task 3 — capture elapsed time and real microphone level

**Files:** modify `src-tauri/src/audio/capture.rs`, `src-tauri/src/audio/mod.rs`, `src-tauri/src/service.rs`; test `src-tauri/tests/audio.rs`.

**Interfaces:**
- Consumes: `Capture` callback and writer counters, `AudioEvent`.
- Produces: `pub fn peak_level(samples: &[i16]) -> f32`; `Capture::level() -> Option<f32>`; `Capture::captured_ms() -> u64`; `AudioEvent::CaptureProgress { recording_id: Option<String>, elapsed_ms: u64, level: Option<f32> }`; `AudioBackend::capture_progress()`. The event carries plain values; the article layer maps them to `ArticleRecordingView` (task 4).

- [ ] Write `peak_level_uses_actual_samples` (silence → 0.0, a known tone → expected peak) and `capture_progress_names_the_recording_and_reports_elapsed`; run the audio test and confirm failure.
- [ ] Compute the peak in the capture callback into an `AtomicU16` (0–1000) without locks or allocation; expose `level()` as `None` only before the first callback (device never opened) and `captured_ms()` from written frames; real silence is `Some(0.0)`, never a decorative value.
- [ ] Emit `CaptureProgress` immediately after a successful `start_capture` and at most every 100 ms while capturing; the audio thread passes `recording_id` so a stale recording cannot update a newer one. `capture_progress()` returns `None` once capture stops.
- [ ] Add `peak_level` and the progress assertions to the controlled `FakeAudio`; rerun the audio test.

```rust
pub fn peak_level(samples: &[i16]) -> f32 {
    let peak = samples.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
    f32::from(peak) / f32::from(i16::MAX)
}
```

## Task 4 — player state machine and presentation contracts

**Files:** create `src-tauri/src/article/player.rs`; modify `src-tauri/src/article/mod.rs`, `src-tauri/src/article/types.rs`, `src/lib/article-contracts.ts`; test `src-tauri/tests/article_player.rs`.

**Interfaces:**
- Consumes: `ArticleSnapshot.audio` consumers, `ArticleSpeaker`, `ArticleTurnView`.
- Produces: `PlayerState`, `PlayerSource`, `PlaybackTrigger`, `PlayerOperation`, `PlayerFailure`, `ArticlePlayerView`, `RecordingState`, `ArticleRecordingView` (`capture_id`, `sequence`, `state`), `PlayerEvent`, `PlayerRuntime { select, apply, view }`, `RecordingRuntime`, and the TS mirror; `ArticleSnapshot` gains `recording: ArticleRecordingView`.

- [ ] Write `preparing_only_becomes_playing_after_playback_progress`, `empty_queue_while_generating_is_buffering_not_ended`, `pause_during_generation_stays_paused_and_keeps_position`, `synthesis_without_chunks_fails_but_keeps_text`, `stop_returns_to_idle_without_a_selection`, `stale_generation_is_ignored_and_sequence_is_monotonic` and `a_finished_recording_ignores_late_active_events`; run `cargo test --manifest-path src-tauri/Cargo.toml --test article_player` and confirm failure.
- [ ] Implement the pure player transitions: `Idle → Preparing` on select; `Preparing → Playing` only on the first progress tick reporting `position_ms > 0` (a queued chunk is not audible proof); `Playing → Buffering` on drain while incomplete; `Buffering → Playing` on the next progress tick after a chunk; `Playing|Buffering|Preparing → Paused` on pause; synthesis completing while paused stays `Paused`; resume returns to `Playing` when audio is queued, otherwise `Buffering`; complete drain → `Ended`; any state → `Idle` on stop; failure → `Failed` with `PlayerOperation`.
- [ ] Implement `PlayerRuntime::apply(generation, PlayerEvent) -> bool`, rejecting a generation other than the current one and incrementing `sequence` only on an accepted change, and `RecordingRuntime` with `capture_id` + `sequence` where `Active`, `Ended` and `Inactive` are explicit and an older capture never overrides a newer one. Expose `player_view()` with `duration_ms` only when complete. Rejecting a lower sequence on merge of a snapshot belongs to the frontend (task 8).
- [ ] Update the Rust DTOs and every existing Rust consumer (`with_audio`, snapshot construction, tests, examples) so the crate compiles; `article_snapshot` must carry both `audio` and `recording`; run the new test plus `cargo test --manifest-path src-tauri/Cargo.toml --test article_domain`.
- [ ] Mirror all enums and `ArticlePlayerView` / `ArticleRecordingView` in `src/lib/article-contracts.ts`, update `makeSnapshot` in `src/test/fake-article-client.ts` (`audio: { sessionId: "session-1", turnId: null, versionId: null, generation: 0, sequence: 0, state: "idle", positionMs: 0, durationMs: null, bufferedMs: null, source: null, trigger: null, failure: null }` and `recording: { sessionId: "session-1", turnId: "", captureId: "", sequence: 0, state: "inactive", elapsedMs: 0, level: null, available: true }`), and migrate the two existing `audio: { playback, turnId }` overrides in `ArticleConversation.test.tsx` plus the minimal `audio.playback` reads in `ArticleConversation.tsx` to the new state names so the suite still compiles. The full UI replacement belongs to tasks 9 and 10. Run `npm.cmd run typecheck`.
- [ ] Add `#[serde(rename_all = "snake_case")]` to the enums (`PlayerState`, `PlayerSource`, `PlaybackTrigger`, `PlayerOperation`, `RecordingState`) and `#[serde(rename_all = "camelCase")]` to the views, and add a round-trip contract test `player_and_recording_views_serialize_with_camel_case` so the TS mirror cannot drift silently.

## Task 5 — local voice cache

**Files:** create `src-tauri/src/article/cache.rs`; modify `src-tauri/src/article/mod.rs`; test `src-tauri/tests/article_voice.rs`.

**Interfaces:**
- Consumes: `sha2`, `atomicwrites` (via existing storage helpers), `hound` for WAV duration, the existing session directory layout `articles/<sessionId>`.
- Produces: `voice_key(text, voice, speed_x10, engine) -> String`; `read_cache(session_dir, key) -> Option<CachedVoice>`; `begin_entry(session_dir, key) -> Result<PathBuf>`; `finish_entry(dir, &[(u32, u64)]) -> Result<()>` (writes the complete manifest); `clear_entry`; `remove_incomplete(session_dir)`.

- [ ] Write `cache_key_changes_with_text_voice_speed_and_engine`, `an_entry_without_a_complete_manifest_is_not_reused`, `corrupt_or_missing_chunk_invalidates_the_entry`, `a_key_outside_the_cache_directory_is_refused` and `a_partial_entry_is_removed_before_regeneration`; run `cargo test --manifest-path src-tauri/Cargo.toml --test article_voice`.
- [ ] Implement the SHA-256 key and the manifest (`version: 1`, key, voice, speedX10, engine, textSha256, ordered chunks with `index`/`durationMs`, `complete`). Validate that the resolved directory stays under `<session>/voice-cache/` and that the key is 64 lowercase hex characters.
- [ ] Implement completion as an atomic manifest write after the last chunk; an existing incomplete directory is cleared in `begin_entry`, so a retry starts from the beginning and partial output is never treated as a cache hit.
- [ ] Extend the article deletion test to assert `voice-cache` disappears with the session; rerun `cargo test --manifest-path src-tauri/Cargo.toml --test article_service --test article_voice`.

```json
{
  "version": 1,
  "key": "<64 hex>",
  "voice": "af_heart",
  "speedX10": 10,
  "engine": "kokoro-82m-v1",
  "textSha256": "<64 hex>",
  "complete": true,
  "chunks": [{ "index": 0, "durationMs": 3120 }]
}
```

## Task 6 — service: player orchestration, cache reuse, autoplay and scoped events

**Files:** modify `src-tauri/src/service/article.rs`, `src-tauri/src/article_commands.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/diagnostics.rs`; test `src-tauri/tests/article_service.rs`.

**Interfaces:**
- Consumes: tasks 1–5 (`ArticlePreferences`, `ArticleRecovery`, `ArticlePlayerView`, `PlayerRuntime`, cache, id-scoped port, progress events).
- Produces: `AppService::{article_player_play, article_player_pause, article_player_resume, article_player_stop, article_view_focus, article_view_blur, article_playback_progress, article_playback_finished}`; commands `article_player_play(sessionId, turnId)`, `article_player_pause(sessionId, generation)`, `article_player_resume(sessionId, generation)`, `article_player_stop(sessionId, generation)`, `article_view_focus(sessionId, token)`, `article_view_blur(token)`; event `article://player`; snapshot `audio` and `recording` always mirror the runtime views.

- [ ] Write the failing service tests: `play_reuses_a_complete_cache_without_a_worker_request`, `a_cache_miss_synthesizes_without_a_text_request`, `a_corrupt_cache_entry_falls_back_to_a_local_synthesis`, `pause_then_resume_keep_generation_and_position`, `pause_while_synthesizing_is_not_ended`, `a_stale_finished_event_does_not_stop_a_new_item`, `a_late_chunk_after_stop_is_not_played`, `restart_during_generation_reuses_its_ordered_prefix`, `a_new_selection_replaces_the_previous_article_playback`, `autoplay_runs_once_only_for_a_fresh_turn_in_the_active_view`, `autoplay_never_plays_a_turn_that_arrived_while_the_view_was_away`, `disabling_autoplay_stops_automatic_voice_but_not_an_explicit_replay`, `a_speed_change_uses_the_matching_cache_entry`, `a_user_recording_ignores_the_tutor_speed`, `an_old_focus_token_cannot_clear_a_newer_view`, `deleting_finishing_or_revoking_cancels_pending_player_work` and `reopening_starts_the_player_idle`; run `cargo test --manifest-path src-tauri/Cargo.toml --test article_service` and confirm failure.
- [ ] Replace `playback: Option<(String, String, ArticlePlayback)>` with `PlayerRuntime` plus `playback_id: Option<PlaybackId>`. A play selection is never a scoped control command: it bumps the generation, starts immediately with `start_playback` when audio already exists (cache hit or original recording), or waits for the first accepted chunk to call `start_playback`, storing the returned id either way. Append/pause/resume/stop pass that id. Publish `article://player` on every state change and include the same views in the snapshot; keep `sequence` monotonic under `lock_article` and never write the database from a tick.
- [ ] Implement play: validate the turn in the active session/branch and remove incomplete cache entries; for a tutor turn compute the cache key from the exact version text, voice, `speedX10` and engine, play a complete entry or synthesize into a fresh entry; for a user turn play `original.wav` at its recorded speed with a known duration, ignoring the tutor speed. One explicit selection stops the previous item, bumps the generation and starts from the beginning. A restart during an active generation keeps that generation's ordered prefix and accepts its remaining blocks once; after cancellation or failure the retry starts from the beginning.
- [ ] Implement pause/resume/stop scoped by `(sessionId, generation)`; a stale identity returns the current view unchanged and records `STALE_SOURCE` without touching devices. `article_view_focus(sessionId, token)` stores `(session, token)` and stops playback plus autoplay intent when a different session takes focus; `article_view_blur(token)` clears only when the stored token matches, so a delayed unmount cannot clear the session that replaced it.
- [ ] Gate autoplay in `run_article_request`: synthesize only when `preferences.autoplay`, the focus is that session, and the turn is fresh; never after reopen, resume or a duplicate event. Saving autoplay off stops only a player whose `trigger` is `Autoplay`.
- [ ] Update `route_speech_message` and `article_playback_finished` to compare the playback id and generation; the first accepted chunk of a rendition calls `start_playback` and stores the id, later chunks call `append_chunk`, and neither may run after the generation changed. Map `AudioEvent::PlaybackProgress` to position updates (≤4 Hz) and finish to `Buffering` or `Ended` through `PlayerRuntime`. A voice block failing after some playback sets `failure.operation = Synthesis` and labels the rendition incomplete without discarding the text. Remove `article_play_turn`, `article_stop_voice` and `article_speak_turn` from commands, client and diagnostics lists. Register `article_view_focus` and `article_view_blur` in `lib.rs` and the diagnostics closed lists.
- [ ] Keep device and helper work outside the database lock and the response path: validate and persist first, then await `audio_task`/`worker_task` (never a `MutexGuard` of `article` across an `await`), and return only identities and bounded metadata, never file paths. `article_finish`, `article_revoke` and `article_delete` invalidate the player generation, force-stop the current playback id directly (not through a session/generation command), cancel autoplay intent and reject late results before touching the database.
- [ ] Rerun the service test and `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`.

## Task 7 — «Responder agora» and the recording lifecycle

**Files:** modify `src-tauri/src/service/article.rs`, `src-tauri/src/lib.rs`; test `src-tauri/tests/article_service.rs`.

**Interfaces:**
- Consumes: `article_start_recording`, `stop_article_speech_locked`, id-scoped port, `AudioEvent::CaptureProgress`, `RecordingRuntime`.
- Produces: `article_start_recording` performing the ordered handoff; `article://recording` with `ArticleRecordingView { capture_id, sequence, state, elapsed_ms, level, available }`; snapshot `recording`; `AppService::article_capture_progress`.

- [ ] Write `respond_now_stops_output_before_capture` (assert the audio adapter step order is `stop → cancel → start_capture`), `a_capture_failure_does_not_restore_voice`, `a_duplicate_respond_now_starts_one_capture_and_one_turn`, `recording_events_carry_capture_identity_and_sequence`, `a_resubscription_gets_the_current_recording_or_inactive`, `a_late_active_event_cannot_revive_a_finished_capture`, `recording_progress_uses_native_elapsed_and_real_level`, `microphone_loss_keeps_the_recoverable_recording_without_a_new_take` and `meter_values_are_not_persisted_or_logged` (no level text in diagnostics/DB).
- [ ] Refactor the existing start path into `start_article_capture(command, handoff: bool)`; validate session/version/consent/limit/request identity before touching devices, disable duplicate actions, invalidate the playback generation, confirm output stopped, request cooperative synthesis cancellation, then start capture. Acknowledge recording only when capture is active and assign a fresh `capture_id` at that point.
- [ ] Publish `article://recording` from `CaptureProgress` only while the capture id matches the current one; set `state = Ended` with the final elapsed and `level = null` on stop or interruption, and `Inactive` after the turn is consumed, deleted or the app restarts. `article_snapshot` includes the current view, so a resubscription recovers `Active`, `Ended` or `Inactive` without waiting for a new event. Never persist or log level/elapsed values.
- [ ] On stop, keep the saved-turn placeholder: the database turn exists with no text until a real transcript arrives; keep the existing automatic send and review path, and never fabricate text. If residual helper work delays the transcription, keep the audio, show that it is saved and route to the existing review path instead of asking for a new recording.
- [ ] Rerun `cargo test --manifest-path src-tauri/Cargo.toml --test article_service --test audio` and update any test that assumed the old stop/start order.

## Task 8 — React client, playback hook, subscription/resync and focus

**Files:** modify `src/lib/article-contracts.ts`, `src/lib/article-client.ts`, `src/test/fake-article-client.ts`; create `src/features/articles/useArticlePlayback.ts`; test `src/features/articles/useArticlePlayback.test.tsx`.

**Interfaces:**
- Consumes: task 4 TS types, task 6 commands/events and focus tokens, task 7 snapshot `recording`.
- Produces: `ArticleClient.play/pause/resume/stop/preferences/savePreferences/focusView(sessionId, token)/blurView(token)/subscribePlayer/subscribeRecording`; `useArticlePlayback(client, sessionId, initial: ArticleSnapshot)` returning `{ player, recording, play, pause, resume, stop, pending, error }`; event constants `ARTICLE_PLAYER_EVENT = "article://player"` and `ARTICLE_RECORDING_EVENT = "article://recording"`; pure helpers `mergePlayer(current, next)` and `mergeRecording(current, next)` (event applies only with a higher sequence; a snapshot applies with an equal or higher sequence).

- [ ] Write hook tests with a probe component: `an_event_between_open_and_subscribe_is_recovered_by_the_resync_snapshot`, `a_snapshot_older_than_an_applied_event_is_ignored`, `an_event_from_another_session_is_ignored`, `an_older_recording_capture_is_ignored`, `focus_is_set_after_the_listeners_and_the_resync`, `a_stale_blur_token_does_not_clear_a_newer_view`, `unmount_before_the_resync_never_focuses_and_removes_late_listeners` (defer `subscribe` and `snapshot`, unmount while pending, then resolve both: `focusView` is never called, both unlisten functions run and no state update happens), `pause_and_resume_use_the_current_generation` and `unmount_stops_only_a_real_selection` (generation `0` is a no-op). Run `npm.cmd test -- src/features/articles/useArticlePlayback.test.tsx`.
- [ ] Extend `ArticleClient` and the Tauri adapter with `diagnosticInvoke` calls; `subscribePlayer`/`subscribeRecording` return the unlisten functions. Remove `playTurn`/`stopVoice`/`speakTurn` usages.
- [ ] Implement the hook in this order on mount and on session change: (1) install both listeners, (2) re-read `client.snapshot(sessionId)` and merge it into the initial views, (3) call `client.focusView(sessionId, token)` with a token generated for this mount. `Articles.tsx` already remounts the conversation per session (`key={session.id}`), so a session change gets a new token. This closes the gap between `Articles.open` and the subscription without letting an old snapshot win.
- [ ] Make the whole initialization cancellable: keep a `cancelled` flag (or `AbortController`) set by the effect cleanup, and check it after **every** await. A listener whose `subscribe` resolves after unmount is unsubscribed immediately instead of being kept; a `snapshot` that resolves after unmount is neither merged nor followed by `focusView`; no state setter runs after unmount. Cleanup sets the flag, calls `client.blurView(token)` and is otherwise fire-and-forget, so it never blocks the unmount. A late blur from a previous mount is a no-op because the core checks the token, and a late focus is impossible because the cancelled branch returns before it.
- [ ] Filter both views by `sessionId`, playback generation, `capture_id` and sequence; `recording.state === "active"` is the only state that shows the indicator, and `ended`/`inactive` hide it. Expose only bounded state and keep React StrictMode's mount/unmount probe harmless.
- [ ] Extend `fakeArticleClient` with player/recording views, snapshot resync and event emitters; keep `emit`/`emitLate`. Run `npm.cmd run typecheck` and the whole Vitest suite.

## Task 9 — persistent bar, per-message controls, placeholder and preferences UI

**Files:** create `src/features/articles/ArticlePlayerBar.tsx`, `ArticleTurnControls.tsx`, `ArticlePreferences.tsx`, `player.ts`; modify `ArticleConversation.tsx`, `ArticleReport.tsx`, `Articles.tsx`, `articles.css`; tests `ArticlePlayerBar.test.tsx`, `ArticleConversation.test.tsx`.

**Interfaces:**
- Consumes: task 8 hook and contracts; `session.recovery` (task 1); `player.failure.operation` (task 4).
- Produces: `primaryAction(session, player) -> { kind, label, disabled, shortcut }` in `player.ts`; `formatClock(ms) -> "m:ss"`; `recoveryLabel(session) -> string | null` using `RetrySend → «Tentar enviar novamente»`, `RetryFeedback → «Gerar feedback novamente»`, `ReRecord → «Gravar novamente»`; `ArticlePlayerBar` props `{ session, player, recording, busy, onPlay, onPause, onResume, onStop, onRecord, onStopRecording, onFinish, onRetry }`; `ArticleTurnControls` props `{ turn, player, onPlay, onPause, onResume, onRestart }`.

- [ ] Write `bar_has_one_primary_action_for_each_conversation_state` (ready, preparing/playing/paused, replaying user turn, recording, transcribing, awaiting tutor, needs review, recoverable error, finishing, completed, draft, paused), `preparing_is_never_presented_as_playing`, `paused_offers_continue_with_the_same_position`, `pausing_audio_does_not_change_the_session_consent_or_answer_count`, `generating_shows_elapsed_and_buffered_without_a_final_total`, `complete_audio_shows_a_read_only_progress_bar`, `missing_original_is_labelled_unavailable`, `historical_playback_is_not_started_while_awaiting_the_tutor_or_draft`, `transcript_placeholder_is_not_fabricated_text`, `recording_shows_elapsed_and_level_or_explicit_unavailable`, `the_indicator_disappears_when_the_recording_view_is_ended`, `failed_send_keeps_the_saved_answer_and_offers_resend`, `recovery_labels_come_from_the_persisted_operation` (retry send / retry feedback / re-record after a reopened session), `failed_voice_and_failed_feedback_offer_distinct_retry_labels` and `autoplay_and_speed_save_through_the_client`.
- [ ] Implement `ArticlePlayerBar` as a sticky region (`position: sticky; bottom: 0`, reserved padding on the turn list) labelled «Controlos da conversa»: state text, selected item, media controls, elapsed/total or elapsed + «A preparar o restante áudio…», labelled buffered amount, meter (`role="progressbar"`, `aria-label="Nível do microfone"`, «Nível indisponível» when null), primary action and a secondary «Terminar e ver feedback». The retry label comes from `session.recovery` and `player.failure.operation`, never from the message text. Never place the bar over messages/errors/focus.
- [ ] Implement per-message «Ouvir», «Pausar», «Continuar» and «Ouvir do início» for tutor turns and original recordings for user turns; the selected message is highlighted and mirrors the bar. Reuse `ArticleReport` citations through the same controls instead of its own play button.
- [ ] Show saved user turns with empty text as «A transcrever…» while `transcribing`; failed transcription keeps the existing review editor. Keep «Gravar resposta» / «Gravar novamente» / «Responder agora» / «Parar e enviar» labels from the matrix, keep the existing automatic-send notice near «Parar e enviar», and include the limit-reached «Terminar e ver feedback» replacement.
- [ ] Move autoplay/speed into `ArticlePreferences` in the header (toggle «Reproduzir automaticamente», speed radios 0.8×/1.0×/1.2×, explanation that changes apply to the next playback). Wire the actions in `ArticleConversation` and remove the old `audio.playback` conditionals.
- [ ] Style with existing tokens, visible focus and reduced-motion support; run `npm.cmd test -- src/features/articles` and `npm.cmd run build`.

## Task 10 — reading position, article panel, shortcuts and announcements

**Files:** create `src/features/articles/useFollowTail.ts`, `useArticleShortcuts.ts`; modify `ArticleConversation.tsx`, `articles.css`; tests `ArticleChat.test.tsx`, `ArticleShortcuts.test.tsx`.

**Interfaces:**
- Consumes: conversation turns, player actions, `primaryAction`.
- Produces: `useFollowTail(ref, revision)` returning `{ atEnd, newCount, jumpToLatest }`; `useArticleShortcuts({ enabled, onRecord, onPauseToggle })`; panel state `panelOpen` local to `ArticleConversation`.

- [ ] Write `new_turns_do_not_move_a_reader_who_scrolled_up`, `a_new_message_indicator_returns_to_the_latest_turn_and_resumes_following`, `a_labelled_jump_link_reaches_the_latest_question`, `opening_a_session_positions_at_the_latest_turn_without_sound_and_focuses_the_heading`, `an_arriving_turn_and_a_recorded_error_never_steal_focus`, `a_primary_action_that_changes_keeps_a_focus_target` (record → stop, reply arrives), `the_article_panel_does_not_hide_recording_controls` (wide and 390 px), `entering_the_article_panel_does_not_change_the_conversation_scroll` and `player_state_changes_are_announced_once_without_meter_or_timer_chatter`.
- [ ] Implement `useFollowTail` with the 80 CSS-pixel threshold, `scrollTo` guarded for jsdom, reduced-motion `auto` behaviour and a «Nova resposta ↓» button with the count. Use `role="log"`/`aria-live="polite"` only for state announcements, never for every message.
- [ ] Replace `<details>Ver o artigo</details>` with a panel: a wide-layout side column opened by «Ver o artigo» and a narrow collapsible panel; toggle uses `aria-expanded`/`aria-controls`, preserves the original article text and language, and is view-local.
- [ ] Implement `useArticleShortcuts`: `Ctrl+Enter` invokes the currently enabled recording action; `Alt+P` toggles pause/resume of the selected item. Ignore repeats, IME composition, inputs/textarea/select/`[contenteditable]`, `[aria-modal="true"]` dialogs and disabled actions; never trigger finish, consent, retry or manual send.
- [ ] Write `shortcuts_are_ignored_in_inputs_dialogs_and_key_repeat` and `alt_p_only_affects_the_selected_item`; run `npm.cmd test -- src/features/articles` and verify shortcuts manually in the Tauri window afterwards (record the result as pending if hardware is unavailable).

## Task 11 — regressions, diagnostics, documentation and verification

**Files:** modify `src-tauri/tests/service.rs`, `src-tauri/tests/practice_service.rs`, `src-tauri/tests/audio.rs`, `src-tauri/src/diagnostics.rs`, `docs/diagnostics/validation.md`, `docs/article-conversation/{roadmap,validation,setup,performance}.md`, `docs/superpowers/specs/2026-11-20-article-player-design.md`, `README.md`.

**Interfaces:**
- Consumes: all previous tasks.
- Produces: green verification record with real counts; diagnostics closed lists including `article_player_play`, `article_player_pause`, `article_player_resume`, `article_player_stop`, `article_view_focus`, `article_view_blur`, `article_preferences_load`, `article_preferences_save` and the operations `article_play_start`, `article_play_stop`, `article_record_handoff`; explicit pending manual criteria R1-01…R1-16.

- [ ] Add `star_playback_finished_still_clears_only_its_own_id`, `practice_playback_is_not_stopped_by_a_stale_article_id`, `a_star_selection_replaces_an_article_playback` and `reopening_an_older_session_uses_defaults_and_never_plays`; run `cargo test --manifest-path src-tauri/Cargo.toml --test service --test practice_service --test audio --test article_migration`.
- [ ] Update the diagnostics closed lists and their documentation; assert no new event carries text, transcripts, audio paths, credentials or meter values. Rerun `cargo test --manifest-path src-tauri/Cargo.toml --test diagnostics` (or the existing diagnostics tests).
- [ ] Run `.\scripts\verify.ps1` and record the actual check count and test counts in `docs/article-conversation/validation.md`, separating automated evidence from pending manual acceptance. Do not mark any R1 criterion accepted.
- [ ] Update `docs/article-conversation/setup.md` with migration 6, the preferences table and the `voice-cache` layout; `performance.md` with the statement that R1 adds no measurement and keeps the R0 baseline for stop-to-first-sound; `roadmap.md` to link this plan; the spec status to «proposed specification; technical plan available, implementation not started».
- [ ] Run the documented release build without bundling and a smoke start/stop with temporary data; record exit codes. Do not build an installer and do not start phase 5.

## Acceptance coverage

| Criterion | Owning task |
| --- | --- |
| R1-01 bar visible without hiding content/focus | 9, 10 |
| R1-02 actions follow conversation/limit/consent | 9 |
| R1-03 one click stops output before capture | 7 |
| R1-04 pause/resume/restart and exclusive ownership | 2, 4, 6 |
| R1-05 truthful preparing/buffering/ended and duration | 4, 6, 9 |
| R1-06 original replay and cache without text inference | 5, 6 |
| R1-07 duration/meter from active capture, failures recoverable | 3, 7, 9 |
| R1-08 placeholder becomes real transcript, text before voice | 7, 9 |
| R1-09 reading position and focus preserved | 10 |
| R1-10 autoplay/speed defaults persist and apply as specified | 1, 6, 9 |
| R1-11 shortcuts never fire in editors/dialogs/inactive views | 10 |
| R1-12 precise isolated recovery for audio/network/report | 6, 7, 9 |
| R1-13 late events cannot affect another item/session/mode | 2, 4, 6, 11 |
| R1-14 finish/revoke/delete/reopen cannot restart output | 6, 7 |
| R1-15 versions, branch/report sources and privacy preserved | 1, 5, 11 |
| R1-16 five-answer conversation usable end to end | manual acceptance, after 1–11 |

## Manual acceptance still required

Real microphone/speaker handoff including speaker bleed, listening at 0.8×/1.0×/1.2×, pause with an empty buffer during real synthesis, device removal, abrupt close/reopen, keyboard use in the packaged Tauri window, and a full five-answer conversation with authorized paid inference. These are not automated-test results and must not be recorded as passed without the real run.

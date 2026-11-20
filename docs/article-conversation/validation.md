# Article conversation — validation

Status on 2026-11-05: **implementation and automated verification complete; manual acceptance, real audio, real voice and paid inference still to do.** Automated verification is not manual acceptance: the mode is **not** accepted.

## Observed automated verification

- `.\scripts\verify.ps1`: **8/8 checks, exit 0, 190.6 s** — TypeScript, **147 React tests** (20 files), Vite build, formatting and Clippy `-D warnings`, **354 Rust tests**, **56 Python tests** without the `models` marker, clean `pip check`.
- `npm run tauri -- build --no-bundle`: exit 0 (release, 1 min 27 s).
- Rehearsal with temporary data (`scripts/diagnostics-smoke.ps1` on an isolated build): window alive, diagnostics exported, shutdown with exit code 0. It does not touch personal sessions.
- The real Python tests with models (`-m models`) **were not run in this verification**.

None of this used a real microphone, real Kokoro voice, real protected credential or paid requests: audio, helper and provider are controlled adapters.

## Design matrix

| Design requirement | Automated evidence | Manual acceptance |
|---|---|---|
| Pasted article, fixed copy, limits without cutting | `article_domain`, `article_repository`, `ArticleSetup.test` | pending |
| Authorization per session, never pre-selected; revoking/changing the destination blocks sends | `ArticleSetup.test`, `article_service` (revocation, changed destination), `article_domain` | pending |
| Recording ends only by user action; audio stored before sending | `article_service` (`audio_is_saved_first…`), `article_domain` | pending (real microphone) |
| Automatic sending only with a stored transcript; empty/failed/excessive is not sent | `article_service`, `article_repository` | pending |
| Immutable sources; sent vs. revised version distinct | `article_repository`, `article_migration` | — |
| Article as untrusted data; quotes only from the user | `article_ai` | pending (semantic resistance) |
| Comment + question, without corrections between turns; suggest ending on the 5th; limit | `article_ai`, `ArticleConversation.test`, `article_repository` | pending (usefulness) |
| Feedback: 4 dimensions, explicit insufficient evidence, no overall score | `article_ai`, `ArticleReport.test` | pending (usefulness) |
| Correction in a new branch, old branch/report intact, explicit regeneration | `article_recovery`, `article_service` | pending |
| Late results, double click, ending/deleting during a request | `article_service`, `article_repository`, `ArticleConversation.test` | — |
| Local recovery without sends at startup; resumable deletion | `article_recovery`, `article_service` | pending (real abrupt shutdown) |
| STAR isolation (microphone/helper) | `article_service` (`star_and_article…`) | pending |
| Incremental voice: order, cancellation, partial failure | `article_voice`, `article_service` (voice), `test_stream` | **pending (real listening; Kokoro in blocks)** |
| Diagnostics without content; closed operations | `article_service` (metrics), `diagnostics` | — |
| Integrated scenario (authorize → 5 turns → correct → regenerate → conclude → reopen) | `article_service::full_conversation_…` | — |

## Known deviations and limits

- **No `synthesisChunks` capability in `health`**: adding a field to `HealthResult` would break the existing literals; the helper is distributed with the application. No automatic fallback to full synthesis if the helper does not know `synthesize_stream`.
- **Voice cancellation**: it asks the helper to stop between blocks; a block already being generated finishes and is discarded. The following transcript waits up to 2 s for the helper to become free.
- **Article limit**: 30,000 bytes (the spec said 32,000) so that title, opening, answer and reply fit in 48,000 bytes of context.
- **Old branches** do not appear in the interface (only the active branch); they are stored and reconstructible (`project_article_branch`).
- **Repeating an answer as a new branch** (recording again) is not implemented; only text correction.
- The Articles page **had no visual validation**.
- Performance measurements and the `beam_size=5/1` comparison: see [performance](performance.md) — to do.

## Pending manual script

1. Real conversation about an article, with at least five answers, including disagreement and elaboration (continuity, dialogue cohesion, perceived wait).
2. Recovery from a recognition error (manual text) and from a voice interruption in the middle.
3. Network failure and explicit repetition; resumption after closing the application in the middle of an answer.
4. Usefulness and fidelity of the feedback, and real cost (paid requests require your authorization).
5. Correction of an old answer and regeneration of the report.
6. Visual validation of the Articles page (desktop and mobile).

## R1 — Article player and chat

The R1 implementation follows the [design](../superpowers/specs/2026-11-20-article-player-design.md) and [technical plan](../superpowers/plans/2026-11-20-article-player-implementation.md). Manual acceptance remains pending.

### Verification on 2026-11-20

- `scripts/verify.ps1` completed **8/8 checks** with exit 0 in 232.2 s. It passed TypeScript, Vite, Rust formatting, Clippy, **432 Rust tests**, **188 Vitest tests** (24 files), **56 Python tests** (2 model tests deselected) and `pip check`.
- The focused post-review battery passed **93 Rust tests** across `article_player`, `article_service` and `article_voice`. The Articles interface passed **68 tests**.
- The controlled Chromium rehearsal verified the open article panel and recording bar at **800×600, 1024×768, 1440×900 and 390×844**. The stop button was visible and clickable. The bar did not overlap the chat or article, and the page had no horizontal overflow or JavaScript errors.
- The isolated no-bundle release build completed with exit 0. Its WebView2 window opened a synthetic completed conversation in a temporary data directory. Alt+P paused and resumed a silent WAV through native playback. Ctrl+Enter had no effect in that completed state. The window closed with exit 0.
- The regular `npm.cmd run tauri -- build --no-bundle` build also completed with exit 0. Cargo took 1 min 48 s and produced `src-tauri/target/release/language-assistant-tutor.exe`.

The rehearsals used controlled or synthetic data. Real recording, listening to Kokoro, paid inference, installer validation and performance measurements remain pending.

### PR #5 regression coverage

After the review fixes, `scripts/verify.ps1` passed all eight checks in 172.0 s, including 433 distinct Rust tests, 190 Vitest tests and 56 Python tests (two model tests deselected). The no-bundle release build also succeeded.

- The conversation continues receiving session snapshots if player initialization fails. The persisted snapshot exposes the saved turn during transcription. When transcription fails, the manual editor and the saved recording are available together.
- Selecting an uncached voice while the helper finishes a cancelled synthesis defers the new request. Stopping playback also discards that pending selection.
- A chunk arriving after the native queue drains receives a new playback ID. The delayed end event belongs to the previous queue. Short recordings can reach the ended state before the first positive position sample.
- A synthesis or chunk protocol failure stops audio already queued for that rendition. The conversation text remains available for an explicit retry.

### R1 acceptance criteria

| ID | Automated evidence | Manual acceptance |
| --- | --- | --- |
| R1-01 | Playwright layout rehearsal at 800×600, 1024×768, 1440×900 and 390×844 with a long chat, open article panel and controlled recording state | Pending |
| R1-02 | `ArticlePlayerBar` action matrix and `ArticleConversation` consent, recording, review and limit tests | Pending |
| R1-03 | `article_service` verifies the output-stop, cancel and capture order, duplicate requests and refusal to capture after an output-stop failure | Pending real speaker-to-microphone handoff |
| R1-04 | `audio`, `article_player` and `article_service` cover scoped controls, position and resuming after synthesis completed during a pause | Pending real listening |
| R1-05 | `ArticlePlayerBar` and `article_player` distinguish playback states and known versus unknown duration | Pending real streaming |
| R1-06 | `article_voice` and `article_service` verify full-cache reuse, rejection of missing or corrupt audio, and local regeneration without a new text request | Pending real Kokoro replay and reopen |
| R1-07 | `audio` and `article_service` verify native elapsed time, bounded level, capture identity and recovery. React hides the meter when capture ends | Pending real silence, speech and device removal |
| R1-08 | `ArticleConversation` verifies the placeholder and actual transcript. `article_service` checks persistence before external requests | Pending real transcription |
| R1-09 | `ArticleChat` checks reading position, jumps, heading focus, primary action focus and quiet progress updates | Pending keyboard and assistive technology use |
| R1-10 | `article_migration`, `article_service` and `ArticleConversation` verify defaults, persistence, speed selection, and suppression of autoplay after disable or blur | Pending real listening at all three speeds |
| R1-11 | `ArticleShortcuts` covers recording and pause/resume, editable and modal targets, repeat and IME suppression. The WebView2 rehearsal checks pause/resume and the inactive recording shortcut | Pending shortcuts with real recording |
| R1-12 | `article_service`, `article_repository` and React cover typed recovery and distinct voice, send, feedback and capture actions | Pending real network and audio failures |
| R1-13 | Rust tests cover stale identities between STAR, practice and articles. `useArticlePlayback` covers sequences, late subscriptions, snapshots and unmount | Pending device use across modes |
| R1-14 | `article_service` covers finish, revoke, delete and silent reopen. React defers replay until the view has focus and reconciles messages saved before subscription | Pending abrupt close and reopen |
| R1-15 | `article_migration`, `article_repository` and diagnostics tests cover migration rollback, content preservation and closed metrics | Pending use with real sources |
| R1-16 | Controlled five-answer service test with correction, regeneration and reopen | Pending full run with real microphone, Kokoro and authorized paid inference |

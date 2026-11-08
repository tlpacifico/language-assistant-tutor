# Conversation about articles — implementation plan

> **For agent execution:** use the `executing-plans` skill for direct execution, task by task; `subagent-driven-development` only if that modality is chosen by the user. The steps use checkboxes for tracking.

**Goal:** converse in English about an article, with turns linked to each other, authorized automatic sending and final feedback on cohesion, coherence, development and interaction.

**Architecture:** a dedicated article domain in the same Rust core and SQLite, reusing capture, local models, credentials, HTTP transport and usage normalization. React presents photographs of the persisted state. The incremental voice delivers chunks only after the tutor's complete text has been validated and saved.

**Stack:** React/TypeScript, Tauri 2/Rust, rusqlite, CPAL/Rodio, Python helper with faster-whisper/Kokoro and OpenCode Go. No new dependencies foreseen.

**Spec:** [approved design](../specs/2026-11-04-article-conversation-design.md).

**Status:** execution completed on 2026-11-05 on the branch `feat/article-conversation`: tasks 1 to 9 implemented, with `verify.ps1` 8/8 (354 Rust, 147 Vitest, 56 Python), `--no-bundle` build and rehearsal with temporary data with exit code 0. Deviations recorded in [validation](../../article-conversation/validation.md) (no capability in `health`, article limit of 30,000 bytes, old branches only in the database, no re-recording as a branch). **Pending and not done**: real measurements of performance and fidelity (task 8), manual script with real audio, voice and inference (task 9) and visual validation. None of this is manual acceptance.

## Global constraints

- Interface and explanations in European Portuguese; practice questions, comments and answers in English.
- The user starts and ends the answer manually. No rating of pronunciation, rhythm or emotional confidence.
- Corrections only at the end; preserve audio, transcript, sent revisions and earlier branches.
- Mode separate from STAR: do not change the mandatory confirmation or the existing follow-up limit.
- Save before sending; only the necessary text goes to the provider, never audio or the key in the logs.
- Explicit consent per session for the article and automatic sending; a changed destination or revoked authorization blocks new sends.
- Tauri identity and diagnostics directory preserved; new operations in the closed lists.
- No paid inference in the automated tests. No URL/PDF import, GPU or transcription during recording in this delivery.
- Do not start phase 5 and do not mark earlier phases as accepted.

## Review focus

1. Double click or resumption during a send: one turn and one application of the result (tasks 2 and 4).
2. Article with hostile instructions, Unicode and excessive size: treat as data and refuse the excess without truncating (tasks 1 and 3).
3. Correction of an old turn with later answers: new branch, without reinterpreting old descendants (task 5).
4. Voice interruption between two chunks or loss of the device: do not restart delayed audio and do not lose text (task 7).
5. Revocation or destination change while a network request is in progress: do not apply a stale answer and do not hide known usage (tasks 3 and 4).

## Technical decisions and common interfaces

These decisions make concrete points left to the plan; they are not performance measurements:

- Article between 1 and 12,000 Unicode characters after removing outer whitespace; optional title up to 200. A transcribed answer may have up to 8,000 characters for sending. Excess keeps the original and asks for an explicit review; it never truncates automatically.
- Up to 20 answers per branch; suggest conclusion after five. Article and utterance context up to 48,000 UTF-8 bytes. Reserve 16,000 bytes within that ceiling for the next answer; if that margin no longer exists, offer conclusion before another recording. Also respect the existing HTTP limit of 160,000 bytes, including instructions and serialization. An article excessive in bytes is refused before creating the session.
- Tutor comment up to 500 characters and question up to 300; the opening contains only a question. Feedback contains the four dimensions with state `clear|partial|insufficient_evidence`, evidence and up to three improvements. Semantic quality still requires manual validation.
- One active article conversation at a time; it may coexist saved with a STAR interview, but only one owner may use capture/models/playback. Navigation does not start or resume external operations.
- SQLite schema v4 with dedicated tables `article_sessions`, `article_branches`, `article_turns`, `article_versions`, `article_requests`, `article_reports`, `article_artifacts`, `article_receipts`. Do not reuse STAR confirmations for automatic sends.
- `article_sessions`: fixed text, title, optimistic version, active branch, state and authorization with destination/notice version. Revocation increments the version and invalidates the generation in progress.
- Branches store the parent and the fork turn. Turns and versions are immutable; the active path is the ancestral prefix up to the fork plus the turns of the new branch. A correction creates a version of the fork turn and replaces only that version in the projection of the new branch.
- Requests store operation, generation, branch, exact sources, public configuration, exact textual context, state and usage. Do not store the key. Reports pin version IDs and branch. Receipts make repeated commands idempotent; repeating after a failure creates an identified attempt, without a hidden HTTP retry.
- Exclusive artifacts in `articles/<sessionId>/...`, resolved by the core. Logical deletion prevents new operations before the files are cleaned; files not removed enter reconciliation, without deleting shared weights or caches.
- States: `draft`, `awaiting_tutor`, `ready`, `recording`, `transcribing`, `needs_review`, `recoverable_error`, `finishing`, `completed`, `paused`. Playback state is independent; transitions validated in the core.

Create `src-tauri/src/article/types.rs` and camelCase equivalents in `src/lib/article-contracts.ts`:

```rust
struct ArticleCommand { request_id: String, session_id: String, expected_version: u64 }
struct ArticleCreate { request_id: String, title: Option<String>, text: String }
struct ArticleConsent { command: ArticleCommand, destination: String, notice_version: String }
struct ArticleCorrection { command: ArticleCommand, turn_id: String, text: String }
struct ArticleSnapshot { session: ArticleSessionView, turns: Vec<ArticleTurnView>, report: Option<ArticleReport>, audio: ArticleAudioView }
```

The views contain only public data: IDs/versions/state, text and origin of each version (`automatic_sent|manual_review`), available playback, recoverable error and optional usage. Do not expose arbitrary paths or credentials. The details of presentation fields may be defined in task 1 without changing these identities.

## File map

- New `src-tauri/src/article/{mod,types,state}.rs`: contracts and pure transitions.
- New `src-tauri/src/db/articles.rs` and migration `004_article_conversation.sql`: transactions, branch projection and recovery.
- New `src-tauri/src/ai/article.rs`: context, prompts and validation; adapt `ai/transport.rs` to share HTTP without converting articles into STAR sources.
- New `src-tauri/src/service/article.rs` and `article_commands.rs`: orchestration and commands; register in `service.rs` and `lib.rs`.
- New `src/lib/article-{contracts,client}.ts` and `src/features/articles/{ArticleSetup,ArticleConversation,ArticleReport,useArticle}.tsx`: flow and subscription. The hook may use the `.ts` extension if it does not contain JSX.
- Change `App.tsx`, `features/layout/TutorNavigation.tsx`, styles and navigation guard to integrate the page and the audio ownership.
- Change the helper, Rust protocol, supervisor and playback for voice chunks, keeping the old requests.
- New tests per task in `src-tauri/tests/article_*.rs`, `src/features/articles/*.test.tsx` and existing Python tests. Add examples in `contracts/article-v1.examples.json` and in the helper contract.

## Task 1 — domain, limits and contracts

**Files:** new `article` modules and the contracts/examples above; `src-tauri/src/lib.rs`; test `src-tauri/tests/article_domain.rs`.

**Interfaces:** `validate_article(input: &ArticleCreate) -> Result<()>`; `transition(state: ArticleState, event: ArticleEvent) -> Result<ArticleState>`; common types published in `article::types`.

- [ ] Write tests `article_limits_are_unicode_aware` (12,000 characters accepted, 12,001 refused; empty refused), `article_context_reserves_next_answer`, `automatic_send_requires_consent` and `recording_only_stops_on_user_action`.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml --test article_domain`; confirm the failure due to missing interfaces.
- [ ] Implement types, states and validators; define examples of opening, turn, correction, report and error. Pin the notice `article-auto-v1` with article, per-turn sending, destination and local audio explicit.
- [ ] Repeat the command, require exit code 0; run `npm.cmd run typecheck` after the TS contracts.
- [ ] Review the diff and create commit `feat: define article conversation contracts`.

## Task 2 — persistence and v4 migration

**Files:** `db/articles.rs`, `db/migrations/004_article_conversation.sql`, `db/mod.rs`; `tests/article_migration.rs` and `tests/article_repository.rs`.

**Interfaces:** methods `Database::create_article(input: ArticleCreate) -> Result<ArticleSnapshot>`, `article_snapshot(session_id: &str) -> Result<ArticleSnapshot>`, `apply_article(command: &ArticleCommand, change: ArticleMutation) -> Result<ArticleSnapshot>`; mutation enum typed in the domain, without SQL or transitions supplied by the frontend.

- [ ] Test v0/v1/v2/v3→v4, a future version without writing and rollback of an invalid migration; compare STAR/practice content before and after. Test two identical command IDs, stale version, forbidden update of sources and deletion restricted to the exclusive files.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml --test article_migration --test article_repository`; observe the initial failures.
- [ ] Implement transactional migration and tables with FKs/triggers; extend the migrations helper and all its callers. Synchronous operations in `spawn_blocking`, without a mutex across `await`.
- [ ] Repeat the tests and the existing migration tests; confirm a single active article session and idempotent receipts.
- [ ] Commit `feat: persist article conversations and immutable sources`.

## Task 3 — dialogue and feedback in the existing AI integration

**Files:** `ai/article.rs`, `ai/mod.rs`, `ai/transport.rs`; `tests/article_ai.rs`; contract examples.

**Interfaces:** `ArticleContext { article, turns, branch_id, source_version_ids }`; `ArticleAiTask::{Opening, Reply, Feedback}`; `build_article_body(settings: &AiSettingsView, task: ArticleAiTask, context: &ArticleContext) -> Result<serde_json::Value>`; `validate_article_reply(content: &str) -> Result<ArticleReply>` and `validate_article_report(content: &str, context: &ArticleContext) -> Result<ArticleReport>`.

- [ ] Test articles with hostile instructions delimited as data, correct roles in the history, byte limits, opening without comment, answer with exactly one question field and reports that refuse nonexistent quotations/quotations from the article attributed to the user. Do not promise that structural tests prove semantic resistance to hostile instructions.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml --test article_ai`; confirm the initial failure.
- [ ] Extract only the common HTTP send to accept an already built body and conversation identity; keep the existing `AiTransport` trait compatible. Add `ArticleTransport::complete(settings, conversation_id, task, context)` with the same normalized result and credential obtained in the core. The concrete adapter uses the common helper, without duplicating authentication, limits, timeout or redirects/retries policy.
- [ ] Build prompts separate from STAR, schema `article-v1`, output limit of 800 tokens for dialogue and 4,000 for the report; use the existing `stream:false`. `x-opencode-session` identity stable per article session, including corrections/retries. Sources always persisted before sending.
- [ ] Simulate transport and test HTTP on the local server used by the existing tests: 401, 429, 5xx, timeout, invalid body and missing usage; no real key. Repeat the new tests and the existing transport/usage tests.
- [ ] Commit `feat: add grounded article dialogue and final feedback`.

## Task 4 — orchestration, recording and consent

**Files:** `service/article.rs`, `article_commands.rs`, `service.rs`, `lib.rs`, diagnostics and `tests/article_service.rs`.

**Interfaces:** commands `article_create(ArticleCreate)`, `article_authorize(ArticleConsent)`, `article_revoke(ArticleCommand)`, `article_start_recording(ArticleCommand)`, `article_stop_recording(ArticleCommand)`, `article_retry(ArticleCommand)`, `article_finish(ArticleCommand)`, `article_snapshot(session_id)` return `ArticleSnapshot`; `article_list()` returns summaries; `article_delete(ArticleCommand)` returns `Result<()>`. Publish the `article_snapshot` event after commit. Asynchronous commands return a pending state; completion arrives by event.

- [ ] Test saving before sending, automatic sending only after stopping and transcribing, no sending with an empty/failed transcript, double click, active STAR session without simultaneous capture, write failure without network and snapshot queried without external effects.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml --test article_service`; verify the initial failures.
- [ ] Integrate capture/normalization and worker with the explicit owner `Article { session_id, turn_id }`; do not assign article events to STAR answer slots. Keep the existing fields and paths. The correlation includes request, session, branch, version and generation.
- [ ] Implement transitions/retry; revocation or destination change invalidates the pending application, keeps known usage and does not pretend to withdraw text already transmitted. On finishing, invalidate the in-flight question and generate the report once over the pinned projection; zero answers concludes without a fabricated evaluation.
- [ ] Test revocation during HTTP, destination change before sending, finishing during HTTP, absence of retry at startup and late result after deletion. Repeat the task suite and the STAR/practice service regressions.
- [ ] Commit `feat: orchestrate uninterrupted article turns`.

## Task 5 — corrections, branches and recovery

**Files:** `db/articles.rs`, `service/article.rs`, `article_commands.rs`, `artifacts.rs`; `tests/article_recovery.rs`.

**Interfaces:** `article_correct(ArticleCorrection) -> Result<ArticleSnapshot>` creates a branch and leaves regeneration pending on action; the `article_retry` of task 4 regenerates in that branch. `article_resume(ArticleCommand) -> Result<ArticleSnapshot>` recovers local state without HTTP. `project_article_branch(session_id: &str, branch_id: &str) -> Result<ArticleContext>` rebuilds sources.

- [ ] Test correction of the first turn after three answers: old branch intact, new context without descendants and old report kept; test successive corrections in branches, manual text after a failed transcript and repeated retry.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml --test article_recovery`; verify the initial failures.
- [ ] Implement ancestral projection, append-only correction, generation invalidation and receipts; distinguish a recognition correction from a new attempt. Repeating an answer records a new turn in a branch and never replaces the original file.
- [ ] Reconcile files and operations after restart; an interrupted capture is flagged, not presented as a complete recording. Pending requests become explicit recovery, without claiming zero cost. Deletion may be repeated and resumes the pending cleanup.
- [ ] Repeat the tests and verify a simulated crash between file/commit/request and during deletion.
- [ ] Commit `feat: recover and branch article conversations safely`.

## Task 6 — conversation interface and history

**Files:** client/contracts and the new components of the map, `App.tsx`, navigation, styles and tests `ArticleSetup.test.tsx`, `ArticleConversation.test.tsx`, `ArticleReport.test.tsx`.

**Interfaces:** `ArticleClient` mirrors the commands of tasks 4–5 and `subscribe(handler: (snapshot: ArticleSnapshot) => void): Promise<() => void>`. Fake client injects states and results out of order. Do not add authoritative transition logic in React.

- [ ] Test article/limits, non-pre-selected authorization, answer without a mandatory editor, zero corrections between turns, comment/question in sequence, recoverable error, stop voice button, suggestion to finish on the fifth answer and blocking by limit before recording.
- [ ] Run `npm.cmd test -- src/features/articles`; confirm the initial failures.
- [ ] Implement the Artigos page with setup, conversation, consultable transcript, correction action, resumption and session list. Show old branches read-only; explain that regenerating may consume API. Autoplay only of a new intervention in the visible session; history, resumption and remount do not start sound.
- [ ] Integrate the navigation/shutdown guard and the audio owner: do not allow changing page during capture/finishing; leaving cancels pending autoplay without losing persistence. The report shows four dimensions, playable quotations and up to three improvements, with explicit insufficiency of evidence.
- [ ] Test duplicated/late events, microphone unavailability, revocation, absence of report and keyboard/focus on errors; run the task tests, `npm.cmd run typecheck` and the App/navigation regressions.
- [ ] Commit `feat: add article conversation interface`.

## Task 7 — incremental voice with cancellation

**Files:** `audio-worker/src/audio_worker/{protocol,engines,__main__}.py`, tests `test_protocol.py`/`test_engines.py`, `src-tauri/src/worker/{protocol,mod}.rs`, `audio/{mod,playback}.rs`, `service/article.rs`, `contracts/worker-v1.examples.json`; `tests/article_voice.rs`.

**Interfaces:** add the opt-in operation `synthesize_stream`; events `audio_chunk { requestId, index, audioPath, sampleRate, durationMs }` with indices from zero and a terminal result with the number of chunks. Complete WAVs per chunk, never a file being written. `synthesisChunks` capability in health; absence uses the existing complete synthesis.

- [ ] Test first chunk delivered before the final result, ordered indices, single terminal, errors after the first chunk and old `synthesize` requests unchanged. Test rejection of paths outside the operation and duplicated/out-of-order chunks.
- [ ] Run `.\.venv\Scripts\python.exe -m pytest audio-worker/tests/test_protocol.py audio-worker/tests/test_engines.py -q` and `cargo test --manifest-path src-tauri/Cargo.toml --test article_voice`; verify the initial failures.
- [ ] Generate Kokoro chunks in temporary files and publish after rename. In Rodio, append sources to the same player instead of using `play()` which replaces the previous one. A generation identifies the queue; a momentary end of the queue does not mean the end of the request.
- [ ] Stop voice clears the queue and invalidates the generation; the supervisor discards late chunks. Release the worker after interruption at a chunk boundary, with a cooperative cancellation mechanism and command reading independent of inference; do not kill the helper or unload models as a normal route. Old requests remain sequential.
- [ ] Test cancellation while the queue is empty, new capture start, loss/restore of output and capability fallback. Partial failure shows text and offers to repeat the voice, without a new AI call. Keep the final manifest for later listening; never reuse a partial sequence as a complete cache.
- [ ] Repeat the Python/Rust tests and the old playback tests; commit `perf: stream validated tutor speech in ordered chunks`.

## Task 8 — measure and adjust the transcription

**Files:** new `scripts/benchmark-audio.ps1`, `audio-worker/src/audio_worker/engines.py`, protocols only if necessary; `docs/article-conversation/performance.md`; engine tests and `tests/article_metrics.rs`; lists in `diagnostics.rs`.

**Interfaces:** the benchmark receives a recordings directory indicated by the user and a local results destination; do not scan personal recordings. It measures cold/loaded models, recording duration, transcription, first chunk and complete synthesis. Execution diagnostics use the closed operations `article_transcribe`, `article_reply`, `article_first_audio`, `article_feedback` with `duration_ms`, without content/free paths.

- [ ] Test metrics with a controlled clock, absence of content in the logs and a reproducible transcription profile. Run `cargo test --manifest-path src-tauri/Cargo.toml --test article_metrics` and the Python tests; confirm the initial failure.
- [ ] Implement measurements and compare the `beam_size=5` baseline with the `beam_size=1` candidate, keeping English, VAD, the small model and CPU/int8. Do not change the default before the fidelity comparison. Benchmark data are not sent and are not versioned.
- [ ] On the same recordings, run five loaded repetitions per configuration and one cold, presenting individual values, median and worst time; do not call p95 a sample of five. Check the text against a human reference and technical terms.
- [ ] Record real results and the decision: keep the baseline if recordings/fidelity acceptance are missing. Define target values after measuring; an improvement in the voice should be evaluated in the time to first sound and in the breaks between chunks.
- [ ] Repeat the tests after any adjustment; commit `perf: measure article turn latency and transcription tradeoffs`.

## Task 9 — integration, documentation and acceptance

**Files:** new `docs/article-conversation/setup.md` and `validation.md`; update README, AGENTS.md, `docs/idea-and-goals.md`, `docs/tech-stack.md`, `docs/layout/validation.md` and `docs/diagnostics/validation.md`. Do not change the acceptance pending items of the existing phases.

- [ ] Add an integrated scenario with controlled capture/worker/HTTP: authorize→five turns→correct an old turn→regenerate→finish→reopen; check artifacts, sources, usage and the absence of calls on reopening.
- [ ] Run `.\scripts\verify.ps1` and require all checks with exit code 0; record the observed numbers, without copying previous counts. Prepare Cargo in the PATH as in the script before the individual commands of this plan.
- [ ] Run `.\.venv\Scripts\python.exe -m pytest audio-worker/tests -q -m models` if the local models are available; absence remains an explicit pending item. Run `npm.cmd run tauri -- build --no-bundle` and the smoke test with temporary data according to the existing procedures, without touching personal sessions.
- [ ] Check documentation/links and `git diff --check`. Update the visual matrix for the new page, without claiming visual validation before doing it.
- [ ] Carry out the manual script: real article, five answers, disagreement, going deeper, recovery from a recognition error, voice interruption, network failure and resumption. Real inference requires authorization for the paid requests; document perceived latency, continuity and semantic usefulness separately from the automated results.
- [ ] Commit `docs: record article conversation verification and remaining acceptance` with only the results actually observed. The automated delivery may be verified while manual acceptance remains pending.

## Order and transition to execution

Run 1→2→3→4→5→6→7→8→9. Task 7 changes a shared contract and requires regression of the old modes; avoid parallel implementation of these files. Direct execution is recommended to keep the context of state, protocol and versions, with commits per task.

The plan was confronted with the design: consent, sources, limits, recovery, dialogue, feedback, voice and measurements have responsible tasks. None of the test/build commands above was executed in this documentary step; the expected results are future criteria, not results obtained.

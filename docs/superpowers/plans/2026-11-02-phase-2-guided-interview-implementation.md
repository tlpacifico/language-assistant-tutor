# Phase 2 — Guided Interview Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans for direct, task-by-task execution. Use superpowers:subagent-driven-development only if the user explicitly chooses delegation. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Allow completing and resuming configurable local interviews, preserving questions, audio, texts and confirmations in a persistent history.

**Architecture:** React/TypeScript presents the setup, the bank, the interview and the history; Rust keeps authority over transitions and persistence. SQLite stores the data of the new sessions, with short transactions; the originals and derivatives remain in files. Adapt the existing service and reuse the audio port and the Python protocol, without creating another device coordinator.

**Tech Stack:** Tauri 2, React, TypeScript, Vite, Rust, rusqlite with bundled SQLite, CPAL, Rodio, Hound, Rubato, atomicwrites and the existing faster-whisper/Kokoro Python helper.

**Spec:** [Phase 2 — Guided Interview](../specs/2026-11-02-phase-2-guided-interview-design.md). Read also [AGENTS.md](../../../AGENTS.md), [stack](../../tech-stack.md), [phases](2026-11-01-star-interview-tutor-phases.md) and [phase 1 validation](../../phase-1/validation.md).

**Status:** tasks 1 to 9 executed on this machine (2026-11-02), with one commit per task; task 10 is partially executed — matrix, documentation and automated verification done, the user's manual validation pending. The checkboxes of tasks 1 to 9 record the actual execution and the corresponding commits; the final evidence is the current suite (151 Rust tests, 50 Vitest, 36 pytest without `models`), rerun after fixing the three important findings of the branch review. The intermediate "confirm the failure" observations were not repeated in task 10. Phase 2 is **not** complete: the real results of this execution are in [docs/phase-2/validation.md](../../phase-2/validation.md).

## Global Constraints

- «One session in progress at a time, with progress and resuming after a restart.»
- «Explicit confirmation of an immutable version of the text.»
- «SQLite for the data of the new sessions; audio in local files.»
- «Choose “Todas as categorias” initially»; initial quantity three, option of one and a limit on the eligible total.
- «Draw eligible questions without repetition.» Store the selection, the order and the copies before presenting the session.
- «Separate “Guardar revisão”, “Confirmar resposta” and “Próxima pergunta”.» On the last one, use «Terminar sessão».
- «Do not reduce the total or repeat questions silently.»
- «After confirming, the answer is read-only.»
- «Do not keep JSON and SQLite as two authorities for the same new sessions.»
- «Do not change session or page during capture/finalization.» Reading during inference does not change the identity of the work.
- «Only navigate or close if the most recent version is saved or has been explicitly discarded.»
- «Do not truncate audio or text silently.»
- Interface and messages in European Portuguese; questions and practice in English.
- Preserve phase 1, configuration, models, originals, automatic results and revisions. Do not confirm old data retroactively.
- No API, follow-ups, feedback, stories, attempt comparison, session deletion or standalone installer.
- Do not change engines, voice, formats or audio deadlines as a side effect of this phase.

## Review Focus

1. Disk full or database locked after finalizing the WAV: preserve the file and repeat the publication after a restart, without a new capture and without duplication — task 4.
2. Editing during «Guardar e sair» and a confirmation concurrent with a new transcript: do not close with pending text and do not confirm another version — tasks 5 and 8.
3. Double click, command repeated after a restart and an old request after another session: do not create extra sessions, confirmations or advances — tasks 3 and 5.
4. Old interruption with `capture: None`, timeout and a new capture: keep the identity and the stop button of the new capture — task 6.
5. Aborted migration, future version or corrupt old practice session: do not recreate data, write an unknown format or hide the remaining valid items — tasks 1 and 9.

---

## 1. Observed baseline and technical choices

### What already exists

- `src-tauri/src/service.rs`: `AppService`, the `transitions` coordinator, `spawn_blocking`, audio/helper supervision, cache and recoverable publication in memory.
- `src-tauri/src/domain.rs`: `AppState`, `AppSnapshot`, `Attempt`, `Transcript`, `Review`, `Action` and errors; the question is still fixed.
- `src-tauri/src/storage.rs`: `AttemptStore` with atomic JSON in `phase1/`; `load` and the reading of manifests are private.
- `src-tauri/src/audio/mod.rs`: one thread owns the devices; `CaptureInterrupted` already carries `recording_id`. Preserve that fix.
- `src-tauri/src/worker/`: protocol v1 for health, synthesis, transcription and shutdown; it does not need to receive session concepts.
- `src/features/practice/`: page and hook centered on a single answer. `saveReview()` already distinguishes `saved`, `pending` and `failed`; preserve the protection against closing with recent edits.
- `src/App.tsx`: mounts only `Practice`; `src/lib/tauri.ts` gathers the commands.
- Rust tests in `src-tauri/tests/`, Vitest and Testing Library in the frontend. `scripts/verify.ps1` runs eight checks and includes every discovered Rust/React suite.

### Dependency and data access

Add only `rusqlite = { version = "0.41.1", features = ["bundled"] }`
to Cargo and keep the lockfile during task 1. It is a reference consulted
for this plan, not yet compiled in this project. If the resolution fails,
record the reason and use a compatible version of the same library; do not
change the stack. Do not install dependencies while preparing the plan.

Use a `rusqlite::Connection` protected by a mutex inside `Database`;
all synchronous calls run in `spawn_blocking`. The connection never crosses
an `await` with a guard acquired. Do not add an ORM, a pool or an SQL plugin in
the frontend or the server. Selection by `ORDER BY random() LIMIT ?` inside the
creation transaction avoids another dependency to draw 20 questions.

On opening: check `user_version` before any persistent change;
refuse a version above 1. Enable `foreign_keys=ON`, `busy_timeout=3000` ms and
`synchronous=FULL`; keep the `DELETE` journal in this phase. Migrate version 0→1 and
insert the initial bank in one transaction; update `user_version` only at the end.
An exception implies a rollback. Do not recover from errors by deleting and recreating the database.

**Recorded deviation (R1).** The implementation separates migration 001 — in its
own transaction, which only writes `user_version = 1` at the end — from the initial
bank, which runs afterwards in a second idempotent transaction in `ensure_question_seed`
(marks `questions-v1` in `content_seeds`). The separation avoids repeating the migration
when the seed fails and keeps each failure reversible in its own transaction; the initial
bank is never reapplied over edits because the mark exists. `docs/phase-2/setup.md`
records this behavior and does not describe it as a single transaction.

### Local files

```text
app_data_dir()/
  tutor.sqlite3
  sessions/<session-id>/<session-question-id>/<recording-id>/
    original.partial.wav # capture not yet published; name already used by Capture
    original.wav          # finalized WAV; never overwritten
    normalized.wav        # derivative, when needed
  phase1/                # preserved; read-only in the new application
    attempts/...
    question-cache/...
```

Keep the configuration in `app_config_dir()`/`LANGUAGE_TUTOR_CONFIG` and the
models in the existing location. The synthesis cache can stay in
`phase1/question-cache/`, because it is shared and the key already includes text/voice/
speed. Do not convert or move earlier practice sessions.

## 2. Files and responsibilities

| Action | Path | Responsibility |
| --- | --- | --- |
| Create | `src-tauri/src/interview/mod.rs`, `types.rs` | Phase 2 types, validation and pure transition rules. |
| Create | `src-tauri/src/db/mod.rs`, `migrations/001_guided_interview.sql` | Connection, migrations and protected synchronous execution. |
| Create | `src-tauri/src/db/questions.rs`, `sessions.rs`, `answers.rs` | Bank, creation/reading of sessions, recordings/revisions/confirmations. |
| Create | `src-tauri/src/db/seed-v1.json` | The 20 questions and the criteria of the spec, with stable UUIDs. |
| Create | `src-tauri/src/artifacts.rs` | Controlled paths, reservation, WAV validation and reconciliation. |
| Create | `src-tauri/src/service/interview.rs` | Session methods of the same `AppService`, with the existing coordinator. |
| Create | `src-tauri/src/legacy.rs` | Read-only facade over `AttemptStore`. |
| Modify | `src-tauri/src/domain.rs`, `service.rs`, `commands.rs`, `lib.rs` | Reuse audio types; integrate the new state, SQLite, commands and recoverable startup. |
| Modify | `src-tauri/src/storage.rs` | Expose reading by ID and enumeration, without changing the old format. |
| Modify | `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock` | Single new dependency and real resolution. |
| Create | `contracts/interview-v2.example.json` | Rust/TypeScript shared example, with confirmation and a pending operation. |
| Create | `src/lib/interview-contracts.ts`, `interview-client.ts` | Phase 2 DTOs and injectable IPC client. |
| Create | `src/features/sessions/SessionSetup.tsx`, `SessionSetup.test.tsx` | Prepare, start and resume. |
| Create | `src/features/questions/QuestionBank.tsx`, `QuestionBank.test.tsx` | List, add and edit questions. |
| Create | `src/features/interview/Interview.tsx`, `useInterview.ts`, `Interview.test.tsx` | Capture, review, confirmation and advance. |
| Create | `src/features/history/History.tsx`, `History.test.tsx` | Sessions, details and old practice sessions. |
| Create | `src/features/navigation/NavigationGuard.tsx`, `NavigationGuard.test.tsx` | Shared save/discard/cancel decision, including closing. |
| Modify | `src/App.tsx`, `src/features/practice/practice.css` | Navigation without a new router and reuse of the existing styles. |
| Create | `src/App.test.tsx` | Integrated journey with a controlled client. |
| Create | `src-tauri/tests/interview_contracts.rs`, `database.rs`, `questions.rs`, `sessions.rs`, `answers.rs`, `artifacts.rs`, `legacy.rs` | Contracts, transactions and failures with temporary directories. |
| Modify | `src-tauri/tests/service.rs`, `src-tauri/tests/domain.rs` | Adapt the harness to the new storage without removing regressions. |
| Modify | `README.md`, `AGENTS.md`, `docs/tech-stack.md`, phase plan | Real state, execution and persistence decisions. |
| Create during execution | `docs/phase-2/setup.md`, `docs/phase-2/validation.md` | Real commands and evidence, matrix F2-01 to F2-16. |

Do not copy the whole service. `service/interview.rs` is a child module of
`service.rs` with `impl AppService`, sharing `transitions`; public methods
acquire the coordinator once and call private helpers without reacquiring it.
Do not keep two services with independent access to the same microphone.

## 3. Contracts and physical model

### Conventions and types

Keep camelCase JSON, snake_case enums, UUIDs as text, RFC3339 UTC dates and
durations in milliseconds. `type Id = String`, `type Version = u64` and
`type Result<T> = std::result::Result<T, AppError>` in the signatures of this plan.
The DTOs have TypeScript equivalents with the same names.

| Type | Fields and values |
| --- | --- |
| `Category` | `conflict_collaboration`, `leadership_initiative`, `failure_learning`, `problem_solving`, `priorities_deadlines`, `change_adaptation`; Portuguese labels from the spec. |
| `QuestionInput` | `text`, `category`, `competencies: Vec<String>`, `criteria: Vec<String>`; editable question-specific criteria. |
| `Question` | `id`, fields of `QuestionInput`, `version`, `createdAt`, `updatedAt`. |
| `StartSession` | `requestId`, `category: Option<Category>` (`null` = all), `count: u32`. |
| `SessionSummary` | `id`, `category`, `count`, `confirmedCount`, `position` (1-based), `status: in_progress/completed`, `mode: local_v2`, `version`, `createdAt`, `completedAt?`. |
| `SessionQuestion` | `id`, `sessionId`, `sourceQuestionId`, `sourceVersion`, `position`, `text`, `category`, effective `competencies`, `criteria`, `selectedRecordingId?`, `selectionVersion`. |
| `AnswerTarget` | `sessionId`, `sessionQuestionId`, `recordingId`, `selectionVersion`; do not accept paths from the frontend. |
| `ReviewOrigin` | `automatic` or `manual_explicit`. |
| `ReviewDraft` | `recordingId`, `sourceTranscriptId?`, `origin`, `text`, `version`, `savedAt`; a zero version represents a review not yet saved. |
| `SaveReview` | `requestId`, `target: AnswerTarget`, `expectedReviewVersion`, `sourceTranscriptId?`, `origin`, `text`. |
| `ConfirmAnswer` | fields of `SaveReview`, plus `expectedSessionVersion`; confirms and saves the text in a single commit. |
| `Confirmation` | `id`, `sessionQuestionId`, `recordingId`, `reviewVersion`, `sourceTranscriptId?`, `origin`, `text`, `confirmedAt`. |
| `AdvanceSession` | `requestId`, `sessionId`, `sessionQuestionId`, `expectedSessionVersion`. |
| `RecordingView` | `id`, `sessionQuestionId`, `ordinal`, `state`, `capture: CaptureMeta?`, `originalFile?`, `normalizedFile?`, `transcripts: Vec<Transcript>`, `review: Option<ReviewDraft>`, `warnings: Vec<AppError>`. |
| `QuestionDetail` | `question: SessionQuestion`, `recordings: Vec<RecordingView>`, `confirmation: Option<Confirmation>`. |
| `SessionDetail` | `summary: SessionSummary`, `questions: Vec<QuestionDetail>`. |
| `OperationTarget` | `sessionId`, `sessionQuestionId`, `recordingId?`, `selectionVersion`, `operationId`, `workerGeneration`; capture also keeps the `recording_id` of the native event. |
| `InterviewSnapshot` | monotonic `revision` in memory, `database: ready/error/opening`, `activeSession: Option<SessionSummary>`, `current: Option<QuestionDetail>`, `availability`, `recordingId?`, `finalizing: bool`, `modelJob?` with target, `playback?`, `warnings`, `lastError?`. |
| `RecoveryReport` | `published: Vec<Id>`, `warnings: Vec<AppError>`, `interruptedOperations: Vec<Id>`. |

`Transcript`, `CaptureMeta`, `Availability` and `AppError` come from `domain.rs`.
Add the effective recognition configuration to the stored results,
without changing the Python protocol: Rust already knows the parameters sent.
Keep the old DTOs to read `phase1/`; the session interface uses the new ones.

New stable errors: `INVALID_INPUT`, `INSUFFICIENT_QUESTIONS`,
`ACTIVE_SESSION_EXISTS`, `ANSWER_CONFIRMED`, `ANSWER_REQUIRED`,
`DATABASE_UNAVAILABLE`, `DATABASE_VERSION_UNSUPPORTED`. Reuse `BUSY`,
`STALE_SOURCE`, `STORAGE_WRITE_FAILED` and the existing audio errors.

### Initial schema

Small arrays of competencies, criteria and segments use JSON TEXT validated
by serde in the application; relations and invariants use columns and FKs. Do not store
the audio as BLOBs.

| Table | Main columns and rules |
| --- | --- |
| `content_seeds` | `version PRIMARY KEY`, `applied_at`; marks `questions-v1` in the same transaction as the seed. |
| `questions` | `id PK`, `text`, `category`, `competencies_json`, `criteria_json`, `version`, dates; required fields. |
| `sessions` | `id PK`, `category NULL`, `question_count CHECK > 0`, `position`, `status`, `mode CHECK = 'local_v2'`, `version`, dates; `position BETWEEN 1 AND question_count`. |
| `session_questions` | `id PK`, `session_id FK`, `source_question_id FK`, `source_version`, `position`, copy of text/category/arrays, `selected_recording_id NULL`, `selection_version`; `UNIQUE(session_id, position)`, `UNIQUE(session_id, source_question_id)`. |
| `recordings` | `id PK`, `session_question_id FK`, `ordinal`, `state: reserved/capturing/publish_pending/ready/interrupted`, relative paths, JSON metadata, dates; `UNIQUE(session_question_id, ordinal)`. |
| `transcripts` | `id PK`, `recording_id FK`, text/status/segments/model/language/configuration/times/date, immutable. |
| `reviews` | `(recording_id, version) PK`, `source_transcript_id NULL`, `origin`, `text`, `saved_at`; add versions instead of rewriting previous ones. |
| `confirmations` | `id PK`, `session_question_id UNIQUE FK`, `recording_id`, `review_version`, `source_transcript_id NULL`, `origin`, `text`, date; FK to the revision version and consistency with the question's recording. |
| `operations` | `id PK`, type/state, session/question/recording/generation, source configuration, error and times; a result only closes its own request. |
| `command_receipts` | `request_id PK`, `kind`, `payload_hash`, `session_id`, `entity_id`, `result_version`; written in the mutation transaction, without duplicating full texts. |

Create `CREATE UNIQUE INDEX one_active_session ON sessions((1)) WHERE
status = 'in_progress'`. Add composite FKs to guarantee that the selection,
the confirmation and the source transcript belong to the same question/recording; use
auxiliary unique indexes `(session_question_id,id)` and `(recording_id,id)` where
necessary. The selection FKs can be `DEFERRABLE INITIALLY DEFERRED` to
allow inserting the recording and changing the selection in the same transaction.

`BEFORE UPDATE/DELETE` triggers prevent changes to transcripts,
confirmations, review versions and the copied contents of the questions. The selection
field of `session_questions` may change before confirming. Do not include
tables from future phases or deletion commands.

### Concurrency and repetition

Mutations receive a UUID `requestId` generated in the client and kept on a repetition
of the same request. Check the receipt before validating versions: the same ID and payload
return the existing result without executing another mutation; a different payload
returns `STALE_SOURCE`. Store a SHA-256 hash of the canonical serialization of the request
fields, without including `requestId` and without depending on map order.

`sessions.version` controls start/advance/close and confirmation;
`selectionVersion` protects the selection of the recording; the revision version controls
edits. Updates use version conditions and check the affected rows.
Model results do not depend on the global version of the session, because saving
a review during inference is allowed: validate the target, generation, operation,
selection and absence of confirmation. A late result never swaps the selection.

The replay of an old start returns the original session, even if completed;
it does not create a new interview. The replay of an advance does not apply another advance.
Do not hold transactions or state mutexes during audio I/O or inference.

## 4. Execution sequence

Before implementing: check the Git state and preserve the changes that already
exist; use the usual isolation of the execution. Reconcile the pending manual criteria
of phase 1 with the user's evidence. This plan does not mark them as
met and does not require repeating evidence already recorded.

The following commands start from the root. In PowerShell, when necessary:
`$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"`. Each task follows the cycle
failing test → confirmation of the pertinent failure → minimal implementation →
green test. Failures due to a missing environment do not count as a regression test.
The commits indicated are suggested points for the execution; include only the
files of the task, without adding preexisting changes indiscriminately.

### Task 1 — Contracts, schema and recoverable SQLite opening

**Files:** create `interview/{mod.rs,types.rs}`, `db/mod.rs`, the migration,
`contracts/interview-v2.example.json`, `src/lib/interview-contracts.ts` and tests
`interview_contracts.rs`/`database.rs`; modify `lib.rs`, Cargo and the lock.
The Rust paths of this section are relative to `src-tauri/src/`.

**Interfaces:**

- `Database::open(path: PathBuf) -> Result<Database>`; `Database: Clone + Send + Sync` through an internal `Arc<Mutex<Connection>>`.
- `Database::with_connection<T>(&self, f: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T>`; synchronous, never called directly in an async command.
- Types from section 3 and `validate_question(input: QuestionInput) -> Result<QuestionInput>`; `validate_count(count: u32, eligible: usize) -> Result<()>`.

- [x] Write `fresh_database_has_version_one`, `failed_migration_rolls_back`, `future_version_is_not_modified`, `foreign_keys_reject_cross_recording_sources` and `v2_contract_matches_fixture`. Assertions: `user_version == 1`; an injected error preserves schema/data/version; version 99 creates neither tables nor seed; a source from another recording is refused; the Rust JSON matches the example.
- [x] Run `cargo test --manifest-path src-tauri/Cargo.toml --test database --test interview_contracts`; confirm failure due to a missing contract/schema.
- [x] Add rusqlite and implement the types, schema, triggers, constraints and migration transaction. Add an SQL test that tries to change a confirmed text and a transcript and receives an error.
- [x] Rerun the command and `npm.cmd run typecheck`; require everything green and exit 0. Keep the lock actually resolved.
- [x] Suggested commit: `feat: add guided interview contracts and sqlite schema`.

### Task 2 — Initial bank and persistent editing

**Files:** create `db/questions.rs`, `db/seed-v1.json`, `src-tauri/tests/questions.rs`; complete the validation and the seed integration in `db/mod.rs`.

**Interfaces:** synchronous methods in `Database`:

- `list_questions(category: Option<Category>) -> Result<Vec<Question>>`.
- `create_question(input: QuestionInput) -> Result<Question>`.
- `update_question(id: &str, expected_version: Version, input: QuestionInput) -> Result<Question>`.
- `ensure_question_seed() -> Result<()>`; invoked inside the initial preparation, marks `questions-v1` atomically.

- [x] Write `seed_is_once_and_preserves_edits`, `bank_covers_six_categories`, `question_trims_only_outer_whitespace`, `blank_fields_are_rejected`, `stale_edit_does_not_overwrite`. Assertions: 20 questions, distribution 4/3/3/4/3/3, repeatable IDs, an edit preserved after reopening, inner text intact and an old version refused.
- [x] Run `cargo test --manifest-path src-tauri/Cargo.toml --test questions`; observe the pertinent failure.
- [x] Transcribe the 20 lines of the spec into the seed with fixed UUIDs. Store question-specific criteria in the question; define common STAR criteria versioned in a constant and copy them when creating the session. Validate the category, non-empty arrays and elements without outer whitespace. Do not use `INSERT OR REPLACE` on the existing bank.
- [x] Rerun the suite; also verify the rollback of an incomplete seed, with no applied version mark.
- [x] Suggested commit: `feat: persist the editable interview question bank`.

### Task 3 — Atomic creation, fixed selection and session reading

**Files:** create `db/sessions.rs`, `src-tauri/tests/sessions.rs`; complete the reading types.

**Interfaces:** methods of `Database`:

- `start_session(request: StartSession) -> Result<SessionDetail>`.
- `active_session() -> Result<Option<SessionDetail>>`.
- `list_sessions() -> Result<Vec<SessionSummary>>`.
- `load_session(session_id: &str) -> Result<SessionDetail>`.

- [x] Write `starts_one_three_and_available_count`, `insufficient_category_creates_nothing`, `snapshots_survive_bank_edits`, `repeated_start_returns_original_session`, `only_one_session_can_be_active`. Assertions: positions 1..N without repetition; a zero or excessive quantity is refused; text/competencies/criteria do not change; the same receipt after reopening does not insert another session; two concurrent connections do not get past the unique index.
- [x] Run `cargo test --manifest-path src-tauri/Cargo.toml --test sessions`; observe the failures.
- [x] Implement `BEGIN IMMEDIATE`, validation, SQL draw, complete copies with STAR criteria and insertion of the receipt in the same transaction. Validate the real count before the commit. Do not test randomness with fragile statistical expectations; test the set, the uniqueness and the preservation of the stored order.
- [x] Rerun the suite and test an injected error after inserting the first question: the whole session disappears through the rollback.
- [x] Suggested commit: `feat: create recoverable sessions with fixed question snapshots`.

### Task 4 — Originals, interrupted publications and transcripts

**Files:** create `artifacts.rs`, `db/answers.rs`, `src-tauri/tests/artifacts.rs`; add cases in the `answers.rs` tests.

**Interfaces:**

- `ArtifactStore::new(root: PathBuf) -> ArtifactStore`.
- `ArtifactStore::reserve(session_id: &str, question_id: &str, recording_id: &str) -> Result<RecordingTarget>`; `RecordingTarget` is the existing audio type.
- `ArtifactStore::inspect(target: &RecordingTarget) -> Result<CapturedAudio>`; validates a finalized `original.wav`, does not promote `original.partial.wav`.
- `Database::reserve_recording(question_id: &str, expected_selection: Version) -> Result<RecordingView>`; generates the ID/ordinal before opening the microphone.
- `Database::publish_recording(capture: CapturedAudio) -> Result<RecordingView>`; idempotent by the reserved ID, binds the target already stored in SQLite.
- `Database::append_transcript(target: OperationTarget, transcript: Transcript, configuration: serde_json::Value) -> Result<RecordingView>`.
- `Database::set_normalized(recording_id: &str, relative_path: &str) -> Result<()>`.
- `reconcile(database: &Database, artifacts: &ArtifactStore) -> Result<RecoveryReport>`.

- [x] Write `finalized_audio_survives_database_failure`, `reopen_reconciles_once`, `part_file_is_not_a_saved_answer`, `older_publication_cannot_replace_newer_selection`, `missing_audio_keeps_text`, `new_capture_failure_keeps_previous_selection`, `transcript_append_does_not_replace_review`. Assertions: the WAV exists after the failure, the publication after reopening creates a single ready record; `original.partial.wav` is never presented as a valid answer; a missing original keeps the texts readable.
- [x] Run `cargo test --manifest-path src-tauri/Cargo.toml --test artifacts --test answers`; confirm failure.
- [x] Reserve a row before the capture; the finalized original is the recoverable evidence if the following write fails. Never depend only on the current `pending_publish` in memory. Reconciliation inspects pending reservations, recovers a closed WAV and marks `incomplete=true` if the closing has no confirmed metadata. The absence of an original makes the capture interrupted, without inventing a duration or a success.
- [x] On publication, change the selection only to a higher ordinal and a question not yet confirmed; increment `selectionVersion`. Keep previous recordings. Validate the WAV with Hound and reuse the normalization. Resolve paths only by IDs/root, refusing `..`, absolute paths and links that escape the managed folder. Add `paths_cannot_escape_artifact_root`.
- [x] Store automatic results and the operation state atomically; never rewrite automatic text or a review. Reconciliation marks old jobs `interrupted`, preserves unknown orphans with a warning and does not delete them.
- [x] Rerun the suites with an injected failure in the commit and the database reopened; require recovery without a new capture and without a duplicated transcript.
- [x] Suggested commit: `feat: preserve recording artifacts across publication failures`.

### Task 5 — Versioned review, confirmation and idempotent advance

**Files:** complete `db/answers.rs`, `db/sessions.rs`, rules in `interview/mod.rs`; tests `answers.rs` and `sessions.rs`.

**Interfaces:** methods of `Database`:

- `save_review(request: SaveReview) -> Result<ReviewDraft>`.
- `confirm_answer(request: ConfirmAnswer) -> Result<Confirmation>`.
- `advance_session(request: AdvanceSession) -> Result<SessionDetail>`; on the last position it completes the session.

- [x] Write `save_does_not_confirm`, `confirmation_freezes_exact_seen_text`, `stale_review_cannot_confirm`, `manual_no_speech_keeps_origin`, `cannot_confirm_without_original`, `advance_requires_confirmation`, `replayed_advance_does_not_skip`, `last_advance_completes_only_full_session`. Assertions: the review keeps the exact text, including spaces; validate `trim()` only to refuse empty; the confirmation does not change the position; the finish has N confirmations and `mode=local_v2`, with no pending feedback.
- [x] Run `cargo test --manifest-path src-tauri/Cargo.toml --test answers --test sessions`; confirm the failures.
- [x] Implement the append-only review and the confirmation in one transaction, with the expected version, the selected target, a finalized original present and belonging to the question. `manual_explicit` allows a review without an automatic result or over `no_speech`; it does not fabricate a transcript. Confirming inserts the exact review if it changed and the confirmation, updates the session version and stores a receipt. An equal retry checks the receipt before refusing as already confirmed.
- [x] Implement the advance with expected position/question/version and a receipt. On the last question, check all the confirmations inside the transaction. Never use the completion of a transcription as an advance action.
- [x] Add `confirmation_failure_rolls_back_review_and_receipt`, `same_request_with_other_text_is_rejected`, `late_transcript_cannot_change_confirmation`, `old_start_replay_after_completion_does_not_create_session`. Reopen the database between the request and the replay. Also verify an SQL attempt to rewrite the confirmation.
- [x] Rerun both suites; exit 0.
- [x] Suggested commit: `feat: confirm immutable answers and advance sessions safely`.

### Task 6 — Integrate the service, commands, events and startup

**Files:** modify `service.rs`, `commands.rs`, `lib.rs`, `domain.rs`, `src-tauri/tests/service.rs` and the test `domain.rs`; create `service/interview.rs` and `src/lib/interview-client.ts`.

**Interfaces:**

- Adapt `AppService::new` to `Database` and `ArtifactStore` in place of `AttemptStore`; retain the existing audio port, launcher, manifest and generation. Initialization/recovery in `spawn_blocking`, with an `opening` snapshot and a usable window.
- `AppService::snapshot() -> InterviewSnapshot`; `subscribe` publishes `interview://snapshot` with that DTO.
- Async methods of `AppService`: `start_session(StartSession)`, `save_review(SaveReview)`, `confirm_answer(ConfirmAnswer)`, `advance_session(AdvanceSession)`, `start_recording(session_question_id: Id, expected_selection: Version)`, `stop_recording()`, `retry_save_recording(recording_id: Id)`, `transcribe_response(target: AnswerTarget)`, all `-> Result<InterviewSnapshot>`.
- Async reading methods: `list_questions`, `create_question`, `update_question`, `list_sessions`, `load_session` with the types of the previous tasks. Reads return DTOs; question mutations return `Question`.
- `play_audio(target: AudioSelection) -> Result<InterviewSnapshot>`, with `AudioSelection = Question { sessionId, sessionQuestionId } | Recording { recordingId } | Legacy { attemptId }`; resolve the path in the core, allow history reading, refuse during capture.
- Reuse `refresh_availability`, `stop_audio`, `restart_worker`, `prepare_close`; add `retry_database() -> Result<InterviewSnapshot>` to repeat the opening/reconciliation without restarting the window.
- `InterviewClient`: corresponding camelCase methods and `subscribe(listener) -> Unsubscribe`; gather the command and event names in this file. New Tauri commands prefixed `interview_`, including `interview_snapshot`, `interview_list_questions`, `interview_create_question`, `interview_update_question`, `interview_start_session`, `interview_list_sessions`, `interview_load_session`, `interview_start_recording`, `interview_stop_recording`, `interview_retry_save_recording`, `interview_transcribe_response`, `interview_save_review`, `interview_confirm_answer`, `interview_advance_session`, `interview_play_audio`, `interview_stop_audio`, `interview_refresh_availability`, `interview_restart_worker`, `interview_prepare_close`, `interview_retry_database`.

- [x] Adapt the existing harness to a temporary database with seed and a session explicitly started with the fixed question of the tests. Keep the assertions of recovery, timeout, cache, playback exclusion and review; change only the contracts and the preparation necessary. Add `recording_requires_active_unconfirmed_question`, `confirmed_question_rejects_capture_and_transcription`, `database_error_keeps_window_recoverable`, `history_read_does_not_retarget_model_job`.
- [x] Run `cargo test --manifest-path src-tauri/Cargo.toml --test service --test domain`; confirm failure of the new contracts before implementing the integration.
- [x] Bind the database methods in `spawn_blocking`, coordinated by the existing `transitions`. Load the active session after commits; snapshots reflect only stored state. Separate the global revision of the snapshot from the persistent versions. Do not acquire `Mutex<AppState>` while calling a helper that reacquires it to record an error.
- [x] Replace the constant question with the text of the current copy when synthesizing. Bind the helper result to `OperationTarget` without changing protocol v1; the operation map in Rust makes that association. Review the automatic start of transcription so that it uses only a published recording and an unconfirmed question. A result whose target was already confirmed or replaced ends the operation as interrupted/obsolete and releases the work in memory, without changing texts or selection and without leaving the interface busy.
- [x] Preserve and run `repeated_start_returns_busy_instead_of_blocking`, `late_interruption_from_an_older_capture_is_ignored`, `timed_out_stop_recovers_capture`. Add `late_worker_result_cannot_target_next_question` and `question_cache_distinguishes_session_texts`. Use short test deadlines to detect a deadlock without leaving the suite stuck.
- [x] Register the commands in `lib.rs`, handle a database error without an `expect` that shuts the application down and connect the new client. Test the IPC serialization against the v2 example and verify the errors in Portuguese. End the capture and the helper on normal shutdown; a publication error prevents indicating that everything was saved.
- [x] Run the previous suites, `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` and `npm.cmd run typecheck`; require exit 0.
- [x] Suggested commit: `feat: connect guided sessions to the existing audio service`.

### Task 7 — Setup, navigation and question bank in the interface

**Files:** create `SessionSetup.tsx`, `QuestionBank.tsx` and their tests in the paths of section 2; modify `src/App.tsx` and the CSS; create an initial `src/App.test.tsx`.

**Interfaces:** `SessionSetup({ client, snapshot, onStarted })` and
`QuestionBank({ client })`; `client: InterviewClient`,
`snapshot: InterviewSnapshot`, `onStarted(snapshot: InterviewSnapshot): void`.
`App` receives an injectable client in the tests and keeps the subscription throughout the
navigation. Local state `page: setup/bank/interview/history`, without a router or an
additional global state manager. The active identity comes from Rust.

- [x] Write `defaults_to_three_and_all_categories`, `insufficient_questions_require_explicit_adjustment`, `active_session_offers_resume`, `bank_edit_survives_reload`, `bank_rejects_empty_fields`. Assertions: a category with two questions keeps quantity three and a warning; it only starts after the explicit change; the bank sends competencies/criteria and the expected version.
- [x] Run `npm.cmd test -- src/features/sessions/SessionSetup.test.tsx src/features/questions/QuestionBank.test.tsx src/App.test.tsx`; confirm the failures.
- [x] Implement the setup and the bank form with labeled fields. Query the eligible total from the bank, revalidated in the core. Block repeated submissions and keep the UUID when repeating a failed submission; a new intention receives a new UUID. Show «Retomar sessão» instead of another start when there is a session in progress.
- [x] Use common navigation and the database availability/error state; `retryDatabase` allows recovery. During capture/finalization, block page changes. While the model is busy, allow reading without starting another incompatible operation.
- [x] Rerun the tests and `npm.cmd run typecheck`; require exit 0 and messages in European Portuguese.
- [x] Suggested commit: `feat: add session setup and editable question bank screens`.

### Task 8 — Interview, confirmation and protection of changes

**Files:** create the files of `features/interview/` and `features/navigation/` from section 2; integrate them in `App.tsx`. Reuse the visual logic/styles of the existing practice, without mounting two audio clients.

**Interfaces:**

- `useInterview(client: InterviewClient, snapshot: InterviewSnapshot)` exposes `review`, `setReviewText(text)`, `saveReview(): Promise<'saved'|'pending'|'failed'>`, `discardReview()`, `confirm(): Promise<boolean>`; refs keep the recent version and text.
- `Interview({ client, snapshot, onSnapshot })`, with `onSnapshot(snapshot: InterviewSnapshot): void`.
- `NavigationGuard` receives save/discard callbacks, dirty/busy state and the intention `navigate | record | close`; a single decision controls a new capture, a page change and closing. Keep drafts by `recordingId` when reading the history during inference.

- [x] Write `shows_question_two_of_three`, `save_does_not_enable_next_until_confirmation`, `confirmation_locks_exact_editor_text`, `last_question_uses_finish_label`, `manual_revision_of_no_speech_is_explicit`, `prior_recordings_remain_readable`. Assertions: confirming and advancing are two distinct requests; the final button calls the advance of the last question; the interface neither shows feedback nor reopens the confirmation.
- [x] Write in the guard `save_and_leave_keeps_newer_edit`, `failed_save_does_not_destroy_window`, `cancel_keeps_editor`, `discard_requires_explicit_choice`, `capture_blocks_navigation`. Simulate a pending save promise, edit again, resolve and verify that neither navigation nor `destroy()` is called.
- [x] Run `npm.cmd test -- src/features/interview/Interview.test.tsx src/features/navigation/NavigationGuard.test.tsx`; confirm the failures.
- [x] Implement an editable copy distinct from the original, versions and an explicit manual origin. During confirmation, prevent editing/switching the recording; the request includes exactly the text presented, the expected versions and a stable UUID. A failure keeps the text; a received confirmation is read-only. «Próxima pergunta» only appears enabled with a persisted confirmation.
- [x] Integrate the Tauri close in a single application-level listener: `prepareClose` finalizes the capture, the decision resolves the reviews and only then is `destroy()` called. A pending confirmation or save cannot be overridden by the close; allow repeating after the error. Do not remove the existing `SaveOutcome` protection when adapting the hook.
- [x] Add `old_snapshot_does_not_erase_draft`, `confirm_failure_preserves_text`, `double_next_click_sends_one_request`, `synthesis_failure_still_allows_recording`. Reject snapshots with a lower monotonic revision. Adapt the valid tests of `Practice.test.tsx` to the new page before removing the old component from the application; keep old modules only if they still serve tests/read-only views, without duplicated UI.
- [x] Rerun the tests and `npm.cmd run typecheck`; exit 0.
- [x] Suggested commit: `feat: guide recording review confirmation and session progress`.

### Task 9 — History, resume and reading of the phase 1 practice sessions

**Files:** create `legacy.rs`, `src-tauri/tests/legacy.rs`, `History.tsx` and `History.test.tsx`; modify `storage.rs`, commands, client, `App.tsx` and `App.test.tsx`.

**Interfaces:**

- Expose in `AttemptStore` `list_saved() -> Result<Vec<RestoreResult>>` and `load_saved(attempt_id: &str) -> Result<RestoreResult>`, reusing the existing reading. Never write while reading; an invalid folder produces `RestoreResult { attempt: None, warnings }` and does not abort the enumeration.
- `LegacyStore::new(root: PathBuf) -> LegacyStore`; `list() -> Result<Vec<RestoreResult>>`, `load(id: &str) -> Result<RestoreResult>`; expose only reading and resolution of the original for playback. No reference to the writer in the runtime flow.
- Commands `interview_list_legacy`, `interview_load_legacy`, client `listLegacy()/loadLegacy(id)` with old types imported from `contracts.ts`. Do not reuse the old `AppSnapshot` as if it were a session.
- `History({ client, onResume })`, with `onResume(sessionId: string): void`. Read sessions through `loadSession`, do not change the current selection when opening details.

- [x] Write `legacy_list_ignores_last_pointer`, `corrupt_legacy_item_does_not_hide_valid_items`, `legacy_read_changes_no_bytes`, `legacy_rejects_path_escape`; test manifests, revisions and originals with hashes before/after the reading. Confirm the absence of tables/sessions created for those items.
- [x] Run `cargo test --manifest-path src-tauri/Cargo.toml --test legacy --test storage`; confirm failure of the new methods.
- [x] Implement enumeration by folders, warnings per artifact and protected reading without changing the JSON format. Keep the old storage tests, including atomic writing, as a regression of that format.
- [x] Write `history_separates_raw_review_and_confirmed`, `resume_keeps_confirmed_question_awaiting_next`, `missing_audio_keeps_confirmed_text`, `legacy_has_no_confirmation_action`; run `npm.cmd test -- src/features/history/History.test.tsx src/App.test.tsx` and observe the failures.
- [x] Implement listing by date, state and progress; details of all the questions and recordings. A completed session shows a summary, not pending feedback. Earlier practice sessions have their own area and are read-only/playback only. Resume obtains the current snapshot without advancing automatically.
- [x] Complete the integrated test `three_question_session_can_resume_and_finish`: create a controlled client, answer/confirm the first, simulate a reopening, advance explicitly, finish the remaining ones and read three confirmations. No AI call or follow-up dismissal exists in the client.
- [x] Rerun the Rust/React suites indicated and `npm.cmd run typecheck`; require exit 0.
- [x] Suggested commit: `feat: browse session history and preserve legacy practice recordings`.

### Task 10 — Whole-set verification and Windows evidence

**Files:** `scripts/verify.ps1` only if the new tests are not already
discovered; `README.md`, `AGENTS.md`, `docs/tech-stack.md`, phase plan,
`docs/phase-2/setup.md` and `docs/phase-2/validation.md`.

**Interfaces:** keep the existing execution commands. Do not create a new
installer or a promise of execution without Python; the target remains Windows in
development and release `--no-bundle`.

- [x] Create matrix F2-01 to F2-16 in `docs/phase-2/validation.md`, initially pending, linked to the tasks and tests of section 5. Document the configuration, directories, SQLite and resume in `setup.md` without copying personal data to Git.
- [x] Run `.\scripts\verify.ps1` and record the real output: all checks must end with 0. Do not copy phase 1 counts as if they were those of this execution. Resolve the pertinent failures before repeating.
- [x] Run `.\.venv\Scripts\python.exe -m pytest audio-worker/tests/test_offline_models.py -m models -q`; record the real result, distinguishing `passed` from `skipped` and preserving the existing configuration/models.
- [x] Run `npm.cmd run tauri -- build --no-bundle` and test the startup/shutdown of the Windows binary. Record that it is not a standalone installer and does not validate the phase 5 distribution.
- [ ] Execute with the user: a session of three real questions, one with technical terms; listening to questions/originals; review and confirmation; leaving and resuming after confirmation before advancing; finishing and opening the history. Record the audio durations and the loading/synthesis/transcription times, including the perception of the wait.
- [ ] Validate a one-question session and another available quantity, insufficiency in a category, an edit in the bank without changing the session copy, the offline journey, a transcript unavailable with an explicit review, a write error and recovery. When simulated failures are used, identify the simulation and do not present it as real hardware.
- [ ] Update the documents with what passed and what is missing. Mark phase 2 concluded only with demonstrated criteria and manual validation; keep phase 1 in the state that its own evidence sustains. Do not start phase 3.
- [x] Suggested commit: `docs: record guided interview setup and validation evidence`.

**Status of task 10 (2026-11-02):** the automatic steps and the
documentation were executed; the three manual validation steps (422, 423 and the closing of 424)
remain open. Step 424 is partially met — the documents were
updated with what passed and what is missing — but phase 2 was **not** marked
as concluded, because the manual validation remains pending. Real results in
[docs/phase-2/validation.md](../../phase-2/validation.md).

## 5. Criteria coverage and plan review

| Criterion | Tasks | Main evidence planned |
| --- | --- | --- |
| F2-01 | 1, 2 | Transactional seed, 20 questions, six categories, edits preserved. |
| F2-02 | 2, 7 | Validation, versions and form, reopening of the database. |
| F2-03 | 3, 7, 10 | Quantities 1/3/other, insufficiency and explicit adjustment. |
| F2-04 | 3 | Selection without repetition, order and persisted copies. |
| F2-05 | 6, 8, 10 | Journey of three questions with real audio and an alternative without synthesis. |
| F2-06 | 1, 4, 5, 9 | Relations, immutability and separate presentation of the versions. |
| F2-07 | 5, 8 | Exact confirmation, version control, rollback and editor locking. |
| F2-08 | 5, 8 | Preconditions, receipts, session version and final button. |
| F2-09 | 4, 6, 8 | Independent captures, selection only after publication, confirmation blocks reopening. |
| F2-10 | 8 | Shared guard and tests of editing during writing/closing. |
| F2-11 | 4, 5, 6, 9 | Real reopening of the database in tests and resume in the window. |
| F2-12 | 4 | Failures between WAV and SQLite, reconciliation and missing files. |
| F2-13 | 5, 6 | Command repetition, timeouts, old interruption and operation targets. |
| F2-14 | 9, 10 | History, playback and unchanged hashes of the old practice sessions. |
| F2-15 | 1, 3 | Migration rollback, future version and single-session index. |
| F2-16 | 6, 9, 10 | No external integration in the code/client and a complete offline journey in the window. |

Documentary review of this plan: every criterion has tasks and verifications;
the five risk conditions have assigned tests; SQLite and files have
separate recovery; the Python protocol remains v1; no step depends on
AI or future entities. The signatures and the v2 example must remain
coherent in Rust and TypeScript during the execution. These are commitments of the
plan, not statements about tests already executed.

## 6. Technical references consulted for the plan

- [rusqlite — official repository](https://github.com/rusqlite/rusqlite): direct access to SQLite and the `bundled` feature.
- [rusqlite — Connection documentation](https://docs.rs/rusqlite/latest/rusqlite/struct.Connection.html): the reference consulted identified 0.41.1; the compilation will be verified in task 1.
- [SQLite — PRAGMA](https://www.sqlite.org/pragma.html): application version, foreign keys, journal and timeout.
- [SQLite — partial indexes](https://www.sqlite.org/partialindex.html): restriction of a single session in progress.

No packages were installed, no product files were modified, no builds were run
and no test commands were tried out to prepare this plan.

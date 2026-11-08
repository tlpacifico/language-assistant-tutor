# Phase 3 — Implementation plan for the AI tutor

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** add contextual follow-ups and grounded feedback to the interviews, with controlled sending, observable usage and recovery.

**Architecture:** Rust is the authority over sessions, targets, requests and evidence. An initial answer and its follow-up use the same audio path through explicit targets; SQLite preserves every text and attempt. HTTP runs outside the service locks and the frontend only presents states and intentions.

**Stack:** React/TypeScript, Tauri 2/Rust, SQLite/rusqlite, existing Python helper; add `reqwest = 0.13.5` (JSON and Rustls TLS) and `keyring = 3.6.3` with `windows-native`, keeping Cargo.lock.

**Spec:** [Phase 3 — AI tutor](../specs/2026-11-02-phase-3-ai-tutor-design.md).

Date: 2026-11-02. State: reviewed and technically approved by the coordinator, by delegation of the user; implementation automatically verified; technical review by the coordinator concluded; manual acceptance pending. The user authorized bringing phase 3 forward and delegated to the coordinator the review of spec, plan and implementation. The manual acceptances of phases 1 and 2 remain pending. One agent implements sequentially; the coordinator reviews the result and can request corrections, without additional review agents.

## Global constraints

- Keep React/TypeScript, Rust/Tauri, SQLite `rusqlite` and the Python helper.
- Introduce `ai_v3` for new sessions; `local_v2` remains available, without follow-ups, fictitious costs or retroactive feedback.
- The application continues to control the interview; the LLM recommends content and does not execute transitions, commands or tools.
- Quantity and progress count only main questions. For N main questions, there are at most N follow-ups.
- Do not send audio, local paths, drafts, automatic transcripts distinct from the confirmed version, the complete question bank or personal notes.
- Do not do automatic retries or automatic paid repairs in this phase.
- Absent is `null`/unknown, never inferred zero.
- Do not hold `AppService`'s `transitions` during the HTTP `await`.
- Valid reports are immutable and keep the sources/prompt/rubric.
- European Portuguese in the interface and explanations; English questions, follow-ups, quotes and corrections in English.
- No phase 4, installer, new engines, overall score or acoustic/emotional evaluation.

## Review focus

1. Session migration v1 in the middle of an answer: identities, paths and receipts are preserved and the local path continues; test in task 1.
2. Waiver while HTTP is suspended: advancing does not wait for HTTP and a late result only adds usage; test in task 5.
3. SQLite failure after a remote answer: repeating the publication preserves the result and does not charge a new request; test in task 5.
4. Unicode quote and absence without text: reject fabricated matches and accept absence with valid sources; test in task 3.
5. Cache larger than input, partial usage or change of model/tariff: do not produce negative costs or fictitious complete totals; test in task 4.

## 1. Structure and integration contracts

Create `src-tauri/src/ai/{mod,types,validation,prompts,usage,transport,credentials}.rs`, `src-tauri/src/db/ai.rs` and `src-tauri/src/service/ai.rs`. They are, respectively, pure contracts/validation, HTTP/secrets adaptation, transactions and coordination. Do not move modules unrelated to the change.

Create `src/lib/ai-contracts.ts`, `src/features/ai/{AiSettings,FeedbackPanel}.tsx` and corresponding tests. Extend the existing `InterviewClient`, the shared fake, setup, interview, history and App. The frontend continues to have a single Tauri adapter.

### Answer targets and compatibility

Add `AnswerSlot { id, sessionQuestionId, role: initial|follow_up, text, selectedRecordingId, selectionVersion }`. The main question remains in `SessionQuestion`, responsible only for the bank/copies/progress; do not insert follow-ups into that table. `QuestionDetail` keeps `question`, `recordings`, `confirmation` for the initial answer and gains `initialAnswerId`, `followUp: FollowUpDetail|null`, `followUpStage`. `FollowUpDetail { answer: AnswerSlot, recordings, confirmation }`. `InterviewSnapshot` gains `currentAnswer: AnswerSlot|null`, `aiJob: AiJobView|null` and `pendingAiPublication: Id|null`; `current` remains the main question, without changing the meaning of `Pergunta X de N`.

`AnswerTarget` gains `answerId`; `sessionQuestionId` continues to identify the main question. `RecordingView` and `Confirmation` gain `answerId`; `Confirmation` keeps `sessionQuestionId` as the main question. `OperationTarget` gains `answerId: Id|null` for synthesis/transcription. `StartSession` gains `mode: SessionMode` with deserialization default `local_v2`, preserving clients/tests that omit the field. The new JSON/TS send the field explicitly. `startRecording(answerId, expectedSelection)` passes a target ID, never a path. For the initial answer, `answerId == sessionQuestionId`.

`AudioSelection` gains `FollowUp { sessionId, sessionQuestionId, answerId }`; the existing variants are kept. The Kokoro cache remains indexed by text/voice/speed. Artifacts of new follow-ups go in `sessions/<session>/<answerId>/<recording>/`; initial targets keep the existing paths. Reconciliation always resolves the persisted owner; it does not depend on the question being current.

### SQLite migration v2

Keep the file `001_guided_interview.sql` intact as the v1 origin. The new migration rebuilds the necessary tables with an explicit copy, in the safe SQLite pattern: `PRAGMA foreign_keys=OFF` only on the exclusive startup connection, before the transaction; create/copy/drop/rename; recreate indexes and triggers; `PRAGMA foreign_key_check` must return zero rows before commit; restore FK ON even after an error. Do not use `writable_schema` or SQL substitutions in `sqlite_master`.

- `sessions`: widen the CHECK of mode to `local_v2|ai_v3`; state remains `in_progress|completed`. Pending feedback does not occupy an active session index.
- `answer_slots`: `id` PK, `session_question_id` FK, `role` CHECK, `text`, selection/versioning; UNIQUE `(session_question_id,role)`. Fill the initial slot with the ID of the main question and copy selection/text. Immutability of identity/role/text; selection only before the confirmation. The application creates the initial slot in the same transaction that creates the main question.
- `recordings`: replace the owner `session_question_id` with `answer_id` FK to the slot; IDs, ordinals, states and paths are copied without change. The composite index becomes `(answer_id,id)`.
- `confirmations`: `answer_id` UNIQUE and composite FK `(answer_id,recording_id)`; copy identities and revision. They remain append-only. Reviews/transcripts do not change content.
- `session_questions`: the selection now lives in the slot; the DTO can still fill the old fields from the initial slot during the transition. Remove the old cyclic FK when rebuilding. The triggers of the content copies are kept.
- local `operations` gains a nullable `answer_id`, filled in the old jobs with the main question; it keeps the FK of the main question for context. The local kind variants remain only `transcribe|synthesize`; external requests have their own table.
- `ai_settings`: singleton with version/public JSON configuration and destination consent; without secret.
- `ai_stages`: one row per main question of an AI session, stage `not_requested|running|failed|not_needed|follow_up|answered|waived`, positive generation, reason/error, ID of the initial confirmation; context FK. Created with the initial confirmation.
- `ai_requests`: single logical operation `(purpose,session_question_id)` for `follow_up|feedback`, immutable sources and versions of prompt/rubric/schema. `ai_attempts`: ID PK, request ID, generation, copied configuration/tariff, `reserved|running|completed|failed|interrupted|discarded`, dates, sending `not_sent|possibly_sent|received`, safe usage and error. Do not store HTTP error bodies or the key.
- `ai_reports`: one immutable valid evaluation per main question, originating attempt, initial confirmation and eventual follow-up, validated JSON. Triggers prevent UPDATE/DELETE.
- `command_receipts`: keep the old receipts and widen kind for AI commands. The payload hash follows the existing SHA-256 pattern. Repetition with the same content does not create a new attempt.

Update `Database::open` to apply v1 and then v2, one migration transaction per opening that changes the version only after the checks. The test `open_with_schema` must continue to test rollback with an invalid schema without inadvertently applying v2 over an arbitrary schema. Future databases are refused before persistent changes.

### Requests and public DTOs

Use the project's `Result<T>`/`AppError`, UUID IDs and RFC3339 dates; camelCase JSON and snake_case enums. Rust types live in `ai/types.rs`, mirrored by `ai-contracts.ts` and fixtures `contracts/ai-v3.example.json`.

- `AiSettingsInput { baseUrl, model, tariff: Tariff|null }`; `Tariff { model, currency:"USD", inputPerMillion, outputPerMillion, cachedInputPerMillion:null|number, observedAt, sourceUrl }`.
- `AiSettingsView { version, baseUrl, model, tariff, credentialConfigured, consent: Consent|null }`; `Consent { destination, noticeVersion:"text-external-v1", acceptedAt }`.
- `SaveAiSettings { requestId, expectedVersion, input }`; `AcceptAiConsent { requestId, expectedSettingsVersion, destination, noticeVersion }`.
- `AiQuestionCommand { requestId, sessionId, sessionQuestionId, expectedSessionVersion }` to request/waive; the waiver adds `reason: unavailable|request_failed|user_cancelled_wait`.
- `AiSessionCommand { requestId, sessionId }` to generate feedback. Only a completed interview, in AI mode, with fixed sources.
- `AiJobView { attemptId, purpose, sessionId, sessionQuestionId, stage: requesting|publishing }`.
- `FeedbackView { sessionId, state: pending|running|partial|failed|ready, reports, priorities, usage, attempts }`; local returns the absence of an AI evaluation, not a fictitious ready state.

`AppService` exposes `load_ai_settings()->Result<AiSettingsView>`, `save_ai_settings(SaveAiSettings)->Result<AiSettingsView>`, `save_ai_key(String)->Result<AiSettingsView>`, `remove_ai_key()->Result<AiSettingsView>`, `accept_ai_consent(AcceptAiConsent)->Result<AiSettingsView>`, `request_follow_up(AiQuestionCommand)->Result<InterviewSnapshot>`, `waive_follow_up(WaiveFollowUp)->Result<InterviewSnapshot>`, `generate_feedback(AiSessionCommand)->Result<FeedbackView>`, `load_feedback(Id)->Result<FeedbackView>`, `retry_ai_publication(Id)->Result<InterviewSnapshot>`.

Tauri commands `interview_ai_*` correspond to the names above. `InterviewClient` uses camelCase names without a prefix; `saveAiKey` only receives the secret, and no response contains it. `requestFollowUp` and `generateFeedback` reserve/dispatch the job and return the state immediately; the conclusion arrives through the event `interview://snapshot`. History/FeedbackPanel reloads `loadFeedback` when the job of its session ends. Events do not replace persisted versions.

### Credentials, HTTP and fixed limits

`CredentialStore: Send+Sync` defines `get()->Result<Option<String>>`, `set(&str)->Result<()>`, `delete()->Result<()>`; access serialized and executed in spawn_blocking. Windows production: `keyring::Entry`, service `language-assistant-tutor`, user `opencode-api`; `NoEntry` is absence, the remaining errors are fixed messages without debug of the error. `cfg(not(windows))` returns explicit unavailability, never a production mock. Key in memory only during saving/calling, without a Debug derive on the types that carry it.

The normalized URL removes trailing slashes, requires HTTPS, host and absence of userinfo/query/fragment; it rejects a base ending in `/chat/completions`, guiding the user to enter the base. Consent bound to the complete canonical base, not only to the host. Any change of the base invalidates consent and removes the previous credential before allowing a key for the new destination; do not automatically forward the OpenCode key to another host. The operation fails safely if removing the key fails. Saving the model/tariff does not use the network.

Async transport `AiTransport::complete(TransportRequest)->Pin<Box<dyn Future<Output=Result<RemoteCompletion>>+Send>>`; the request contains the effective configuration, messages and transient secret; the response contains model, finishReason, content and usage. Tests inject implementations with barriers. Production `reqwest` with redirects disabled, retries explicitly disabled, total timeout 90 s and connection 10 s; without streaming, tools or authenticated catalog. Read the response in chunks up to 1 MiB, refusing excess even without Content-Length. Validate HTTP and envelope before the content; `finish_reason=length` is invalid. Do not expose the URL with parameters or the raw body in an error. Classify 401, 402/403, 429, 5xx, timeout/network, invalid envelope and limit.

Limits in UTF-8 bytes: follow-up question 2,000; confirmed answer 64,000 per source; question+criteria 16,000; serialized input body including prompts 160,000; HTTP output 1,048,576. `max_tokens`: 800 for the decision, 4,000 for the report. Excess generates a local error before sending, without truncating text. Limits are technical byte limits, not a claim of exact token capacity. Model/base defaults from the spec; reference tariff dated and editable, always identified as the reference of 2026-11-02 and disabled for another model.

### Model JSON and usage

Prompts `follow-up-v1`, `feedback-v1`, rubric `star-text-v1`, schema `ai-v3-1`. Ask only for one JSON object, without fences; use `serde(deny_unknown_fields)` in the structures. User material in a user message serialized as data; the system fixes the rules without tools. Do not depend on `response_format:json_schema`.

Decision: `{schemaVersion, decision:"ask"|"continue", reason, missingDimension:null|dimension, question:null|string}`. Dimensions: `relevance|situation|individual_responsibility|actions|result|learning`; `ask` requires a dimension and a non-empty question; `continue` requires both null. `reason` non-empty. Only one slot creation per main question, in a transaction.

Report: `{schemaVersion, content:[ContentObservation], strengths:[QuotedObservation], gaps:[GapObservation], english:{necessaryCorrections:[Correction], optionalStyle:[Correction]}, priorities:[Priority], fewerPrioritiesReason:null|string}`. Six unique `ContentObservation`, one per dimension, with `{id,dimension,rating:"clear"|"partial"|"not_identified",explanation,evidence:[Evidence]}`. `Evidence` is a tagged union `quote {confirmationId,role,quote}` or `absence {dimension,assessedConfirmationIds}`. `clear` requires a quote, `partial` requires a quote, `not_identified` requires an absence. `QuotedObservation {id, explanation,evidence:[quote]}`; `GapObservation` allows a quote or an absence. `Correction {id,explanation,evidence:quote,replacement}`. `Priority {observationId,action}` references an observation of the report itself. Unique IDs in the report, arrays up to 20 observations per section and three priorities; explanatory texts up to 4,000 bytes, replacements up to 8,000. Absences only for the provided dimensions and the list exactly equal to the received sources; a non-empty quote must be a literal substring of the correct source/role. `fewerPrioritiesReason` mandatory if fewer than three priorities. Reject an invalid complete report.

Normalize `usage.prompt_tokens` as total input including cache; `completion_tokens` as output; `prompt_tokens_details.cached_tokens` as a subset of the input. Absent ones remain null. Negative/non-integer token numbers, cache greater than the input or non-finite/negative costs become unknown with a warning; they do not annul valid content. `usage.cost`, when provided in USD, is the reported cost; preserve the metadata `opencode.billing_source` and `opencode.estimated` when present, without inferring final billing. Unknown envelope fields are not sent to the frontend nor stored in full.

Local estimate = `((input-cache)*inputRate + cache*cacheRate + output*outputRate)/1e6` when the necessary fields/rates exist; without a separate cache use normal input and mark conservative. If the cache exists and the cache tariff is missing, use normal for all the input and mark conservative. Without known input or output, the complete estimate is null. Do not extrapolate zero output. Use finite decimal values, present at least six decimal places for small costs; do not calculate with text rounded by the UI. Totals: `knownReportedSubtotal`, `knownEstimatedSubtotal`, `unknownAttemptCount`, partial tokens and completeness; never add reported and estimated from the same request as spent. Priority aggregation uses the order defined in the spec and preserves IDs/origins.

## 2. Execution tasks

Each task: write the indicated tests, run them and observe failure before implementation; implement, repeat until green and record commands/results. No automatic commits in this plan: the coordinator decides integration after review. The commands below assume Cargo in the PATH; when necessary use `$env:USERPROFILE\.cargo\bin\cargo.exe`, as in verify.ps1.

### Task 1 — Migration and shared audio targets

**Files:** create `src-tauri/src/db/migrations/002_ai_tutor.sql`; modify `db/{mod,sessions,answers,operations}.rs`, `interview/types.rs`, `service{.rs,/interview.rs}`, `artifacts.rs`, contracts/fake and TS consumers of the affected fields; create `src-tauri/tests/ai_migration.rs` and extend `tests/{answers,artifacts,service,sessions}.rs`.

**Interfaces:** produce AnswerSlot/currentAnswer/answerId and the DTOs of §1; `Database::load_answer_slot(&str)->Result<AnswerSlot>`; adapt reservation, selection, confirmation, listings and counts for the slot. Initial capture keeps the same paths and guards. In this task there is still no remote call.

- [x] Write `migrates_populated_v1_without_changing_sources_or_paths`, `migration_failure_rolls_back_schema_and_version`, `future_database_is_refused_before_write` and `fresh_database_reaches_v2`. The v1 fixture includes a completed session and another in progress, a confirmation, a manual review without transcript, a pending operation, a receipt and recording files; compare content/IDs after opening v2 and an empty `foreign_key_check`.
- [x] Write `follow_up_slot_has_own_recordings_and_confirmation`, `initial_count_ignores_follow_up` and `follow_up_cannot_select_initial_recording`; attempt to violate uniqueness and FK directly in SQLite.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml --test ai_migration` and the new audio tests; confirm failure due to a missing contract, not due to an error in the fixture.
- [x] Implement the migration and targets. Extract a current answer resolver used by `automatic_target`, capture/review/transcribe validation and frontend selection; do not duplicate the pipelines. `ArtifactStore::annotate` and reconciliation traverse initial and follow-up.
- [x] Run the complete Rust tests and `npm.cmd run typecheck`; pass regressions before the next task. Update existing fixtures where the contract changed without weakening assertions.

### Task 2 — Configuration and protected storage

**Files:** create `ai/{mod,types,credentials}.rs`, configuration in `db/ai.rs`; modify Cargo.toml/Cargo.lock, `lib.rs`, `service.rs`, `commands.rs`; create `src-tauri/tests/ai_settings.rs`.

**Interfaces:** consume Database and the types of §1; produce CredentialStore/WindowsCredentialStore and methods/commands of configuration/consent. New constructor `AppService::new_with_ai(..., Arc<dyn AiTransport>, Arc<dyn CredentialStore>)` preserves `new` for existing consumers, delegating to the production adapters; tests can use the new constructor.

- [x] Write tests for the key never reappearing in the DTOs/Debug/SQLite configuration, replace/remove, store error without fallback, invalid URL, old version, consent for a different destination and change of base without reusing the secret.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml --test ai_settings`; observe failure.
- [x] Implement configuration, receipts, validation and storage with I/O outside the UI thread. Serialize configuration changes and reading of the secret for the request snapshot; a request in progress keeps the old destination and transient key without contaminating the following ones.
- [x] Repeat the tests and `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`. Do not access the personal credential in tests; tests use an instance fake, not a global store.

### Task 3 — Prompts and validation of decisions/evidence

**Files:** create `ai/{validation,prompts}.rs`, fixtures `contracts/ai-v3.example.json`, `src-tauri/tests/ai_validation.rs`.

**Interfaces:** `build_messages(purpose: AiPurpose, sources: &AiSources)->Result<Vec<ChatMessage>>`; `validate_follow_up(raw:&str)->Result<FollowUpDecision>`; `validate_feedback(raw:&str,sources:&AiSources)->Result<QuestionReport>`; types as per §1. `AiSources` contains only the copied question/criteria and the identified source confirmations.

- [x] Test decision ask/continue, non-existent dimension, empty/long question, JSON with fences/truncated/extra fields and an attempt at a second question.
- [x] Test literal Unicode quote, changed punctuation, confirmation outside the request, wrong role, partial without quote, absence with incomplete sources, orphan/duplicate priority, missing dimension and overall score/extra acoustic field. Valid fixtures include a negative/qualitative result and learning without metrics.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml --test ai_validation` and confirm the expected failures.
- [x] Implement the prompts and validator; test a prompt with an answer containing instructions to ignore the rubric: it remains in the data message, without becoming system. Do not claim that this test proves semantic immunity.
- [ ] Repeat the tests; validate the Rust/TS fixture with the same fields and references.

### Task 4 — Limited HTTP and accounting

**Files:** create `ai/{transport,usage}.rs`, `src-tauri/tests/{ai_transport,ai_usage}.rs`; Cargo.toml adds reqwest and the necessary features verified in the installed version.

**Interfaces:** produce AiTransport/RemoteCompletion and `estimate_usage(&NormalizedUsage,&Tariff)->CostEstimate`; `aggregate_usage(&[AttemptUsage])->SessionUsage`. The configuration/limits/normalization in §1 are the authority.

- [x] Controlled local server in the tests: capture body/auth; respond with valid JSON, 401/402/403/429/500, redirect, timeout, false/absent Content-Length with excessive body, invalid JSON and finish_reason length. Loopback HTTP endpoint allowed only by the internal test constructor; public validation remains HTTPS.
- [x] Test absent/real zero tokens, explicit zero provider cost, cache, impossible cache, partial usage, missing price, model change and totals with a discarded/unknown attempt. `input=1000,cache=200,output=100` and tariff `0.30/0.006/1.20` → estimate USD `0.0003612`.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml --test ai_transport --test ai_usage`; observe failure.
- [x] Implement the client without retries/redirects; the body sent only contains model/messages/max_tokens/stream=false. Remove the raw bodies of errors and do not persist the secret. The transport has no access to SQLite, audio or professional files.
- [x] Repeat the tests, including a single request received by the server even on a 5xx response; confirm that the redirect does not receive Authorization on the second server.

### Task 5 — Persistent follow-up and asynchronous coordination

**Files:** create `service/ai.rs`, extend `db/ai.rs`, `service{.rs,/interview.rs}`, `commands.rs`, `lib.rs`; create `src-tauri/tests/ai_service.rs`.

**Interfaces:** produce request_follow_up/waive_follow_up/retry_ai_publication; the transactions `reserve_ai_attempt`, `finish_ai_attempt` and `waive_follow_up` validate source/generation/receipt. Attempt and result types private to the module can be defined according to the data of §1, without changing the public DTOs. A new `AiRuntime` in the service contains a single network slot and a map of results to publish; do not reuse the `model_job` of the Python helper.

- [x] Write tests with a suspended transport: non-existent confirmation/missing consent does not call the network; old version refused; clicks with the same request ID reserve only one attempt; a second job receives busy; the result creates a single slot; the answer to the follow-up never generates another; a local advance does not bypass the AI stage.
- [x] Test waiver during the network, advancing and recording the next one before releasing the transport; on release, usage is stored and the content discarded. Audio/consultation/navigation remain operational during HTTP.
- [x] Test restart with a running request → interrupted/possibly_sent without resending, a repeated interrupted request only by explicit action, closing the app during HTTP without depending on the timeout.
- [x] Inject a publication failure after receiving the answer: the state shows `pendingAiPublication`; the retry publishes the same content/usage and the transport continues with counter 1. Do not accept a new paid call while a publication is pending. After a waiver, republication only closes the attempt/accounts for it, without creating a follow-up.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml --test ai_service`; observe failure.
- [x] Implement the flow reserve under transitions → release → HTTP in a task → reacquire → validate/publish → snapshot. Mark possibly_sent before starting the transport; earlier configuration/key failures are not charged calls. If storing usage/result fails, keep the package in memory; only release the pending item after the commit. Uniqueness in SQLite is the last defense.
- [x] Repeat the ai_service test and the service/answers/artifacts regression. Update the capture/playback of the follow-up to pass the slot and not the confirmed main question.

### Task 6 — Granular feedback, history and recovery

**Files:** extend `db/ai.rs`, `service/ai.rs`, `ai/usage.rs`; create `src-tauri/tests/ai_feedback.rs`.

**Interfaces:** produce generate_feedback/load_feedback/FeedbackView. A batch reserves a receipt, goes through the main questions without a valid report and requests sequentially; it uses the same global network slot of task 5 during the batch. A failure ends the batch and keeps the earlier reports. A retry with a new request ID only goes through the missing ones; a replay of the same ID does not restart the batch. After a crash it does not resume the network automatically.

- [x] Write tests for a session with three questions: first valid report, second failure → partial and only two calls; retry receives the second and third → ready without reevaluating the first. Evaluation of a completed session does not block a new local/AI capture.
- [x] Test initial+follow-up sources, invalid report with known usage, immutability of the valid one, historical consultation during another session, generation refused before ending the interview and an SQLite error followed by publication without a new charge.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml --test ai_feedback`; observe failure.
- [x] Implement the state derived from sources/reports/attempts; ready only with all the valid reports. Build priorities locally by content gaps and then necessaryCorrections, preserving origin and a maximum of three. Do not add factual narration by algorithm.
- [x] Repeat the ai_feedback/ai_service/ai_usage tests; confirm that the totals include follow-up requests and all feedback attempts, including discarded ones.

### Task 7 — Integrated interface and real commands

**Files:** create `src/lib/ai-contracts.ts`, `src/features/ai/{AiSettings,FeedbackPanel}.tsx` and tests; extend `interview-{contracts,client}.ts`, fake, `App.tsx`, `SessionSetup.tsx`, `Interview.tsx`, `useInterview.ts`, `History.tsx` and the existing CSS. Register all commands in `src-tauri/src/lib.rs`.

**Interfaces:** consume the commands/DTOs of §1; no persistent transition or exclusive cost calculation in React. The answer resolver in useInterview selects currentAnswer and finds the respective confirmation/recordings. QuestionDetail continues to be used in the history with initial and follow-up separated.

- [x] Write tests: default local mode and explicit ai_v3; configuration masks/clears the key on success/error/unmount, a change of destination asks for a new consent; confirm does not call requestFollowUp; an explicit click sends; missing configuration offers saving/waiving with a reason.
- [x] Test the flow confirmed initial → request → follow-up → review/confirm → advance, without an extra counter; failure/retry/waiver and delayed result; changing page and closing during HTTP do not get stuck. Do not allow waiving a follow-up already created.
- [x] Test pending/partial/ready feedback and consultable sources in the history; unknown costs appear in full, the subtotal is not called a total; publication retry is distinct from repeating a paid request. Initial and follow-up originals play back by their IDs.
- [ ] Run `npm.cmd test`; observe the failures of the new scenarios.
- [x] Implement and integrate the existing screens, AI configuration in the navigation and FeedbackPanel in the summary/history. Reuse the existing style/focus/labels, without a generic redesign. The external service warning presents the complete destination before consenting and a retry error alerts to a possible new cost.
- [x] Run `npm.cmd run typecheck`, `npm.cmd test`, `npm.cmd run build`; repeat the Rust contract/command tests if the integration changed fields. The fake must simulate real states, not authorize operations that Rust refuses.

### Task 8 — Final verification and faithful documentation

**Files:** create `docs/phase-3/{setup,validation}.md`; update README, AGENTS.md, stack, phase plan, state of the spec and of this plan. Do not delete the pending items of the earlier phases.

**Interfaces:** consume the final implementation, real commands and outputs; produce configuration/execution instructions and the matrix F3-01…F3-16 with the automatic/manual distinction.

- [x] Run the complete `.\scripts\verify.ps1` and record the real counts/exit 0. If it fails, fix the cause and repeat the affected checks, then the aggregator before declaring the automation green.
- [x] Run `npm.cmd run tauri -- build --no-bundle` and the startup/shutdown of the binary as a desktop regression; record the effective commands of the local setup, without claiming a validated installed package.
- [x] Check the local links, `git diff --check` and the diff against the spec: each criterion has evidence or an explicit pending item. Verify the absence of the key/real answers in the new artifacts and that snapshots/errors do not expose the secret.
- [ ] Record a real test of Windows credentials and paid inference only if there is a credential/balance configured by the user and an authorized action with test text. Without that, mark it pending; do not request or read the key through the chat and do not do automatic paid inference for tests.
- [ ] Leave manually pending F3-16, semantic relevance/usefulness, wait measurements and effective security of the protected storage when not exercised. Document a script with three main questions with failure/retry/waiver/resumption, partial feedback and consultation of sources.
- [x] Deliver to the coordinator the list of files, deviations/decisions, commands/results and limitations. A phase implemented/automatically verified is not equivalent to accepted/completed.

## 3. Coverage and verified decisions

| Criteria | Tasks/planned evidence |
| --- | --- |
| F3-01 | 1, 7, 8: migration with data, local/legacy regression and desktop |
| F3-02 | 2, 7; real Windows exercise pending in 8 |
| F3-03 | 2, 3, 4, 5, 7: authorization/source/payload observed |
| F3-04, F3-05 | 1, 5, 7: slots/audio/limits in the core and UI |
| F3-06, F3-07, F3-08 | 5, 6, 7: barriers, receipts, restart and publication |
| F3-09, F3-10 | 3, 6, 7: literal validation, absence and presentation |
| F3-11 | 3 and 8: contract fixtures and human validation pending |
| F3-12 | 6, 7: partial report and granular retry |
| F3-13 | 4, 6, 7: normalization, cache, attempts and labels |
| F3-14, F3-15 | 2, 4, 5, 6, 7: controlled failures and UI without blocking |
| F3-16 | 8: human acceptance and real inference pending until execution |

Primary sources consulted on 2026-11-02: [reqwest ClientBuilder 0.13.5](https://docs.rs/reqwest/0.13.5/reqwest/struct.ClientBuilder.html), [keyring 3.6.3](https://docs.rs/keyring/3.6.3/keyring/), [OpenCode Inference API](https://opencode.ai/v2/docs/console/api/inference/). `keyring` 3.6.3 was chosen deliberately because it documents `windows-native` and Entry; line 4 changes the composition of stores. Confirm the exact reqwest features when resolving dependencies, without changing the requirements of TLS, limits and absence of retry. There was no paid inference when writing the plan.

Self-review: F3 coverage mapped; public names defined in §1 and used in the tasks; shared target resolver avoids forking the audio; publication failures and uncertain billing are tested; mode/local and human validations remain explicit. Internal details that do not change these contracts can be adjusted by the implementer and recorded in the report.

## Execution record

The eight tasks have implementation and automatic evidence in the [report](../../phase-3/implementation-report.md) and in the [validation](../../phase-3/validation.md). The test names/groupings were adapted (internal HTTP, ai_service aggregates coordination and feedback). The steps of observing failure before each implementation were not fully complied with: TDD was partial and those boxes remain unchecked. The new ai-v3 fixture documents sources/decision; the local v2 contract continues to be tested by exact Rust/JSON equality. Real credentials and paid inference were not exercised and the manual acceptance remains pending.

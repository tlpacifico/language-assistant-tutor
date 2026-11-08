# Phase 4 — Implementation plan: improve and repeat

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans or superpowers:subagent-driven-development only when the implementation is authorized. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** allow rewriting, listening to, repeating, evaluating and comparing answers, preserving facts and versions, with local preparation stories.

**Architecture:** individual practices belong to the fixed question of a completed session, but have their own owner and answer target. Rust/SQLite keep sources, variants, operations and snapshots; React presents the data and intentions. Reuse the current capture, Python helper, HTTP, consent and usage through explicit target resolvers.

**Stack:** the existing one — React/TypeScript, Rust/Tauri 2, rusqlite/SQLite, faster-whisper/Kokoro in the Python helper and DeepSeek Flash through the paid OpenCode API. No new dependency planned.

**Spec:** [Improve and repeat](../specs/2026-11-03-phase-4-improve-repeat-design.md), reviewed by the coordinator.

Date: 2026-11-03. Status: executed on 2026-11-03 with the user's authorization («termine as tarefas»); tasks 1 to 8 completed and automatically verified, with real evidence in [phase 4 validation](../../phase-4/validation.md). The checkboxes below remain as the original script, unmarked, because acceptance (F4-16 and paid inference) is pending. Historical text of the original plan: «só documentação autorizada; os comandos descritos eram futuros». Do not install, migrate data, read credentials/personal data or perform paid inference. The phase 3 existing in the checkout is preserved; manual acceptance of the previous phases remains pending. Do not start phase 5.

## Global constraints

- Never changes `position`, `question_count`, follow-up step, confirmation, selected recording or original report.
- Stories are exclusively local in this phase: no story field or identifier enters the API.
- One unconfirmed practice at a time; start practices only when there is no interview in progress.
- A confirmed practice never creates a follow-up, even if the model recommends one.
- B1 initially, with B1/B2/C1 selection in the rewrite; level is a language target, not a CEFR diagnosis.
- Do not generate feedback/rewrite automatically on confirming.
- One remote call at a time, without automatic paid retries.
- The network does not hold the SQLite Mutex or a global transition lock; queries and navigation remain possible.
- Valid suggestions are immutable; reuse a valid result for the same origin/level/versions.
- The recording starts and ends by user action, without automatic interruptions by duration.
- Do not truncate text, audio, story or JSON silently; distinguish unknown cost from zero.
- European Portuguese in the interface and explanations; questions, quotes, answers and rewrites in English.

## Review focus

1. The microphone starts, but writing the snapshot fails: continue the manual capture and preserve audio/candidate support; task 3 tests pending metadata and recovery.
2. An old command points to the original question while a practice is open: refuse cross association, without modifying originals; tasks 1–2 test owners and FK.
3. A B1 remote answer arrives after selecting B2 or opening another practice: store only the variant/target of origin without changing the selection in the UI; tasks 5 and 7.
4. A word that exists only in the story: must never appear in the HTTP body, automatic transcript, pre-filled editor or confirmation; tasks 3, 4 and 7.
5. Repeated network attempts appear in the practice and in the provenance session: distinct totals per ID, without double counting or a fictitious known total; task 6.

## 1. Real starting point and file structure

The current code has AI types in `src-tauri/src/ai/mod.rs`, validated JSON in `ai/validation.rs`, transport in `ai/transport.rs`, transactions in `db/ai.rs` and coordination in `service/ai.rs`. Do not assume the file names foreseen in an old plan if they do not exist in the checkout.

`answer_slots` v2 contains `session_question_id`, `session_id`, role `initial|follow_up` and selection. The historical column `recordings.session_question_id` **already references the slot ID**, as does the owner of `confirmations` and `operations`; do not rename it globally as part of this phase. `QuestionDetail::active()` presents initial/follow-up; do not create a fictitious `QuestionDetail` for practices. `ai_requests` is unique per purpose/question, `ai_reports` per question. These are the points that require migration.

Create focused modules `src-tauri/src/practice.rs`, `db/{practice,stories,rewrites}.rs`, `service/practice.rs` and `ai/rewrite.rs`. Adapt `ai/{mod,validation,transport}.rs`, `db/ai.rs`, `interview/types.rs`, `service{.rs,/interview.rs,/ai.rs}`, `artifacts.rs` and the Tauri commands/registry. Extract only the shared answer/capture helpers currently dependent on the active interview; do not build a second audio service.

Frontend: create `src/lib/practice-contracts.ts`, `src/features/practice/{Practice,RewritePanel,Comparison}.tsx` and `src/features/stories/{StoryBank,StorySupport}.tsx`, with nearby tests. Extend the single client/fake, `App.tsx`, history, interview, the review hook and the existing CSS. `practice.css` already exists: extend it, do not replace the application style.

## 2. Contracts and build decisions

### 2.1 Identities, states and targets

New types in `practice.rs`, with UUID IDs, u64 versions, RFC3339 UTC dates and camelCase JSON/snake_case enums:

```text
ResponseOrigin = {type:"original", sessionId, sessionQuestionId}
               | {type:"practice", practiceId}
CaptureOwner = {type:"interview", sessionId, sessionQuestionId, answerId}
             | {type:"practice", practiceId, answerId}
PracticeStatus = preparing | answering | confirmed | abandoned
RewriteLevel = B1 | B2 | C1
CreatePractice = {requestId, sessionId, sessionQuestionId, previousPracticeId:null|Id}
PracticeCommand = {requestId, practiceId, expectedVersion}
PracticeDetail = {id, sourceSessionId, sessionQuestionId, ordinal,
  previousPracticeId, status, version, createdAt, confirmedAt, abandonedAt,
  question:{text,category,competencies,criteria}, answer:AnswerSlot,
  recordings:RecordingView[], confirmation:Confirmation|null, preparation:PreparationView}
PracticeAnswerTarget = {owner:CaptureOwner, recordingId, selectionVersion}
ExerciseSnapshot = {revision, activePractice:PracticeDetail|null,
  activeOwner:CaptureOwner|null, pendingPreparation:Id|null}
RewriteCommand = {requestId, origin:ResponseOrigin, level:RewriteLevel}
EvaluatePractice = {requestId, practiceId, expectedVersion}
PracticeReportView = {practiceId,state:pending|running|failed|ready,
  report:null|JSON,rubricVersion,promptVersion,usage,networkAttempts}
ComparisonRequest = {left:ResponseOrigin,right:ResponseOrigin}
ComparisonView = {left:ComparisonEntry,right:ComparisonEntry,rubricCompatibility:same|different|unknown}
StoryView = {id,version,title,situation,task,actions,outcome,competencies,
  missingFields:(situation|task|actions|outcome)[],createdAt,updatedAt}
PreparationView = {answerId,version,lockState:editable|pending_start|locked|start_uncertain,
  selectedStoryId:null|Id,selectedStoryVersion:null|Version,
  snapshot:null|{storyId,storyVersion,content:StoryInput},
  support:none|story|historical_none,lockedCaptureId:null|Id}
RewritePreference = {version,level:RewriteLevel,updatedAt}
ReportVersions = {rubricVersion,promptVersion,schemaVersion}
NetworkAttemptView = {networkAttemptId,requestId,purpose,origin:ResponseOrigin,
  state:running|completed|failed|interrupted|discarded,generation,
  usage:NormalizedUsage,createdAt,finishedAt:null|String}
ComparisonEntry = {origin:ResponseOrigin,createdAt,confirmedAt,
  question:{text,category,competencies,criteria},
  answers:{role:initial|follow_up|attempt,recordings:RecordingView[],confirmation:Confirmation}[],
  report:null|{content:JSON,versions:ReportVersions},
  reportState:pending|running|failed|ready|not_available,
  rewrites:RewriteView[],preparation:PreparationView}
UsageGroup = {totals:SessionUsage,networkAttemptCount:u64,
  byPurpose:{purpose,usage:SessionUsage,networkAttemptCount:u64}[]}
TrainingUsageView = {sessionId,originalInterview:UsageGroup,laterPractice:UsageGroup,combined:UsageGroup}
```

`ResponseOrigin::original` covers the initial confirmation and the confirmed follow-up. `practice` covers only its confirmation with role `attempt`. `CaptureOwner` distinguishes a single answer: initial/follow-up identified by slot in the interview case. `practiceId` is always a practice attempt; `networkAttemptId` identifies the HTTP call. Do not expose ambiguous `attemptId` fields in the new contracts.

`ComparisonEntry` contains origin, dates, question, answers separated by role, recordings/automatic/confirmations, report with versions or pending, and suggestions in their own area. The core requires the same session question and distinct confirmed origins; `original` origins are from a completed session. Sort by creation, regardless of the requested order, and do not create analytical text about improvement.

Add `practice: ExerciseSnapshot` to the `InterviewSnapshot` photograph, keeping the interview fields. Never place a practice in `active_session` or change `current` to simulate an interview question. The single capture/helper/playback state remains shared. `AnswerSlot` gains `practiceId:null|Id` and role `attempt`; `sessionQuestionId` remains the provenance main question. The historical `session_id` field of the slot is the **provenance** session, not a new active session.

Internal resolver `Database::resolve_capture_owner(&CaptureOwner)->Result<ResolvedAnswer>` produces `ResolvedAnswer {owner:CaptureOwner,slot:AnswerSlot,sourceSessionId:Id,sessionQuestionId:Id,question:{text,category,competencies,criteria},confirmation:Option<Confirmation>,ownerVersion:Version,writable:bool,artifactParent:{sourceSessionId,answerId}}`. The selection is in the slot; `ownerVersion` is the version of the session or practice according to the owner. It contains no stories and no credentials. `AppService` uses it in capture, transcription, review saving, confirmation and delayed events. For old interview requests, adapt `AnswerTarget` to the interview owner and refuse `attempt` slots; for practices use an explicit owner. Do not accept a practice just because the slot ID belongs to the same historical session.

### 2.2 SQLite schema v3

Create `003_improve_repeat.sql`; keep the v1/v2 migrations. On opening, support empty→v1→v2→v3, v1→v2→v3 and v2→v3 in one transaction per opening; a future version is refused before any write. Disable FK only on the exclusive connection before the transaction if reconstruction requires it, re-enable it on all paths, check `foreign_key_check` before the commit. Recreate triggers/indexes without bypassing product immutability; rollback keeps user_version and data. Do not use `writable_schema`.

**Practices:** `practices(id PK, session_question_id FK, source_session_id FK, ordinal>=1, previous_practice_id FK nullable, answer_id UNIQUE FK deferred, status CHECK, version>=1, created_at, confirmed_at, abandoned_at)`. UNIQUE `(session_question_id,ordinal)`, unique constant partial index for the `preparing|answering` states. CHECK for date/state coherence. Trigger requires the question to belong to a completed provenance session and an existing initial confirmation; the predecessor, if present, confirmed and from the same question. Identity/provenance/ordinal/predecessor immutable. Abandonment only from a pending state, without active recording/finalization/publication pending; keeps all data and prevents further changes/evaluation.

**Exclusion between practices:** triggers on INSERT of practices and INSERT/UPDATE of sessions prevent having a pending practice and an `in_progress` interview simultaneously. Also verify in the service under transitions. A confirmed/abandoned practice does not block another one; pending AI requests are not an active practice. The original session counter/selections do not change when creating/finishing a practice.

**Slots:** rebuild `answer_slots` widening the role to `initial|follow_up|attempt` and adding `practice_id` FK deferred. Remove the global UNIQUE `(session_question_id,role)` and create a unique partial index for `initial|follow_up`, plus a partial UNIQUE per `practice_id` for `attempt`. CHECK `(role='attempt')=(practice_id IS NOT NULL)`. Context trigger requires coherent session/question and a practice linked to the same question; composite FK to prevent swapping answer_id. Old slots copy IDs/texts/selections, `practice_id=null`; the sync_initial_selection trigger acts only on `initial`. Confirmation, revision and blocked selection triggers continue to cover all slots.

**Stories:** `stories(id PK,version>=1,title,situation,task,actions,outcome,competencies_json,created_at,updated_at)` with validation in Rust and a title CHECK. `story_versions(story_id,version,content_json,created_at)` composite PK, append-only. Changing the bank does an UPDATE with the expected version plus an INSERT of the version in the same transaction; no DELETE. `preparations(answer_id PK FK,version,selected_story_id nullable,selected_story_version nullable,locked_capture_id nullable FK recordings,lock_state:editable|pending_start|locked|start_uncertain)`. Follow-up support resolves the preparation of the initial slot, never creates its own association. `preparation_captures(recording_id PK FK,answer_id,preparation_version,snapshot_json nullable,state:candidate|started|failed|uncertain,created_at)` keeps the fixed copy of the version; a marker for the absence of a story distinguishes «sem apoio» from a lack of persistence. The snapshot never contains data obtained from the story later. For v2 targets with audio already published or a confirmation, initialize the preparation as a blocked historical absence, without inventing a story snapshot; only targets still without capture evidence may choose new support. Interrupted v2 reservations without an original are indicated as historical without sufficient evidence, kept and handled by reconciliation before allowing a new selection.

**AI:** rebuild `ai_requests` preserving old IDs/sources/versions; add `practice_id nullable`, `origin_key`, `variant_key` and `generation>=1`; widen purpose to `follow_up|feedback|practice_feedback|rewrite`. `question_id/session_id` remain provenance. The `origin_key` CHECK corresponds to `original:<question_id>` or `practice:<practice_id>`; a practice requires a composite FK with question/session. `variant_key` is `default` for evaluations/follow-ups; for rewrite it is level+prompt+rubric+schema. UNIQUE `(purpose,origin_key,variant_key)`. Old requests gain origin original and default, without rewriting sources_json or versions. Trigger prevents changing sources/origin/variant/versions after the reservation; only the retry generation may increase. `ai_attempts` keeps format/IDs/usage; add a snapshot of origin metadata if necessary for queries, never story data.

Keep `ai_reports` original and its triggers intact. Create `practice_reports(practice_id PK FK, network_attempt_id UNIQUE FK ai_attempts, rubric_version, prompt_version, schema_version, json)` append-only. `evaluation_families(session_question_id PK FK,rubric_version,schema_version,created_at)` fixes the rubric of the family; initialize it from the existing ai_reports by joining the request, without changing the report. When there is no original report, the first practice evaluation fixes the current rubric `star-text-v1`; a future original evaluation of the family uses a supported compatible version. If an old version is not executable, refuse a new evaluation with an explanation and allow consultation; never replace old reports. Comparison of imported data/different versions shows the limitation.

`rewrites(id PK, request_id UNIQUE FK ai_requests, network_attempt_id UNIQUE FK ai_attempts, origin_key, level CHECK B1|B2|C1, json, spoken_text, created_at)` append-only; FK/origin/level must match the request. `rewrite_audio(rewrite_id FK,config_hash,relative_file,text_hash,voice,speed,state:pending|ready|failed, PRIMARY KEY(rewrite_id,config_hash))`; derivable metadata may be updated, valid text may not. `practice_preferences` singleton with `rewrite_level` default B1, version and date; separate from configuration/AI consent.

New receipts in the existing `ai_receipts` (kind/hash/entity) for commands without network and for mutations; do not widen the command_receipts CHECK unless necessary. Concrete decision: resolver `lookup_command_identity(requestId)` queries `ai_attempts.command_id`, `ai_receipts.id` and `command_receipts.request_id` inside the transaction before every new reservation/mutation; more than one historical occurrence is refused as a conflict, not chosen arbitrarily. The same table/kind/hash returns the original effect; an occurrence in another table or with different content refuses reuse. The old writers go through the same preflight under an SQLite transaction, avoiding races between families without changing old IDs. Changing level/preference sends nothing.

### 2.3 Stories and first capture

`StoryInput {title,situation,task,actions,outcome,competencies}`: outer trim on all fields/competencies, title 1…200 bytes, each note up to 16,000 bytes, sum of notes up to 48,000, 1…20 competencies of up to 120 bytes. Require at least one non-empty note and one competency; deduplicate exact competencies after trim, preserving the interior. Do not require an outcome or full STAR. Completeness is a label derived from the missing fields, not a quality evaluation. Simple local filter by normalized competency with trim and case-insensitive comparison.

`SelectStory {requestId,owner:CaptureOwner,expectedPreparationVersion,storyId:null|Id,expectedStoryVersion:null|Version}` only allows the current initial of an interview or a pending practice before a successful first capture. Confirm the story version at commit; an old client does not silently select different content. The snapshot contains a copy of StoryInput and IDs/version exclusively local.

Before opening the microphone, a transaction reserves the recording and an immutable candidate for snapshot/absence (`pending_start`). If this transaction fails, do not open the microphone. When `AudioPort::start_capture` succeeds, immediately keep owner/recording/candidate in the in-memory state and block editing of the support; mark started/locked in SQLite. If this write fails, **do not stop the capture**: the snapshot exposes `pendingPreparation`, the metadata retry is local, finishing manually remains accessible and publication saves audio before depending on the snapshot. Abandonment/new recording/successful shutdown wait to resolve the pending item; a shutdown requested by the user may finalize audio, but must not indicate success while there is a pending write.

For recovery of the window between audio and SQLite, create in the already reserved folder `preparation-start.json` with recordingId, snapshot/hash and start result (`pending|started|failed`), by atomic write. The durable candidate exists before the hardware. Success fixes the started marker and the DB and moves the practice from preparing to answering; a known failure fixes failed and frees the preparation, even if a write retry is necessary. A marker failure after success leaves the candidate/state in memory and the pending item visible; never delete a folder with a finalized original.

On startup, started or a finalized original.wav allows consolidating the candidate snapshot; failed does not fix support and keeps the interrupted recording for consultation. Pending without sufficient evidence means `start_uncertain`: keep the candidate, show the interruption/uncertainty and require an explicit local resolution before editing support/recording again. «Manter este apoio e continuar» fixes the candidate; «O microfone não chegou a iniciar» records the user's decision and frees the selection, keeping the audit. Do not pretend there is atomicity between device, file and SQLite. Never infer new notes from the story edited during recovery. Even without a story, persist the fixed absence to prevent retrospective selection.

### 2.4 Rewrite, evaluation and limits

`Database::load_exercise_sources(&ResponseOrigin)->Result<ExerciseSources>` resolves the fixed question/criteria and confirmations; original requires a completed session and an initial confirmation; practice requires status confirmed. `ExerciseSources` contains `origin`, `AiSources` and versions/rubric, **without Preparation/Story**. Transport DTOs serialize only `{question,criteria,answers}`; origin identity/story IDs do not enter the message. Do not call `ai_sources(...,"feedback")` for a local original: create a separate exercise sources resolver, preserving the local restriction in the interview feedback.

Extend `TransportRequest` with a typed `task: AiTask` (`FollowUp`, `OriginalFeedback`, `PracticeFeedback{rubricVersion}`, `Rewrite{level,rubricVersion}`) instead of treating every non-follow_up purpose as feedback. A single build_body function chooses prompts/maximum output. Adapt the original calls without changing the effective bodies of phase 3; keep regression tests. Only role `attempt` in a practice evaluation/rewrite, without an old initial or an old follow-up. The current validator compares role with sources; make the authorized set of roles and the practice prompt explicit, do not replace attempt with initial.

Rewrite prompt `rewrite-v1`, schema `rewrite-v1`, factual rubric `factual-rewrite-v1`; output:

```text
{schemaVersion:"rewrite-v1", level:"B1"|"B2"|"C1",
 segments:[{id,text,evidence:[{confirmationId,role,quote}]}],
 necessaryChanges:[{source:{confirmationId,role,quote},segmentIds:[Id],explanation}],
 optionalStyle:[{source:{confirmationId,role,quote},segmentIds:[Id],explanation}],
 missingInformation:[{dimension,assessedConfirmationIds,preparationQuestion,explanation}]}
```

Require 1…40 segments, unique IDs, non-empty text up to 2,000 bytes per segment, maximum 12,000 bytes of total text joined by `\n\n`. The voice text is built **only** from that join; the model does not supply a second divergent voice field. Each segment has 1…10 non-empty literal quotes from authorized sources (up to 64,000 bytes per quote). Necessary changes/style up to 20 per list, literal source and 1…40 existing unique segmentIds, PT explanation up to 4,000 bytes. Gaps up to six unique STAR dimensions, IDs of all assessed sources, English preparation question up to 500 bytes and PT explanation up to 4,000. JSON without extra fields/fences, level equal to the request; reject invalid references/segments without evidence or a completely truncated result. No placeholder/explanation is introduced by the program into the voice text; the prompt forbids markers and new facts. Literal validation does not guarantee the fidelity of the paraphrase nor the semantic absence of placeholders; present the limitation and test manually.

Practice evaluation prompt `practice-feedback-v1`, existing schema `ai-v3-1`, rubric `star-text-v1` when compatible with the family. Same JSON/limits/quotes/absences/classifications/correction versus style/priorities of phase 3, with role `attempt` and only one confirmation; do not include a follow-up field or grade. Record the rubric and the source in each own report.

Existing limits preserved: 64,000 UTF-8 bytes per confirmation, 16,000 for question+criteria, 160,000 serialized body, 1 MiB HTTP response, 90 s timeout/10 s connection, no redirects/retries. Rewrite `max_tokens=4.000`; practice evaluation 4.000. A result that does not fit is a recoverable failure, not partial JSON. Preferences do not change the limits of texts previously stored; an excess only prevents the operation that cannot process it, with a message without loss.

Audio has no stop timer: keep the capture capability and transcription deadlines proportional to the duration already existing in the worker. Do not introduce a warning timer or a new duration limit in this phase; «Terminar gravação» remains by user action. Do not trim audio or promise to recover a recording not yet finalized. A transcription failure preserves the original and allows retry/manual review as currently. The rewrite synthesis text is limited to the already validated 12,000 bytes; local synthesis costs no API.

### 2.5 Audio, concurrency, recovery and costs

Reuse the artifact root `sessions/<sourceSessionId>/<answerSlotId>/<recordingId>/`: the practice uses an exclusive slot and the session only as provenance, without a fictitious active session row. Check the owner in the core on all paths. `PendingRecording`/reconciliation gain a discriminated owner so that practices are not lost while looking for the active session. Existing local operations may use the provenance session_id and the slot as currently; the result applies to its owner and generation, not to the presented practice.

Add `AudioSelection::Rewrite{rewriteId}` and `PracticeQuestion{practiceId}`. Rewrite cache in `rewrite-cache/<rewriteId>/<sha256(texto+voz+velocidade+versão)>.wav`, resolved in the core, with metadata in rewrite_audio. Text saved before synthesizing; the absence of the derivative reopens only local work. Capture interrupts playback; a single local transcription/synthesis operation; a late publication does not start playback if a capture started in the meantime. The original question cache is not moved. Reconciliation validates paths, missing/incomplete file and keeps a warning without deleting data; a failed synthesis keeps the text.

The current `ReservedAttempt` evolves to store origin/purpose/variant/generation; new DTO names say networkAttemptId. The reservation validates an existing valid result before occupying a remote slot: the same variant returns the existing one without charge. A new requestId after a failure creates a new call linked to the logical request; replay of the same id is a no-op. A single `AiRuntime` for busy/pending continues to coordinate all purposes; a pending publication blocks new network until resolved, but not audio/consultation. finish_ai dispatches by enum, compares origin/sources/generation/variant, records usage and only then applies the result to the correct target. A new open practice does not invalidate old immutable sources nor makes a late result change the UI level selection.

Prepare shutdown: finalize the capture by explicit request, save the review according to the existing guard and try the pending local publication. If the write pending item was not resolved, do not declare a safe shutdown; do not wait 90 s for the network, mark the operation interrupted/uncertain and invalidate the in-memory generation. On restart do not resend network or restart the capture automatically; a remote result already received but lost after a crash remains uncertain. Never clear the pending package before the local commit.

`load_training_usage(sessionId)->TrainingUsageView` returns `originalInterview`, `laterPractice`, `combined` (each with SessionUsage and counts per purpose); selection by DISTINCT ai_attempts.id. Original = existing follow_up/feedback; later = rewrite (including the original one) and practice_feedback. `load_practice`/report show the subset of the practice, without adding it again to combined. Derive usage without UPDATE in the original report. Reported/estimated costs remain separate, tariffs copied per call, invalid/discarded results included, unknowns/partial visible. Do not renew prices as part of this phase.

### 2.6 Public methods and UI integration

New async `AppService` methods return `Result<T,AppError>`; the respective commands `interview_<nome_snake_case>` and InterviewClient camelCase:

```text
list_stories(competency:Option<String>)->Vec<StoryView>
create_story(requestId:Id,input:StoryInput)->StoryView
update_story(requestId:Id,id:Id,expectedVersion:Version,input:StoryInput)->StoryView
select_story(SelectStory)->InterviewSnapshot
resolve_preparation(recordingId:Id,requestId:Id,decision:keep|did_not_start)->InterviewSnapshot
retry_preparation_publication(recordingId:Id)->InterviewSnapshot
create_practice(CreatePractice)->PracticeDetail
load_practice(practiceId:Id)->PracticeDetail
list_practices(sessionQuestionId:Id)->Vec<PracticeDetail>
abandon_practice(PracticeCommand)->PracticeDetail
start_practice_recording(owner:CaptureOwner,expectedSelection:Version)->InterviewSnapshot
save_practice_review(requestId:Id,target:PracticeAnswerTarget,expectedReviewVersion:Version,
  sourceTranscriptId:Option<Id>,origin:ReviewOrigin,text:String)->InterviewSnapshot
confirm_practice_answer(requestId:Id,target:PracticeAnswerTarget,expectedReviewVersion:Version,
  expectedPracticeVersion:Version,sourceTranscriptId:Option<Id>,origin:ReviewOrigin,text:String)->PracticeDetail
transcribe_practice_response(target:PracticeAnswerTarget)->InterviewSnapshot
evaluate_practice(EvaluatePractice)->PracticeReportView
load_practice_report(practiceId:Id)->PracticeReportView
request_rewrite(RewriteCommand)->RewriteView
list_rewrites(origin:ResponseOrigin)->Vec<RewriteView>
compare_responses(ComparisonRequest)->ComparisonView
set_rewrite_level(requestId:Id,expectedVersion:Version,level:RewriteLevel)->RewritePreference
load_training_usage(sessionId:Id)->TrainingUsageView
```

`RewriteView {id:null|Id,origin:ResponseOrigin,level:RewriteLevel,state:pending|running|failed|ready,segments,spokenText:null|String,necessaryChanges,optionalStyle,missingInformation,versions:ReportVersions,usage:SessionUsage,networkAttempts:NetworkAttemptView[]}`; content lists use the contract of §2.4 and stay empty until ready, which always has an id and validated content. `PracticeReportView` adds `schemaVersion`, uses `networkAttempts:NetworkAttemptView[]` and `usage:SessionUsage`; `report` has validated phase 3 JSON with attempt sources. Reuse stopRecording/retrySaveRecording/playAudio/stopAudio/retryAiPublication/prepareClose, which now resolve the owner internally. A new practice query does not retarget the capture/work. Create/confirm/abandon are idempotent and use versions. No page receives a secret; new requests go through the same existing configuration/consent guards.

UI: story bank in the navigation; optional support before recording and read-only after lock; practice in the history without an artificial interview; shared form/review with a save/discard/cancel guard. Abandonment has clear text about preservation and future reading. RewritePanel shows B1/B2/C1, valid variants, necessary changes/style and gaps outside the voice; «Sugestão da IA — confirme os factos». Comparison distinguishes initial/follow_up/attempt, audio and automatic/confirmed, suggestion in a separate area; a pending report/a different rubric does not look like proven improvement. Costs show later practice separately. During the network you may navigate, record when eligible and resume without repeating the call.

## 3. Future executable tasks

Each task follows red→green: create named tests with the values/contracts of §2, run and confirm failure through the missing behavior, implement only what is necessary, repeat the checks. The commands below are **future instructions**, not execution evidence. Cargo may require `$env:USERPROFILE\.cargo\bin\cargo.exe`, according to the existing setup. Do not create commits, publish or integrate without an instruction applicable to the future execution.

### Task 1 — v3 migration and ownership of answers

**Files:** create `db/migrations/003_improve_repeat.sql`, `practice.rs`, `db/practice.rs` and `src-tauri/tests/phase4_migration.rs`; change `db/mod.rs`, `interview/types.rs`, `db/{answers,sessions,operations,ai}.rs`.

**Interfaces:** produce the §2.1 types, the §2.2 tables and `resolve_capture_owner`. Initially expose practice queries without network; original/follow_up keep the previous contracts. Preserve the existing CHECKs, triggers, paths and data content.

- [ ] Write `migrates_v0_v1_v2_preserving_all_source_ids`, `migration_rollback_restores_v2`, `future_version_is_refused_without_write`: temporary v2 fixture with a local/AI session, follow-up, revisions/confirmations, report, failed attempts and receipts; compare values and empty foreign_key_check.
- [ ] Write `slot_owner_cannot_cross_practice_or_source_question`, `multiple_attempt_slots_keep_unique_initial_and_follow_up` and `pending_practice_and_active_interview_are_mutually_exclusive`, including direct SQL to try to violate constraints.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml --test phase4_migration`; observe the expected failures; implement the migration/resolver and repeat until zero failures.
- [ ] Run the existing Rust migration/sessions/answers/AI tests to prove there is no regression; update fixtures only when new fields require it, without weakening old assertions.

### Task 2 — Complete local practice cycle

**Files:** create `service/practice.rs`, `src-tauri/tests/practice_service.rs`; adapt `service{.rs,/interview.rs}`, `db/answers.rs`, `artifacts.rs`, commands/lib and `practice.rs`.

**Interfaces:** consume the owner/resolver of task 1; produce create/load/list/abandon, review/confirmation and shared capture/transcription adaptation. Practice state independent of the AI work. Capture/worker/playback remain single.

- [ ] Write tests for idempotent creation from completed local_v2/ai_v3; refuse the current session, JSON practice, origin without confirmation, predecessor from another question and a second pending practice.
- [ ] Test that re-recording keeps takes and ordinal; exact immutable confirmation; transcription failure and retry use the same file; publication failure keeps the previous recording/final audio; abandonment keeps revision/recordings and forbids reopening/evaluation.
- [ ] Test that an old command for the original does not capture in the practice slot; a late event applies only to the owner; a confirmed practice frees a new practice; restart resumes review and state without opening the microphone. A prolonged capture on a controlled clock does not call stop_capture; stopping remains exclusively manual and the proportional worker deadlines keep the existing behavior.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml --test practice_service`; confirm red, implement the shared helpers and repeat green. Repeat the existing service/artifacts tests and check that the original totals/selections/reports are unchanged.

### Task 3 — Local stories and snapshot at capture start

**Files:** create `db/stories.rs`, `src-tauri/tests/stories.rs`; extend `service/practice.rs`, `service/interview.rs`, `artifacts.rs`, commands and types.

**Interfaces:** produce list/create/update/select/resolve/retryPreparation of §2.3/§2.6. The AI sources resolver never receives StoryInput/PreparationView.

- [ ] Test outer trim, a valid partial story, the exact limit/excess, concurrent version, filter and append-only; no deletion commands.
- [ ] Test the snapshot of the chosen version on the first successful capture; editing the story afterwards does not change the snapshot; removing support before the capture fixes the absence; the follow-up shares the main one; a known microphone failure allows choosing another support.
- [ ] Inject an SQLite failure before start: no microphone opened. Failure after start: the capture remains active, Terminar works and audio is preserved; the local retry consolidates the same snapshot without querying the current version. Restart with a started marker/finalized original consolidates; ambiguous pending shows start_uncertain and resolves explicitly without discarding files.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml --test stories`; confirm failure, implement and repeat green. Test that the exclusive marker `ONLY_IN_STORY_742` never pre-fills review/confirmation/transcript and does not enter the sources resolver.

### Task 4 — AI contracts for rewrite and isolated evaluation

**Files:** create `ai/rewrite.rs`, `src-tauri/tests/rewrite_validation.rs` and the fixture `contracts/practice-v4.example.json`; adapt `ai/{mod,validation,transport}.rs` and the sources in `db/rewrites.rs`.

**Interfaces:** `load_exercise_sources`, `AiTask`, `validate_rewrite(raw:&str,sources:&AiSources,level:RewriteLevel)->Result<ValidatedRewrite>`, `build_rewrite_messages(&AiSources,RewriteLevel)->Result<Vec<Value>>`. Reuse validate_feedback for the attempt role with an explicit prompt contract, original limits intact.

- [ ] Write tests for correct Unicode segments/quotes, wrong role/source, orphan references, different level, extra/truncated JSON, segment without evidence, excess bytes and absence with incomplete sources. Test that spokenText is exactly equal to the join of the segments, without explanations/gaps attached.
- [ ] Capture the body of a local/AI original with a follow-up versus a practice: the practice contains only the attempt confirmation, never an old answer/report/rewrite/story. Marker `ONLY_IN_STORY_742` absent in all purposes, including old follow_up and feedback.
- [ ] Fixtures with team versus individual, negation, uncertainty, number and a qualitative/negative result exercise the contract and the prompt instructions; do not claim that a literal test guarantees semantic fidelity. An answer with malicious instructions remains serialized as data.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml --test rewrite_validation`; confirm red, implement and repeat green. build_body/feedback/HTTP regressions demonstrate preserved old bodies/limits and the absence of a paid retry.

### Task 5 — Persistent requests and immutable results

**Files:** create `db/rewrites.rs`, `src-tauri/tests/practice_ai.rs`; adapt `db/ai.rs`, `service/ai.rs`, `service/practice.rs`, commands and types.

**Interfaces:** produce request/listRewrite and evaluate/loadPracticeReport, B1 preferences and integration of purposes/variants in ReservedAttempt. Reuse AiRuntime, consent and publication pending; do not create a second remote gate.

- [ ] Test that the same origin/level/versions returns a ready variant without network; changing B1→B2 creates a new variant; changing the preference does not alter previous ones. Idempotent replay, a different requestId after an error creates only a new networkAttempt; a collision of IDs/kinds is refused.
- [ ] Test that evaluation requires a confirmed practice and only role attempt, does not create a follow-up, keeps the original; a valid report is not regenerated. The family rubric is born from the existing original or star-text-v1 in the first evaluation and stays fixed; an incompatible version produces an explicit error without replacing history.
- [ ] Use a suspended transport for a B1 result after the UI selects B2/opens another practice: it only stores the B1 of origin. A new simultaneous operation receives busy; navigation/eligible audio continue. A publication failure followed by a retry keeps the network counter at 1 and preserves usage; closing/restarting does not resend automatically.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml --test practice_ai`; confirm red, implement the transactions and purpose dispatch, repeat green and the existing `--test ai_service`, which includes feedback scenarios.

### Task 6 — Suggestion voice, comparison and usage

**Files:** extend `artifacts.rs`, `service/practice.rs`, `service/interview.rs`, `db/rewrites.rs`, `ai/usage.rs`; create `src-tauri/tests/practice_outputs.rs`.

**Interfaces:** produce AudioSelection Rewrite/PracticeQuestion, ComparisonView and TrainingUsageView. Validated text is the single source for the voice; the cache/index resolves the ID without external paths.

- [ ] Test that Kokoro receives only spokenText; level/text/voice change the cache; a missing file or synthesis failure repeats only the helper and not HTTP; capture interrupts playback and a late synthesis result does not play over the microphone.
- [ ] Test comparison of original+practice or two practices of the same main question; refuse draft/abandoned/another question. Preserve initial/follow-up separated, automatic/confirmed audio and report/versions; indicate different versions and a missing report without creating an evaluation.
- [ ] Test costs with a failed retry, a discarded result, an original rewrite and a practice one: originalInterview/laterPractice/combined by disjoint ID sets and combined counts each network once. Null usage remains incomplete; local audio adds no API cost.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml --test practice_outputs`; confirm red, implement and repeat green, including reconciliation of a missing derivative and an invalid path.

### Task 7 — Complete interface for later practice

**Files:** create the frontend contracts/components of §1 and tests; adapt the client/fake, App, History, Interview, useInterview and CSS; register the effective Tauri commands.

**Interfaces:** consume only the DTOs/commands of §2.6, without persistent rules or prompt composition in the frontend. The draft resolver receives the owner and keeps a single editable revision per recording.

- [ ] Write tests for trying again without feedback/rewrite; blocking by interview/pending practice; resume/abandonment preserves data; save/discard/cancel before navigation/shutdown; capture/practice without a fictitious session.
- [ ] Test a partial story, selection/substitution/removal before capture, lock/snapshot after success and a metadata error without hiding Terminar; editing the bank does not change the saved support nor fill the answer.
- [ ] Test default level B1 and variants, requesting explicitly, listening only to the suggestion, attempt evaluation, publication retry separate from network, comparison with sources/follow-up/suggestion and a limited rubric; later costs separated and unknowns labeled.
- [ ] Test that a remote answer outside the page and for another level does not replace the selection; loading history/comparison/confirming/editing a story sends no API. Without a credential the local functions are kept.
- [ ] Run `npm.cmd test`; observe the relevant failures, implement the integrated interface and repeat `npm.cmd run typecheck`, `npm.cmd test`, `npm.cmd run build`. Fake tests reproduce the domain refusal; do not use the fake to bypass Rust requirements.

### Task 8 — Future verification and execution documentation

**Files:** create `docs/phase-4/{setup,validation}.md`; update README, AGENTS.md, the stack only if the decision changed, the phase plan and the spec/plan status after authorized execution. Documentation must only state results actually obtained.

- [ ] Run the future complete `.\scripts\verify.ps1`; record real counts/outputs, fix failures and repeat the affected checks. Release build `npm.cmd run tauri -- build --no-bundle` and smoke with temporary data, without touching personal data.
- [ ] Check the diff against F4-01…16, local links, `git diff --check`, Tauri/fake contracts and that no story identifier/notes are in the payloads. Do not use a real key/paid inference in automated tests.
- [ ] Create the Windows manual script F4-16: original with follow-up, local origin, result without numbers/negative/learning, two practices, story with facts not said, B1/B2/C1 variants, Kokoro voice, re-recording, resume and network unavailable. Measure wait/fidelity/usefulness/cost only when executed in the future with authorization.
- [ ] Mark the differences between automation, real inference and human acceptance; keep all previous pending items. Phase 4 is not accepted just because the checks pass. Deliver a report of files, decisions/deviations, real commands and limitations to the coordinator.

## 4. Coverage and documentary self-review

| Criteria | Tasks and future verifiable result |
| --- | --- |
| F4-01 | 1: empty/v1/v2 migration, rollback, future versions and SQL invariants |
| F4-02, F4-03 | 4–5: payloads by origin, segments/quotes, versions; human fidelity in 8 |
| F4-04 | 5, 7: levels, reuse, immutable variants and local preference |
| F4-05 | 6–7: synthesis without explanations and local recovery |
| F4-06, F4-07, F4-08 | 1–2, 7: owner, versions, exclusive practice, abandonment and resume |
| F4-09 | 4–5: attempt role, single source, own report and retry |
| F4-10 | 6–7: local comparison and explicit versions/pending items |
| F4-11, F4-12 | 3–4, 7: stories, snapshot, exclusive marker and isolated payloads |
| F4-13, F4-14 | 2–5, 7: concurrency, generations, failures, publication, shutdown and restart |
| F4-15 | 6–7: disjoint usage by IDs and partial subtotals |
| F4-16 | 8: future human validation, not produced by this documentary task |

Plan elaborated from the checkout code and the referred spec, without depending on new prices or unverified external features. New public interfaces defined in §2; internal operations may be split into private helpers without changing invariants. The preparation snapshot has an explicit uncertainty state because device/file/SQLite do not form a common transaction. The eight results are separately testable, but the order respects dependencies: owners → practice/stories → AI contracts → AI persistence → outputs → UI → evidence. The technical approval of this document does not authorize executing the tasks; the current request ends with the reviewed spec and plan.

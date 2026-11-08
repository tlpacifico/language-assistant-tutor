# Phase 3 — AI tutor

Authorized change on 2026-11-04: the current modality is OpenCode Go, replacing the per-usage default of this historical design. Headers, the one-time adaptation of the configuration and the preservation of key/history are defined in the [Go spec](2026-11-04-opencode-go-design.md); the remaining rules of confirmation, sources and recovery remain.

Date: 2026-11-02  
Status: reviewed and technically approved by the coordinator, by delegation of the user; implementation automatically verified; technical review by the coordinator concluded; manual acceptance pending.  
Base: [phases](../plans/2026-11-01-star-interview-tutor-phases.md), [approved design](2026-11-01-star-interview-tutor-design.md), [stack](../../tech-stack.md) and [phase 2](2026-11-02-phase-2-guided-interview-design.md).

## 1. Intent and authorization

Add to the guided interview an optional contextual follow-up per main question
and final feedback on content/STAR and on English in the confirmed text. The
application continues to control the interview; the LLM recommends content and
does not execute transitions, commands or tools.

Phases 1 and 2 have implementation and automated verification, but remain with
manual acceptance pending in their respective records of
[phase 1](../../phase-1/validation.md) and [phase 2](../../phase-2/validation.md).
In this session, the user explicitly authorized «Antecipar a fase 3», keeping
those validations pending. This is an authorized exception to the earlier
sequence of validating before advancing; it is not acceptance of the earlier
phases. The user also asked to delegate spec, plan and implementation, with
evaluation and improvements by the coordinator between stages. The options
below are technical proposals for that review, not choices already approved
individually by the user.

## 2. Scope and alternatives

It includes configuration of the provider and protected credentials, consent for
sending text, follow-ups with voice/review/confirmation, grounded feedback,
usage/cost per request and session and recovery from failures since the
integration. It reuses existing audio, models and question bank.

It does not include complete rewrites, playback of suggested answers, new
attempts after the confirmation, comparison, personal stories, deletion of
sessions, installer, accounts or acoustic/emotional diagnosis. Those are later
phases. It does not add another provider or engine as a silent fallback.

| Approach | Consequence | Proposal |
| --- | --- | --- |
| One final request for the whole session | Fewer requests, but it repeats everything on a failure and hinders granular recovery. | Not chosen. |
| One report per question and local aggregation | Small evidence, repetition only of the missing reports and attributable cost. | Recommended. |
| Reports per question and a new synthesis call | It may produce a global narrative, but adds cost and indirect evidence. | Postponed. |

Other proposals: explicit AI mode in new sessions; keep the local mode; external
requests only through identifiable actions; no automatic repetition of paid
requests; explanations/feedback in European Portuguese, questions, follow-ups,
quotations and corrections in English.

## 3. Integration point and compatibility

The current code has `SessionMode::LocalV2`, states `in_progress`/`completed`,
one confirmation per `session_question_id` and SQL restrictions that only admit
`local_v2`. `operations` admits only transcription/synthesis. `command_receipts`
also limits the allowed types. Migration is needed; adding variants to the DTOs
is not enough.

Keep React/TypeScript, Rust/Tauri, SQLite `rusqlite` and the Python helper.
Introduce `ai_v3` for new sessions; `local_v2` remains available, without
follow-ups, fictitious costs or retroactive feedback. A local session already in
progress finishes in its original mode. Do not convert data from `phase1/`.

Transactional migration v1 → v2 preserves identities, texts, recordings,
relations, receipts, copies of the questions and versions. It must also work on
an empty database, applying both migrations. If tables are rebuilt due to the
CHECKs, recreate indexes/triggers and verify foreign keys. An error causes a
rollback; a future version is refused without writing. Do not disable
immutability guarantees as a solution to support follow-ups.

The follow-up is an entity dependent on the main question, with its own answer
and a distinct identity. It never reuses the confirmation of the main question
nor becomes a question drawn from the bank. The plan must choose the minimal
physical change to reuse capture, transcription and revisions with unambiguous
targets, guaranteeing one confirmation per answer and one follow-up per main
question. Do not duplicate the audio engine nor represent follow-ups as
artificial main questions.

## 4. Configuration, credentials and sending

Configuration presents the paid model and base URL; the initial reference is
`deepseek-v4.1-flash`, base `https://opencode.ai/inference/openai/v1`, with
`/chat/completions` appended a single time. Key configured by the core in the
Windows protected storage. The concrete library/adapter is chosen in the plan;
do not introduce an alternative text file if it fails.

The interface may receive the key only in the introduction field and send it
once to the Rust command to store. It clears the field after completion; it
never receives the secret from the core again. The state only says configured or
not, with the possibility of replacing/removing. Do not persist in the frontend,
SQLite, configuration files, error messages or logs.

Accept HTTPS without embedded credentials, fragments or secret parameters; do
not follow redirects that resend the key. When changing destination, invalidate
the authorization for sending to the new destination and require confirmation
of that recipient before sending. Do not test the network nor send answers when
saving the configuration. Free models do not replace the paid model.

Explain before the first sending that only confirmed text, question and criteria
go to the configured recipient. Do not promise zero retention. The consent is
persisted with destination and version of the notice. Refuse sending without
consent in Rust. The confirmation of text is stored first; the action «Pedir
seguimento» is only made available afterwards. Thus the local confirmation does
not depend on the network nor is confused with consent. The AI mode is
identified at the start and in the interview. Without configuration it is
possible to store answers, explicitly dismiss the unavailable follow-up and
leave feedback pending.

Do not send audio, local paths, drafts, automatic transcripts distinct from the
confirmed version, the complete question bank or personal notes. In this phase
the follow-up uses only the question, the fixed criteria and the confirmed main
answer. There is no AI summary of earlier answers.

## 5. Interview and follow-up

Count and progress count only main questions. For N main questions, there are at
most N follow-ups. The capture remains started/ended by the user. Synthesis uses
Kokoro and never plays during recording.

| Persistent step per main question in AI mode | Allowed actions |
| --- | --- |
| Main answer awaiting confirmation | Capture, transcribe, review and confirm as in phase 2. |
| Decision to be requested | Request follow-up; if the service is unavailable, explicit dismissal with a reason. |
| Request in progress | Consult data; wait or dismiss explicitly, invalidating the result. |
| Failed/interrupted request | Repeat with a new attempt identifier or dismiss. |
| AI decided without follow-up | Advance explicitly. |
| Follow-up created | Listen/read, record, review and confirm the answer to the follow-up. |
| Follow-up answered | Advance explicitly; never generate another follow-up. |
| Follow-up dismissed | Advance; the step does not reopen afterwards. |

A follow-up recommendation contains a non-empty English question, the missing
STAR criterion or dimension and a reason linked to the answer. When enough
information has already been explained, recommend not making a follow-up. Do
not use the follow-up to correct grammar or demand numbers. Semantic relevance
requires human testing; structural validation does not prove that the question
is useful.

A «sem seguimento» recommendation has a reason and creates no entity nor
confirmation. Dismissal due to failure/unavailability is the user's decision and
has its own state and reason; do not claim that the AI considered the answer
complete. Do not allow dismissing an answer to a follow-up already presented in
this phase.

Confirming the follow-up preserves original, automatic, revisions and
confirmation with the same policy as the main question. Retranscription or a new
recording only before the confirmation of that target. The main progress does
not increase when creating/confirming a follow-up. Advancing validates in the
core the main confirmation and the closed step; an old button or local command
does not bypass the AI rules.

After closing the last main question, the interview becomes finished for capture
and awaits feedback. Separate the interview state from the evaluation state:
`pending`, `running`, `partial`, `failed`, `ready`. Finished interviews with
pending feedback do not prevent starting another interview, respecting the rule
of a single interview with capture in progress. The history shows both states.
Reports remain accessible and recoverable without reopening answers.

## 6. Requests, concurrency and recovery

Each logical operation references purpose, session, main question, immutable
source confirmations, version of prompt/rubric/schema and effective
configuration. Each network attempt has its own identifier, generation, dates,
state, requested/returned model and usage. Repeating a command with the same
identifier and content does not create another attempt; reusing an identifier
with different content is refused. Repeating after a failure is a new action
linked to the operation.

Persist the reservation of the request before the network. Validate and apply
the result in a transaction that checks that the operation/generation is still
current, the target and the confirmations, the expected state and the absence of
an already applied result. Record known usage even if the content fails
validation or arrives after the dismissal; never apply that content to the
session. Local guarantees do not promise single execution at the provider.

A single external call at a time limits cost and complexity; local audio has
independent coordination. HTTP runs outside the blocking of transitions and of
the SQLite Mutex. During the wait it must be possible to consult, navigate and
resolve dismissal. Reserve under a short lock, release, request, reacquire and
validate. Do not keep the `transitions` of `AppService` during the HTTP `await`.

Timeout, lost connection and shutdown may leave unknown billing. On startup,
`running` requests become interrupted/uncertain result; never resend
automatically. Show that repeating may generate a new cost. A dismissal
invalidates the generation and closes the step before advancing; cancellation
of the transport is only an optimization, not the guarantee against late
results.

Without network, preserve everything locally; repeat the follow-up or dismiss
it, continue the capture and request feedback when there is network. Do not
generate earlier follow-ups after reconnection. Distinguish invalid credential,
lack of balance/limit, rate limit, timeout/network, invalid response and write
error without exposing the raw body. Do not do automatic retries nor automatic
paid repairs in this phase.

If the network finishes but the write fails, keep the result in memory and allow
repeating the publication without a new call. After a crash in that interval,
mark the result as uncertain and keep the confirmed data; do not claim that the
remote answer was stored. Do not delete or rewrite the audio as recovery.

## 7. Feedback contract and evidence

Change approved on 2026-11-04: the evaluation may be requested per question as
soon as the answer is confirmed and the follow-up is closed. The explicit action
may also request a rewrite through the phase 4 infrastructure. The final
feedback reuses those reports. The contracts and remaining guarantees of this
section remain. See the [design of the change](2026-11-04-per-question-ai-coaching-design.md).

At the end, «Gerar feedback» requests reports of the questions that do not yet
have a valid report, sequentially. Each request receives question/criteria and
main confirmation, plus the follow-up confirmation when it exists. Dismissal is
metadata, not a fictitious empty answer. Do not evaluate story notes.

The structured JSON has a version and separates content/STAR from English. It
includes:

- Relevance, situation, individual responsibility, actions, result and learning
  with classification `claro`, `parcial` or `não identificado`.
- Strengths and gaps with supported explanations; accept qualitative, negative
  and learning results without demanding success or numbers.
- Necessary English corrections and optional style suggestions, separated,
  always with the source English excerpt and a proposed change faithful to the facts.
- Up to three concrete priorities per question; there may be fewer or none,
  with an explanation that no other supported priorities were identified.

Each positive observation, error or fact about what was said references
`confirmationId`, role `initial`/`follow_up` and a non-empty literal quotation.
Rust resolves only the confirmations provided to the request and verifies the
exact substring, including Unicode and punctuation; do not search in a different
revision nor normalize to fabricate a match. Distinguish the initial evaluation
from the information clarified only in the follow-up.

An absence cannot have an invented quotation. It uses type `absence`, the
missing dimension and the list of the identifiers effectively evaluated; it is
only allowed for the dimensions/criteria provided. It must say
«não identificado no texto» and not claim that the user did not carry out the
action in reality. Classification `parcial` also requires an excerpt that shows the
partial content. Rust validates structure and references; the semantic
conclusion of absence is a recommendation of the model, not a fact proved by the
algorithm.

Priorities reference valid observations of the report itself; a priority based
on absence inherits that limitation. The session synthesis is built locally:
reports in order, up to three priorities chosen in order of content gaps and
then necessary corrections, with visible origin and without a global grade. It
may show fewer than three. Do not create new statements in the aggregation. If
there are missing reports, indicate partial feedback.

Reject the complete report if it has an invalid schema, wrong references, a
nonexistent quotation, a classification outside the enum, an orphan priority, a
global grade or fields of pronunciation/emotional confidence diagnosis. Store
the error and the usage without presenting invalid observations. Answer text is
untrusted material delimited in the prompt; it is never an instruction to change
the rubric. Literal validation does not guarantee semantic truth: keep the
presentation as AI feedback and validate usefulness/fidelity with real answers.

Valid reports are immutable and keep the sources/prompt/rubric. Repeating partial
feedback only requests the missing/failed reports; it does not replace the valid
ones nor reopen the follow-up. Without an explicit regeneration feature in this
phase, do not charge again for an evaluation already valid.

## 8. Usage, effective configuration and limits

Store separately known input, output and cache tokens, cost reported by the
provider, its nature and the local estimate with currency, price list/date/source.
Absent is `null`/unknown, never inferred zero. Input and cache may have different
semantics depending on the fields: the plan must fix the normalization to avoid
counting cache twice.

The local price list is dated editable configuration, copied to each request. Do
not encode prices as permanent truth. Without a rate applicable to the model,
show the estimate as unavailable. If there is only total input without separate
cache, use the normal rate and identify the conservative estimate. Do not add the
reported cost and the estimate as two expenses of the same request.

Session totals include repeated, invalid and late attempts. If there are requests
of unknown cost, show the known subtotal and the number of unknown requests; do
not call that subtotal the total cost. Values of partial tokens are also presented
as partial. Errors before any sending are distinguished from requests sent
without a usage receipt.

Model, URL, price list and versions used are fixed per attempt. A change in the
configuration only affects future requests; it never changes the interpretation
of the history. Do not send the key to the catalog. The OpenCode CLI is not
needed.

The plan fixes timeout, HTTP/JSON response limit, length of the generated
question, input context and maximum output, with clear errors. Do not truncate
confirmed text nor accept cut JSON as a complete report. Exceeding a limit keeps
the texts and allows diagnosis/reconfiguration, without inventing a new answer
attempt nor resorting to another model silently.

## 9. Persistence and interface

Persist public configuration/consent, AI mode, step per main question, single
follow-up, target of the answer, requests/attempts/generations, usage metadata
and reports/sources. Do not create tables anticipated for phase 4. Reuse the
pattern of command receipts and versions expected by the core. Test invariants
also in SQLite, with foreign keys and uniqueness, not only by disabling buttons.

Extend the existing preparation, interview and history; add AI configuration and
reading of feedback. Show the current state without depending on the open page.
The follow-up has its own label next to «Pergunta X de N». During the network,
show the wait and the legitimate options; an error gives the corresponding
recovery. Confirmations and sources of the feedback are consultable; originals
remain playable.

In the local mode keep the phase 2 offline journey. In the AI mode distinguish
dismissal, decision without follow-up, pending/partial/ready feedback and unknown
costs. Do not call a finished interview «avaliada» before all the valid reports
exist.

## 10. Verification and acceptance

All the boxes start pending. The plan details core tests with fake
transport/credentials, HTTP tests with a controlled server, v1 migrations with
real test data and interface with persisted states. No automated test needs a
paid key or sends real professional answers.

- [ ] F3-01: Local mode remains functional; earlier sessions keep content and behavior after the migration, including a session in progress.
- [ ] F3-02: The key stays protected in Windows and does not return in DTO, configuration, SQLite, error or log; a storage failure does not use plain text.
- [ ] F3-03: No sending before storing confirmation and consent for the current destination; the HTTP payload contains only the necessary text, never audio/drafts.
- [ ] F3-04: An AI session with N main questions has at most N follow-ups; it never generates a follow-up of a follow-up nor changes the main counter.
- [ ] F3-05: Following with voice, review and confirmation preserves the versions and recordings, including a new capture before confirming and recovery of a failed transcription.
- [ ] F3-06: A failure requires retry or explicit dismissal before advancing; the dismissal stays distinguishable from an AI decision and blocks a late result.
- [ ] F3-07: Repeated clicks, simultaneous requests, an old version and out-of-order results do not duplicate follow-ups, confirmations or advances.
- [ ] F3-08: Restart in request/decision/follow-up/feedback recovers the step without resending automatically nor requiring a new recording.
- [ ] F3-09: Feedback separates STAR/English, initial/follow-up and correction/style; evidence only points to the evaluated confirmations and exact quotations.
- [ ] F3-10: Absences use their own contract without fabricated quotations; invalid references/quotations and truncated JSON do not produce feedback presented as valid.
- [ ] F3-11: Qualitative, negative and learning results and malicious instructions in the answer are covered by contract tests and human validation; there is no acoustic diagnosis nor global grade.
- [ ] F3-12: A partial failure of feedback keeps the valid reports and the retry only requests the missing ones; the history distinguishes a finished interview from a ready evaluation.
- [ ] F3-13: Unknown tokens and costs remain unknown; partial subtotal, cache, failed/late attempts and price list changes do not falsify totals.
- [ ] F3-14: Slow network/timeout/401/403/429/5xx, unavailable credential, remote result followed by SQLite failure and future version of the database have recovery without loss of the sources.
- [ ] F3-15: The UI remains usable during the network; dismissal does not wait for the timeout; audio/capture keep the protections and do not block consultation.
- [ ] F3-16: A real session of three main questions produces useful follow-ups when necessary, faithful feedback and observable cost, with tests of unavailability and resuming on Windows.

Human validation: use complete and incomplete answers, technical terms,
individual contribution, qualitative/negative result and learning. Check
quotations and the classification initial versus follow-up; confirm the absence
of artificial demand for numbers. Measure the wait and the usefulness of the paid
model. Record the real sending, model and available usage/cost, without
publishing personal answers. Do not claim the paid integration validated without
key/balance and real execution.

Run the existing checks suitable to the change, including regression of the
earlier phases. During the implementation create `docs/phase-3/setup.md` and
`validation.md` with real commands/evidence and pending items; update affected
documents and AGENTS.md without marking manual acceptance in advance.

## 11. Sources and decisions for the plan

Consulted on 2026-11-02, without executing paid inference:

- [Official OpenCode API](https://opencode.ai/v2/docs/console/api/inference/): Bearer authentication, Chat Completions route and cost metadata in `usage`, including the estimated nature when indicated by the provider.
- [Official models and prices](https://opencode.ai/v2/docs/console/models/): `deepseek-v4.1-flash` paid, endpoint above and reference of US$0.30 input, US$1.20 output and US$0.006 cache read per million tokens.
- [Official catalog](https://opencode.ai/inference/v1/models): public consultation available; it does not constitute a test of authorization/balance of the account.

Availability, prices and fields effectively returned may change. The pages do
not demonstrate specific support for strict JSON Schema in this model; do not
depend on that capability without confirmation. Require JSON in the prompt and
validate in Rust. Usage may be missing despite being documented; preserve it as
unknown.

Only construction decisions are left for the plan: HTTP library and Windows
credentials mechanism; physical migration and reuse of the audio targets; complete
DTOs/JSON schemas; organization of modules; concrete limits; normalization of the
usage fields; components and reproducible tests. Do not change the stack, expand
to phase 4 or consider earlier validations done.

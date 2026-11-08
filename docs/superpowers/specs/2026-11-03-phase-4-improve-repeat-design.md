# Phase 4 — Improve and repeat

Date: 2026-11-03  
Status: reviewed and technically approved; implemented on 2026-11-03 by later authorization of the user and automatically verified ([validation](../../phase-4/validation.md)). Criteria F4-01 to F4-16 below remain unchecked: manual acceptance (F4-16) and real paid inference are pending.
Base: [MVP design](2026-11-01-star-interview-tutor-design.md), [phases](../plans/2026-11-01-star-interview-tutor-phases.md), [stack](../../tech-stack.md), [phase 3](2026-11-02-phase-3-ai-tutor-design.md) and [current validation](../../phase-3/validation.md).

## 1. Intention, authorization and limits

Close the practice cycle: consult what was answered, understand feedback,
read/listen to a faithful rewrite, answer again and compare without losing
previous versions. Add factual stories as local support for preparation.

The user authorized preparing the next phase and expressly limited the request
to «somente a spec e o plano de implementação». This document does not authorize
writing code, installing dependencies, running migrations, using the configured key
or executing paid inference. The coordinator evaluates the spec before delegating the plan.
The decisions proposed below are not presented as choices already approved
individually by the user.

Previous phases still have incomplete manual acceptance. The report of a
satisfactory manual test did not include paid API. The later local configuration
does not demonstrate real authentication, balance, semantic usefulness or cost. Preparing
this phase does not change that evidence nor marks the previous phases as accepted.

Included: English rewrite with a chosen level, correction/style distinction,
local Kokoro synthesis, individual attempts with their own evaluation, local
comparison, story bank and optional selection, persistence and recovery.

Excluded: follow-ups for new attempts, comparison through a new AI call,
story or fact generation, vector search, global/proficiency scoring,
acoustic/emotional diagnosis, bulk import, deletion of sessions or
stories, installer and autonomous distribution. Do not start phase 5.

## 2. Approaches and proposed decisions

| Approach | Consequence | Proposal |
| --- | --- | --- |
| Reopen the original answer/session | Few new entities, but it changes sources, progress and historical reports. | Rejected. |
| Create a complete interview per repetition | Reuses the flow, but confuses counters, modes and follow-ups. | Rejected. |
| Individual practice linked to the fixed question, with its own answer target | Preserves originals and reuses audio, confirmation and requests. | Recommended. |

Use React/TypeScript, Rust/Tauri, SQLite and the existing engines. No new
provider. Comparison through local presentation of the texts and reports;
without a second evaluator or synthesis call. Stories exclusively local in
this phase: no story field or identifier enters the API.

Simplicity proposals: B1 initially, according to the approved design §2,
with B1/B2/C1 selection in the rewrite;
one unconfirmed practice at a time; start practices only when there is no
interview in progress. Level is a chosen language goal, not a diagnosis
of the user's CEFR level. The initial value B1 preserves the existing decision;
the remaining construction choices are technical proposals for review.

## 3. Starting point and invariants

Change approved on 2026-11-04: evaluation and rewrite of the original can occur in an ongoing AI interview, after confirmation and closing of the question's follow-up. Creating practices still requires a completed session. See [design of the change](2026-11-04-per-question-ai-coaching-design.md).

The SQLite v2 schema has `answer_slots` for `initial`/`follow_up`, recordings,
transcripts, revisions and confirmations. Uniqueness per question/role prevents
adding repetitions by merely reusing a role. `ai_requests` is unique per
purpose/question and `ai_reports` per question. The service resolves answer
targets of the active interview. These contracts have to evolve deliberately.

A new attempt has its own identity, state, ordinal and target, linked to the
question of the original session and, optionally, to the previous confirmed attempt.
It never changes `position`, `question_count`, follow-up stage, confirmation,
selected recording or original report. Do not create a new question from the bank.
Text, skills and criteria are those of the fixed copy of the source session,
even if the bank is edited meanwhile.

Reuse the capture/transcription/review/confirmation cycle through a typed
target that distinguishes initial, follow-up and attempt. SQLite and Rust must
prevent associating a recording, confirmation or request with the wrong target. The
physical choice of extending `answer_slots` or adapting relations is left to the plan;
do not duplicate the engines nor relax immutability to fit the practice.

Transactional v2 → v3 migration, also applicable sequentially to empty
and v1 databases. Preserve all data, receipts, references, indexes and triggers;
failure reverts and a future version is not written. Do not convert `phase1/` nor fabricate
confirmations for the old JSON practice sessions; they remain read-only.

## 4. Rewrite: sources, level and contract

Available for confirmed questions of completed `local_v2` or
`ai_v3` interviews, and for confirmed individual attempts. It does not require a prior report:
the sources of truth are confirmations, not the feedback text. A
local session remains local; asking for an AI exercise from it does not convert it
into an AI interview, does not generate retroactive follow-ups nor session feedback.

In the main origin, send exclusively the question, fixed criteria and confirmed
texts of the initial answer and of the respective follow-up, if any. In the
attempt origin, send only the confirmation of that attempt with the same criteria. Never
merge facts from the old answer into a new attempt. Never include audio,
story notes, drafts, another question, reports or rewrites as
if they were answers. All sources have an unambiguous identifier and role.

«Gerar reformulação» allows choosing B1/B2/C1; B1 is the initial suggestion. Store
that public preference locally for future requests. Each request/result
fixes the effective level; a later change does not alter previous suggestions.
Do not attempt to measure pronunciation, oral fluency, confidence or certify proficiency.

The structured result distinguishes:

- Suggested English text, complete and reproducible, organized and adequate to the
  requested level, without new numbers, responsibilities, causality or results.
- Necessary language changes, separated from optional style/organization
  alternatives; each one links a literal excerpt of the confirmation and the corresponding
  segment of the suggestion, with an explanation in European Portuguese.
- Segments of the rewrite linked to literal quotations from one or more confirmed
  sources. Information coming only from the follow-up keeps that origin visible.
- Missing information and preparation questions outside the voice text. An
  absence identifies dimension and evaluated sources; it does not invent a quotation.

Keep individual versus collective responsibility, negation, uncertainty, relevant
temporal order, negative/qualitative result and learning. Do not complete
STAR with invented facts, nor impose numbers or success. If there is no data
for a complete answer, suggest only what is supported and flag the
limitation outside the voice text. Do not insert placeholders for Kokoro to read.

Rust validates version/schema, limits, level, authorized sources, exact quotations,
references to the segments and the existence of non-empty voice text. Reject a
malformed result or one with invalid references; do not present the partial
version as validated. Answer text is untrusted material in the prompt,
never an instruction to ignore the rules.

Correct quotations do not prove that a paraphrase preserves the meaning. The system
cannot automatically guarantee semantic fidelity with literal validation.
Present as an AI suggestion, with consultable sources and an indication to
check the facts. Human validation includes changes of responsibility,
certainty, negation, numbers, results and chronology. Do not add a second
automatic paid request as supposed proof of fidelity.

Valid suggestions are immutable. A repeated request with the same identifier
does not generate another suggestion. For the same origin/level/prompt and rubric
version, reuse a valid result; changing the level creates another variant, preserving
the previous ones. Explicit regeneration of the same variant is not part of this
phase; repeat only failed/interrupted requests.

## 5. Listening to the suggestion

«Ouvir reformulação» passes only the suggested text to the local Kokoro. It does not read quotations,
explanations, notes, gaps or labels. It keeps English and the local voice/speed
configuration. Use the existing helper and audio coordinator; do not open
a new service, do not send text to the provider for synthesis.

The identity of the derived audio includes suggestion, text and voice configuration;
do not reuse audio from another variant. Store text before synthesizing.
A Kokoro failure keeps the rewrite readable and allows repeating only the synthesis,
without a new LLM call. A missing file allows regenerating that derivative without
changing the textual result. Capture interrupts output; never play during
recording. Synthesis/transcription remain coordinated as a single local job.

## 6. New attempt and evaluation

In the history of a completed interview, choose a question and «Tentar novamente».
It can be done without available rewrite or feedback. `local_v2` and `ai_v3`
with confirmation are eligible; JSON practice sessions without confirmation are not.
Do not start during an ongoing interview; show the option to resume it. A
pending practice prevents starting another interview or practice until explicitly
confirming or abandoning, without deleting data. Feedback and rewrites pending
from already confirmed practices do not block a new practice session.

| State of the practice | Behavior |
| --- | --- |
| Preparation | Fixed question, optional story and the option to listen to the question/rewrite. |
| Answer to confirm | Own manual capture, playback, transcription, review and confirmation. |
| Confirmed | Immutable text; evaluation not yet requested or recoverable. |
| Abandoned before confirming | Store state, recordings and review; consultation only, without evaluating or reopening. |

Creating a practice is an idempotent transaction. Each practice has a stable ordinal;
re-recording before confirming keeps the old recordings of the practice itself and
does not create a new confirmed attempt. Confirmation fixes exactly the review
seen, with references to the audio and automatic/manual origin. A recording or write
failure does not lose the previous selection nor produce a false confirmation.

A confirmed practice never creates a follow-up, even if the model recommends it.
«Avaliar tentativa» is an action distinct from confirming and uses the existing
credential/consent. The payload contains only the original question/criteria and the
confirmation of this attempt; it excludes the old initial answer, the old follow-up, stories and suggestions.
Evidence uses its own `attempt` role, never `initial` to pretend that
the attempt belongs to the original interview. It uses a rubric and version compatible
with the original evaluation; where no original
report exists, it fixes the current rubric for the first evaluation of that family.

Preserve the STAR/English feedback contract of phase 3, quotations/absences, up to
three supported priorities, correction versus style and the absence of a global score.
The report belongs to the attempt and is immutable. An error allows repeating only its
evaluation without recording again; do not generate feedback/rewrite automatically
on confirming. Without network, one can continue with local practice and evaluate later.

## 7. Comparison without changing sources

In the history, compare the original with one confirmed attempt or two confirmed
attempts of the same question. Present in order of creation, with dates,
original audios, automatic transcripts, confirmations and separate reports. A draft
or abandoned practice does not count as a comparable confirmed answer.

In the original, distinguish the initial answer and the follow-up. Never concatenate and
present as if everything had been said before the follow-up. Rewrite
appears in an «Sugestão da IA» zone, not in the column of what was answered. Notes
appear only as «Apoio à preparação», outside the evidence.

Comparison is local: show classifications and priorities side by side, without
inventing an evolution score, calling textual difference an improvement or
asking for a new combined evaluation. If an evaluation does not exist, indicate pending
and allow requesting it for that eligible target. The original interview evaluation
only remains available through the original flow, without altering `local_v2` to create it.

Store the rubric version in each report. If they differ, indicate a comparison
limited by different criteria/versions; do not recalculate or replace
old reports automatically. The absence of a follow-up in an attempt
has an explicit label, to avoid a misleading comparison with an expanded original.

## 8. Factual stories and isolation

Local bank with listing, addition and editing. Each story has an identifier,
version, title, situation notes, responsibility/task, actions, result
or learning, skills and dates. Non-empty title, at least one
factual field filled and one non-empty skill; accept an incomplete story
identified as such. Ignore outer spaces in validation/persistence,
without rewriting inner content. Notes can be in Portuguese or English;
do not treat them as a practice answer nor generate facts automatically.

No deletion, import, automatic extraction from answers or automatic
association. Simple filtering by skill is enough. A story can
support several questions/practices; association is optional and manual, one per target.

In the preparation of each main question or practice, before the first capture,
allow choosing, replacing or removing the support. On the first capture successfully
started, fix a snapshot of the selected content/version; do not wait for
confirmation to preserve the effective preparation. Afterwards it is read-only
in that target. Editing the bank later does not change previous snapshots.
The follow-up shares the preparation of the main question, without its own association.
Do not require a story to record nor block for missing STAR fields.

Show notes during preparation/review with an indication that they are notes,
not a transcript. Never fill the answer editor, confirmation or prompt
from them. Do not send notes, title, story skills or snapshot
in follow-up, evaluation or rewrite requests. Criteria/skills of the
fixed question remain allowed; they are different entities.

Information only in the notes can be seen locally as something to consider
in a future practice session; do not produce an automatic analysis of what «faltou dizer» with
those notes nor incorporate it into a voice suggestion. If the user says that
information in a new attempt, the confirmation of that speech becomes a legitimate
source; the story snapshot never is.

## 9. Persistence, operations and recovery

Persist stories and versions/snapshots, practices and links, answer targets,
rewrites/variants/sources/level, own reports, requests/attempts,
usage and references of the voice derivatives. Do not create a parallel database nor
overwrite source files/rows. Evaluation and suggestion state is
independent of the practice state; an AI failure does not undo the confirmation.

Reuse the phase 3 patterns: reserve before the network, command identifiers
and content hash, generations and immutable sources, configuration/price list
fixed per attempt, transactional application only to the expected target/generation.
Widen purposes for attempt evaluation and rewrite,
without colliding with the current uniqueness per question nor confusing HTTP `attempt` with a
practice attempt. The plan must use distinct names/contracts.

One remote call at a time, without automatic paid retries. Network does not hold the
SQLite Mutex or a global transition lock; consultation and navigation remain
possible. On returning, update only the origin target, even if another practice
is open. Do not replace the variant selected in the interface with a late
result for another level. Discarded outputs remain in the usage.

On restart, capture does not start again and ongoing remote requests become
interrupted/uncertain, without automatic resending. Stored review and confirmation
persist. A local publication that fails after a remote response allows a new write
without new network while the result is in memory; a crash in that interval
keeps the result uncertain, it does not promise a stored answer. A finished recording
without publication follows the existing reconciliation, without requiring another capture.

Costs per purpose, practice, origin and provenance session: include
failed or repeated evaluations/rewrites, preserving historical records.
Do not rewrite the original evaluation to add costs; the derived summary
can distinguish «entrevista original» and «prática posterior», with an eventual
«total combinado» explicitly labeled. One network
attempt is counted once, without duplicating between practice and session. Unknowns
remain unknown; present partial subtotals and uncertain attempts.

Store price list/date/source and reported cost separately from the estimate, according to
phase 3; do not fix new prices in this spec. Local synthesis is not API usage.
Without key/consent, keep local functions and show how to configure, without
reading a secret into the frontend. Do not send anything automatically when editing a story,
selecting a level, opening a comparison, confirming a practice or loading history.

Input/output limits, size of stories, rewrite text and audio duration
will be made concrete in the plan, preserving the existing limits and
without silent truncation. Results outside the limit are recoverable errors,
not partially accepted versions.

## 10. Verification and acceptance

All criteria below start pending. This documentary task only verifies
coherence and links; it does not produce implementation evidence. The plan must foresee
temporary data, controlled credentials/HTTP and failure tests, without using
personal data or paid inference in the automatic tests.

- [ ] F4-01: Migration preserves local/AI sessions, targets, originals, reports and receipts; rollback and future version are safe, including an empty/v1/v2 database.
- [ ] F4-02: Rewrite fixes source/level; only authorized confirmations enter the payload, including follow-up origin and exclusion of the old answer when rewriting an attempt.
- [ ] F4-03: Invalid schema/quotations/segments are refused; cases with negation, uncertainty, team versus individual, numbers and negative/qualitative result have human fidelity verification.
- [ ] F4-04: B1/B2/C1 and the initial value B1 are not a diagnosis; variants preserve previous ones and a repeated request does not charge again for an already valid result.
- [ ] F4-05: Gaps, quotations and explanations do not enter the Kokoro text; failure/missing derived audio allows local synthesis without new inference.
- [ ] F4-06: A new attempt only arises from a confirmed question of a completed session; it does not reopen the session, change a counter or generate follow-ups. It stores distinct confirmations and reports.
- [ ] F4-07: Capture/transcription/review/confirmation and re-recording preserve versions; a write failure and restart resume the practice without losing stored audio or duplicating the confirmation.
- [ ] F4-08: A pending practice can be resumed or abandoned without deletion; a single active capture/practice does not interfere with an interview or playback.
- [ ] F4-09: Evaluation only uses the practice confirmation and fixed criteria; without network it stays pending and a retry does not repeat valid reports nor require a new recording.
- [ ] F4-10: Comparison distinguishes initial, follow-up, attempt and suggestion, presents different rubrics/pending items and does not claim improvement from a mere textual difference.
- [ ] F4-11: The story bank adds/edits with validation, preserves historical preparation by snapshot and does not require a complete STAR to be used.
- [ ] F4-12: A story with a unique marker does not appear in any AI payload, transcript, confirmation or generated evidence; a later edit does not change the fixed snapshot.
- [ ] F4-13: Identities/generations prevent applying a result to another practice/level; repeated commands, concurrency and old clicks do not duplicate effects or costs.
- [ ] F4-14: Timeout, unavailable credential, 401/403/429/5xx, invalid JSON, failed publication and shutdown keep sources and explicit recovery; there is no automatic resending.
- [ ] F4-15: Usage includes new purposes and discarded results, without double counting or an unknown treated as zero; practice costs are distinguished from the original interview.
- [ ] F4-16: On Windows, the user listens to the suggestion, repeats with their own words, confirms, evaluates and compares; validates factual fidelity, usefulness, wait and isolation of the notes.

Future human validation: at least one origin with a follow-up, a local session,
an answer without numbers with a negative result/learning and two attempts
of the same question. Use a story with facts that were not said and verify that
they do not appear in the suggestion/feedback. Test level variants, Kokoro audio,
re-recording, leaving/resuming and network unavailability. Record times, model,
known usage/cost and real assessment, without publishing professional data.
Do not consider paid inference validated before executing it with future authorization.

## 11. What remains for the technical plan

Physical schema/v3 migration and constraints, Tauri/DTO contracts, adaptation of the audio
targets and source validation, complete prompts/schemas, rubric version
relations, concrete limits, history/practice/stories components,
voice cache and recovery tests. Reuse existing transports, credentials,
consent and usage without changing the stack.

The plan must separate executable tasks and verifications, indicate future
updates in setup/validation and preserve existing changes of phase 3.
Technical approval of the spec allows writing the plan, not implementing: the current
request ends with these two reviewed documents. Implementation and commands that
change the product require a later instruction from the user.

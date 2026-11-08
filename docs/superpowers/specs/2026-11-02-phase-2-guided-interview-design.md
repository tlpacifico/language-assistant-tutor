# Phase 2 — Guided interview

Date: 2026-11-02  
Status: reference spec; implementation executed and automatically verified on 2026-11-02, with the user's manual validation pending.  
Base: [phase plan](../plans/2026-11-01-star-interview-tutor-phases.md), [MVP design](2026-11-01-star-interview-tutor-design.md) and [technical stack](../../tech-stack.md).

Technical plan: [Phase 2 implementation](../plans/2026-11-02-phase-2-guided-interview-implementation.md), with tasks 1 to 9 executed and task 10 partial. Real evidence in [docs/phase-2/validation.md](../../phase-2/validation.md).

## 1. Intent and starting point

Turn the practice of one fixed question into a local interview with several
questions: choose category and count, listen and answer, review and confirm
each text, finish and consult what was stored.

Phase 1 already implements capture, playback, synthesis, transcription, review
and recovery of one answer in JSON files. Phase 2 reuses those engines and
introduces the editable bank, the interview state and SQLite. It does not
integrate the API.

The user asked to move on to the next phase's spec. The
[phase 1 validation](../../phase-1/validation.md) record still contains pending
manual criteria. Preparing this spec is not the same as accepting phase 1 or
proving those criteria. Before implementing phase 2, reconcile that record with
the user's real tests, including the acceptability of the wait. Do not repeat
tests that already have enough evidence; record the evidence that is missing.

## 2. Scope

### Included

- Initial bank of 20 questions in English, in the six categories of the MVP.
- Consultation, addition and editing of questions, category, competencies and criteria.
- Configuration of the category and of the number of main questions: three by
  default, one or another available count.
- Selection without repetition and order fixed at the start, with a copy of the contents.
- One session in progress at a time, with progress and resuming after a restart.
- Manual capture, playback, local transcription and review on each question.
- Explicit confirmation of an immutable version of the text.
- History of sessions, questions, recordings and stored texts.
- SQLite for the data of the new sessions; audio in local files.
- Preservation and consultation of the earlier phase 1 data.

### Excluded

Follow-ups, feedback, grades and AI calls belong to phase 3. Personal stories,
rewrites, new attempts at answers already confirmed and comparison belong to
phase 4. Deletion of sessions, standalone installer and consolidation of the
distribution belong to phase 5.

Do not add accounts, synchronization, automatic search for stories, live
transcription, recording ended by silence or speech diagnosis. Do not present a
finished session as an evaluated interview.

## 3. Approach and proposed decisions

The base chosen by the user remains: React/TypeScript, Rust/Tauri 2, SQLite,
faster-whisper and Kokoro in the Python helper. The division into phases already
approved determines the functional approach; there is no need to choose another
stack.

| Evolution option | Consequence | Proposal |
| --- | --- | --- |
| Extend the JSON to all sessions | Avoids SQLite now, but postpones the planned delivery and disperses the interview transactions. | Discarded for contradicting the approved scope. |
| Introduce SQLite for new sessions and keep the phase 1 artifacts | Allows transactions without converting or rewriting old recordings. Requires a separate consultation of earlier data. | Recommended in this spec. |
| Convert all the old data automatically | Uniformizes the history, but adds migration of incomplete content before validating the sessions. | Postponed; it is not needed for this increment. |

The following choices are proposals of this spec, subject to review:

- Choose «Todas as categorias» initially; also allow a single category, without
  multiple selection in this increment.
- Draw eligible questions without repetition. Do not optimize diversity or
  avoid questions from earlier sessions in this phase.
- Separate «Guardar revisão», «Confirmar resposta» and «Próxima pergunta».
  Confirming fixes the text; advancing is another explicit action.
- Allow leaving and resuming the single session in progress; do not introduce
  abandonment, skipping questions or several simultaneous interviews in this phase.
- Keep the phase 1 answers accessible in a consultation of «Treinos anteriores à
  fase 2», without converting them into interviews or confirming texts
  retroactively.

## 4. Question bank

### Structure and editing

Each question has a stable identifier, text in English, one of the six
categories, one or more competencies and one or more content criteria. Dates
and version identify changes. The criteria guide the future textual evaluation;
in this phase they are only stored and presented.

The form requires text, category, at least one competency and one criterion,
all non-empty. Ignore outer spaces in validation and persistence; do not alter
the inner content. Do not depend on automatic language detection to accept the
text: the fields state that the question must be in English. The interface and
the explanations are in European Portuguese.

Allow adding and editing. Deletion, batch import, editing of the six categories
and autonomous management of a catalog of competencies stay out of this
increment. Competencies and criteria are edited in the question itself.

Insert the initial bank a single time, with stable identifiers and content
version. Restarting or updating the application does not restore edited
questions, does not duplicate records and does not insert the bank again over
personal changes.

### Proposed initial content

The criteria accept qualitative, negative and learning results. No question
requires numbers or a success. Do not provide model answers to the transcriber.

| # | Category | Question in English | Competency | Specific criterion |
| --- | --- | --- | --- | --- |
| 1 | Conflitos e colaboração | Tell me about a time you disagreed with a colleague. | Gestão de conflitos | Explicar a divergência, a ação própria e o desfecho. |
| 2 | Conflitos e colaboração | Describe a time you helped a team work together more effectively. | Colaboração | Identificar o obstáculo e a contribuição individual. |
| 3 | Conflitos e colaboração | Tell me about a time you had to explain a technical issue to someone without a technical background. | Comunicação | Explicar a adaptação da mensagem e como verificou a compreensão. |
| 4 | Conflitos e colaboração | Describe a time you worked with someone whose approach was different from yours. | Colaboração | Explicar como coordenaram o trabalho e o que aconteceu. |
| 5 | Liderança e iniciativa | Tell me about a time you took initiative without being asked. | Iniciativa | Distinguir a decisão própria e o efeito ou aprendizagem. |
| 6 | Liderança e iniciativa | Describe a time you helped someone develop a skill. | Liderança | Identificar a necessidade, o apoio prestado e o resultado observado. |
| 7 | Liderança e iniciativa | Tell me about a time you influenced a decision without formal authority. | Influência | Explicar os argumentos e a contribuição para a decisão. |
| 8 | Falhas e aprendizagens | Tell me about a time you made a mistake at work. | Responsabilidade | Reconhecer o papel próprio, a reação e a aprendizagem. |
| 9 | Falhas e aprendizagens | Describe a time you received feedback that was difficult to hear. | Aprendizagem | Explicar a reação e como usou ou ponderou o feedback. |
| 10 | Falhas e aprendizagens | Tell me about a time an approach you tried did not work. | Reflexão | Explicar o que falhou e a mudança ou aprendizagem seguinte. |
| 11 | Resolução de problemas | Tell me about a time you solved a difficult problem at work. | Resolução de problemas | Explicar o problema, o raciocínio, as ações próprias e o desfecho. |
| 12 | Resolução de problemas | Describe a time you investigated a problem with incomplete information. | Análise | Explicar como recolheu informação e lidou com a incerteza. |
| 13 | Resolução de problemas | Tell me about a time you had to choose between different solutions. | Decisão | Explicitar alternativas, compromisso e consequência. |
| 14 | Resolução de problemas | Describe a time you improved a process that was causing problems. | Melhoria de processos | Distinguir o problema inicial, a mudança e o efeito observado. |
| 15 | Prioridades e prazos | Tell me about a time you had competing deadlines. | Priorização | Explicar critérios de prioridade, comunicação e desfecho. |
| 16 | Prioridades e prazos | Describe a time you realised you might miss a deadline. | Gestão de expectativas | Explicar quando identificou o risco e como atuou. |
| 17 | Prioridades e prazos | Tell me about a time you had to reduce the scope of a task. | Gestão de âmbito | Justificar a escolha e as consequências para o objetivo. |
| 18 | Adaptação a mudanças | Tell me about a time your priorities changed unexpectedly. | Adaptação | Explicar a mudança e como reorganizou o trabalho. |
| 19 | Adaptação a mudanças | Describe a time you had to learn a new tool or process quickly. | Aprendizagem | Explicar a necessidade, a estratégia e o resultado. |
| 20 | Adaptação a mudanças | Tell me about a time you worked through a major change in your team. | Gestão da mudança | Descrever a mudança, a ação individual e a aprendizagem. |

Also store common STAR criteria: context, individual responsibility, actions,
result or learning. The session copy includes the common and specific criteria
in effect, without depending on the future version of the bank.

## 5. Preparing and starting a session

Show the category, the intended count and the number of eligible questions.
Accept only integers between one and the eligible total. If there are fewer than
three, keep the suggestion of three with a warning and require the user to
adjust the count; do not reduce the total or repeat questions silently.

On start, the core validates the availability again, draws and stores, in one
transaction, the session, the configuration, the source identifiers, the order
and the copies of the text, category, competencies and criteria of each
question. Only after persisting does it present the first question as started.

Repeated clicks on the same request do not create several sessions. If a
session in progress already exists, present the option to resume it. A
completed session allows starting another. Later edits in the bank affect only
future sessions.

## 6. Answering, reviewing and confirming

### Audio and transcription

Show «Pergunta 2 de 3» and the fixed English text. Provide synthesis and
repetition with Kokoro; a synthesis failure keeps the text and allows answering.
The cache distinguishes text and voice configuration, without reusing audio
from another question. Reuse the capture and the formats validated in phase 1.

The user starts and ends the recording. Starting the capture interrupts the
output; no audio plays during the capture. Store the original before
conversions and transcription. Preserve the derivative separately. Identify
incomplete recordings and allow listening to them before deciding whether a new
recording is needed.

Each recording has its own identity. Before confirmation, «Gravar novamente»
creates another record for the same question, preserving the earlier ones,
without counting them as extra questions. The new recording only replaces the
current selection after being finalized and stored; a capture failure keeps the
earlier one. The earlier recordings remain consultable. The confirmation uses
only the selected recording and the review that belongs to it.

A transcription failure allows repeating over the existing file. Each automatic
result is immutable and identified. Reprocessing does not replace stored
revisions nor change automatically the source result of a revision.

### Review and confirmation

Present the automatic text as read-only and an editable copy. Saving the review
keeps the editable text; it does not confirm or advance. The confirmation
creates a separate record with the exact revised text, date and references to
the question, recording, transcription and source review.

«Confirmar resposta» states that the text will become fixed. The core receives
the expected version and confirms exactly the version the user saw. Persist
review and confirmation atomically; a failed write or an outdated version does
not confirm or advance. While the request is pending, prevent editing and
changing the recording. Repeating the request does not create another
confirmation.

Empty text or text with only spaces cannot be confirmed. When there is no
recognized speech, allow a new recording or an explicit introduction of a
review faithful to the existing audio; record that the text was introduced
manually and preserve the `no_speech` result. Do not accept an answer without a
finalized original nor present silence as a pronunciation problem.

After confirming, the answer becomes read-only. Do not provide undo of the
confirmation, new capture or retranscription of that answer in this phase. The
future new attempts will be independent.

### Advancing and finishing

«Próxima pergunta» is only available with the current answer confirmed. Rust
validates the expected session, position and version; repeating the command
does not skip questions. Do not allow advancing a question without a confirmed
answer.

On the last question, the action is called «Terminar sessão». It only concludes
when all the planned questions have a confirmed answer. Show a consultable
summary, without evaluation, grade or pending feedback. Store that the session
was carried out in the phase 2 local mode: do not create fictitious follow-up
dismissals nor AI requests for these sessions when phase 3 is installed.

## 7. Navigation, history and resuming

The interface provides preparation/resuming, question bank, interview and
history. The visual details are left for the technical plan.

List sessions with date, category, count, progress and state in progress or
completed. Opening a session allows reading the fixed questions, listening to
originals, consulting automatic results, the stored review and the confirmed
text in distinct areas. Confirmed answers remain for reading. In a session in
progress, the resume action leads to the persisted step, including a confirmed
answer that still awaits explicit advancing.

Do not change session or page during capture/finalization. During model work it
is possible to consult stored contents; that navigation does not change the
identity of the work nor start another one. Returning to the interview shows the
real state of the core.

When leaving the editor or closing normally, resolve changes with
save/discard/cancel. Only navigate or close if the latest version is stored or
was explicitly discarded. If there is an edit during a write, the new version
remains unsaved. Failures keep the editor and the text. On shutdown with an
active capture, try to finalize and store before leaving, according to the phase
1 contract. Do not promise to save unfinalized audio on an abrupt failure.

On restart, recover the single session in progress and the last stored step.
Interrupted work becomes a recoverable state; it does not stay processing
forever and does not resume the capture automatically. A completed session opens
in the history, without new recording or automatic advancing.

## 8. Persistence and compatibility

### Data of the new sessions

SQLite is the authority for the bank, the copies of the questions, sessions,
position, recordings, transcripts, revisions, confirmations and operations. The
audio files stay in the application data folder, separate from the models and
the code. Do not keep JSON and SQLite as two authorities of the same new
sessions.

The logical model distinguishes:

| Record | Content and invariants |
| --- | --- |
| Bank question | Current content, category, competencies, criteria and version. |
| Session | Configuration, local mode, state, position, dates and concurrency version. |
| Session question | Unique position and complete copy of the contents at the start date. |
| Recording | Session question, files, metadata, integrity and current selection. |
| Automatic transcript | Source recording, original text, state, model/configuration and times. |
| Revision | Stored text, version, automatic or explicitly manual origin. |
| Confirmation | At most one per session question; immutable text and references. |
| Operation | Identity, target, generation, state, error and times. |

Store the metadata already produced in phase 1 without losing information.
Details of tables and normalization are left for the technical plan; do not
create empty tables yet for stories, feedback or costs.

Creating a session with questions, confirming an answer and advancing are
transactions. The core and database restrictions prevent duplicate
confirmations, repeated positions and more than one session in progress.

SQLite and files do not share a transaction. Finalize the audio before
publishing the reference; if the publication fails, keep the file with a
recoverable identity and allow repeating without recording again. On startup,
reconcile interrupted publications, without duplicating records or deleting
orphan audio automatically. Missing or invalid files produce a warning per
artifact; they do not prevent reading the remaining texts.

Version the schema and run transactional migrations. A failure does not
recreate or clean the existing database; show a recoverable error and preserve
the data. A database of a future version is not opened for writing by an older
version.

### Phase 1 data

Keep the `phase1/` tree, the personal configuration, the cache and the models.
Provide a listing of earlier practice sessions from the existing manifests,
reusing the reading of originals, transcripts and review. The
consultation is read-only and does not depend only on `last-attempt.json`.

Do not fabricate categories, criteria, completed sessions or confirmations for
old data. Identify invalid items individually and continue listing the valid
ones. Do not duplicate them in SQLite, move or delete them in this increment.

## 9. Operations and recovery

Rust keeps the authority over session and versions; React presents the state.
Do not block the interface during audio, models or slow data access. Keep a
single local model work and a single capture at a time.

Every result and audio event is associated with the session, question,
recording and operation that originated it, including generation of the
device/helper when needed. An old event, even without finalized audio, cannot
cancel a new capture nor remove its finish action.

| Failure | Required recovery |
| --- | --- |
| Repeated or incompatible request | Return the current state or a busy error without blocking and without duplicating effects. |
| Interrupted microphone | Recover what can be finalized, warn about incompleteness and allow a new capture. |
| Failed transcription or blocked helper | Preserve audio/review; free the operation and allow repeating. |
| Synthesis unavailable | Keep the question readable and allow answering without synthesized voice. |
| Failed write of revision or confirmation | Preserve the text, do not confirm and do not advance. |
| Failed advancing | Keep the confirmation and the persisted position, allowing a safe repeat. |
| Restart after confirmation | Recover the confirmation without duplicating it; keep the advancing still pending. |
| Late result or result of an old version | Do not modify the current selection, confirmation or progress. |
| Missing audio | Show the unavailability of that audio and keep texts and history. |
| SQLite unavailable | Do not start or confirm sessions as if they were stored; allow repeating the opening. |

Reuse the existing audio deadlines and limits, unless a justified change is made
in the plan. Do not truncate audio or text silently. There are no network
requests, API keys or AI costs in this phase; do not present zero usage as a
measurement of a provider that was not called.

## 10. Verification and acceptance criteria

The technical plan must test the transitions and persistence in the core, the
navigation and the editing protections in the interface and the integration
with the local adapters. Use temporary data and controlled failures to test
interruptions between audio publication, confirmation and advancing. Preserve
the phase 1 regression.

The following criteria remain the acceptance criteria. On 2026-11-02 the
implementation and the automated verification were recorded in
[docs/phase-2/validation.md](../../phase-2/validation.md): F2-01, F2-02, F2-04,
F2-06, F2-07, F2-08, F2-09, F2-12, F2-13 and F2-15 have automated evidence;
F2-03, F2-05, F2-10, F2-11, F2-14 and F2-16 have partial evidence and depend on
the manual validation. The boxes below are only marked when the criterion is
demonstrated, including the manual validation.

- [ ] F2-01: Initial bank contains 20 questions in the six categories; repeating the startup does not duplicate nor restore edits.
- [ ] F2-02: Adding and editing text, category, competencies and criteria persists after a restart, with validation of the required fields.
- [ ] F2-03: Initial count three; sessions with one, three and another eligible count; explicit insufficiency, without silent reduction or repetition.
- [ ] F2-04: Selection and order without repetition persist; editing the bank does not change any existing session copy.
- [ ] F2-05: The user completes three questions with voice, manual capture and correct progress; a synthesis failure allows answering from the text.
- [ ] F2-06: Original audio, derivative, automatic results, review and confirmation remain distinct and traceable.
- [ ] F2-07: Saving does not confirm; confirming fixes exactly the text seen; a failure, an old version or a repeated request does not create an undue confirmation.
- [ ] F2-08: It is not possible to advance without confirming, skip by double click or finish before all the confirmations; repeating the advancing is safe.
- [ ] F2-09: A new capture before the confirmation preserves earlier ones; a failure of the new capture does not lose the valid selection. A confirmed answer is not reopened.
- [ ] F2-10: Shutdown and navigation with changes resolve save/discard/cancel and do not lose an edit made during a write.
- [ ] F2-11: Restarting during transcription, after saving a review, after confirming and after advancing recovers the persisted step, without duplications.
- [ ] F2-12: Write/publication failures allow recovery without a new recording; missing references do not delete texts nor block other items.
- [ ] F2-13: Repeated requests do not block the service; old events and results do not affect new recordings or confirmations.
- [ ] F2-14: History allows consulting sessions and listening to originals; phase 1 practices remain consultable without alteration of their files.
- [ ] F2-15: A failed migration preserves the database; a future version is not written; only one session remains in progress.
- [ ] F2-16: With the models prepared, the complete session works offline, without AI calls, fictitious dismissals or invented feedback.

### Manual validation of this phase

On the user's Windows, complete a session of three questions with real answers,
including technical terms. Listen to questions and answers, correct and confirm
texts, leave and resume in the middle and consult the history after finishing.
Also complete a session of one question and verify the insufficiency in a
category. Edit a question after starting the session and confirm that the copy
remains the same. Repeat the offline journey.

Record the duration of the audio files, loading, synthesis, transcription,
waiting experience and any failures. Document commands really executed and
results in `docs/phase-2/` during the implementation, without inventing
anticipated results. The automated tests do not replace this experience.

## 11. What is left for the technical plan

- Rust library for SQLite, version, access without blocking the interface and migrations.
- Physical schema, indexes, restrictions, identifiers and concurrency control.
- Organization of modules, Tauri contracts and adaptation of the audio service to the session.
- File structure and reconciliation of interrupted publications.
- Components, navigation and messages, respecting the flow of this spec.
- Tests of transactions, concurrency, restart and compatibility with the old artifacts.

Do not install dependencies nor start implementation when creating this spec.
Approval of the document allows preparing the technical plan of phase 2 only; it
does not authorize starting the following phases.

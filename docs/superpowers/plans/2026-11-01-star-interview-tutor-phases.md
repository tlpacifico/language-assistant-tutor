# Phase plan — STAR interview tutor

Date: 2026-11-01  
State: division into phases approved; phases 1 and 2 implemented and automatically verified, with the user's manual validation pending in both records; phase 3 brought forward by user authorization; manual acceptance pending; phase 4 implemented and automatically verified, acceptance pending.

## Goal

Progressively build a desktop tutor to practice behavioral interviews in English by voice, with contextual follow-up, STAR feedback and new attempts.

This document organizes the deliveries and the completion criteria. It is not a technical implementation plan: only the next phase will be detailed when we execute it. Later phases will be adjusted with what we learn in the earlier ones.

The [approved MVP design](../specs/2026-11-01-star-interview-tutor-design.md) continues to define the final behavior of the application.

## Technical base

- Interface: React + TypeScript.
- Desktop application and logic: Rust + Tauri.
- Local transcription: faster-whisper in an auxiliary Python process.
- Local speech synthesis: Kokoro-82M in the helper process.
- Textual evaluation: DeepSeek Flash through the OpenCode API.
- Data: SQLite and local audio files.
- First target: Windows.

## Overview

| Phase | Main outcome | Dependency |
| --- | --- | --- |
| 1 — Validate local audio | Listen to a fixed question, answer and review the transcript. | No previous implementation. |
| 2 — Guided interview | Complete a configurable session and recover stored answers. | Local audio validated. |
| 3 — AI tutor | Receive contextual follow-ups and feedback supported by the answers. | Sessions and persistence available. |
| 4 — Improve and repeat | Listen to a rewrite, record another attempt and compare versions. | Textual evaluation available. |
| 5 — Consolidate the MVP | Install and use the application with validated failure recovery. | Complete functional cycle. |

## Phase 1 — Validate local audio

**Goal:** verify the integration of Tauri, Rust, React and the audio models on the user's computer before building the rest of the product.

**Spec:** [Validate local audio](../specs/2026-11-01-phase-1-local-audio-design.md).

**Technical plan:** [Phase 1 implementation](2026-11-01-phase-1-local-audio-implementation.md).

### Deliveries

- Desktop application with a fixed question in English.
- Playback of the question through Kokoro.
- Recording started and ended manually.
- Playback of the recording and transcription through faster-whisper.
- Display and editing of the transcript, preserving the automatic text separately.
- Saving the audio to a local file.
- Indication of processing and errors, without blocking the interface.
- Record of the execution requirements and of the dependencies of the local models.

### Completion criteria

- [ ] The user hears the question in English.
- [ ] Can record, end and listen to their own answer.
- [ ] The transcript is presented and can be corrected.
- [ ] A transcription failure allows repeating the processing of the stored audio.
- [ ] The operation is validated with a few real recordings, including technical terms.
- [ ] The processing time is measured and considered acceptable by the user.

**Left for later:** question bank, complete interviews, SQLite and integration with the AI in the cloud.

## Phase 2 — Guided interview

**Goal:** turn the single-question experience into a practice session with several questions and persistent data.

**Spec:** [Guided interview](../specs/2026-11-02-phase-2-guided-interview-design.md). Defines the flow, the immutable confirmation, the recovery, the compatibility with the phase 1 data and the criteria F2-01 to F2-16.

**Technical plan:** [Phase 2 implementation](2026-11-02-phase-2-guided-interview-implementation.md), with tasks 1 to 9 executed and task 10 partial (manual validation missing).

**State:** complete implementation, automatically verified on 2026-11-02. `.\scripts\verify.ps1` gave 8/8 checks (151 Rust tests, 50 Vitest, 36 pytest without `models`), the real `-m models` test gave `2 passed`, the release build `--no-bundle` and the startup/shutdown of the binary ended with code 0. The matrix F2-01 to F2-16 and the evidence are in [docs/phase-2/validation.md](../../phase-2/validation.md); the manual criteria remain pending and are not taken as complete. Phase 3 was brought forward by explicit authorization of the user, keeping the earlier acceptances pending.

### Deliveries

- Initial bank of about 20 questions, distributed across the design categories.
- Addition and editing of questions, skills and evaluation criteria.
- Configuration of the category and of the number of main questions, with three by default and an option of one.
- Selection and order of the questions fixed at the start of the session.
- Visible progress, for example, “Pergunta 2 de 3”.
- Capture, transcription and confirmation of the answer for each question.
- SQLite for sessions and texts; local files for the recordings.
- Consultation of the sessions and recovery of the answers already stored.

### Completion criteria

- [ ] The user can complete a session with three questions.
- [ ] Can practice a single question or choose another available quantity.
- [ ] The application informs if the category does not have enough questions.
- [ ] Editing the bank does not change questions of sessions already started.
- [ ] Original audio, automatic transcript and confirmed transcript remain distinct.
- [ ] Restarting the application preserves the data already stored and allows recovering the session.

**Left for later:** automatic follow-ups and evaluation by AI. In this phase, the advance between questions is guided by the application, without decisions from the model.

**Progress on 2026-11-02:** the deliveries above are implemented (SQLite with `rusqlite`, editable bank, sessions with fixed copies, artifacts and reconciliation, revisions and immutable confirmations, history and read-only consultation of the phase 1 practice sessions) and the automation is green. The completion criteria remain **unchecked**: they depend on the user's manual validation described in [docs/phase-2/validation.md](../../phase-2/validation.md), section 4. Do not mark the phase as complete before that evidence.

## Phase 3 — AI tutor

**Spec:** [AI tutor](../specs/2026-11-02-phase-3-ai-tutor-design.md).

**Technical plan:** [Phase 3 implementation](2026-11-02-phase-3-ai-tutor-implementation.md).

**State:** implementation brought forward by explicit authorization of the user on 2026-11-02. See [setup](../../phase-3/setup.md) and [validation](../../phase-3/validation.md). The manual acceptances of all the earlier phases remain pending.

**Goal:** add useful follow-ups and specific feedback, preserving the control of the interview in the application.

### Deliveries

- Direct integration with the paid DeepSeek Flash model through the OpenCode API.
- Configurable model and base URL; key protected by the operating system mechanism.
- Sending of text only after the confirmation of the transcript.
- Contextual decision on the need for a follow-up.
- At most one follow-up per main question, with an answer by voice and review of the transcript.
- Feedback at the end, separated between content/STAR and English in the confirmed text.
- Observations supported by real excerpts, distinguishing the initial answer and the follow-up.
- Simple ratings and concrete priorities, without an overall score or pronunciation diagnosis.
- Record of model, token usage and estimated cost per request and session.
- Recovery from API failures without requiring a new recording.

### Completion criteria

- [ ] The model receives the confirmed transcript, without sending the audio.
- [ ] A main question never generates more than one follow-up.
- [ ] The follow-ups do not increase the counter of main questions.
- [ ] The follow-ups clarify real gaps, without repeating information already explained.
- [ ] The feedback cites valid excerpts and accepts qualitative, negative and learning results.
- [ ] Failing a request allows repeating it without losing the answer or duplicating the follow-up.
- [ ] Advancing without a follow-up after a failure requires an explicit waiver; delayed follow-ups never appear.
- [ ] The cost is presented as an estimate and unknown usage is not presented as zero.

**Left for later:** rewrite with audio, comparison of attempts and bank of personal stories.

## Phase 4 — Improve and repeat

**State (2026-11-03):** [spec](../specs/2026-11-03-phase-4-improve-repeat-design.md) and [technical plan](2026-11-03-phase-4-improve-repeat-implementation.md) executed by authorization of the user; implementation automatically verified (8/8 checks: 246 Rust, 97 Vitest, 36 pytest). See [setup](../../phase-4/setup.md) and [validation](../../phase-4/validation.md). **Not accepted**: the manual script F4-16 and the real paid inference are missing; the completion criteria below remain to be validated with the user.

**Goal:** close the learning cycle with concrete suggestions and autonomous practice.

### Deliveries

- Rewrite of the answer with a clearer organization and English suited to the configured level.
- Separation between necessary corrections and optional style suggestions.
- Playback of the suggested version through Kokoro.
- New attempt of a question, with recording, reviewed transcript and its own evaluation.
- Comparison between the initial answer, its follow-up and the new attempts.
- Bank of personal stories with title, factual STAR notes and associated skills.
- Optional selection of a story as support for the preparation.

### Completion criteria

- [ ] The rewrite preserves the facts, without inventing numbers, responsibilities or results.
- [ ] Missing information is flagged, instead of being filled in by the model.
- [ ] The user can hear the suggestion and answer again with their own words.
- [ ] A new attempt does not overwrite or reopen the original interview.
- [ ] The comparison clearly distinguishes what was said, corrected and suggested.
- [ ] The notes of a story are not evaluated as if they had been said in the answer.

**Outcome of this phase:** the functional cycle is available — listen, answer, deepen, receive feedback and try again. The validation of the complete MVP continues in phase 5.

## Phase 5 — Consolidate the MVP

**Goal:** make the complete cycle usable in a normal installation and validate the behavior in the face of real failures.

### Deliveries

- Windows installer with the helper process and the runtime dependencies.
- Setup of the local models with progress indication or selection of existing files.
- Deletion of sessions and of the respective audios, versions and exclusive derived files.
- Review of the recovery after network, model, helper process and restart failures.
- Validation of performance, feedback quality and costs with about ten real answers.
- Installation, configuration and usage instructions.

### Completion criteria

- [ ] The installed application allows recording, transcribing and generating speech without preparing Python manually.
- [ ] After the models are prepared, the local audio functions work without a network.
- [ ] Network failures preserve data and allow resuming the necessary requests.
- [ ] Delayed or repeated results do not unduly change the session.
- [ ] Deletion removes all the data and exclusive files of the session, signaling removal failures.
- [ ] The performance, the real cost and the usefulness of the feedback were evaluated by the user.
- [ ] It is possible to complete a short interview, consult feedback and repeat an answer without losing the earlier versions.

## Rules common to all phases

- Store the real data as soon as each feature starts producing it; do not postpone the necessary persistence to the final phase.
- Protect the key and inform about the sending of external text from the first integration with the API.
- Implement the basic recovery of each component when it enters; phase 5 validates and consolidates the whole.
- Do not present a transcript or textual feedback as an evaluation of pronunciation.
- Do not add other languages, real-time conversation or automatic interruptions in this MVP.
- Validate each phase before advancing and detail only the next phase, avoiding planning the whole implementation in advance.

## Next step

The implementation and the automated verification of phase 1 are recorded in [phase 1 validation](../../phase-1/validation.md); the manual criteria pending there are not considered complete without evidence. The same applies to phase 2: implementation and automation are recorded in [phase 2 validation](../../phase-2/validation.md), with the matrix F2-01 to F2-16 and the list of what remains to be validated with the user. The user authorized bringing phase 3 forward; the implementation and the evidence are in the documents above. The next step is the manual validation of phases 1, 2 and 3 and the exercise of the real paid integration. At the user's request on 2026-11-03, phase 4 was specified, planned and implemented, with green automated verification; the manual acceptance and the real paid inference remain pending and phase 5 was not started. None of this constitutes acceptance of the earlier phases.

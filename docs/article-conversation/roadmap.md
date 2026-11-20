# Article debate and conversation player — roadmap

Date: 2026-11-20.

Status: **proposed roadmap; implementation not started**. This document captures
the improvements discussed with the user in this chat. The user requested the
roadmap; this is not approval of a detailed implementation design, manual
acceptance of existing features, or authorization to start phase 5.

## Outcomes and boundaries

- Make listening, answering and following the conversation feel continuous.
- Develop a position through reasons, counterarguments and a conclusion.
- Turn specific feedback into a new spoken attempt, preserving the original.
- Distinguish recognition corrections, assisted practice and independent answers.

Keep React/TypeScript, Rust/Tauri, SQLite, faster-whisper, Kokoro and OpenCode Go.
Keep audio local, recording explicitly started/stopped by the user, per-session
authorization for automatic text sending, immutable sources and recoverable
operations. UI and explanations remain in European Portuguese; tutor dialogue,
quotes and practice examples remain in English. Do not infer pronunciation,
acoustic fluency or emotional confidence from transcripts.

This is a product roadmap, not a technical plan for every milestone. Specify
contracts, migrations and implementation tasks only for the next selected
milestone. There are no delivery-date or performance promises before measurement.

## Starting point

The existing implementation provides short tutor comments and questions,
incremental voice blocks, automatic transcript sending, four feedback dimensions
with validated quotes, and post-session text correction in preserved branches.
The current UI exposes only the active branch. Spoken retries in this mode,
mid-conversation correction and general per-message playback controls are gaps.
Priority improvements in reports are currently free-form strings.

Real microphone use, voice continuity, paid inference, semantic usefulness and
visual acceptance remain pending in the [validation record](validation.md).
Existing instrumentation and the real-recording benchmark are documented in
[performance](performance.md). No new measurements were performed for this roadmap.

## Delivery sequence

All milestones below are **proposed / not started**. Dependencies describe the
recommended delivery order, not permission to implement subsequent work.

| Milestone | Priority | Outcome | Depends on |
| --- | --- | --- | --- |
| R0 — Observe the current experience | First | Reproducible audio, dialogue and usability baseline | Existing build and real-use prerequisites |
| R1 — Conversation controls and chat | High | A predictable, accessible listen/respond loop | R0 observations; independent UI work need not wait for provider access |
| R2 — Debate progression | High | Contextual challenge with appropriate language and purpose | R0 dialogue observations |
| R3 — Corrections and visible history | High | Repair recognition errors without losing context or sources | R1 interaction model |
| R4 — Feedback and spoken practice | High | Apply one concrete improvement and compare attempts | R2 and R3 |
| R5 — Voice continuity tuning | Measurement-led | Reduce actual waiting and audible gaps | R0 measurements and R1 player lifecycle |
| R6 — Optional support and progress | Later | Supported practice and evidence-based longitudinal review | R2 and R4 |

Recommended first implementation slice after the baseline: R1. R2 follows as the
first pedagogical improvement. Bring R5 forward if measurements identify audio
waiting as the main obstacle to completing a conversation.

R1 now has a [proposed specification](../superpowers/specs/2026-11-20-article-player-design.md)
dated 2026-11-20 and an [implementation plan](../superpowers/plans/2026-11-20-article-player-implementation.md)
for the next milestone only. Specification and planning do not change its
implementation status.

## R0 — Observe the current experience

Run a real conversation with at least five answers, including disagreement,
elaboration and a technical term. Observe a recognition correction, voice
interruption, network failure and resumption. Use explicitly authorized provider
access for paid inference; record unavailable checks as pending.

Measure cold and warm transcription, provider response, time to first audible
sound, gaps between voice blocks and the full stop-recording-to-tutor-audio wait.
Separate existing diagnostic timings from any additional measurements needed.
Capture durations and stable codes without article text, transcripts or audio
in logs. Assess transcript fidelity by listening and comparing locally.

Exit evidence: reproducible scenario, observed timings and usability problems in
the validation/performance documents; pending checks remain explicit. Agree
performance targets from the baseline rather than inventing a latency budget.

## R1 — Conversation controls and chat

- Persistent bottom control bar with stable placement, visible state and one
  primary action: listen/respond, record, stop and send, or processing status.
- **Respond now** stops tutor playback and starts recording through that explicit
  click. Prevent residual playback from entering the recording; cancellation of
  synthesis must not silently discard a user action.
- Recording duration and a real microphone-level indicator. Do not use decorative
  waveforms as evidence of microphone input.
- Per-message play, pause, resume and restart for tutor audio; original-recording
  playback for user turns. Only one recording plays at a time. Reuse available
  local audio; missing voice can be synthesized without another text-model call.
- Show a saved-turn placeholder while transcribing, then its transcript. Show
  validated tutor text as soon as available, independently of voice preparation.
- Auto-scroll only when the user is near the bottom; otherwise preserve reading
  position and offer a new-message indicator. Keep the latest question accessible.
- Optional article side panel on wide windows and a collapsible view on narrow
  windows, without hiding the conversation controls.
- Locally saved autoplay and voice-speed preferences; discoverable keyboard
  controls that do not activate recording inside text inputs or inactive views.
- Specific recovery actions for sending, voice and feedback instead of a generic
  retry label. Voice failure must allow continuing with text.

Exit evidence: keyboard and narrow-window checks; real playback/recording handoff;
no overlapping audio, duplicate submissions or accidental recording; preserved
scroll position; no autoplay or external request merely from reopening a session.
Playback progress must distinguish buffered audio from a known complete duration.

## R2 — Debate progression

Offer three purposes: explore the topic, challenge my position, and simulate a
professional discussion. Choose English complexity separately from the depth of
the argument; this is a preference, not a proficiency assessment.

Guide a flexible progression through understanding, position, reasons,
counterargument and conclusion. Follow the user's actual answer before changing
topic, ask one question at a time, and avoid automatic agreement or obligatory
disagreement. Distinguish article claims, user opinions and unsupported claims.
Allow a natural concluding response before feedback without forcing a rigid script.

Exit evidence: real sample debates demonstrate continuity, a relevant
counterargument and an opportunity to qualify or defend a position. Feedback must
not reward agreement with the tutor. Preserve short interventions and the absence
of unsolicited grammar correction between turns.

## R3 — Corrections and visible history

Add an optional mid-conversation **That is not what I said** path. Separate
**Correct transcript** (recognition repair) from **Improve answer** (new practice).
Do not introduce mandatory review into every turn.

Correction preserves the original audio, automatic transcript, sent version and
responses. Regeneration is explicit and may consume API usage. Later turns remain
with their original context; pending results cannot be applied to a corrected
branch. Expose previous branches and reports as readable history, with clear source
and active-branch labels. A recognition repair is not evidence of improved English.

Exit evidence: correct a negation and a technical term during real use; verify
branch history, interrupted requests and reopening without silent regeneration.

## R4 — Feedback and spoken practice

Keep the four evidence-backed dimensions and at most three priorities. Structure
each priority around a real quote, a Portuguese explanation, an English alternative
that preserves meaning, and a short exercise. Separate necessary corrections from
optional style changes; validate quote/source references for improvements as well
as dimension evidence.

Add **Practise this answer** from feedback: present the original question, focus
on one improvement and record a distinct attempt. Compare clarity, supporting
reasons and handling of the counterargument with excerpts from both attempts.
Keep the original conversation and report intact. Suggested wording must not
invent facts or be attributed to the user's original answer.

Exit evidence: complete feedback → spoken retry → comparison → reopen; show a
concrete supported difference or explicitly state that improvement is not evident.
Check preservation, failed requests and fidelity with real audio and inference.

## R5 — Voice continuity tuning

Use R0 data to test block boundaries, a small playback buffer and cancellation
behaviour. Balance time to first sound against pauses between blocks. Prioritize
the next transcription over remaining background voice work. Reuse generated
audio and keep the text usable after partial voice failure.

Compare Whisper decoding settings only with the same real recordings and a local
human reference; retain the current baseline until fidelity supports a change.
Do not assume that streaming text, a different engine or concurrency is necessary.

Exit evidence: before/after measurements on comparable recordings and real
listening, including cold/warm runs, interruption and partial failure. Report
tradeoffs and regressions; do not claim gains from automated tests alone.

## R6 — Optional support and progress

- On-demand question rephrasing, a small hint or useful expressions without
  supplying the full answer. Preserve the original question and record assistance.
- One optional session objective, such as supporting opinions with examples.
- Local history showing concrete examples related to that objective across
  comparable attempts, distinguishing assisted and independent practice.

Exit evidence: support remains optional, does not overwrite the user's response,
and is reflected honestly in feedback. Progress claims cite actual attempts;
there is no global proficiency score. Start with Articles; reuse in STAR requires
a separately scoped design that preserves interview rules.

## Verification and documentation policy

For each implemented milestone, record automated verification separately from
manual acceptance. Test relevant state transitions, preserved versions, evidence,
late results, retries and recovery. Use the existing documented verification
commands; validate the packaged application if distribution is changed.

Update the affected design, setup, validation and performance documents when
behaviour changes. Do not mark previous phases accepted or alter their pending
manual criteria through this roadmap. The present change is documentation only.

## References

- [Current setup](setup.md), [validation](validation.md) and [performance](performance.md)
- [Approved article design](../superpowers/specs/2026-11-04-article-conversation-design.md)
- [Original implementation plan](../superpowers/plans/2026-11-04-article-conversation.md)
- [Product goals](../idea-and-goals.md) and [technical stack](../tech-stack.md)
- [Layout validation](../layout/validation.md)
- [Local diagnostics](../diagnostics/validation.md)

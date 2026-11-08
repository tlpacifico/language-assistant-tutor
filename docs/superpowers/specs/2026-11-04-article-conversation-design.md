# Article conversation — proposed design

Date: 2026-11-04. The functional direction and the automatic sending after finishing each answer were accepted by the user. Design explicitly approved on 2026-11-04 («aprovo»). The [technical plan](../plans/2026-11-04-article-conversation.md) makes the implementation concrete, not yet started. It does not start phase 5 nor change the acceptance of the previous phases.

## Goal and decisions

Practice the ability to converse in English about an article, keeping cohesion, coherence and development of ideas. The tutor follows the reasoning without interrupting the speech to correct. Processing pauses should be reduced; simultaneous conversation and zero latency are not promised.

Reading the article is preparation, with optional reading aloud and without pronunciation scoring. The focus is the conversation. Interface and explanations in European Portuguese; questions, comments and practice answers in English.

## Approaches considered

- Mandatory review of the transcript in each turn: keeps the current path, but introduces the pause the user wants to avoid.
- Turn-based conversation, with sending authorized at the start and final feedback: chosen direction. The user ends the recording and the application transcribes and sends without intermediate confirmation.
- Simultaneous conversation with automatic speech end detection: greater complexity, risk of interrupting natural pauses and change of the recording rules. It is outside this design.

React/TypeScript, Rust/Tauri, SQLite, faster-whisper, Kokoro and OpenCode Go are kept. No new provider or engine is introduced.

## Path

1. Paste the text of an article in English, with optional title. The first version does not import URLs or PDF. Store a fixed copy for the session.
2. Read the article at one's own pace. Listening to excerpts through Kokoro can reuse the voice mechanism; it is not a requirement to start the conversation.
3. Explicitly authorize, per session, the sending of the article and of the transcripts to the configured destination. Explain that ending the recording triggers the sending and that the audio remains local. Without authorization, do not start the external conversation.
4. The tutor asks an open question about the article. The user starts and ends the answer manually.
5. Store audio and transcript before the external request. Fix a send version, marked as a transcript sent automatically, never as text reviewed by the user.
6. The tutor produces a brief comment linked to the answer and a deepening question. Store and validate the text before the voice. Play automatically with the option to stop; the following recording starts only by user action.
7. Repeat until the user chooses to finish; generate final feedback about the available turns. Proposed pace: suggest ending after five answers, without cutting speech nor imposing that number.

There are no grammar corrections between turns. The tutor can ask for clarification when it does not understand an idea. It must not agree automatically: it distinguishes facts from the article, the user's opinion and information not supported by the text.

## Conversation and feedback

Each intervention follows the previous answer and the accumulated context. Proposal: a comment of one or two sentences and a question, avoiding lists of questions and long expositions. Progression can go through comprehension, summary, opinion, examples and application to personal experience, without a rigid script.

The final feedback evaluates separately:

- Coherence: relevance, continuity of the central idea and contradictions.
- Cohesion: relations between sentences, connectors and comprehensible references.
- Development: reasons, examples and support of opinions.
- Interaction: response to the tutor's comment and ability to deepen.

Each observation quotes real turns and excerpts. Choose two or three priority English improvements; distinguish correction from style suggestion. Do not apply STAR to this mode, do not use notes from the article as if they had been said and do not assign a global proficiency rating. A short session may have insufficient evidence.

Do not evaluate pronunciation, rhythm, hesitations or acoustic fluency from the transcript. Natural pauses are not errors. Repeating an answer is a new attempt, preserving the original and the previous comments.

## Consent, versions and recovery

This is a mode distinct from the STAR interview. The mandatory review and confirmation of the existing modes remain. The existing consent about the destination remains necessary, added to the authorization for automatic sending in this session. Changing the destination or revoking the authorization prevents new sends until a new explicit authorization.

The user can consult the transcript and choose «Corrigir transcrição». Store a new version without deleting audio, automatic transcript, sent version or tutor response. Correcting recognition is not equivalent to retroactively improving the English actually said.

Proposal for simple recovery: suspend progress, invalidate pending requests of the old version and allow regenerating the tutor intervention from the correction. If later turns already exist, keep the old sequence as a historical branch and resume from the corrected turn; never reassign later answers to the new context. The report identifies the branch used and keeps the sources. Regeneration is an explicit action that can consume API again.

An empty or failed transcript is not sent: allow repeating the transcription or entering a manual review. A network failure keeps data and allows repeating the request explicitly. A voice failure allows reading and continuing or repeating only the synthesis. Resuming a session does not repeat requests nor play voice automatically.

Results associate with the session, the branch, the turn, the version and the operation; late results do not advance the conversation. Closing during a request prevents a late question; the report uses only persisted turns and the context version fixed for that operation. The core controls the state, not the LLM.

The article is untrusted content, never a source of instructions for the system. Define explicit article and context limits in the plan, with validation before sending, without silent truncation. On reaching the conversation limit, propose ending and generate the report before accepting another turn. Store the exact context of each request for local traceability, outside the logs.

Store the key in protected storage, known usage and unknown cost when not provided. Diagnostics remain without free text, audio or interview content; new operations require updating the closed lists. Deleting the session also deletes the article and exclusive associated artifacts.

## Performance

Existing base: resident models with preload, Whisper small on CPU/int8 with fixed English and VAD, voice cache and sequential execution of local jobs. The helper currently delivers the voice after joining all blocks.

Proposed priorities for the plan:

1. Measure separately stop→transcription, send→validated text and text→first sound, distinguishing cold and loaded models. Use real short and long recordings, with technical terms; record fidelity, duration and memory, without content in the diagnostics.
2. Compare Whisper decoding parameters, including beam_size, on the same recordings. Only adopt a faster configuration if the loss of fidelity is acceptable in manual validation.
3. Deliver and play voice blocks incrementally after validating the tutor text, keeping the order, cancellation and partial failure handling. Do not speak text of an AI answer not yet validated.
4. Use short interventions to reduce generation and listening time. Early voice preparation only for already known text; do not guess the next question while the user answers.

Background audio cannot delay the following transcription. Concurrency, GPU and transcription during recording are left for measurement and later decision, they are not requirements of this first version. Target latency values will be fixed after the baseline measurement; there are no measured gains in this proposal.

## Verification and scope

The plan must cover per-session consent and revocation, source immutability, mode isolation, branching by correction, idempotency, late results, resumption, transcription/network/voice failures, context limits, deletion and unknown usage. Test that the article cannot change the tutor instructions and that the feedback quotes the user instead of attributing the article content to them.

Manual acceptance requires a real conversation about an article with at least five answers, including disagreement and deepening; evaluate continuity, dialogue cohesion, feedback usefulness, transcript fidelity and perceived wait. Real inference and real voice are not replaced by automatic tests.

This design does not include document import, pronunciation evaluation, spaced repetition vocabulary, debate with multiple characters or automatic change of the existing interviews. The later plan defines schema, commands, concrete limits and implementation sequence only of this scope.

## Local references

- [Current goals](../../idea-and-goals.md)
- [Stack and audio](../../tech-stack.md)
- [Current per-question evaluation](../../ai-coaching/validation.md)
- [Local diagnostics](../../diagnostics/validation.md)
- [OpenCode Go integration](../../opencode-go/validation.md)

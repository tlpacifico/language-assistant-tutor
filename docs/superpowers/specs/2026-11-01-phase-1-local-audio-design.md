# Phase 1 spec — Validate local audio

Date: 2026-11-01  
Status: accepted as reference for the technical plan; phase 1 implemented and automatically verified, with the manual validation of the user pending.

References: [phase plan](../plans/2026-11-01-star-interview-tutor-phases.md), [approved design of the MVP](2026-11-01-star-interview-tutor-design.md) and [technical stack](../../tech-stack.md).

Detailed execution in the [technical plan of phase 1](../plans/2026-11-01-phase-1-local-audio-implementation.md).

## 1. Objective

Validate, in a Tauri application for Windows, the complete local audio journey: listen to a fixed question in English, record an answer, listen to it, obtain a transcript and correct the recognized text.

This phase must reveal the difficulties of capture, playback, local models, communication between Rust and Python and processing time on the user's computer. The result is a functional base for the guided interview of phase 2.

The phase does not evaluate the content of the answer nor the pronunciation. It does not use an AI API, does not ask for credentials of providers and does not send recordings or transcripts to external services.

## 2. Scope and approach

The approach chosen in the design of the project is maintained: React + TypeScript in the interface, Rust + Tauri 2 in the core and an auxiliary Python process for faster-whisper and Kokoro-82M.

This phase does not compare frameworks again nor swap the audio engines. An alternative native integration will only be considered later if the validation demonstrates a concrete limitation.

### Included

- Desktop application executable in development on Windows.
- A fixed question presented in text and reproduced by local voice.
- Manual recording of the answer, playback and stored original file.
- Local transcription and manual review, without changing the automatic version.
- Simple persistence of the artifacts and recovery of the last stored answer.
- Processing states, recoverable errors and measured times.
- Documentation of the requirements and of how to run the real integration.

### Out of this phase

- Editable question bank, categories, question count or interview session.
- Follow-ups, STAR evaluation, English feedback, rewrites and OpenCode.
- SQLite, story bank, history listing and comparison of attempts.
- Pronunciation scoring, live transcription and automatic interruptions.
- Complete device selectors and advanced audio configurations.
- Model download assistant, final installer and support for other operating systems.

## 3. Reference values of this spec

The following values are specification choices to allow a first verifiable implementation; they may be adjusted with the validation, without being presented as preferences previously indicated by the user.

| Element | Initial reference |
| --- | --- |
| Fixed question | `Tell me about a time you solved a difficult problem at work.` |
| Interface and messages | European Portuguese. |
| Speech and transcription | English; transcribe, without translating or improving the answer. |
| Capture and playback | Default devices of Windows. |
| Transcription | faster-whisper, `small` multilingual model, CPU/int8, language set to English. |
| Synthesis | Kokoro-82M with an English voice. The concrete voice is chosen and recorded in the technical plan. |
| Concurrency | One transcription or synthesis job at a time. |
| Answers | One active answer in the interface; each new recording has its own identifier and files. |

The use of CPU is the initial reference, not a guarantee of performance. The phase does not require a GPU. The model may be adjusted if the measurements demonstrate the need, recording the change and its reason.

## 4. User journey

1. Open the application and consult the availability of audio and of the models.
2. Read the fixed question and request its reproduction in English.
3. Start the recording when ready. Clearly show that the microphone is recording and the elapsed time.
4. End the recording by explicit action. Finalize and store the file before starting the transcription.
5. Automatically start the transcription of the finalized recording and indicate the processing.
6. Listen to the stored recording, consult the automatic transcript and edit a review copy.
7. Store the review or start a new answer to the same question.

Listening to the question is optional: it does not prevent recording if the synthesis is unavailable. Transcribing also does not prevent playing back an answer file already stored.

The capture does not depend on the Python process being ready. If the transcriber is unavailable when the recording ends, store the audio and make the transcription available when the component is prepared, without requiring a new capture.

Do not play audio while the microphone is recording. On starting the recording, end any playback in progress. After recording, it is possible to listen again to the question or the answer, one at a time.

A new recording does not overwrite the previous one. It only becomes the active answer after being finalized and successfully stored; a capture attempt that fails does not remove the last valid answer.

## 5. Functional behavior

### 5.1 Preparation and availability

- Distinguish availability of the microphone, playback, transcription and synthesis. The lack of a model must not prevent consulting the texts and files already existing.
- In this phase, prepare the models and the dependencies through documented instructions and local development configuration.
- Show which component is unavailable and which step is necessary to prepare it. Do not present a missing installation as an error of the user's answer.
- Once the models are prepared, run the journey without a network. Do not start silent downloads as a consequence of pressing record or listen.
- Document the downloads necessary for the initial preparation; that preparation does not authorize the sending of training data of the user.

### 5.2 Question by voice

- Generate the real audio of the fixed question with Kokoro, using an English voice.
- Store the generated audio and allow reproducing it again without repeating the synthesis when text, voice and configuration have not changed.
- If the generation fails, keep the question in text and allow repeating the generation.
- Do not silently replace Kokoro with an API, a system voice or an example file. Test duplicates are only used in tests and identified as such.

### 5.3 Capture and playback

- Use the default input of Windows. The concrete library and the management of the formats of the device belong to the technical plan.
- Present explicit actions to start and end. Do not end an answer automatically due to silence.
- Store the original in WAV, recording the effective sample rate, channels, duration and size. Do not replace it with the file converted for transcription.
- Normalize a derivative to 16-bit PCM WAV, mono, 16 kHz when necessary for the transcription. The playback of the answer uses the original.
- Do not cut nor silently discard parts of the answer. A technical limit, if necessary, must be defined in the plan and communicated before recording.
- If there is no device, access or space to store, present a recoverable error and do not indicate that the answer was stored.
- If the device fails during the capture, try to finalize the captured audio; only mark it as recovered if there is a playable file. Indicate that it may be incomplete.

### 5.4 Transcription

- Transcribe exclusively the audio of the answer, without including the voice of the question.
- Store the original before any conversion or inference.
- Keep the automatic transcript as it was returned by the engine, with the model and the configuration used.
- Do not correct grammar, summarize the answer or provide the engine with an ideal answer to try to bring the recognition closer to that text.
- In case of failure, make available an action to repeat the transcription from the stored audio, without a new recording.
- Do not process empty files as valid answers. When there is no recognized speech, show that situation and allow listening to the audio and recording again.
- Treat signs of silence or low confidence as recognition information, not as a diagnosis of pronunciation. Do not promise perfect detection of absence of speech.
- A successful transcription is not overwritten automatically. If it is necessary to process again an answer already transcribed, create another identified result and preserve the previous one.

### 5.5 Review of the text

- Show the automatic transcript as a read-only reference and allow editing a separate copy.
- Initialize the review with the automatic transcript when there is not yet a review of the user. A repetition of the processing never replaces changes already made in that review.
- The editing serves to recover what was said, not to present an evaluation or an improved answer.
- Make available an explicit action to store the review. Distinguish unsaved changes, recording in progress and stored review.
- When switching to another answer or closing the application normally, store the changes or allow the user to decide whether they want to store them, discard them or continue editing. A write failure keeps the changes in the interface and cannot be presented as success.
- In this phase, storing a review is not equivalent to confirming an answer for evaluation. The immutable confirmation of the interview is introduced in phase 2; there is no sending to the AI in phase 1.

## 6. Technical responsibilities

| Component | Responsibility in this phase |
| --- | --- |
| React/TypeScript | Question, actions, state of the capture, playback, automatic text, review and messages. |
| Rust/Tauri | State of the operation, capture, files, persistence, communication with Python and events for the interface. |
| Python process | Load models, transcribe, synthesize and return results or structured errors. |

Rust is the authority over the active answer and the work in progress. Repeated clicks do not start several recordings or transcriptions for the same operation. The interface prevents incompatible operations and the core validates the same rule.

The auxiliary process uses JSON messages delimited by line in standard input/output, with request identifier. The diagnostic logs go to stderr; the audio circulates as files, not as base64 content in the messages.

The contract distinguishes availability, transcription, synthesis and shutdown. The exact schemas and the Tauri commands are defined in the technical plan. Paths and identifiers are resolved by the core from artifacts of the application.

The models may be loaded only when necessary and reused. The interface distinguishes initial loading from execution of a transcription or synthesis. The heavy work does not block the window nor the processing of events.

## 7. Files and minimum persistence

Use the local data folder of the application, obtained through Tauri. Do not write recordings in the source code tree or depend on the directory from which the application was started.

For each answer, keep:

- Unique identifier and question used.
- Original audio and, when it exists, normalized derivative.
- Metadata of capture and dates.
- Automatic results of transcription, with identifier, model and configuration.
- Stored manual review, associated with the result that originated it.
- State and error of the last operation, when applicable.
- Measurements of the duration of the processing.

In this phase, these metadata and texts may use JSON, with atomic writing to avoid replacing a valid file with an incomplete write. It is not necessary to build a database or a listing of history.

Keep a reference to the last answer successfully finalized. On reopening the application, recover its files and the review already stored. Other files of previous answers remain preserved, even without navigation between them in the interface.

A reference to a missing or invalid file must produce explicit information, without deleting the remaining artifacts nor blocking the possibility of recording another answer. The last stored state of an interrupted work becomes recoverable, it does not stay indefinitely as processing in progress.

Do not promise recovery of a recording that was never finalized or of text changes that were not yet stored before an unexpected failure.

## 8. States and recovery

| Situation | Expected behavior |
| --- | --- |
| Models in preparation or loading | Indicate the component busy; allow consulting contents already stored. |
| Active capture | Show state and duration; allow ending; prevent playbacks and another capture. |
| Original to store | Only transcribe after confirming that the file was finalized. |
| Transcription in progress | Indicate processing and the corresponding answer; do not start another model job. |
| Text available | Allow listening to the recording, editing and storing the review. |
| Transcription failure | Keep the original and the existing review; allow repeating the processing. |
| Synthesis failure | Keep the text of the question; allow repeating the generation. The capture remains available when there is no incompatible operation in progress. |
| Python process terminated | Mark the request in progress as failed, keep files and allow restarting the helper to repeat the operation. |
| Write error | Do not signal as stored; keep data recoverable and allow repeating. |

A blocked operation must reach a deadline defined and documented in the technical plan, without leaving the application permanently busy. The initial loading time may have a different deadline from the processing of a request.

Accept results only for the operation identifier and the answer that originated them. After a timeout, failure or restart of the helper, a delayed result does not replace the active answer nor the revised text.

During the capture or a model job, do not start a new recording or another model job. The user may continue to consult the interface and, when there is audio already stored and they are not recording, listen to it. On closing normally, end the capture in progress and store what is finalized, resolve reviews to store and shut down the helper. An unexpected failure follows the recovery limits of section 7.

## 9. Validation and measurements

### Automated verification

The technical plan must foresee verifications of the behaviors with a risk of loss of work: separation between automatic text and review, persistence, repetition after error, exclusion of incompatible operations and rejection of delayed results.

The tests may use adapters and simulated results. The conclusion of the phase also requires the real integration of microphone, faster-whisper and Kokoro.

### Manual validation with real data

Use at least three recordings: a short answer, an answer with professional or technical terms and a problematic recording, such as silence or interruption of the device. Add cases according to the problems found.

Record the hardware used, relevant versions, model, voice and configurations. Measure separately the loading of the models, the synthesis of the question and the transcription of each recording; distinguish first use from repetitions and record the duration of the audio transcribed.

Do not fix a latency promise before measuring. The user validates whether the wait allows practicing comfortably. If it does not allow it, document the limitation and the proposed change before marking the phase as concluded.

Document the Windows/Rust/Node/Python requirements and the native dependencies effectively used, together with the real installation, execution and verification commands. Do not declare minimum hardware requirements without measuring them.

## 10. Acceptance criteria

- [ ] CA-01: The application opens on Windows and presents the correct fixed question.
- [ ] CA-02: Kokoro generates English voice for that question and the user can listen to it again.
- [ ] CA-03: The user starts and ends a recording manually, with state and duration visible.
- [ ] CA-04: There is no playback during the capture nor duplicated operations due to repeated clicks.
- [ ] CA-05: The finalized audio is stored and can be played back, even when the transcription fails.
- [ ] CA-06: faster-whisper transcribes a real answer in English, without translation or language evaluation.
- [ ] CA-07: The automatic text is preserved and the review can be stored separately.
- [ ] CA-08: Repeating a failed transcription does not require recording again; later results do not erase existing reviews.
- [ ] CA-09: A new recording and the restart of the application do not erase the last finalized answer nor the stored review.
- [ ] CA-10: Empty audio, missing files, absence of device and absence of recognized speech have explicit and recoverable behavior.
- [ ] CA-11: A failure or block of the helper allows recovering control and repeating the operation, without applying delayed results.
- [ ] CA-12: The interface remains usable during loading and processing of the models.
- [ ] CA-13: With the models prepared, the journey works without a network and does not make calls to AI services.
- [ ] CA-14: The real recordings and the measured times are recorded, and the user considers the wait acceptable.
- [ ] CA-15: There are verified instructions to prepare and run the integration in the Windows development environment.

The spec being written does not mean that the criteria are met. The phase only ends after the implementation, the technical verification and the manual validation; it does not automatically start phase 2.

## 11. Decisions left for the technical plan

- Rust libraries for capture, playback, WAV and resampling.
- Versions of tools and dependencies; development, build and test commands.
- Configuration of models, choice of the English voice and location of its files.
- Organization of the modules, Tauri commands, events and JSON schemas of the helper.
- Concrete names of the persistence files, atomic writing mechanisms and formats of the metadata.
- Deadlines and necessary technical audio limits, with clear messages and without silent truncation.

These decisions must respect the behavior of this spec. It is not necessary to decide now the contracts of the sessions, the SQLite schema or the integration with the API of phase 3.

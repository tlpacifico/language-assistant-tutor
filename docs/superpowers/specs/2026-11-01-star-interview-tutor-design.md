# STAR interview tutor — MVP design

Date: 2026-11-01  
Status: design approved by the user; execution organized in the [phase plan](../plans/2026-11-01-star-interview-tutor-phases.md).  
Stack chosen by the user: Rust + React + Tauri.

## 1. Intention

Create a desktop application for personal use to practice behavioral interviews in English by voice. The user must be able to tell their professional experiences clearly, organize answers according to STAR and improve their English through concrete suggestions and new attempts.

The main cycle is: listen to a question, answer by voice, review the transcript, answer a follow-up when necessary, consult feedback and repeat an answer.

Recordings and history stay on the computer. The textual evaluation uses a model in the cloud through the OpenCode API. There is no automatic evaluation of pronunciation in this MVP.

## 2. Decisions and technical proposals

### User decisions

- Desktop interface with Rust, React and Tauri.
- Practice in English with the STAR structure, an editable question bank and personal professional stories.
- Configurable number of main questions; initial suggestion of three.
- Recording started and ended manually, without automatic interruptions.
- Local transcription with faster-whisper and local voice with Kokoro-82M.
- Evaluation and follow-ups with DeepSeek Flash through OpenCode, with a configurable model.
- Review of the transcript before sending; feedback at the end of the interview.
- At most one follow-up per main question, without counting it in the configured total.
- Local history, preservation of versions, new attempts and deletion of the sessions with the respective audio files.
- Feedback with evidence, without an overall score and without inventing facts.

### Implementation proposals of this design

- Tauri 2, React with TypeScript and Vite; a production web server is not necessary.
- Windows as the first target of development and distribution, given the current environment. Support for macOS and Linux is left for later validation.
- Portuguese in the interface and in the explanations; questions, follow-ups, suggested answers and their respective voice in English.
- An auxiliary Python process, started by the application, for transcription and synthesis. Avoid a simultaneous migration of the audio engines to Rust.
- Rust as the authority over the session state, SQLite, files, credentials and API requests.
- Configurable English level to guide rewrites; B1 as the initial value, editable, without presenting it as a diagnosis of the user's level.
- One active session at a time. The local audio jobs are processed sequentially in the MVP.
- After confirmation, the transcript of that answer stays fixed. New attempts are independent and do not reopen decisions of the original interview.
- On the failure of a follow-up request, it is necessary to repeat the request or explicitly dismiss the follow-up before advancing. Do not present delayed follow-ups out of the order of the interview.

These proposals may be adjusted in the design review; they are not presented as preferences already expressed by the user.

## 3. Approaches considered

| Approach | Advantages in this project | Trade-offs | Decision |
| --- | --- | --- | --- |
| Python + PySide6 | Direct integration with the Python engines; concentrates the application in a single language. | Interface in the Qt ecosystem. | Considered, not chosen. |
| Electron + React | Web interface and Node.js/TypeScript core; suitable for chat and textual review. | Distribution of Chromium and additional integration with the audio process. | Considered, not chosen. |
| .NET with WPF or Avalonia | Allows a C# application; can call Python or other transcription engines. | Additional choice of toolkit; WPF limited to Windows. | Considered, not chosen. |
| Tauri + React + Python process | React interface, Rust core and maintenance of the proposed audio engines. | Three languages and packaging of the auxiliary process; testing the integration with the WebView. | Rust/React/Tauri base chosen; the Python process is the initial technical proposal. |

Tauri uses the platform WebView. The size and the usage of the complete application will also depend on the local models; a performance gain in inference is not assumed merely by using Rust.

## 4. Scope and exclusions

### Included

- Configuration of category, number of questions, level intended for the suggestions and voice in English.
- Initial bank of about 20 questions, with editing and adding by the user.
- Simple bank of personal stories, with title, factual STAR notes and associated skills.
- Spoken questions, capture and playback of answers, transcription and manual review.
- One contextual follow-up when useful, final feedback and rewrite with audio.
- New attempts of an answer, with comparison and specific feedback on the attempt.
- History, token usage, estimated cost and recovery of requests that failed.
- Configuration of the API key and preparation of the local models.

### Out of the MVP

- Pronunciation diagnosis, scoring by phoneme, acoustic evaluation of fluency or inference of emotional confidence.
- Interruptions during the answer, simultaneous conversation or live transcription.
- Other training languages, synchronization, accounts, collaboration and mobile application.
- Model training, vector search, agents with tools or unlimited autonomous selection of questions.
- Certified evaluation of proficiency or guarantee of accuracy of the classifications.

A correct transcript does not prove correct pronunciation. The LLM receives text and must not claim that it heard or evaluated the oral delivery.

## 5. Experience and session rules

### Preparation

The user chooses a category and the number of main questions. The initial value is three, and it is possible to choose one or another number up to the number of existing eligible questions. The application informs when there are not enough questions for the intended number, without silently repeating the same question.

On starting, Rust fixes the selection and the order of the questions and stores a snapshot of the respective texts and criteria. Later changes in the bank do not modify an existing interview. The interface presents the progress of the main questions.

### Question and answer

1. Present the text and synthesize the question in English; allow listening again.
2. Start the recording only by user action; end it by explicit action. The question is not played while the microphone is recording.
3. Store the audio before starting the transcription. A transcription failure allows repeating the processing of the same file.
4. Store the automatic transcript as an immutable version and create an editable revision. Allow listening to the recording during the review.
5. Only send the answer to the API when the user confirms the revised transcript. This act confirms the text to be evaluated, it does not improve or replace the original answer.

The revised transcript remains editable until confirmation. Once confirmed, that version is read-only in the MVP. To improve or rectify an answer already confirmed, the user starts an independent new attempt from the question; the follow-up decisions and the report of the original interview keep the versions and evidence of origin. A new attempt does not change the current answer of an interview in progress.

Empty audio, silence without recognized speech or an empty transcript require a new recording or an explicit review; they are not sent as a valid answer nor classified as bad pronunciation.

### Follow-up

The LLM receives the question, the criteria, the revised answer and the relevant textual context of the session. It recommends continuing or asking a single follow-up question linked to a real gap. Rust validates the recommendation and applies the limit.

The follow-up must clarify content, not interrupt the interview to teach grammar. If the answer is already sufficient, the application advances. If the generation fails, the session keeps the state and allows repeating the request or explicitly advancing without a follow-up. Advancing records the follow-up as dismissed by the user due to unavailability of the service, instead of stating that the AI considered the answer complete. That decision closes the follow-up step of that question; it does not reopen it when the network returns. While it waits for a decision or an explicit dismissal, the session does not advance to the next main question.

An answer to the follow-up goes through the same capture and review, but never originates another follow-up. At most, a session with N main questions has 2N questions in total.

### End and new attempts

After the last confirmed answer and the decision on the last follow-up, the interview moves to waiting for feedback. The application may request a report per main question with the respective follow-up and aggregate the feedback of the session; the internal format of the requests is left for the plan.

In the history, the user chooses a question to repeat. They may consult the feedback, listen to the rewrite and record another answer. Each attempt is stored separately, receives a textual evaluation according to the same criteria and may be compared with the initial answer. The original report remains unchanged. The new attempts are individual exercises without new automatic follow-ups in the MVP.

## 6. Questions, stories and context

The initial bank covers conflicts and collaboration, leadership and initiative, failures and learnings, problem solving, priorities and deadlines and adaptation to changes.

Each question stores text in English, category, skills and criteria. Examples are optional and illustrative; they do not constitute mandatory answers.

Each story stores title, factual notes for situation, responsibility, actions and result or learning, in addition to skills. The user may select a story as support for a question; the association is optional. Automatic search among all the stories is not necessary.

In the evaluation, the notes of the story are not provided as part of the spoken answer. The evaluation uses exclusively the question and the revised texts effectively answered. This avoids attributing to the user information that they did not explain.

To rewrite an answer, the MVP uses only the facts present in that answer and in its follow-up. The stored notes help in the preparation, but information that exists only in those notes appears as a suggestion of something the user may add in a future attempt, never as something they already said.

The context of the calls is limited to what is necessary: criteria of the question, revised texts, versions of the rubric and, when relevant for the follow-up, a factual summary of the previous answers. Do not send audio nor the entire bank of stories. The current question has priority over references to previous questions.

## 7. Feedback contract

Separate content/STAR from English in the revised transcript.

- Content: relevance, situation, individual responsibility, actions, result and learning.
- English: grammar, vocabulary, clarity, organization and textual naturalness.
- Use clear, partial and not identified when applicable, with explanations. Do not create an overall score.
- Associate each factual observation with a quotation and with the identifier of the answer or follow-up from which it was taken.
- Rust validates that quotations are found in the version of the text evaluated. Reject observations without valid evidence instead of presenting them as confirmed.
- Distinguish what was present in the initial answer from what only appeared in the follow-up.
- Accept qualitative results, negative results and learnings. Do not require numeric metrics.
- Present strengths, gaps and three concrete priorities for the next practice session, without inventing defects to complete the list. If there are only one or two supported priorities, present only those and explain that no others were identified.
- Separate necessary language corrections from optional style alternatives.

The rewrite keeps the facts, the responsibility and the original degree of certainty. It does not invent numbers, successes or experiences. Missing information is marked outside the text for reproduction by voice, so as not to produce artificial sentences with spoken markers.

The outputs of the LLM follow a structured schema validated in Rust. Content of answers and stories is treated as material to be evaluated, not as instructions to change the rubric or the rules of the session. Invalid outputs may be requested again; previous versions are not overwritten.

## 8. Architecture

### React/TypeScript interface

It presents configurations, question bank, stories, interview, transcript review, feedback, comparison of attempts and history. It calls Tauri commands and receives progress events. It does not contain the API key nor decide persistent transitions of the session.

### Rust/Tauri core

- Manages the session state, progress, follow-up limit and pending operations.
- Accesses SQLite and the files in the application data folder.
- Captures audio through a local adapter and provides files to the transcription process. The concrete library is a decision of the plan, validated in the Windows integration.
- Manages credentials through the protected storage of the operating system, outside the content database and the logs.
- Makes HTTP calls to OpenCode with a deadline, validation of the response and recording of usage.
- Starts, monitors and terminates the Python process. Audio and network jobs do not block the interface.

### Python audio process

Runs transcription with faster-whisper and synthesis with Kokoro-82M. Keeps models loaded when possible; processes requests sequentially to limit concurrency and make recovery easier.

It communicates with Rust through JSON messages delimited by line through standard input/output, with request identifier, operation, parameters, progress, result and error. The diagnostic logs go to stderr. The audio files are local files, referenced by paths controlled by the core; they are not transported in base64 through this channel.

The contract exposes only availability state, transcription, synthesis and shutdown. The communication does not need to open a local HTTP port.

### Audio, models and distribution

- Normalize locally the audio delivered to the transcription to 16-bit PCM WAV, mono, 16 kHz; keep the original recording and treat the conversion as a derivative.
- Store the output of the synthesis as an audio file playable locally.
- Propose faster-whisper with the small multilingual model on CPU/int8 as an initial reference that can be adjusted. The final choice depends on the validation with the hardware and the real recordings, without promising latency.
- Use an English voice of Kokoro, with adjustable speed. The concrete voice and dependencies are validated in the Windows packaging.
- Distribute the auxiliary process with the runtime dependencies; the end user does not need to prepare a Python environment manually.
- Do not automatically include all the weights in the installer. In the initial preparation, present the necessary models and make available a download with progress or the selection of existing local files, according to the options of the engine.
- Once prepared, audio and transcription work without a network. Follow-ups and evaluation require a connection to the provider.
- Validate the recording and playback in the WebView and in the audio adapter effectively used. Validate the installed package, in addition to running in development.

## 9. OpenCode, costs and configuration

Use the OpenCode inference API directly from Rust; it is not necessary to install or run the OpenCode agent/CLI in the tutor.

Initial proposed configuration, verified in the official catalog and documentation on 2026-11-01:

- Paid model: `deepseek-v4.1-flash`.
- Endpoint: `https://opencode.ai/inference/openai/v1/chat/completions`.
- Catalog: `https://opencode.ai/inference/v1/models`.
- Model identifier and base URL configurable; do not depend on identifiers with the `-free` suffix.

The provider documents that free chat models are restricted to use inside OpenCode and may answer 403 to external clients. This design uses the paid model.

> **Change of 2026-11-04:** the current modality is OpenCode Go, with endpoint
> `https://opencode.ai/inference/go/openai/v1/chat/completions`, after explicit
> authorization of the user. See [Go design](2026-11-04-opencode-go-design.md) and
> [validation](../../opencode-go/validation.md).

The table consulted presents US$0.30 per million input tokens and US$1.20 of output, with a distinct price for cache reading. These values are a dated reference, not a guarantee nor a fixed price in the code. Store the version/date of the tariff used in the estimate. If it is not possible to distinguish cached tokens, use the normal input tariff as a conservative estimate.

Hypothetical example: 20,000 input tokens and 4,000 of output amount to US$0.0108, before other charges. One hundred equal sessions would correspond to US$1.08. Confirm the values in the provider account and measure real sessions.

Record the model effectively returned, tokens and estimate per request and per session. Include follow-up requests, report, rewrite and evaluation of new attempts. If the provider does not return usage, present it as unknown; do not show zero as a real measurement. The estimate does not replace the provider's billing.

## 10. Persistence and recovery

Minimum entities:

- Question and criteria, with category and skills.
- Personal story and skills.
- Session, configurations and persistent state.
- Question of the session, with text and criteria fixed at the start and position.
- Initial answer, follow-up and answer to the follow-up, explicitly associated.
- Attempt, with relation to the question and to the previous attempt when applicable.
- Original audio, automatic transcript, confirmed revised version and derived files.
- Evaluation and rewrite, with versions of the rubric/prompt and source texts.
- AI request, purpose, state, model, usage and tariff used.

The automatic transcripts, review drafts, confirmed versions sent and rewrites are distinct. Changing the draft before confirmation does not change the automatic transcript. After confirmation, the version sent stays fixed. New attempts create new records and evaluations; existing evaluations keep the reference to the text they evaluated.

Store audio and texts before the external call. Each persistent operation has its own identifier. Apply a result only to the version and the state that originated the request; a repeated request cannot insert two follow-ups nor advance the session twice.

After a network failure or a restart of the application, recover the last stored step and allow repeating the request. A recording interrupted by a process failure may have to be redone; do not promise recovery of audio that has not yet been finalized and stored.

A repetition of a request after a timeout may have additional costs if the provider has already processed the previous one. Record the known usage and mark the uncertain state, without promising exactly-once execution at the provider.

Deleting a session removes the respective answers, versions, evaluations, requests and exclusive audio files/derivatives. It does not remove questions from the bank, personal stories nor shared models. Failures when removing files stay flagged for a new attempt; do not indicate complete deletion while files of the session remain.

## 11. Quality, failures and trust limits

- Store finalized recordings before long operations and communicate the current state to the user.
- Distinguish microphone failure, model failure, missing local model, network failure, invalid credential, provider limits and invalid AI response.
- Allow repeating transcription, playback and external requests without a new recording.
- Losing a Python process must not close the application nor delete finalized answers.
- Do not log the API key. Diagnostics must not include complete professional answers by default.
- Inform in the configuration and before the first sending that revised text and criteria go to an external service. Do not claim zero retention nor exclusively local processing.
- Allow continuing to record and store locally during network unavailability. After confirming each main question, the user may repeat the request or explicitly dismiss the follow-up to advance. The final feedback stays pending until there is a connection. Dismissed follow-ups are not generated later nor presented out of the order of the interview.

## 12. Validation and definition of done

Validate with approximately ten real answers, including complete answers, incomplete ones, with technical terms, qualitative or negative result and problematic recordings.

Verify:

1. The meaning of the speech remains recoverable through the transcription and review.
2. Recording, transcription and synthesis work in the Windows package, without preparing Python manually.
3. The wait between turns is measured and considered acceptable by the user; the interface remains usable.
4. The configured total corresponds to main questions, without more than one follow-up per question.
5. The follow-ups clarify real gaps and do not repeat information already answered.
6. The observations point to real excerpts and distinguish the initial answer from the follow-up.
7. The rewrites do not invent facts and are adequate to the configured level.
8. A network failure and a restart preserve audio and text already stored, allowing resuming.
9. A new attempt can be evaluated and compared without overwriting the original.
10. Deletion removes all the data and audio files exclusive to the session.
11. The usage and the cost of a real session are recorded and presented without false precision.
12. Without a network, advancing requires an explicit dismissal of the follow-up, and recovering the connection does not insert previous questions into the session.
13. A confirmed answer is not changed by a new attempt nor by results that arrive late.

The MVP is ready for personal use when the user completes a short interview, consults useful feedback and repeats an answer without losing the previous versions. The performance of the AI requires human validation with real data; automated tests of the session rules do not replace that validation.

## 13. Decisions left for the plan and implementation

- Concrete libraries for capture, playback, resampling, SQLite and protected storage of credentials.
- Final local model/voice and hardware requirements, after measurement on the user's computer.
- Complete JSON schemas, module organization, migrations and distribution strategy of the Python process.
- Visual style, React components and navigation details, preserving the flow and the distinction between versions.
- Technical limits of duration/text and deadlines of the requests, visible before they cause loss of work; do not truncate an answer silently.
- Strategy for splitting the report requests to control cost and context, keeping the evidence and the consistency.

These choices do not change the functional scope. Adding other languages, swapping the audio engines or removing the Python process requires a later decision supported by the validation.

## 14. Sources consulted

- Tauri, concepts and architecture: https://v2.tauri.app/start/
- Tauri, WebViews: https://v2.tauri.app/reference/webview-versions/
- Tauri, distribution of auxiliary processes: https://v2.tauri.app/develop/sidecar/
- faster-whisper, execution and requirements: https://github.com/SYSTRAN/faster-whisper
- Kokoro-82M, synthesis model: https://huggingface.co/hexgrad/Kokoro-82M
- Whisper, purpose and limitations: https://github.com/openai/whisper/blob/main/model-card.md
- OpenCode, models and tariff: https://opencode.ai/v2/docs/console/models/
- OpenCode, inference API: https://opencode.ai/v2/docs/console/api/inference/
- OpenCode, runtime catalog: https://opencode.ai/inference/v1/models

The sources of catalog, prices and endpoints were verified in this session. The availability and the conditions of the provider may change; confirm when implementing and using.

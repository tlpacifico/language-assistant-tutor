# Guidelines for working in this project

## Context and documents

This project is a desktop tutor for behavioral job interviews in English, by voice, based on the STAR structure. Development is organized in phases; phases 1 and 2 are implemented and automatically verified, with the user's manual validation still pending. Phase 3 was brought forward by explicit authorization of the user on 2026-11-02; implementation and automated verification are not the same as manual acceptance.

Before working, consult the documents relevant to the task:

- [Layout validation](docs/layout/validation.md): visual reference, adaptations to the contracts, audio recovery on every page and a reproducible seven-page matrix.
- [Per-question evaluation and corrections](docs/ai-coaching/validation.md): change approved on 2026-11-04; evaluation and rewrite during the AI interview, preservation of the sources, reuse in the final report and fix for the editor initialization.

- [Idea and goals](docs/idea-and-goals.md): problem, audience, learning cycle and MVP limits.
- [Technical stack](docs/tech-stack.md): chosen technologies, responsibilities and integrations.
- [Phase plan](docs/superpowers/plans/2026-11-01-star-interview-tutor-phases.md): deliveries, dependencies and completion criteria.
- [Phase 1 spec](docs/superpowers/specs/2026-11-01-phase-1-local-audio-design.md): scope, local audio behavior and acceptance criteria.
- [Phase 1 technical plan](docs/superpowers/plans/2026-11-01-phase-1-local-audio-implementation.md): tasks, contracts and checks of the first implementation.
- [Real preparation on Windows](docs/phase-1/setup.md): observed versions, commands executed, models and personal configuration.
- [Phase 1 validation](docs/phase-1/validation.md): matrix CA-01 to CA-15 with evidence and what remains to be validated with the user.
- [Phase 2 preparation and execution](docs/phase-2/setup.md): what changed, SQLite, session directories, commands and reproduction from a clean checkout.
- [Phase 2 validation](docs/phase-2/validation.md): matrix F2-01 to F2-16, automated evidence and pending manual validation.
- [Phase 2 spec](docs/superpowers/specs/2026-11-02-phase-2-guided-interview-design.md): guided interviews, question bank, confirmation, SQLite and recovery.
- [Phase 2 technical plan](docs/superpowers/plans/2026-11-02-phase-2-guided-interview-implementation.md): ten tasks, contracts, transactions, recovery and checks; tasks 1 to 9 executed and task 10 partial (manual validation missing).
- [Approved design](docs/superpowers/specs/2026-11-01-star-interview-tutor-design.md): detailed functional and technical rules.
- Phase 4 [preparation](docs/phase-4/setup.md) and [validation](docs/phase-4/validation.md), with the [spec](docs/superpowers/specs/2026-11-03-phase-4-improve-repeat-design.md) and the [plan](docs/superpowers/plans/2026-11-03-phase-4-improve-repeat-implementation.md): rewrite, practice sessions, comparison and local stories.
- Phase 3 [spec](docs/superpowers/specs/2026-11-02-phase-3-ai-tutor-design.md), [plan](docs/superpowers/plans/2026-11-02-phase-3-ai-tutor-implementation.md), [preparation](docs/phase-3/setup.md) and [validation](docs/phase-3/validation.md): AI tutor, consent, credentials, follow-ups, evidence and recovery.
- [OpenCode Go integration](docs/opencode-go/validation.md): modality authorized on 2026-11-04 (endpoint `/inference/go/openai/v1`, the tutor's own identity, a stable session per conversation and a one-time adaptation of the old configuration), with what is verified and what remains to be confirmed with the provider.
- [Conversation about an article](docs/article-conversation/setup.md): mode approved on 2026-11-04 (English conversation about a pasted article, automatic sending authorized per session, final feedback and corrections in branches), with [validation](docs/article-conversation/validation.md), [performance](docs/article-conversation/performance.md), [spec](docs/superpowers/specs/2026-11-04-article-conversation-design.md) and [plan](docs/superpowers/plans/2026-11-04-article-conversation.md).

The user's current instructions take precedence. These documents record earlier decisions; keep them coherent when a decision changes.

## How to work

- Work within the scope of the requested task and phase. Do not start the following phases automatically.
- Detail the technical plan only for the next phase to be executed; avoid planning the whole implementation in advance.
- Complete the necessary steps autonomously within the authorized scope, without asking for confirmation on every routine decision.
- Prefer direct execution and simple solutions, keeping costs and complexity under control.
- Use European Portuguese when talking to the user. Write the project documentation and specs in English. The application interface and every user-facing text remain in European Portuguese. Interview questions, follow-ups and practice answers are in English.
- Do not change the stack or the chosen engines as part of a task that does not ask for that change.
- Update the affected documentation and phase criteria when there are relevant changes.
- Do not mark features or phases as complete without concrete verification.

## Chosen technical base

- React + TypeScript for the interface.
- Rust + Tauri 2 for the desktop application, sessions, data and API calls.
- Auxiliary Python process for faster-whisper and Kokoro-82M.
- SQLite for structured data and local files for audio.
- DeepSeek Flash through OpenCode Go for follow-ups and textual evaluation (modality authorized on 2026-11-04, replacing the default per usage; see the [Go integration](docs/opencode-go/validation.md)).
- Windows as the first target.

Consult the [technical stack](docs/tech-stack.md) before introducing dependencies. The recording, resampling and credential libraries will be chosen in the plan of the corresponding phase.

## Essential product rules

- The application controls the interview state; the LLM only recommends within the rules.
- The configured quantity counts main questions. There is at most one follow-up per main question.
- Recording starts and ends by user action, without automatic interruptions.
- Preserve the audio and the automatic transcript. The user's review is a distinct version.
- Send text for evaluation only after confirmation. The confirmed version stays fixed.
- Store new attempts and rewrites separately; do not overwrite the original answer.
- Distinguish information said in the initial answer, added in the follow-up and existing only in the notes of a story.
- Support the feedback with real excerpts and accept qualitative, negative and learning results.
- Rewrites preserve facts; do not invent responsibilities, numbers or results.
- Do not present a transcript or a textual evaluation as a diagnosis of pronunciation, acoustic fluency or emotional confidence.

## Data, integration and recovery

- Keep the recordings on the computer. Only the necessary text and the criteria go to the OpenCode API.
- Store the key through the operating system's protected storage; do not put it in the frontend, code, content SQLite or logs.
- Store audio and text before external calls. A network failure must not require a new recording.
- Apply results only to the operation and the version that originated them; avoid duplicating follow-ups or advancing twice.
- After a follow-up generation failure, repeat the request or dismiss it explicitly before advancing. Do not present delayed follow-ups.
- Run audio and network without blocking the interface.
- Record known usage and estimated cost. Do not present unknown usage as zero and do not assume fixed provider prices.
- Implement persistence and basic recovery when each component enters; do not postpone everything to the final phase.

## Verification

- Local diagnostics and build identity are described in the [spec](docs/superpowers/specs/2026-11-04-local-diagnostics-design.md), the [plan](docs/superpowers/plans/2026-11-04-local-diagnostics.md) and the [validation](docs/diagnostics/validation.md). Preserve the Tauri identity and `app_data_dir()/diagnostics` between versions. Do not log free-form messages, credentials, audio or interview content; new operations/codes require updating the closed lists. Distribution through an installer remains to be validated.

- Run checks appropriate to what changed. Documentation-only changes require checking content, coherence and local links.
- For implementation, test especially the session rules, version preservation, feedback evidence and failure recovery.
- Validate audio and performance with real recordings; automated tests do not replace that experience.
- Also validate the packaged application when working on distribution.
- Do not invent build/test commands or results. When creating the project base, document the real installation, execution, test and distribution commands.

## State and next steps

The design and the division into five phases were approved. Phase 1 is implemented (Tauri 2 + React, native audio, faster-whisper/Kokoro helper, persistence and review) and the automated verification is green: `.\scripts\verify.ps1` with 8/8 checks (77 Rust tests, 15 React tests, 36 Python tests without the `models` marker, `pip check` clean), the real `-m models` test with `2 passed` and the models prepared outside the repository (Whisper `small` and Kokoro-82M, 776.4 MiB). The results are in [docs/phase-1/validation.md](docs/phase-1/validation.md) and the preparation in [docs/phase-1/setup.md](docs/phase-1/setup.md).

Phase 1 is **not** accepted or complete in the validation record: the user's validation with three real recordings (short, with technical terms and problematic) is missing, as are listening to the Kokoro voice, the microphone removal/restore test, repeating the offline journey through the window and the decision on the acceptability of the wait (CA-14). Do not mark the phase as complete before that validation.

Phase 2 is implemented (SQLite with `rusqlite`, editable bank of 20 questions, sessions with fixed copies, artifacts and reconciliation, revisions and immutable confirmations, history and read-only consultation of phase 1 practice sessions) and the automated verification is green: `.\scripts\verify.ps1` with 8/8 checks (151 Rust tests, 53 Vitest tests, 36 Python tests without the `models` marker, `pip check` clean), the real `-m models` test with `2 passed`, the release build `--no-bundle` and the start/stop of the binary with exit code 0. The data are in [docs/phase-2/validation.md](docs/phase-2/validation.md) and [docs/phase-2/setup.md](docs/phase-2/setup.md).

Phase 2 is **not** complete or accepted: the manual validation described in spec §10 and in [docs/phase-2/validation.md](docs/phase-2/validation.md) is missing — a real session of three questions (one with technical terms), listening to questions and originals, review/confirmation, leaving and resuming, history, a one-question session and another quantity, insufficiency in a category, editing the bank without changing the session copy, the offline journey through the window, manual review with the transcript unavailable and recovery from a write failure. Do not mark the phase as complete before that validation. Phase 3 (AI tutor) was brought forward by explicit authorization of the user and has its implementation and validation recorded in [docs/phase-3/validation.md](docs/phase-3/validation.md); manual acceptance remains pending. Phase 4 has only a design and a technical plan; phase 5 remains unplanned.

Phase 3 has an implementation and a green automated verification on 2026-11-02: 8/8 checks (175 Rust, 57 Vitest, 36 Python), release build --no-bundle and smoke with temporary data/exit 0. See [phase 3 validation](docs/phase-3/validation.md). Paid inference, a real protected credential, real audio and semantic usefulness remain to be validated. Do not mark the phase as accepted and do not start phase 4 automatically.


On 2026-11-03, the user asked for the phase 4 spec and plan and then authorized executing it («termine as tarefas»). Phase 4 (improve and repeat) is implemented (schema v3, practice sessions, B1/B2/C1 rewrite, suggestion voice, local comparison, local stories, practice usage) and the automated verification is green: `.\scripts\verify.ps1` with 8/8 checks (246 Rust, 97 Vitest, 36 Python), release build `--no-bundle` and smoke with temporary data/exit 0. See [phase 4 validation](docs/phase-4/validation.md). Phase 4 is **not** accepted or complete: the F4-16 manual script is missing, as are real paid inference (fidelity, usefulness, wait, cost) and the pending items of the earlier phases. Do not start phase 5 without instruction.

On 2026-11-04, the user approved and authorized evaluation, explained corrections and per-question rewrite during the AI interview. Implementation automatically verified: verify.ps1 with 8/8 checks (252 Rust, 112 React and 36 Python), TypeScript/Vite build, formatting/Clippy and pip check approved. Evaluation only after confirmation and a closed follow-up; the final report reuses valid results. Fixed the reconciliation of an empty draft when the automatic transcript arrives. See [validation](docs/ai-coaching/validation.md). Paid inference, semantic usefulness, real audio and manual acceptance remain pending; do not start phase 5.

On 2026-11-04, after comparing the Marlo Expenses container, the user asked for the same modality («utilize o msm forma do Marlo Expense»). The integration moved to OpenCode Go: endpoint `/inference/go/openai/v1`, the tutor's own `User-Agent` and an `x-opencode-session` stable per interview or practice, without copying the Marlo key or identity. The default no longer has a fixed price list; the old reference configuration is adapted once, preserving key and history and requiring new authorization for the destination. Automated verification: 8/8 checks (270 Rust, 119 React, 36 Python), release build `--no-bundle` with exit 0 and smoke of the binary with exit code 0. See [Go validation](docs/opencode-go/validation.md). It remains to verify whether the provider accepts tutor traffic in the Go modality, the real cost and the semantic usefulness; there has been no paid request. Do not start phase 5.

On 2026-11-04 the previous work was published: commit `d3f66c4`, merge into `main` (`c3cba56`) and release [v0.1.0](docs/releases/v0.1.0.md) with an NSIS installer, MSI and portable executable. The installer build exited with code 0 and the binary passed the same smoke (window alive, shutdown with exit code 0). The **installer was not installed**: installation, upgrade over a previous version and uninstallation remain to be validated. The manual acceptance pending items of phases 1 to 4 remain; phase 5 has not been started.

On 2026-11-04, at the user's request, the helper started loading the models when the interview opens (`preload` in protocol v1), to eliminate the wait for the first «Ouvir pergunta» (Kokoro, ~8 s) and for the first transcript. Measurements on this machine: Whisper 1.2 s/690 MiB, Kokoro 8.2 s/1.35 GiB, both ~10 s and 1.7 GiB peak. Application startup still does not load weights; while availability is `loading`, the audio buttons wait instead of receiving `BUSY`. See [stack](docs/tech-stack.md) and [README](README.md).

On 2026-11-04 release [v0.2.0](docs/releases/v0.2.0.md) was published, with the voice models loaded when the interview opens and local validation of the OpenCode key (`oc_sk_…`) before any request. The diagnosis of `AI_CREDENTIAL` is in [Go validation](docs/opencode-go/validation.md): the stored key had the `sk_<uuid>` format and the provider answered 401 on both endpoints. Verification 8/8 checks (275 Rust, 120 Vitest, 39 Python), NSIS/MSI installers and portable build with hashes in the release document, smoke of the portable build with exit code 0. Installing the packages, upgrading over v0.1.0, manual acceptance and paid inference remain to be validated.

On 2026-11-05 the article conversation was implemented (plan `2026-11-04-article-conversation`, tasks 1 to 9). Automated verification: `verify.ps1` with 8/8 checks (354 Rust, 147 Vitest, 56 Python without the `models` marker), release build `--no-bundle` and a rehearsal with temporary data with exit 0. The mode is **not** accepted: the real conversation with microphone, Kokoro voice and paid inference are missing, as are the transcription/voice measurements (`beam_size` keeps the baseline) and the visual validation of the Articles page. See [validation](docs/article-conversation/validation.md). Do not start phase 5.

On 2026-11-05 release [v0.3.0](docs/releases/v0.3.0.md) was published, with the conversation about an article (NSIS/MSI installers and portable build with hashes in the document; smoke of the portable build with exit code 0). The article conversation has no manual acceptance, installing the packages remains to be validated and phase 5 has not been started.

On 2026-11-06, at the user's request, the `tlpacifico/language-assistant-tutor` repository went from **private to public**, under the MIT licence (`LICENSE`). Before the change, the whole history was audited: no real key or credential, no sensitive file versioned and no blob above 230 KiB; the `oc_sk_…` keys in the code are test fixtures. The preparation no longer exposes the Windows user name in the phase 1 documents and in the examples of the helper contracts (`C:\Users\Utilizador Exemplo`). The email `thacio.pacifico@gmail.com` remains visible in the commit metadata, by the user's decision not to rewrite the history. Visibility does not change the state of the phases: 1 to 4 without manual acceptance, phase 5 not started.

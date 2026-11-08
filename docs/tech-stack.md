# Technical stack

## Main decision

The application will be built with **Rust + React + TypeScript + Tauri 2**, starting with Windows. The audio models remain in a helper Python process.

The purpose of this division is to keep a web interface, concentrate the logic and the data in Rust and use the audio engines already chosen. It is not assumed that using Rust, by itself, speeds up the inference of those models.

## Components

| Component | Technology | Execution and responsibility |
| --- | --- | --- |
| Interface | React + TypeScript | Local; questions, transcript review, feedback and history. |
| Interface tooling | Vite | Development and compilation of the web assets. |
| Desktop application | Tauri 2 | Integration of the interface with the system and distribution. |
| Application core | Rust | Sessions, rules, persistence, files and external communication. |
| Transcription | faster-whisper | Local, in a helper Python process. |
| Speech synthesis | Kokoro-82M | Local, in the helper Python process. |
| Follow-ups and evaluation | DeepSeek Flash via OpenCode | Cloud; receives confirmed text and criteria. |
| Structured data | SQLite (`rusqlite`, bundled SQLite) | Local; question bank, session copies, answers, revisions and confirmations of the new phases. |
| Recordings and derivatives | Audio files | Local data folder managed by the application. |
| API key | Operating system protected storage | Access by the core, outside the SQLite content and the frontend. |

## Responsibilities and communication

### React and TypeScript

They present the user experience and communicate with Rust through Tauri commands and events. The interface does not contain the provider API key and is not the authority over the persistent interview state.

### Rust and Tauri

They control the number of questions, the follow-up limit, session transitions, answer versions, SQLite access and file management.

Rust calls the OpenCode API and validates the structured results before applying them. Each result stays associated with the request, the session and the text version that originated it.

The core also manages local audio capture and the lifecycle of the Python process. The concrete recording and playback library will be chosen in the phase 1 technical plan.

### Python audio process

It performs transcription and synthesis, receiving requests from Rust as line-delimited JSON messages through standard input/output. Each request has an identifier, an operation and parameters; the response includes a result or an error. Diagnostics go over stderr.

The audio files are local files referenced in the requests. The models stay loaded between operations when possible. The MVP processes local audio jobs sequentially.

Tauri allows shipping helper executables. The goal of the final package is not to require the user to set up Python manually. Packaging the dependencies needs validation on Windows.

## Data flow

1. The application captures and stores the recording locally.
2. The Python process transcribes the audio with faster-whisper.
3. The user reviews and confirms the text in the interface.
4. Rust sends the necessary text and the criteria to OpenCode.
5. Rust validates and stores the follow-up, feedback or rewrite.
6. When a text has to be spoken, the Python process generates audio with Kokoro and the application plays the local file.

Neither the audio files nor the complete bank of stories is sent automatically to the provider. Automatic transcript, confirmed text, suggestions and new attempts are distinct records.

## Local models and formats

- Transcription engine: faster-whisper.
- Initial reference to try: multilingual Whisper small model on CPU/int8, adjustable after measurement on the user's computer.
- Format handed to transcription: 16-bit PCM WAV, mono, 16 kHz, converted locally when necessary.
- Synthesis: Kokoro-82M with an English voice and adjustable speed.
- Store the original recording separately from conversions and synthesized audio.
- Prepare weights and dependencies with visible progress or selection of existing files, depending on the engine.
- Preloading since 2026-11-04: when an interview is opened, the application requests `preload` from the helper and the models stay resident in the process (≈10 s warm, ≈14 s cold, peak of ~1.7 GiB on this machine). The audio buttons wait while availability is `loading`, instead of receiving a request refused because the helper is busy; application startup still does not load weights.

The models may need network access during the initial setup. Once available, transcription and synthesis work locally. Text evaluation still depends on the connection to the API.

## Text AI provider

Reference configuration updated with the user's authorization on 2026-11-04:

| Parameter | Value |
| --- | --- |
| Provider | OpenCode Go, the inference API associated with the subscription. |
| Model | `deepseek-v4.1-flash` |
| Endpoint | `https://opencode.ai/inference/go/openai/v1/chat/completions` |
| Catalog | `https://opencode.ai/inference/go/v1/models` |

The tutor calls the API directly. It does not depend on installing the OpenCode agent/CLI. The model and the base URL remain configurable; the MVP does not depend on the access conditions of the free models.

The client uses its own `User-Agent` and an `x-opencode-session` stable per interview or practice, like the Marlo Expenses integration. It does not copy its key or identity. The old reference configuration is adapted once, preserving the credential and the history and requiring new authorization for the Go destination. See [design](superpowers/specs/2026-11-04-opencode-go-design.md) and [validation](opencode-go/validation.md).

The Go default has no fixed rate card: included usage and the monthly fee are not additional charges per request. The cost and usage returned by the API prevail; an explicit zero is a known value, the absence of a cost remains unknown. The previous rate cards remain in the historical snapshots. A manual rate card is still possible and must record date/source.

The usage record includes follow-up, report, rewrite and new-attempt evaluation requests. If the provider does not return usage, present it as unknown. The estimate does not replace the provider's billing.

## Persistence and recovery

Phase 3 was brought forward by user authorization. It adds `reqwest` 0.13.5
(HTTP JSON/Rustls, without automatic redirects/retries) and `keyring` 3.6.3
(`windows-native`, without a plain-text fallback). The SQLite schema v2
introduces `answer_slots`, public configuration/consent, AI steps,
requests/attempts and immutable reports. The `local_v2` sessions keep the
existing path; only new sessions explicitly marked `ai_v3` have follow-ups and feedback.
See [phase 3 setup](phase-3/setup.md) for concrete contracts and limits.

- SQLite is the authority for the data of the new phase 2 sessions (question bank, fixed copies of the questions, session position and state, recordings, automatic transcripts, append-only revisions, immutable confirmations, operations and command receipts). The connection is made in Rust with `rusqlite` and the `bundled` feature, which compiles the SQLite bundled in the library and does not require SQLite installed on the system. A single `Connection` protected by `Arc<Mutex<…>>`; the synchronous operations run in `spawn_blocking`, outside the interface thread, and the guard never crosses an `await`. There is no ORM, no pool and no SQL access from the frontend.
- The phase 1 JSON files (`phase1/attempts/…`) are no longer written by the new application and are kept only for reading and querying the previous practice sessions. There are no two authorities for the same sessions: the new data lives exclusively in `tutor.sqlite3` and in the audio files.
- The schema is versioned (`PRAGMA user_version`) and migrated in a transaction; a future version is refused without writing and a failure neither deletes nor recreates the database. The constraints and triggers prevent rewriting transcripts, revisions and confirmations, and a partial index guarantees at most one session in progress.
- The application data folder stores recordings and derivatives; the concrete path is resolved by the operating system.
- Store audio and text before external calls.
- Do not apply late responses to another version and do not duplicate transitions when repeating requests.
- Preserve the stored state on restart. Recordings not yet finalized may have to be redone.
- Do not block the interface with inference, audio conversion or network.
- Delete the files exclusive to a session when deleting it, preserving the shared models.

## Local diagnostics

Added on 2026-11-04 to support real use. It uses only Rust/std,
Serde and UUID, already existing, without a telemetry provider. JSONL events in
`app_data_dir()/diagnostics`, preserved between versions, with a bounded queue and a
separate writer. It does not store free messages, credentials or interview
content. The «Diagnóstico local» panel identifies version/commit/build date
and exports JSON to a folder indicated by the user. The Tauri identity
`dev.thacio.languageassistanttutor` remains stable. See
[design](superpowers/specs/2026-11-04-local-diagnostics-design.md),
[plan](superpowers/plans/2026-11-04-local-diagnostics.md) and
[validation](diagnostics/validation.md).

## Choices still to be closed

- Concrete integration with the protected credential storage.
- Final versions of the dependencies, transcription model and Kokoro voice.
- How to package the Python process, its native components and the installers.
- Duration/text limits and request timeouts, without silent truncation.

The phase 1 audio choices are implemented and verified: CPAL, Rodio, Hound, Rubato, atomicwrites and Python 3.11 with the `af_heart` voice (detail in [docs/phase-1/setup.md](phase-1/setup.md)). SQLite access was decided in phase 2 (`rusqlite` with `bundled`) and compiles in the release build of this machine. Credentials, final packaging and final limits remain for the corresponding phases.

The [phase 1 spec](superpowers/specs/2026-11-01-phase-1-local-audio-design.md) defines the behavior of the first audio integration and the minimal file persistence, prior to the introduction of SQLite in phase 2.

The [phase 2 technical plan](superpowers/plans/2026-11-02-phase-2-guided-interview-implementation.md) implemented `rusqlite 0.41.1` with `bundled`, a single protected connection and the frontend without direct SQL access, with transactions, migrations and reconciliation with the audio files. The decision is recorded in [docs/phase-2/validation.md](phase-2/validation.md), section 6.

## References

- [Tauri: introduction](https://v2.tauri.app/start/).
- [Tauri: WebViews used](https://v2.tauri.app/reference/webview-versions/).
- [Tauri: helper executables](https://v2.tauri.app/develop/sidecar/).
- [faster-whisper](https://github.com/SYSTRAN/faster-whisper).
- [Kokoro-82M](https://huggingface.co/hexgrad/Kokoro-82M).
- [OpenCode: models and pricing](https://opencode.ai/v2/docs/console/models/).
- [OpenCode: inference API](https://opencode.ai/v2/docs/console/api/inference/).

See also the [idea and goals](idea-and-goals.md), the [phase plan](superpowers/plans/2026-11-01-star-interview-tutor-phases.md) and the [approved design](superpowers/specs/2026-11-01-star-interview-tutor-design.md).

## Article conversation (2026-11-04)

A separate mode in the same core: `article/` domain (states, limits, branches), SQLite schema v5 (`article_*`), prompts and validation in `ai/article.rs` on top of the common HTTP send (OpenCode Go), orchestration in `service/article.rs` with its own capture, transcription and voice (no STAR answer slots) and React page `Artigos`. The tutor voice uses the opt-in `synthesize_stream` operation of the helper (complete WAV blocks, `audio_chunk`, cooperative `cancel`) and a queue in the same Rodio player. No new dependencies. See [setup](article-conversation/setup.md), [validation](article-conversation/validation.md) and [performance](article-conversation/performance.md).

# Phase 1 — Local Audio Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver a Windows application that reads a fixed question, records an answer, transcribes it locally and saves a revision without losing the original.

**Architecture:** React/TypeScript presents the practice session and communicates with Tauri commands. Rust controls operations, capture/playback and files; a persistent Python process runs faster-whisper and Kokoro over JSON on standard input/output. The phase uses local JSON, without SQLite, external services or a final installer.

**Tech Stack:** Tauri 2, Rust, React, TypeScript, Vite, CPAL, Rodio, Hound, Rubato, atomicwrites, Python 3.11, faster-whisper and Kokoro-82M.

**Spec:** [Phase 1 spec](../specs/2026-11-01-phase-1-local-audio-design.md). Also read [AGENTS.md](../../../AGENTS.md) and respect the [phase plan](2026-11-01-star-interview-tutor-phases.md).

**Status:** phase 1 implementation executed from task 1 to 8. Automated verification green (`.\scripts\verify.ps1`, 8/8 checks; real `-m models` test with `2 passed`) and models prepared outside the repository. The phase is **not** accepted: manual validation with real recordings, listening to the voice, microphone removal and acceptability of the wait remain pending in [docs/phase-1/validation.md](../../phase-1/validation.md). It does not record approval of the phase nor start phase 2.

## Global Constraints

- “Desktop application executable in development on Windows.”
- “Interface and messages | European Portuguese.”
- Exact question: `Tell me about a time you solved a difficult problem at work.`
- “English; transcribe, without translating or improving the answer.”
- “faster-whisper, `small` multilingual model, CPU/int8, language set to English.”
- “One transcription or synthesis job at a time.”
- “One active answer in the interface; each new recording has its own identifier and files.”
- “Capture does not depend on the Python process being ready.”
- “Normalize a derivative to 16-bit PCM WAV, mono, 16 kHz when necessary for transcription.”
- “The finalized audio is saved and can be played back, even when transcription fails.”
- “In this phase, saving a revision is not the same as confirming an answer for evaluation.”
- No OpenCode, credentials, SQLite, pronunciation scoring, interview sessions or features of the following phases.
- Simulated results are exclusive to the tests. The normal application uses the real engines and reports unavailability.

## Review Focus

1. Write failure or full disk: keep the last valid answer and the previous review, without showing false success. Task 2 tests.
2. Device removed, unexpected format or slow consumer: do not block the callback nor discard audio silently. Task 3 tests.
3. Incomplete models, voice or language resources: indicate the missing component without downloading during practice. Task 4 tests.
4. Blocked process, unexpected stdout or delayed result: release the operation, preserve artifacts and do not apply results from another generation. Tasks 5 and 6 tests.
5. Unsaved review when recording another answer or closing: save, discard or cancel explicitly, preserving the edit if the write fails. Task 7 tests.

## 1. Environment and technical decisions

### Environment observed in this session

- The folder contains documentation and `.gitignore`, without an implemented application. In the final verification, the Git repository was already initialized, with files prepared in the staging; preserve that preexisting state.
- Node `24.11.1`, npm `11.6.2`; Python 3.11, 3.10 and 3.13 enumerated by the `py` launcher.
- Rust/Cargo not found in the PATH nor in the usual path checked. Visual Studio 2022 Community with C++ tools was found; check the Windows SDK and WebView2 during execution.
- No software was installed, no model weight was downloaded and no build was executed to prepare this plan.

### Reference versions

Values consulted in the official registries on 2026-11-01, to be pinned in the manifests and lockfiles during execution:

| Component | Reference |
| --- | --- |
| Rust | `1.99.1`, MSVC toolchain `x86_64-pc-windows-msvc`; rustfmt and clippy components. |
| Node | Keep `24.11.1` available; use `npm.cmd` in the PowerShell commands. |
| React / React DOM | `19.3.1`. |
| Vite / TypeScript / Vitest | `8.3.5` / `7.0.2` / `5.0.5`. |
| Tauri Rust / API and npm CLI | `2.12.5` / `2.12.4`; keep the 2 family. |
| CPAL / Rodio / Hound / Rubato | `0.18.2` / `0.23.0` / `3.5.1` / `5.0.1`. |
| atomicwrites | `0.4.4`, for atomic replacement of JSON also on Windows. |
| Python | Use `py -3.11`, creating `.venv`; do not use the default Python 3.13. |
| faster-whisper / Kokoro / Misaki | `1.2.1` / `0.9.4` / `0.9.4`, with English extras of Misaki. |

The published Kokoro/Misaki requires Python earlier than 3.13. Resolve the compatible transitive dependencies, including PyTorch CPU, spaCy, Transformers, SoundFile and tests, in task 4. Save `package-lock.json`, `Cargo.lock` and `audio-worker/requirements.lock.txt` after resolution and verification; consulting versions does not prove compatibility of the set.

Use `serde`/`serde_json`, `uuid`, `tokio`, `sha2` and `tempfile` as Rust support; Vitest, Testing Library and jsdom for interface tests. Do not add an HTTP server, a global state framework or a contract generator in this phase.

### Decisions closed for the phase

- Tauri identifier: `dev.thacio.languageassistanttutor`.
- Native capture and playback in Rust: CPAL for input, Rodio for output; one audio thread owns the streams and receives messages. Do not store potentially non-transferable streams in a mutex shared between commands.
- Original WAV PCM16 at the effective rate and number of channels of the input; accept I16, U16 and F32, converting samples to PCM16 without changing the rate. Other formats return an explicit error, without panic.
- Hound reads/writes WAV; Rubato resamples, with downmix before the conversion to 16 kHz. Compensate the filter delay and finish the last block without cutting the answer.
- Do not set an answer time limit in this phase. Write the audio in streaming with limited memory; capacity, write or buffer errors produce an explicit interruption and an attempt to recover the partial file.
- Kokoro on CPU, American English `lang_code='a'`, voice `af_heart`, speed `1.0`; WAV PCM16 mono output at 24 kHz.
- Run Python by a configured absolute path, `-u -m audio_worker`, with an explicit working directory. In development use the `.venv`; do not depend on the directory from which the app was opened.
- Deadlines: handshake 10 s; loading of each model 180 s; transcription `max(120 s, 5 × audio duration)` after loading; synthesis 120 s after loading; shutdown 3 s. A progress does not renew a deadline indefinitely. A timeout shuts down the helper and invalidates its generation.
- Persistence in `app_data_dir()/phase1`; models in a separate directory indicated by the configuration. Do not write weights or recordings into the source code.

## 2. Structure of the files to create

| Files | Responsibility |
| --- | --- |
| `package.json`, `package-lock.json`, `tsconfig.json`, `vite.config.ts`, `index.html` | React/Vite base, tests and npm commands. |
| existing `.gitignore`; new `rust-toolchain.toml`, `README.md` | Preserve/review exclusions; pin toolchain and real instructions. |
| `src/main.tsx`, `src/App.tsx` | Startup and mounting of the experience. |
| `src/features/practice/{Practice.tsx,usePractice.ts,practice.css}` | Phase 1 interface and local review state. |
| `src/lib/{contracts.ts,tauri.ts}` | Frontend types and command/event adapter. |
| `src/test/setup.ts`, `src/features/practice/Practice.test.tsx` | Test environment and observable behavior. |
| `src-tauri/{Cargo.toml,Cargo.lock,build.rs,tauri.conf.json,capabilities/default.json}` | Tauri base, permissions and window. |
| `src-tauri/src/{main.rs,lib.rs,domain.rs}` | Startup and contracts/rules independent of the hardware. |
| `src-tauri/src/storage.rs` | Artifacts, atomic JSON and recovery. |
| `src-tauri/src/audio/{mod.rs,capture.rs,playback.rs,normalize.rs}` | Audio port, native adapters and conversion. |
| `src-tauri/src/worker/{mod.rs,protocol.rs}` | Helper supervision and JSON protocol. |
| `src-tauri/src/{service.rs,commands.rs}` | Flow coordination and thin Tauri commands. |
| `src-tauri/tests/{domain.rs,storage.rs,audio.rs,worker.rs,service.rs}` | Rule tests and integration with controlled adapters. |
| `src-tauri/tests/fixtures/worker_stub.py` | Test helper with failure, delay and known responses. |
| `audio-worker/{pyproject.toml,requirements.in,requirements.lock.txt}` | Python dependencies and verified lock. |
| `audio-worker/src/audio_worker/{__init__.py,__main__.py,protocol.py,engines.py,prepare.py}` | Protocol, local engines and explicit preparation. |
| `audio-worker/tests/{test_protocol.py,test_engines.py,test_offline_models.py}` | Contract, loading and optional real tests. |
| `contracts/worker-v1.examples.json`, `contracts/app-v1.example.json` | Examples shared and verified in the three languages. |
| `config/audio.example.json`, `scripts/verify.ps1` | Documented configuration and execution of the checks. |
| `docs/phase-1/{setup.md,validation.md}` | Windows preparation and real acceptance results. |

The Rust tests of each module may remain in the indicated test file; production does not need one module per test. Default icons/configurations generated by Tauri are part of the base of task 1. Do not generate the project over the existing documentation.

## 3. Shared contracts

### Domain types

Define them in `src-tauri/src/domain.rs`; duplicate only the serialized types in `src/lib/contracts.ts`, verified against the examples. JSON uses camelCase; enums use snake_case. Identifiers are UUIDs as text, UTC RFC3339 dates and integer durations in milliseconds.

| Type | Mandatory fields and values |
| --- | --- |
| `AppError` | `code: String`, `message: String`, `retryable: bool`; stable codes, messages in Portuguese. |
| `ComponentState` | `status: ready \| missing \| loading \| error`, `message: Option<String>`. |
| `Availability` | `microphone`, `playback`, `transcription`, `synthesis`: `ComponentState`. |
| `CaptureMeta` | `sampleRate: u32`, `channels: u16`, `durationMs: u64`, `bytes: u64`, `incomplete: bool`. |
| `Transcript` | `id`, `status: completed \| no_speech`, `text`, `segments: Vec<Segment>`, `model`, `language`, `createdAt`, `loadMs`, `processingMs`. |
| `Segment` | `startMs`, `endMs`, `text`; keep the content returned by the engine. |
| `Review` | `sourceTranscriptId`, `text`, `savedAt`. |
| `Attempt` | `id`, `question`, `createdAt`, `capture: CaptureMeta`, `originalFile`, `normalizedFile: Option<String>`, `transcripts: Vec<Transcript>`, `review: Option<Review>`, `lastOperation: Option<OperationRecord>`. Files are relative references. |
| `OperationRecord` | `id`, `kind: transcribe \| synthesize`, `state: running \| completed \| failed \| interrupted \| no_speech`, `error: Option<AppError>`. |
| `OperationState` | Rust enum `Running`, `Completed`, `Failed`, `Interrupted`, `NoSpeech`, serialized with the values of the previous line. |
| `ModelJob` | `id`, `attemptId: Option<String>`, `kind`, `stage: loading \| processing`. |
| `PlaybackTarget` | `question \| response`; it only resolves files already belonging to the app. |
| `AppSnapshot` | `revision: u64`, `question`, `availability: Availability`, `attempt: Option<Attempt>`, `recordingId: Option<String>`, `modelJob: Option<ModelJob>`, `playback: Option<PlaybackTarget>`, `lastError: Option<AppError>`. |
| `RestoreResult` | `attempt: Option<Attempt>`, `warnings: Vec<AppError>`; it allows recovering text/review even if an audio or derivative is missing. |

Minimum errors: `BUSY`, `DEVICE_UNAVAILABLE`, `DEVICE_FORMAT_UNSUPPORTED`, `AUDIO_TIMEOUT`, `CAPTURE_INTERRUPTED`, `STORAGE_WRITE_FAILED`, `ARTIFACT_MISSING`, `INVALID_AUDIO`, `MODEL_MISSING`, `WORKER_UNAVAILABLE`, `WORKER_PROTOCOL`, `WORKER_TIMEOUT`, `STALE_SOURCE`. Absence of speech is a `no_speech` result, not a pronunciation error.

`AppState` and `Action` also stay in `domain.rs`. `AppState::can_start_recording(&self) -> Result<(), AppError>` checks occupation, but does not require models to be ready; `AppState::accepts_result(&self, request_id: &str, generation: u64) -> bool` protects results. The helper generation is internal and does not go to the interface.

### Persistence

```text
phase1/
  last-attempt.json                         # {schemaVersion: 1, attemptId}
  attempts/<attempt-id>/
    original.wav
    normalized.wav
    attempt.json                           # metadata/capture/operation; does not duplicate texts
    transcripts/<transcript-id>.json        # immutable original Transcript
    review.json                            # stored Review, atomic replacement
  question-cache/<sha256>.wav
```

The internal type `AttemptManifest` contains the fields of `Attempt` except `transcripts` and `review`, accompanied by `schemaVersion: 1`. The complete answer is rebuilt from that manifest, from the transcription JSON files and from `review.json`; do not keep two authoritative copies of the same text. Transcription results use the request ID and are sorted by date and ID. A result persisted with the ID of an operation marked as `running` allows that operation to be recovered as `completed` or `no_speech`, according to the result; without a result, it becomes `interrupted`.

Write metadata before updating `last-attempt.json`; only then publish the new active answer. If the pointer update fails, preserve the previous one and keep the new recording available to repeat the publication. Never use remove-then-rename to replace JSON on Windows.

`AttemptStore` in `storage.rs`: `new(root: PathBuf) -> Self`, `allocate() -> Result<RecordingTarget, AppError>`, `publish(capture: CapturedAudio) -> Result<Attempt, AppError>`, `restore_last() -> Result<RestoreResult, AppError>`, `append_transcript(attempt_id: &str, transcript: Transcript) -> Result<Attempt, AppError>`, `save_review(attempt_id: &str, source_id: &str, text: &str) -> Result<Review, AppError>`, `set_operation(attempt_id: &str, operation: OperationRecord) -> Result<(), AppError>`, `set_normalized(attempt_id: &str, reference: &str) -> Result<Attempt, AppError>` (ruling R8: public extension used by the service to record `normalized.wav` after the conversion). Publication and writing keep the old references in case of failure; `restore_last` recovers valid artifacts and returns warnings if a component is missing. An unreadable manifest/pointer gives a recoverable error, without deleting files nor preventing a new capture.

### Audio

`RecordingTarget { id: String, directory: PathBuf }` and `CapturedAudio { id: String, original: PathBuf, meta: CaptureMeta }` defined in `audio/mod.rs`.

Port `AudioPort: Send + Sync`: `availability() -> AvailabilityAudio`, `start_capture(target: RecordingTarget) -> Result<(), AppError>`, `stop_capture() -> Result<CapturedAudio, AppError>`, `play(path: PathBuf) -> Result<(), AppError>`, `stop_playback() -> Result<(), AppError>`. `AvailabilityAudio` contains `microphone` and `playback: ComponentState`. `AudioEvent` events: `CaptureInterrupted { capture: Option<CapturedAudio>, error }` and `PlaybackFinished`. Implementation `NativeAudio` uses the owning thread; tests use `FakeAudio`.

`normalize_wav(source: &Path, destination: &Path) -> Result<CaptureMeta, AppError>` in `audio/normalize.rs`; it never modifies `source`.

### Python protocol v1

One JSON line per message, with `protocolVersion: 1`, `id` and `op`. Operations: `health`, `transcribe`, `synthesize`, `shutdown`. `health` checks availability without loading the models or downloading files.

| Operation | `params` | Successful `result` |
| --- | --- | --- |
| `health` | `{}` | `{transcription: ComponentState, synthesis: ComponentState}` |
| `transcribe` | `{audioPath, modelPath, device: "cpu", computeType: "int8", language: "en"}` | `{status: "completed" or "no_speech", text, segments, model: "small", language: "en", loadMs, processingMs}` |
| `synthesize` | `{text, outputPath, voice: "af_heart", speed: 1.0}` | `{audioPath, sampleRate: 24000, durationMs, loadMs, processingMs}` |
| `shutdown` | `{}` | `{stopped: true}`, followed by process exit. |

Final response: `{protocolVersion, id, event: "result", ok: true, result}` or `{protocolVersion, id, event: "result", ok: false, error: AppError}`. Progress: `{protocolVersion, id, event: "progress", stage: "loading" or "processing"}`. Exactly one final response per valid request; keep stdout exclusively for the protocol and drain stderr continuously in Rust.

`WorkerClient` in `worker/mod.rs`: `start(config: WorkerConfig) -> Result<Self, AppError>`, `submit(request: WorkerRequest) -> Result<(), AppError>`, `restart() -> Result<(), AppError>`, `shutdown() -> Result<(), AppError>`. `WorkerConfig` contains absolute paths of the executable and of the models manifest, the fixed module name and the deadlines. `WorkerEvent { generation: u64, message: WorkerMessage }` is delivered to the service through a channel; `WorkerPort` offers `submit` and `restart` for substitution in the tests.

The personal file `audio.json`, in `app_config_dir()` or indicated by `LANGUAGE_TUTOR_CONFIG`, contains `schemaVersion: 1`, `pythonExecutable` and `modelsManifest`, both absolute paths. The module is fixed `audio_worker`, installed in the `.venv`; the process receives `--models-manifest <path>` and uses the data folder as working directory. The prepared manifest contains `whisperDir`, `kokoroConfig`, `kokoroWeights`, `kokoroVoice`, model revisions and an inventory of language resources. The example uses Windows paths with spaces; do not store the personal configuration in Git.

### Service and interface

`AppService` in `service.rs` receives `AttemptStore`, `AudioPort` and `WorkerPort`; it uses a serial coordinator for transitions, delegating IO/inference to asynchronous work. It never holds a state lock while waiting for a device, process or file.

Tauri commands, defined in `commands.rs`, delegate to same-named `AppService` methods and return `Result<AppSnapshot, AppError>`:

```text
get_snapshot()
refresh_availability()
start_recording()
stop_recording()
retry_save_recording()
transcribe_response(attempt_id: String)
play_audio(target: PlaybackTarget)
stop_audio()
save_review(attempt_id: String, source_id: String, text: String)
restart_worker()
prepare_close()
```

`AppService::on_audio_event(event: AudioEvent)` and `AppService::on_worker_event(event: WorkerEvent)` apply results and publish `practice://snapshot` with an increasing revision. `prepare_close` finalizes a valid capture, flags interrupted jobs and shuts down the helper; a failure that could lose data prevents closing silently.

The TS adapter exposes `PracticeClient { snapshot(), refresh(), startRecording(), stopRecording(), retrySaveRecording(), transcribe(attemptId), play(target), stop(), saveReview(attemptId, sourceId, text), restartWorker(), prepareClose(), subscribe(listener) }`, with `AppSnapshot` Promises; `subscribe` returns a listener removal function. The mapping between TS names and commands is only in `src/lib/tauri.ts`.

## 4. Implementation tasks

Run sequentially. Each task ends in a verifiable delivery and a local commit when it is possible to do so without including other people's changes. Git already exists: do not reinitialize it nor change the preexisting staging. Before each commit, check the content and limit the change to the task. Do not publish or create a PR. The following commands are future instructions, not results already obtained.

### Task 1: Desktop base and operation contracts

**Files:** create the manifests/configurations of the base, `rust-toolchain.toml`, `src/{main.tsx,App.tsx}`, `src/lib/contracts.ts`, `src-tauri/src/{main.rs,lib.rs,domain.rs}`, examples in `contracts/` and `src-tauri/tests/domain.rs`; review the existing `.gitignore` only if a necessary exclusion is missing.

**Interfaces:** produces `AppState`, `Action`, the types of section 3 and a window with the exact question. The initial `AppSnapshot` has `revision=0`, without an answer/recording/job; unverified components stay `loading` with the message “A verificar disponibilidade”, until the real verification.

- [ ] Prepare Rust 1.99.1/MSVC, check the SDK/WebView2 and create the base without overwriting docs. npm scripts: `dev`=Vite, `build`=typecheck and Vite build, `typecheck`=tsc --noEmit, `test`=vitest run, `tauri`=Tauri CLI. Ignore `.venv`, `target`, `node_modules`, models, audio and personal configuration; keep lockfiles.
- [ ] Write `recording_gate` and `stale_generation`, with these assertions, against the domain not yet implemented:

```rust
assert!(idle_without_worker.can_start_recording().is_ok());
assert_eq!(recording.can_start_recording().unwrap_err().code, "BUSY");
assert_eq!(transcribing.can_start_recording().unwrap_err().code, "BUSY");
assert!(!waiting.accepts_result("old-request", 1));
```

- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml --test domain`; confirm failure due to behavior/types still missing after resolving the compilation of the base.
- [ ] Implement the indicated types and rules. Read `contracts/app-v1.example.json` in the test to pin camelCase, enums and question. The initial UI only presents the question; do not simulate audio.
- [ ] Verify the previous test, `npm.cmd run typecheck`, `npm.cmd run build` and open `npm.cmd run tauri -- dev`. Expected: tests/build with exit 0 and a window with the question.
- [ ] Commit: `chore: create phase one desktop foundation`.

### Task 2: Local artifacts and recovery without SQLite

**Files:** create `src-tauri/src/storage.rs` and `src-tauri/tests/storage.rs`; update `domain.rs` only for the contracts already defined.

**Interfaces:** consumes `RecordingTarget`, `CapturedAudio`, `Attempt`, `Transcript` and `Review`; produces `AttemptStore` and the file structure of section 3. Accept a temporary test root and obtain the real root through Tauri only in the integration.

- [ ] Write tests `failed_publish_preserves_last`, `failed_review_write_preserves_saved_text`, `append_does_not_overwrite_raw`, `restore_reports_missing_original` and `restore_interrupted_job`. Pin assertions:

```rust
assert_eq!(store.restore_last()?.attempt.unwrap().id, previous_id);
assert_eq!(read_saved_review()?.text, "original review");
assert_eq!(restored.transcripts.len(), 2);
assert_eq!(restored.last_operation.unwrap().state, OperationState::Interrupted);
```

- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml --test storage`; confirm failure before implementing `AttemptStore`.
- [ ] Implement the signatures of section 3, with `atomicwrites::AtomicFile`, temporary files on the same volume and reference validation. Rebuild transcripts and review from the authoritative files, without duplicating the texts in the manifest. Do not update the last answer pointer until publication finishes.
- [ ] Exercise real replacement of an already existing JSON on Windows and injected failures before the writing/replacement. Invalid metadata or missing files return a diagnosis and preserve the remaining data; a recoverable original allows resuming transcription.
- [ ] Repeat the test. Expected: exit 0, old data intact in the failure scenarios and new IDs without folder collision.
- [ ] Commit: `feat: persist local audio attempts and reviewed transcripts`.

### Task 3: Native capture, playback and normalization

**Files:** create `src-tauri/src/audio/{mod.rs,capture.rs,playback.rs,normalize.rs}` and `src-tauri/tests/audio.rs`; add the audio dependencies to Cargo.

**Interfaces:** consumes `RecordingTarget`; produces `NativeAudio`, `CapturedAudio`, `AudioEvent` and `normalize_wav`, according to section 3. The audio thread owns the streams; callbacks deliver samples through a bounded channel and do not write to disk nor wait for locks.

- [ ] Write `normalizes_stereo_48k_without_mutating_source`, `empty_audio_is_invalid`, `capture_failure_preserves_previous`, `overflow_is_reported` and `recording_stops_playback`. For the conversion of 1 s of stereo signal, verify:

```rust
assert_eq!((output.sample_rate, output.channels, output.bits_per_sample), (16000, 1, 16));
assert!(output.frames.abs_diff(16000) <= 16);
assert_eq!(sha256(source_before), sha256(source_after));
assert_eq!(overflow_error.code, "CAPTURE_INTERRUPTED");
```

- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml --test audio`; confirm failure without the adapters/conversion.
- [ ] Implement CPAL/Hound and Rodio. Convert I16/U16/F32, detect a missing device and unsupported format. Write to a provisional file, finalize the WAV header before publishing; on error preserve a valid partial capture with `incomplete=true` or report that no recoverable audio exists. A full channel must not discard samples silently.
- [ ] Implement `normalize_wav` with channel averaging, Rubato and PCM16, compensating filter delay/end. Add a duration test at 44.1 kHz and an attenuation test of a signal above 8 kHz, to avoid a conversion by simple sample dropping.
- [ ] Repeat the tests; run a local manual input/output check when the adapter is wired in task 6. Automated tests do not turn on the real microphone. Expected now: tests with synthetic files/controlled adapter pass.
- [ ] Commit: `feat: add native audio capture playback and normalization`.

### Task 4: Python process with real engines and offline preparation

**Files:** create the `audio-worker` package, `config/audio.example.json`, protocol examples and the three Python test files listed in the structure.

**Interfaces:** `parse_request(line: str) -> WorkerRequest` and `encode_message(message: dict) -> str` in `protocol.py`; `EngineRegistry.health() -> dict`, `transcribe(params: dict, progress: Callable[[str], None]) -> dict` and `synthesize(params: dict, progress: Callable[[str], None]) -> dict` in `engines.py`. `prepare.main(argv: list[str] | None = None) -> int` explicitly prepares the resources; `__main__.main() -> int` serves JSON. `WorkerRequest` is a dataclass with `protocol_version`, `id`, `op`, `params`.

- [ ] Create `.venv` with `py -3.11 -m venv .venv`. Install PyTorch CPU from the official index `https://download.pytorch.org/whl/cpu`, the dependencies of `requirements.in` and the editable package with `.\.venv\Scripts\python.exe -m pip install -e "audio-worker[test]"`. Pin faster-whisper, Kokoro and Misaki at the versions of section 1; after verifying imports and compatibility, save the result of `pip freeze --exclude-editable` in `requirements.lock.txt`. Document the reinstallation with the CPU index and the needed spaCy resources. Document the eventual eSpeak-NG/fallback requirement on Windows after validating the chosen combination.
- [ ] Write `test_protocol_examples`, `test_missing_assets_does_not_connect`, `test_stdout_contains_only_json` and `test_voice_and_text_cache_key`. Pin values:

```python
assert request.op == "transcribe"
assert health["synthesis"]["status"] == "missing"
assert all(json.loads(line)["protocolVersion"] == 1 for line in stdout.splitlines())
assert first_key != changed_voice_key
```

- [ ] Run `.\.venv\Scripts\python.exe -m pytest audio-worker/tests/test_protocol.py audio-worker/tests/test_engines.py -q`; confirm failure due to contracts/behaviors still missing, not due to badly installed dependencies.
- [ ] Implement the parser and the sequential server. Engine imports are late; redirect library stdout to stderr. Apply `HF_HUB_OFFLINE=1` and `TRANSFORMERS_OFFLINE=1` before the imports in the server; `health` checks paths, voice, dependencies and English resources without downloading them.
- [ ] Implement explicit online preparation: download `Systran/faster-whisper-small`, configuration/weights of `hexgrad/Kokoro-82M` and `voices/af_heart.pt`; install/prepare the English spaCy resource required by Misaki and also materialize the chosen language fallback. Record model revisions, absolute paths and resources in `models.json`. Run a fixed synthesis during preparation to reveal late dependencies; do not use personal recordings for that step.
- [ ] Implement loaders: `WhisperModel(local_path, device="cpu", compute_type="int8", local_files_only=True)` and transcription `language="en", task="transcribe", vad_filter=True`, preserving text/segments. Kokoro receives local configuration/weight paths and a local voice tensor, CPU model and English pipeline; check Misaki resources before building the pipeline, since the library may try to obtain them automatically. Concatenate all generated audio segments, without truncating the question.
- [ ] Add tests that prevent network connections and download subprocesses during `health` and normal use. Register the pytest marker `models` in `pyproject.toml`; `test_offline_models.py` uses it and only runs with prepared resources: generate the question, read the 24 kHz WAV and transcribe it with Whisper. Missing models must be an explicit skip only in this optional test, never a false success of the real validation.
- [ ] Repeat the unit tests and `.\.venv\Scripts\python.exe -m pip check`. Expected: exit 0; model failures identify the missing resource without contaminating stdout.
- [ ] Commit: `feat: add offline whisper and kokoro audio worker`.

### Task 5: Helper supervision and recovery

**Files:** create `src-tauri/src/worker/{mod.rs,protocol.rs}`, `src-tauri/tests/worker.rs` and `src-tauri/tests/fixtures/worker_stub.py`.

**Interfaces:** consumes protocol v1 and `AppError`; produces `WorkerClient`, `WorkerPort`, `WorkerConfig`, `WorkerRequest`, `WorkerMessage` and `WorkerEvent` defined in section 3. `WorkerConfig` obtains the executable/paths from the configuration file, validating existence and absolute paths.

- [ ] Write `worker_crash_releases_job`, `timeout_rejects_old_generation`, `stderr_does_not_block`, `non_json_stdout_is_protocol_error` and `handshake_does_not_load_models`. Essential assertions:

```rust
assert_eq!(timeout.code, "WORKER_TIMEOUT");
assert!(restarted_generation > previous_generation);
assert!(!state.accepts_result(&old_id, previous_generation));
```

- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml --test worker`; confirm failure before the supervision.
- [ ] Implement the process with `tokio::process`, separate arguments, stdin/stdout piped and stderr drained. Use a process without a visible window on Windows. The `health` handshake does not import heavy models. Apply per-stage deadlines and a new generation after a restart; EOF, invalid JSON and timeout fail the pending request and allow a new start.
- [ ] On timeout, terminate the process, wait for the exit and invalidate any provisional results/files of the request. A new request never reuses the identifier of a terminated request. On shutdown, send `shutdown`, wait up to 3 s and terminate forcibly if necessary.
- [ ] Repeat the tests with the configured Python 3.11 and the test stub. Expected: exit 0, no hanging child process and no dependency on models or microphone.
- [ ] Commit: `feat: supervise local audio worker with recoverable jobs`.

### Task 6: Coordinate the flow and expose Tauri commands

**Files:** create `src-tauri/src/{service.rs,commands.rs}` and `src-tauri/tests/service.rs`; integrate into `lib.rs` and `capabilities/default.json`; implement `src/lib/tauri.ts`.

**Interfaces:** consumes `AttemptStore`, `AudioPort`, `WorkerPort` and all the types of section 3; produces `AppService`, the public commands and `practice://snapshot`. `PracticeClient` is the only Tauri access used by the interface.

- [ ] Write `can_record_with_worker_missing`, `failed_stop_keeps_previous_attempt`, `retry_transcription_reuses_audio`, `late_result_does_not_change_active_review`, `save_publication_can_retry`, `retranscribe_preserves_existing_review` and `close_finalizes_capture`. Essential assertions:

```rust
assert_eq!(snapshot.attempt.unwrap().id, previous_id);
assert_eq!(retry.input_path, original_normalized_path);
assert_eq!(after_late_result.attempt.unwrap().review.unwrap().text, "my words");
assert_eq!(pending_jobs, 0);
```

- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml --test service`; confirm failure before the coordination.
- [ ] Implement start/stop: end playback, reserve folder/ID and start capture. On stop, finalize, publish and only then activate the new answer. If the transcriber is ready, normalize and automatically start the transcription; otherwise, keep the audio with a transcribe action. `retry_save_recording` repeats the publication of an already finalized capture, without recording it again.
- [ ] Implement the question voice and the SHA-256 cache of text+voice+speed+model revision; `play_audio(question)` uses the cache or starts synthesis, validating the WAV before playing. `play_audio(response)` resolves only the original of the active answer; the frontend never provides arbitrary paths.
- [ ] Apply results only to the originating job/generation. Save the `Transcript` before publishing it; create an initial revision only if none exists. Preserve previous versions in a new transcription. A result without speech saves an explicit state, allows listening/recording again and does not create any note.
- [ ] Implement restore, refresh, review save and normal shutdown, with interrupted capture/playback finished events. The snapshot has an increasing `revision`; out-of-order events/commands do not replace a more recent state. Do not automatically activate a request after a timeout.
- [ ] Repeat all Rust tests and check `npm.cmd run typecheck`. Expected: exit 0. Open the app and confirm that the microphone/files work even if Python is not configured.
- [ ] Commit: `feat: connect local audio workflow to tauri commands`.

### Task 7: Practice interface and protection of reviews

**Files:** create `src/features/practice/{Practice.tsx,usePractice.ts,practice.css,Practice.test.tsx}`, `src/test/setup.ts`; wire `App.tsx` and the Tauri adapter.

**Interfaces:** consumes `PracticeClient` and `AppSnapshot`; `Practice({client}: {client: PracticeClient})` allows testing with a controlled adapter. `usePractice(client)` keeps the snapshot, the review text and the local writing state; it does not control the persistent state of the capture nor of the jobs.

- [ ] Write Testing Library tests: `manual_start_stop`, `shows_loading_without_freezing_editor`, `raw_transcript_is_readonly`, `save_failure_keeps_dirty_text`, `new_recording_with_dirty_review`, `close_with_dirty_review` and `older_snapshot_is_ignored`. Pin the observable behaviors:

```typescript
expect(screen.getByRole('button', {name: 'Terminar gravação'})).toBeEnabled();
expect(screen.getByLabelText('Transcrição automática')).toHaveAttribute('readonly');
expect(screen.getByLabelText('Revisão da transcrição')).toHaveValue('my edited words');
expect(client.startRecording).not.toHaveBeenCalled(); // enquanto a decisão sobre a revisão está pendente
```

- [ ] Run `npm.cmd test -- src/features/practice/Practice.test.tsx`; confirm behavior failures before implementing the experience.
- [ ] Implement the question and the actions “Ouvir pergunta”, “Iniciar gravação”, “Terminar gravação”, “Ouvir resposta”, “Parar áudio”, “Transcrever resposta”, “Guardar revisão” and “Reiniciar áudio local”, presented according to the state. Show recording duration, availability per component, loading/processing and errors with recovery. The automatic text remains read-only.
- [ ] Implement save/discard/cancel when starting another recording with a pending edit. Only start the capture after resolving the decision. Do not clear `dirty` when `saveReview` fails. Transcription events never replace an existing local edit.
- [ ] Intercept the normal close with `onCloseRequested`, prevent immediate closing and resolve pending reviews. After a valid choice, call `prepareClose`; close effectively only when it finishes successfully. If the user cancels, keep the window and the data. If there is a write error, keep the review editable and explain the error.
- [ ] Show review missing/pending/saved, without calling the saving of the text “confirmar” in this phase. Guarantee keyboard navigation, control labels and state legible beyond color; do not introduce scores, fictitious chat or AI feedback.
- [ ] Repeat `npm.cmd test`, `npm.cmd run typecheck` and `npm.cmd run build`. Expected: exit 0 and flow tests without real calls to the microphone/models.
- [ ] Commit: `feat: add local voice practice and transcript review interface`.

### Task 8: Real validation and reproducible instructions

**Files:** create `docs/phase-1/{setup.md,validation.md}`, `scripts/verify.ps1`; update `README.md`, `AGENTS.md` and the phase status only with evidence. Adjust `config/audio.example.json` and locks if the validation requires different compatibility.

**Interfaces:** consumes the integrated application and the CLI `python -m audio_worker.prepare --models-dir <absolute path>`. Produces verified instructions, measurements and the CA-01 to CA-15 matrix; does not produce phase 2 features.

- [x] Implement `scripts/verify.ps1` running, with exit code control and interruption at the first failure: typecheck, React tests, Vite build, cargo fmt --check, cargo clippy --all-targets -- -D warnings, Rust tests, pytest without the `models` marker and pip check. The success messages depend on all commands having finished with exit 0.
- [x] Document preparation: Python 3.11 `.venv`, locked dependencies, needed English resources, explicit preparation of the models, personal configuration outside Git and execution `npm.cmd run tauri -- dev`. Use paths with spaces in the example and do not require execution from an implicit directory to resolve models.
- [x] Run `.\scripts\verify.ps1`. Expected: all checks with exit 0. Run separately `.\.venv\Scripts\python.exe -m pytest audio-worker/tests/test_offline_models.py -m models -q` with prepared models; a skip here does not satisfy the real journey.
- [ ] Open the application and validate with the user at least three recordings: short, with technical terms and problematic. Test listening to real Kokoro, saving/restoring a review, repeating transcription without a new capture, removing/restoring the device and stopping/restarting the helper. Personal capture happens by manual action of the user, not by an automated test. **Pending user validation** (the window and the question were verified; the real recordings were not).
- [ ] Repeat the journey without network, after preparation, and with a voice resource missing. Expected: offline operation when prepared, explicit error when incomplete, without an attempt to download during practice. Verify that starting another recording or closing does not lose a review. **Pending in the window**; the engines and the missing voice error have already been verified by automation.
- [x] Record in `validation.md` hardware, versions, model IDs/revisions, audio duration, initial loading, synthesis and transcription cold and on repetition. Do not include personal audio in Git. Mark each CA as verified, failed or pending, with evidence; the acceptability of the wait depends on the user.
- [x] Optionally verify `npm.cmd run tauri -- build --no-bundle` to anticipate release compilation problems; do not present that binary as a standalone installer with Python included. The final package is from phase 5.
- [x] Update the status documentation only if all criteria are verified, or record the concrete blockers. Commit: `docs: record phase one setup and local audio validation`.

## 5. Spec coverage

| Criteria | Tasks and main proof |
| --- | --- |
| CA-01 | 1 and 7: window and exact question; real execution in 8. |
| CA-02 | 4 and 6: real Kokoro/cache; playback validated in 8. |
| CA-03, CA-04 | 1, 3, 6 and 7: exclusion of operations, manual capture and controls. |
| CA-05 | 2, 3 and 6: finalized file, publication and playback independent of transcription. |
| CA-06 | 4 and 8: Whisper CPU/int8 in English with real recordings. |
| CA-07, CA-08 | 2, 6 and 7: originals, versions, review and repetition without overwriting. |
| CA-09 | 2 and 6: atomic pointer, restore and new capture with safe publication. |
| CA-10 | 2, 3, 4 and 6: missing file/device/model, invalid audio and no_speech. |
| CA-11 | 5 and 6: death, timeout, generation and rejection of delayed results. |
| CA-12 | 5, 6 and 7: work outside the interface thread and progress states. |
| CA-13 | 4 and 8: complete local models and validation without network. |
| CA-14, CA-15 | 8: documentation, measurements, three recordings and validation by the user. |

## 6. Plan review and execution

- [x] Read the spec and map the 15 criteria to tasks.
- [x] Define files, shared contracts, reference versions and limits per operation.
- [x] Associate the five review risks with tests of the responsible tasks.
- [x] Distinguish tests with adapters from the integration with real hardware/models.
- [x] Keep SQLite, API, complete interview and final installer outside this phase.
- [x] Do the plan review directly, without delegating a second review.

Recommendation for future execution: **direct execution in the same session**, with `executing-plans`, because the tasks depend on the same contracts and it is important to control costs. Do not start the implementation as a consequence of creating this document; the user asked for the plan. Do not automatically start the following phases when this plan ends.

## 7. Verified technical references

- [Tauri: Windows prerequisites](https://v2.tauri.app/start/prerequisites/), [Rust commands](https://v2.tauri.app/develop/calling-rust/) and [API tests](https://v2.tauri.app/develop/tests/mocking/).
- [CPAL 0.18.2](https://docs.rs/cpal/0.18.2/cpal/), [Rodio 0.23.0](https://docs.rs/rodio/0.23.0/rodio/), [Hound 3.5.1](https://docs.rs/hound/3.5.1/hound/), [Rubato 5.0.1](https://docs.rs/rubato/5.0.1/rubato/) and [atomicwrites 0.4.4](https://docs.rs/atomicwrites/0.4.4/atomicwrites/).
- [faster-whisper: requirements and usage](https://github.com/SYSTRAN/faster-whisper).
- [Kokoro 0.9.4: Python requirements](https://pypi.org/project/kokoro/0.9.4/), [model/voice](https://huggingface.co/hexgrad/Kokoro-82M) and [builder with local files](https://github.com/hexgrad/kokoro/blob/main/kokoro/model.py).
- [Misaki: preparation of the English resources](https://github.com/hexgrad/misaki/blob/main/misaki/en.py).
- Versions consulted in the official registries [npm](https://registry.npmjs.org/), [crates.io](https://crates.io/), [PyPI](https://pypi.org/) and in the [stable Rust channel](https://static.rust-lang.org/dist/channel-rust-stable.toml).

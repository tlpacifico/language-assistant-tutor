# language-assistant-tutor

Desktop tutor for behavioral interviews in English, by voice, based on the
STAR structure. This repository contains the implementation of **phases 1 to 4**:
a Tauri 2 + React/TypeScript application that prepares a guided interview with
a bank of 20 questions, records the answers, plays the audio back, transcribes
locally with faster-whisper, allows reviewing and confirming each text and
stores the history in SQLite. The local mode runs on the computer. The optional
AI mode sends only the confirmed text needed for follow-ups and feedback through
OpenCode Go; the recordings stay local. The Go integration was authorized on
2026-11-04 to use the same modality as Marlo Expenses; see
[setup and validation](docs/opencode-go/validation.md).

The product context, the stack and the phase plan are in
[`docs/`](docs/) and in [`AGENTS.md`](AGENTS.md). The actual environment setup
is in [`docs/phase-1/setup.md`](docs/phase-1/setup.md) and the status of the
phase 1 acceptance criteria in [`docs/phase-1/validation.md`](docs/phase-1/validation.md).
Phase 2 is documented in [`docs/phase-2/setup.md`](docs/phase-2/setup.md)
(execution and data) and [`docs/phase-2/validation.md`](docs/phase-2/validation.md)
(matrix F2-01 to F2-16).

Phase 3 adds protected configuration, single follow-ups, feedback with
verified sources and observable usage/costs. See [setup](docs/phase-3/setup.md)
and [validation](docs/phase-3/validation.md). It was brought forward by user
authorization; the manual acceptances of the three phases remain pending.

Phase 4 adds B1/B2/C1 rewriting with Kokoro voice, new attempts
(practices) with their own evaluation, local comparison and supporting local
stories. See [setup](docs/phase-4/setup.md) and [validation](docs/phase-4/validation.md).
It is implemented and automatically verified, but **not accepted**: the F4-16
manual script and real paid inference are missing.

The layout was updated from `docs/tutor-layout`, with side
navigation, session preparation and adaptive views. See [validation and screenshots](docs/layout/validation.md).

## Requirements (Windows)

Verified on this machine (details and versions in
[`docs/phase-1/setup.md`](docs/phase-1/setup.md)):

- Windows 10/11 with the **Microsoft Edge WebView2** runtime.
- **Node.js** `24.11.1` and **npm** `11.6.2` (use `npm.cmd` in PowerShell).
- **Rust** `1.99.1` with the `x86_64-pc-windows-msvc` toolchain and the
  `rustfmt` and `clippy` components (pinned in `rust-toolchain.toml`).
- **Visual Studio** with C++ tools and the **Windows SDK** (on this machine:
  Visual Studio Community 2026 `18.10.12224.181`, MSVC `14.44.35207` and Windows
  SDK `10.0.26100.0`).
- **Python 3.11** (do not use the default 3.13) for the `.venv` with the helper.
- About **800 MB** free for the Whisper `small` and Kokoro-82M models.

Cargo is not in the `PATH` by default on this machine. In a new session:

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
```

## Installation and first run

```powershell
npm.cmd install

# Python environment (once): see docs/phase-1/setup.md
py -3.11 -m venv .venv
.\.venv\Scripts\python.exe -m pip install --upgrade pip
.\.venv\Scripts\python.exe -m pip install torch --index-url https://download.pytorch.org/whl/cpu
.\.venv\Scripts\python.exe -m pip install -r audio-worker/requirements.in
.\.venv\Scripts\python.exe -m pip install -e "audio-worker[test]"

# Models (the only step that needs the network; folder outside the repository)
.\.venv\Scripts\python.exe -m audio_worker.prepare --models-dir "$env:LOCALAPPDATA\language-assistant-tutor\Modelos de Voz"
```

Then create the personal configuration `audio.json` in
`%APPDATA%\dev.thacio.languageassistanttutor\audio.json` (or in the path of
`LANGUAGE_TUTOR_CONFIG`), with absolute paths. The template is in
[`config/audio.example.json`](config/audio.example.json); **do not commit** the
real file:

```json
{
  "schemaVersion": 1,
  "pythonExecutable": "C:\\Projetos\\language-assistant-tutor\\.venv\\Scripts\\python.exe",
  "modelsManifest": "D:\\Modelos de Voz\\language-assistant-tutor\\models.json"
}
```

Open the window:

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
npm.cmd run tauri -- dev
```

Without Python, `audio.json` or models, the window opens and indicates the
missing component; preparing the session, editing the question bank, reviewing
text and consulting the history remain available.

On the first opening of an interview, the application asks the helper to
load the voice models in the background (Whisper and Kokoro). On this machine
that is about 10 s warm and 14 s cold, with a peak of ~1.7 GiB of memory;
meanwhile the audio buttons wait, instead of failing with the helper
busy. After the first load, the models stay resident in the helper process
until the application closes.

On first startup, `phase1\` and `tutor.sqlite3` are created in the application
data folder; the `sessions\` folder appears on the first recording. See
[`docs/phase-2/setup.md`](docs/phase-2/setup.md) for the format and the meaning
of each path.

## Commands

| Goal | Command |
| --- | --- |
| All automated checks (stops at the first error) | `.\scripts\verify.ps1` |
| Window in development (Vite + Tauri) | `npm.cmd run tauri -- dev` |
| Frontend only in the browser | `npm.cmd run dev` |
| Type-check TypeScript | `npm.cmd run typecheck` |
| Frontend build (typecheck + Vite) | `npm.cmd run build` |
| Interface tests (Vitest) | `npm.cmd test` |
| Rust tests (all suites) | `cargo test --manifest-path src-tauri/Cargo.toml` |
| Rust formatting | `cargo fmt --manifest-path src-tauri/Cargo.toml` |
| Rust lint | `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` |
| Build without installer (not a standalone package) | `npm.cmd run tauri -- build --no-bundle` |
| Generate NSIS/MSI installers (release; installation still to be validated) | `npm.cmd run tauri -- build` |
| Create the Python helper environment | `py -3.11 -m venv .venv` |
| Prepare models offline (network) | `.\.venv\Scripts\python.exe -m audio_worker.prepare --models-dir <absolute folder>` |
| Python helper tests | `.\.venv\Scripts\python.exe -m pytest audio-worker/tests -q` |
| Real test with prepared weights | `.\.venv\Scripts\python.exe -m pytest audio-worker/tests/test_offline_models.py -m models -q` |
| Verify the Python environment | `.\.venv\Scripts\python.exe -m pip check` |

`.\scripts\verify.ps1` runs typecheck, React tests, Vite build,
`cargo fmt --check`, `clippy -D warnings`, Rust tests, pytest without the
`models` marker and `pip check`. Actual result of the most recent verification: 8/8 checks with
exit code 0 (275 Rust tests, 120 Vitest, 39 pytest without `models`); the duration depends
on the compilation cache and on a cold Vitest. The test with the
`models` marker is run separately, because it requires prepared weights and a manifest
given by `LANGUAGE_TUTOR_MODELS_MANIFEST`; on this machine it gave `2 passed`,
not skip.

## Current structure

```text
index.html, package.json, tsconfig.json, vite.config.ts
rust-toolchain.toml
scripts/verify.ps1            Automated checks with exit-code control
src/main.tsx                  React entry point
src/App.tsx                   Navigation: prepare, bank, interview, history
src/lib/contracts.ts          v1 contract types (phase 1 practice sessions, read-only)
src/lib/interview-contracts.ts Guided interview v2 contracts
src/lib/interview-client.ts   InterviewClient adapter over Tauri
src/features/sessions/        Session preparation and resuming
src/features/questions/       Editable question bank
src/features/interview/       Capture, review, confirmation and advancing
src/features/navigation/      Save/discard/cancel decision (navigate and close)
src/features/history/         History of sessions and phase 1 practice sessions
src/features/practice/        Later practice: practice, rewrite, comparison, usage
src/features/stories/         Local story bank and preparation support
src-tauri/src/                Domain, SQLite, artifacts, audio, worker, service, commands
src-tauri/tests/              275 domain, database, artifact, audio, worker, session, service tests
contracts/                    Shared examples of the v1 and v2 contracts
config/audio.example.json     Documented personal configuration (do not commit the real one)
audio-worker/                 Python helper (faster-whisper + Kokoro)
docs/phase-1/                 setup.md and validation.md
docs/phase-2/                 setup.md and validation.md
docs/releases/                Notes, artifacts and pending items for each release
```

The data of new sessions is stored in
`%APPDATA%\dev.thacio.languageassistanttutor\tutor.sqlite3` and the audio files in
`sessions\<session-id>\<session-question-id>\<recording-id>\`. The `phase1/`
tree of the data folder is preserved and is only read: previous
practice sessions remain viewable in the history. None of this is in the code tree.
Models, `.venv`, audio, database and `.superpowers/` are in `.gitignore`.

## Status

The implementation of **phase 1** is complete and the automated verification is
green (77 Rust tests, 15 React tests, 36 Python tests without the `models` marker, 2
real Python tests with prepared weights, clean `pip check`, release build without
installer — counts from that time; the current suite includes the phase 2 tests). The
actual preparation of the models was executed and the compatibility risk between
`transformers 5.22.1` and the Kokoro 0.9.4 weights did not materialize on this
machine.

Phase 1 is **not** accepted: the user validation with three
real recordings, listening to the voice, removing/restoring the microphone and the decision
on the wait (CA-14) are missing. See the honest matrix in
[`docs/phase-1/validation.md`](docs/phase-1/validation.md).

The implementation of **phase 2** (guided interview) is complete and the automated
verification is green: `.\scripts\verify.ps1` with 8/8 checks, 151 Rust tests,
53 Vitest tests and 36 Python tests without the `models` marker, plus 2 real
engine tests with prepared weights; release build without installer, startup and
shutdown of the binary with exit code 0. Phase 2 is **not** complete nor accepted: the
manual validation by the user is missing (real session of three questions, listening,
review/confirmation, leaving and resuming, history, session of one question,
insufficiency, editing the question bank, offline path and recovery). See
[`docs/phase-2/validation.md`](docs/phase-2/validation.md).

Phase 3 (AI tutor) is implemented and automatically verified; manual acceptance and real paid inference remain pending. The integration moved to OpenCode Go on 2026-11-04. See [phase 3 validation](docs/phase-3/validation.md) and [Go validation](docs/opencode-go/validation.md).

Phase 4 (improve and repeat) is implemented and automatically verified; it is not accepted: the F4-16 manual script and the real inference (fidelity, usefulness, wait, cost) are missing. See [phase 4 validation](docs/phase-4/validation.md).

On 2026-11-04 the first release was published, [v0.1.0](docs/releases/v0.1.0.md), with an NSIS installer, MSI and portable executable. The installer build and the binary smoke passed; the packaged installation, the upgrade over another version and the uninstallation remain to be validated.

Also on 2026-11-04 the [v0.2.0](docs/releases/v0.2.0.md) was published: voice models loaded when the interview opens (end of the wait for the first playback/transcription) and local validation of the OpenCode key `oc_sk_…` before any paid request, with actionable messages for 401 and for the follow-up block. Verification 8/8 checks (275 Rust, 120 Vitest, 39 Python), installer build and portable smoke with exit code 0. Installation of the packages remains to be validated and no paid request was made.

On 2026-11-05 the **article conversation** was implemented (new mode, outside the five phases): Articles page, automatic sending authorized per session, final feedback, corrections in branches, local recovery and tutor voice in blocks. Automated verification: `.\scripts\verify.ps1` with 8/8 checks (354 Rust, 147 Vitest, 56 Python without the `models` marker), release build `--no-bundle` and rehearsal with temporary data with exit code 0. **It is not accepted**: the real conversation with audio, voice and paid inference, the performance measurements and the visual validation are missing. See [setup](docs/article-conversation/setup.md), [validation](docs/article-conversation/validation.md) and [performance](docs/article-conversation/performance.md).

On 2026-11-05 the release [v0.3.0](docs/releases/v0.3.0.md) was published, with the article conversation (NSIS/MSI installers and portable with hashes in the document; portable smoke with exit code 0). The article conversation has no manual acceptance, installation of the packages remains to be validated and phase 5 was not started.

On 2026-11-06 the repository went from private to public, under the MIT license.

## License

Code distributed under the [MIT](LICENSE) license (copyright 2026 Thacio Pacifico).

The model weights (Whisper `small` and Kokoro-82M, downloaded by
`audio_worker.prepare`), the `.venv`, the recordings and the local database are
not part of this repository or of this license. Each model keeps the license of
its respective provider. The fonts in [`public/fonts`](public/fonts/) keep the
SIL Open Font License licenses that accompany them.

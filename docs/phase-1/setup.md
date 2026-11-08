# Phase 1 setup and execution on Windows

Verification date: 2026-11-02 · Branch: `feature/phase-1-local-audio`

This document records the environment and the commands that were really executed
on this machine for the Tauri application, the Python helper and the local models.
The acceptance results are in [validation.md](validation.md). The product context
and the phase rules are in the
[phase 1 spec](../superpowers/specs/2026-11-01-phase-1-local-audio-design.md).

## 1. Verified environment

| Component | Observed version |
| --- | --- |
| Windows | Windows 11 Enterprise, `10.0.26300` (build 26300) |
| CPU / memory | Intel Core i7-12700H (14 cores, 20 threads), 34,010,849,280 bytes of RAM |
| GPU | NVIDIA GeForce RTX 3060 Laptop and Intel Iris Xe; **not used** (engines on CPU) |
| WebView2 Runtime | `156.0.4314.50` / `156.0.4314.58` (runtime installed in `C:\Program Files (x86)\Microsoft\EdgeWebView\Application`) |
| Visual Studio | Visual Studio Community 2026 `18.10.12224.181`, MSVC C++ tools `14.44.35207` |
| Windows SDK | `10.0.26100.0` |
| Rust | `rustc 1.99.1` / `cargo 1.99.1`, toolchain `1.99.1-x86_64-pc-windows-msvc` (pinned in `rust-toolchain.toml`), `rustfmt`, `clippy` and `rust-std` components installed |
| Node / npm | `24.11.1` / `11.6.2` (always use `npm.cmd` in PowerShell) |
| Python | `3.11.9` in the `.venv`; the machine's default Python is `3.13.14`, so use `py -3.11` |
| PyTorch | `2.15.0+cpu` |
| Python engines | `faster-whisper 1.2.1`, `ctranslate2 4.8.4`, `kokoro 0.9.4`, `misaki 0.9.4`, `transformers 5.22.1`, `spacy 3.8.18` |

Verified notes:

- Cargo **is not on the `PATH`** in a new console. Before any Rust or `tauri`
  command:

  ```powershell
  $env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
  ```

  `scripts/verify.ps1` already adds that path when `cargo` is not
  found. `npm.cmd run tauri -- ...` **also requires** `cargo` on the `PATH`:
  without it the CLI fails immediately with `program not found` (observed in this session).
- The WebView2 runtime was already installed; nothing had to be installed for the
  window to open.
- It was not necessary to install eSpeak-NG on the system: the
  `espeakng-loader 0.2.4` package brings `espeak-ng.dll` and `espeak-ng-data` inside the
  `.venv`.
- This machine's `.venv` was created and populated in task 4 and was reused
  in this verification; `pip check` confirmed the installed set.

## 2. Creating the `.venv` (Python 3.11)

From the repository root. CPU PyTorch is installed **first**, from
the official index, so that the CUDA wheels are not brought in:

```powershell
py -3.11 -m venv .venv
.\.venv\Scripts\python.exe -m pip install --upgrade pip
.\.venv\Scripts\python.exe -m pip install torch --index-url https://download.pytorch.org/whl/cpu
.\.venv\Scripts\python.exe -m pip install -r audio-worker/requirements.in
.\.venv\Scripts\python.exe -m pip install -e "audio-worker[test]"
.\.venv\Scripts\python.exe -m pip check
```

Result observed during creation (`audio-worker/README.md` records the detail):
`torch-2.15.0+cpu`, then 100+ packages from `requirements.in` (no build
error), editable `audio-worker-0.1.0` with `pytest 9.2.0`, and
`No broken requirements found.`

To rebuild from the versioned lock, after CPU PyTorch is **pinned to the
lock version** (`torch==2.15.0+cpu`):

```powershell
.\.venv\Scripts\python.exe -m pip install torch==2.15.0 --index-url https://download.pytorch.org/whl/cpu
.\.venv\Scripts\python.exe -m pip install -r audio-worker/requirements.lock.txt --extra-index-url https://download.pytorch.org/whl/cpu
.\.venv\Scripts\python.exe -m pip install -e "audio-worker[test]"
.\.venv\Scripts\python.exe -m pip check
```

The `--extra-index-url` keeps PyPI as the main index for the remaining
lock versions and adds the CPU index only to resolve
`torch==2.15.0+cpu` (that wheel does not exist on PyPI). Without that parameter, pip
would look for a `+cpu` wheel on PyPI and fail. The sequence was executed in a
throwaway `.venv` in `%TEMP%` (356.8 s, with downloads; all steps with
exit 0, `No broken requirements found.` and `torch 2.15.0+cpu`) and the result
is recorded in [validation.md](validation.md); the throwaway `.venv` was
deleted afterwards.
The creation above (without the lock) resolves the current versions from the CPU index; it is the
rebuild from the lock that pins the exact set.

The resolved set (`audio-worker/requirements.lock.txt`) includes the English
extras of `misaki[en]` and the real transitive dependencies: `soundfile`,
`numpy`, `spacy`, `transformers`, `espeakng-loader`, `av`, `ctranslate2` and
`pytest`.

### Language resources

- **English spaCy**: `misaki.en.G2P` uses `en_core_web_sm`. It is a data resource,
  not a pip dependency, and it stays inside the `.venv` (not in `models/`). The
  `audio-worker-prepare` installs it explicitly; recreating the `.venv` requires
  repeating the setup.
- **eSpeak-NG**: included in the `espeakng-loader` wheel; verified on this machine
  in `C:\Repos\Thacio\language-assistant-tutor\.venv\Lib\site-packages\espeakng_loader\espeak-ng.dll`.

## 3. Preparing the models (the only step that uses the network)

```powershell
.\.venv\Scripts\python.exe -m audio_worker.prepare --models-dir "$env:LOCALAPPDATA\language-assistant-tutor\Modelos de Voz"
```

- The indicated folder is **absolute and stays outside the repository**. On this machine
  `C:\Users\Utilizador Exemplo\AppData\Local\language-assistant-tutor\Modelos de Voz`
  was used (it contains spaces, like the configuration example). `models/` inside the
  repository is also ignored by Git, but it was not used.
- The command downloads `Systran/faster-whisper-small` (revision
  `536b0662742c02347bc0e980a01041f333bce120`), the configuration, the weights
  `kokoro-v1_0.pth` and the voice `voices/af_heart.pt` from `hexgrad/Kokoro-82M`
  (revision `f3ff3571791e39611d31c381e3a41a3af07b4987`), installs
  `en_core_web_sm 3.8.0` when it is missing, validates eSpeak-NG, writes
  `<models-dir>\models.json` and ends with a fixed verification synthesis.
- Real result: **34.3 s** on the first run — measured with the temporary folder
  without spaces `%LOCALAPPDATA%\language-assistant-tutor\models`, before the
  files were moved to the final path with spaces (full download,
  spaCy installation and verification synthesis) — and **10.2 s** on the repeat, already
  on the final folder. Final size: 25 files,
  **814,144,526 bytes (776.4 MiB)**, of which
  `faster-whisper-small\model.bin` 461.1 MiB and
  `kokoro\kokoro-v1_0.pth` 312.1 MiB.
- The verification synthesis produced
  `<models-dir>\smoke-test\af_heart-smoke.wav` with 3,900 ms at 24 kHz.
- The `models.json` contains absolute paths with spaces (`whisperDir`,
  `kokoroConfig`, `kokoroWeights`, `kokoroVoice`), the two revisions and the resource
  inventory (`spacyModel`, `spacyModelVersion`, `espeakLibrary`, `espeakData`,
  `espeakFallback`).
- The command is idempotent as far as files go: repeating it validates the revisions on
  the Hugging Face Hub and reuses the local cache (on the second run the 6 Whisper
  files were recounted at ~200 files/s and the weights were not
  transferred again), but it **still contacts the Hub** and therefore requires
  the network — just like the first run. That is what happened in the
  10.2 s repeat.

The optional real test runs explicitly with the prepared manifest:

```powershell
$env:LANGUAGE_TUTOR_MODELS_MANIFEST = "$env:LOCALAPPDATA\language-assistant-tutor\Modelos de Voz\models.json"
.\.venv\Scripts\python.exe -m pytest audio-worker/tests/test_offline_models.py -m models -q
```

Result: `2 passed` in 12.18 s (question synthesis + transcription; silence →
`no_speech`). Without weights, the same command performs an explicit *skip* — a skip does not
prove the real path.

## 4. Personal `audio.json` configuration (outside Git)

The application reads the configuration once at startup, from
`app_config_dir()/audio.json` or from the path given by
`LANGUAGE_TUTOR_CONFIG`. On this machine the file ended up at:

```text
C:\Users\Utilizador Exemplo\AppData\Roaming\dev.thacio.languageassistanttutor\audio.json
```

Content used (absolute paths; the models folder contains spaces):

```json
{
  "schemaVersion": 1,
  "pythonExecutable": "C:\\Repos\\Thacio\\language-assistant-tutor\\.venv\\Scripts\\python.exe",
  "modelsManifest": "C:\\Users\\Utilizador Exemplo\\AppData\\Local\\language-assistant-tutor\\Modelos de Voz\\models.json"
}
```

This file **is not versioned** and must not be copied into the repository.
The generic example is in
[`config/audio.example.json`](../../config/audio.example.json). The Python module is
fixed (`audio_worker`), installed in the `.venv`; the process receives
`--models-manifest <path>` and uses the application's data folder as the working
directory, so the application can be opened from any directory.

After changing `audio.json` the application must be restarted. Without Python or
without models, the window opens and indicates the missing component; recording, listening and
saving revisions remain available.

## 5. Verifying the project

```powershell
.\scripts\verify.ps1
```

It runs, in order, and stops at the first error: `npm.cmd run typecheck`,
`npm.cmd test`, `npm.cmd run build`, `cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings`, `cargo test`, `pytest` without the
`models` marker and `pip check`. It only prints success if all of them end with exit 0.
Result on this machine: 8/8 checks with exit 0 (23.4 s with everything compiled and 116.4 s
when clippy and the tests needed to recompile and Vitest ran cold; the
duration depends on the cache).

## 6. Running the application

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
npm.cmd run tauri -- dev
```

`tauri dev` starts Vite on port 1420 and opens the «Tutor de
entrevistas» window with the fixed question
`Tell me about a time you solved a difficult problem at work.`

Release build without an installer (anticipates compilation problems; it is **not**
a standalone package with Python included — the final package belongs to phase 5):

```powershell
npm.cmd run tauri -- build --no-bundle
```

The binary ends up in `src-tauri\target\release\` and, to work, it still
depends on the `.venv`, on the `audio.json` and on the models prepared on this machine.

## 7. Real problems encountered

| Symptom | Cause | Resolution |
| --- | --- | --- |
| `failed to run 'cargo metadata' ... program not found` | `cargo` not on the `PATH` when calling the Tauri CLI | add `%USERPROFILE%\.cargo\bin` to the `PATH` (section 1) |
| `npm warn Unknown user config "python"` | global npm configuration on this machine, not the project's | warning only; it does not affect the commands |
| The first synthesis (and the verification synthesis) takes ~7–8 s to produce audio | initial Kokoro load on CPU | measured and recorded in [validation.md](validation.md); on the following calls the model is reused (~1.2 s per synthesis) |
| `UserWarning`/`FutureWarning` warnings from PyTorch and Misaki | third-party libraries during loading | they do not prevent synthesis or transcription; they were recorded |

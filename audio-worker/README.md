# audio-worker

Phase 1 local helper: a persistent Python 3.11 process that serves
transcription (faster-whisper, `small` model, CPU/int8) and speech synthesis (Kokoro-82M,
American English, `af_heart` voice, mono PCM16 WAV at 24 kHz) over JSON lines on
`stdin`/`stdout`.

## Installation (Windows)

CPU PyTorch must be installed first, from the official index, so that it does
not bring the CUDA wheels:

```powershell
py -3.11 -m venv .venv
.\.venv\Scripts\python.exe -m pip install --upgrade pip
.\.venv\Scripts\python.exe -m pip install torch --index-url https://download.pytorch.org/whl/cpu
.\.venv\Scripts\python.exe -m pip install -r audio-worker/requirements.in
.\.venv\Scripts\python.exe -m pip install -e "audio-worker[test]"
.\.venv\Scripts\python.exe -m pip check
```

Pinned versions: `faster-whisper==1.2.1`, `kokoro==0.9.4`, `misaki[en]==0.9.4`.
The resolved set (including `torch==2.15.0+cpu`, `ctranslate2`, `spacy`,
`transformers`, `soundfile`, `espeakng-loader` and `pytest`) is recorded in
`requirements.lock.txt`, generated with `pip freeze --exclude-editable` after a
clean `pip check`. To rebuild the environment from the lock:

```powershell
.\.venv\Scripts\python.exe -m pip install torch==2.15.0 --index-url https://download.pytorch.org/whl/cpu
.\.venv\Scripts\python.exe -m pip install -r audio-worker/requirements.lock.txt --extra-index-url https://download.pytorch.org/whl/cpu
.\.venv\Scripts\python.exe -m pip install -e "audio-worker[test]"
```

### Language resources

- **English spaCy (`en_core_web_sm`)**: required by `misaki.en.G2P`. It is not a
  pip dependency of `requirements.in` because it is a data resource;
  `misaki` tries to install it on its own (`spacy.cli.download`) on the first
  build of the `G2P`. Since that is an implicit download during practice, the
  worker checks the resource before building the pipeline and
  `audio-worker-prepare` installs it explicitly
  (`.\.venv\Scripts\python.exe -m spacy download en_core_web_sm`).
- **eSpeak-NG**: `misaki[en]` uses the `misaki.espeak.EspeakFallback` fallback
  for words outside the dictionary. On Windows it is **not** necessary to install
  eSpeak-NG on the system: the `espeakng-loader` package brings `espeak-ng.dll` and
  `espeak-ng-data` inside the wheel. Validated on this machine with
  `EspeakFallback(british=False)("difficult")` → `dˈɪfɪkᵊlt`.

## Offline model preparation

The only command that accesses the network. It downloads `Systran/faster-whisper-small`, the
`hexgrad/Kokoro-82M` configuration/weights and `voices/af_heart.pt`, installs the
English spaCy resource, validates the bundled eSpeak-NG, writes the manifest and performs
a fixed synthesis to reveal late dependencies:

```powershell
.\.venv\Scripts\python.exe -m audio_worker.prepare --models-dir "D:\Modelos de Voz\language-assistant-tutor"
```

The manifest (`models.json`, by default in `<models-dir>/models.json`) uses
absolute paths with spaces and contains `whisperDir`, `whisperModel`,
`kokoroConfig`, `kokoroWeights`, `kokoroVoice`, `revisions` and the
`resources` inventory (`spacyModel`, `spacyModelVersion`, `espeakLibrary`, `espeakData`,
`espeakFallback`). The weights stay outside the repository (`models/` is ignored).

## Service

```powershell
.\.venv\Scripts\python.exe -u -m audio_worker --models-manifest "D:\Modelos de Voz\language-assistant-tutor\models.json"
```

- One JSON message per line, `protocolVersion: 1`, `id`, `op`; operations
  `health`, `transcribe`, `synthesize` and `shutdown`.
- `stdout` carries only protocol; whatever the libraries write is sent
  to `stderr`, which the app drains continuously.
- `HF_HUB_OFFLINE=1` and `TRANSFORMERS_OFFLINE=1` are applied before any
  engine import, so practice never downloads anything.
- `health` checks files, voice, dependencies and English resources without
  loading models. `shutdown` answers `{"stopped": true}` and the process exits.
- The verified message examples are in
  [`contracts/worker-v1.examples.json`](../contracts/worker-v1.examples.json).

## Tests

```powershell
.\.venv\Scripts\python.exe -m pytest audio-worker/tests/test_protocol.py audio-worker/tests/test_engines.py -q
.\.venv\Scripts\python.exe -m pytest audio-worker/tests -q -m models
```

The unit tests do not need weights. `test_offline_models.py` is marked
with the `models` marker and only runs after `audio-worker-prepare`; without weights
it produces an explicit *skip*, never a false success.

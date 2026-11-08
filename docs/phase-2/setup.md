# Phase 2 setup and execution on Windows

Verification date: 2026-11-02 · Branch: `feature/phase-2-guided-interview`

This document records what changed for people using the application in phase 2
(guided interview), where the data lives and the commands that were really
executed on this machine. The criteria matrix F2-01 to F2-16 is in
[validation.md](validation.md). The base environment (Python, models, audio and
`audio.json`) remains the phase 1 one and is described in
[`docs/phase-1/setup.md`](../phase-1/setup.md); it is not repeated here.

Phase 2 is **not** complete: the implementation and the automated verification
are done, but the user's manual validation described in section 6 is missing.

## 1. What changed for the user

| Before (phase 1) | Now (phase 2) |
| --- | --- |
| One fixed question in English. | Session setup: category and number of main questions (three by default, one or another eligible number). |
| One answer per run, saved in JSON. | Interview with several questions, progress «Pergunta 2 de 3» and one session in progress at a time. |
| — | Editable bank of 20 questions in six categories, with competencies and criteria. |
| — | Saved review, immutable confirmation of the text and explicit advance, kept separate. |
| — | Session history with questions, recordings, automatic results, revisions and confirmed texts. |
| Practice sessions in `phase1/` (JSON). | The old practice sessions continue to be consulted in «Histórico», in read-only mode. |

The window pages are «Preparar», «Banco de perguntas», «Entrevista» and
«Histórico». The interface, the explanations and the errors are in European
Portuguese; the questions and the practice answers are in English.

What phase 2 **does not** do: follow-ups, feedback, evaluation, scores,
calls to the AI API, new attempts of confirmed answers, session deletion
and a standalone installer. A finished session is not an evaluated
interview; the history presents a summary with no score and no pending feedback.

## 2. Where the data lives

The paths are resolved by the operating system through Tauri's `app_data_dir()` and
`app_config_dir()`. In this development installation both point
to the same folder in `%APPDATA%`:

```text
%APPDATA%\dev.thacio.languageassistanttutor\
  audio.json                 # personal phase 1 configuration (not versioned)
  tutor.sqlite3              # authority for the phase 2 data
  tutor.sqlite3-journal      # DELETE journal while a transaction is open
  sessions\<session-id>\<session-question-id>\<recording-id>\
      original.partial.wav   # capture not yet finalized
      original.wav           # finalized WAV; never overwritten
      normalized.wav         # 16 kHz derivative, when needed
  phase1\                    # preserved practice sessions; read-only, except the cache
      attempts\...           # phase 1 practice sessions and revisions
      question-cache\...     # shared synthesis cache (write planned, R8)
```

- **`tutor.sqlite3`** stores the question bank, the fixed copies of each
  session, the session position and state, the recordings, the automatic
  transcripts, the append-only revisions, the immutable confirmations, the operations
  and the command receipts. It is the only authority for the new sessions; there is no
  second JSON with the same sessions.
- **`sessions\`** stores the audio. The path is assembled from
  identifiers generated in the core; the interface never sends paths. Only the
  finished `original.wav` is published as an answer; `original.partial.wav`
  is never presented as a valid answer.
- **`phase1\`** keeps the previous JSON format. The new application only reads the
  practice sessions: it lists them from the attempt folders (not only from
  `last-attempt.json`), opens originals, transcripts and revisions, and does not create
  sessions, confirmations or categories for that data. The only write in
  `phase1\` is the shared synthesis cache in `phase1\question-cache\`, foreseen
  in the technical plan (recorded deviation R8): the key includes text, voice and speed, so
  the cache does not reuse audio from another question.
- **Models and `.venv`** remain outside the data folder and outside the repository
  (see [phase 1](../phase-1/setup.md), sections 2 and 3).
- **None of this is versioned.** `.gitignore` still excludes `.venv/`,
  `models/`, `dist/`, `node_modules/`, the personal `audio.json` and `.superpowers/`.
  The recordings and the database must never be copied into the repository.

On the first startup the application creates `phase1\` and `tutor.sqlite3` (migration 001
plus the initial bank). The `sessions\` folder only appears on the first recording, when
reserving the original's directory. Migration 001 (with `user_version = 1`)
runs in its own transaction and the initial question bank runs afterwards in a
second idempotent transaction, marked by `questions-v1`: they are not a single
transaction (see the recorded deviation in the technical plan). A failure in either
of the two reverts only its own transaction and does not delete or recreate the existing
database. On this machine, the first startup of the phase 2 version created
`tutor.sqlite3` with 135,168 bytes, `user_version = 1` and 20 questions
(distribution 4/3/3/4/3/3), with no sessions.

## 3. Installing from a clean checkout

Starting from the repository root and a Windows with the phase 1 prerequisites
(WebView2, Node, Rust, Visual Studio C++, Python 3.11):

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
npm.cmd install

py -3.11 -m venv .venv
.\.venv\Scripts\python.exe -m pip install --upgrade pip
.\.venv\Scripts\python.exe -m pip install torch --index-url https://download.pytorch.org/whl/cpu
.\.venv\Scripts\python.exe -m pip install -r audio-worker/requirements.in
.\.venv\Scripts\python.exe -m pip install -e "audio-worker[test]"
.\.venv\Scripts\python.exe -m pip check

.\.venv\Scripts\python.exe -m audio_worker.prepare --models-dir "$env:LOCALAPPDATA\language-assistant-tutor\Modelos de Voz"
```

Then create `%APPDATA%\dev.thacio.languageassistanttutor\audio.json` with
absolute paths to the `.venv` Python and to the prepared `models.json`
(template in [`config/audio.example.json`](../../config/audio.example.json)).

Phase 2 adds no installation steps: the only new dependency is
`rusqlite 0.41.1` with the `bundled` feature, resolved by `Cargo.lock`
(it compiles the SQLite included in the library; it does not require SQLite installed on the
system). A `cargo build`/`npm.cmd run tauri -- dev` the first time compiles
that dependency and takes longer than in the following runs.

## 4. Running

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"   # Cargo is not on the PATH by default
npm.cmd run tauri -- dev
```

`tauri dev` opens the «Tutor de entrevistas» window on «Preparar». Without Python,
without `audio.json` or without models, the window opens and indicates the missing component;
setup, the bank, the review and the history remain usable. A database opening
failure shows a recoverable error with the action to retry the opening,
without closing the window.

Release build **without an installer** (it is not a standalone package; it depends on the
`.venv`, on `audio.json` and on this machine's models — distribution belongs to phase
5):

```powershell
npm.cmd run tauri -- build --no-bundle
```

The binary ends up in `src-tauri\target\release\language-assistant-tutor.exe`.

## 5. Verifying

```powershell
.\scripts\verify.ps1
```

It runs, in order, and stops at the first error: `npm.cmd run typecheck`,
`npm.cmd test`, `npm.cmd run build`, `cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings`, `cargo test`, `pytest` without the
`models` marker and `pip check`. Real result of this verification: **8/8 checks with exit
0** (151 Rust tests, 53 Vitest tests in 6 files, 36 Python tests
with 2 deselected, `pip check` clean). The detail and the output are in
[validation.md](validation.md).

The real test of the engines runs separately, with the prepared manifest:

```powershell
$env:LANGUAGE_TUTOR_MODELS_MANIFEST = "$env:LOCALAPPDATA\language-assistant-tutor\Modelos de Voz\models.json"
.\.venv\Scripts\python.exe -m pytest audio-worker/tests/test_offline_models.py -m models -q
```

Real result: `2 passed` in 48.14 s (it was not a skip). Without a valid `models.json` the
same command performs an explicit skip; a skip does not prove the real path.

## 6. Manual validation still missing (user action)

With the application open via `npm.cmd run tauri -- dev` and the models prepared:

1. **Session of three real questions** in «Preparar»: category
   (for example «Todas as categorias») and a quantity of three. Answer the three
   questions with voice, including one answer with technical terms. Record the
   duration of each audio and the loading, synthesis and transcription times,
   and the perception of the wait.
2. **Listen** to the synthesized question (and repeat it without a new synthesis) and listen to the
   recorded originals of each answer.
3. **Review and confirm**: correct the automatic text, save the review,
   confirm the answer and only then advance. Confirm that the confirmed answer
   becomes read-only.
4. **Exit and resume**: after confirming an answer and before advancing,
   close the application and reopen it; the session must return to the persisted step, with
   the confirmation pending explicit advance.
5. **Finish and consult the history**: complete the last question, consult
   the session summary and listen to the originals from the history.
6. **Session of one question** and **another eligible quantity**; in a category
   with fewer questions than requested, confirm the insufficiency warning and
   that it only starts after an explicit adjustment of the quantity.
7. **Edit the bank** (add or change a question) with a session already
   started and confirm that that session's copy does not change.
8. **Offline path**: turn off the network and repeat a session; the application must
   not try to download anything or contact AI services.
9. **Transcription unavailable**: stop the helper (or use audio without
   recognized speech) and confirm that the explicit manual review is possible, that the
   `no_speech` result is preserved and that nothing is presented as a
   pronunciation problem.
10. **Write error and recovery**: with a review not yet saved, close the
    window and confirm the save/discard/cancel options; confirm that a
    failure keeps the text and the window. If a simulated failure is used (for example,
    locking the file), identify it as a simulation — it is not real hardware.

The recordings and the database stay in `%APPDATA%\dev.thacio.languageassistanttutor\`,
never in the repository. The phase 1 manual criteria that remain pending
(CA-02, CA-03, CA-05, CA-06, CA-10, CA-13 partial and CA-14) are **not**
closed by this phase 2 validation; see
[`docs/phase-1/validation.md`](../phase-1/validation.md).

## 7. Real problems encountered in this phase

| Symptom | Cause | Resolution |
| --- | --- | --- |
| `failed to run 'cargo metadata' ... program not found` | `cargo` not on the `PATH` when calling the Tauri CLI | add `%USERPROFILE%\.cargo\bin` to the `PATH` |
| `npm warn Unknown user config "python"` | global npm configuration on this machine, not the project's | warning only; it does not affect the commands |
| First compilation slower after phase 2 | initial compilation of `rusqlite`/`libsqlite3-sys` with `bundled` SQLite | expected; it is cached in the following runs |
| A database created by a future version is refused | `PRAGMA user_version` higher than the supported one | intended behavior: recoverable error, without writing or recreating the database |
| Rust tests with the simulated helper can leave Python processes | test hygiene, not application behavior | terminate the orphan processes; also recorded in phase 1 |

# Phase 1 validation — CA-01 to CA-15 matrix

Date: 2026-11-02 · Branch: `feature/phase-1-local-audio` · Environment described in
[setup.md](setup.md).

This document distinguishes **what was verified** (with the command and the result)
from what **only the user can validate** (real microphone, listening to the voice, three
recordings and acceptability of the wait). Phase 1 **is not complete**: criteria
CA-02, CA-03, CA-05, CA-06, CA-10 and CA-14 depend on that manual validation
and are marked as pending; CA-13 has partial verification on the
engines and remains pending in the window.

## 1. Environment and models used

| Element | Value |
| --- | --- |
| System | Windows 11 Enterprise `10.0.26300`, build 26300 |
| CPU / RAM | Intel Core i7-12700H (14 cores / 20 threads), 34,010,849,280 bytes |
| GPU | not used; engines forced to CPU (`device="cpu"`, `compute_type="int8"`) |
| Rust / Node / Python | `1.99.1` / `24.11.1` (npm `11.6.2`) / `3.11.9` |
| PyTorch | `2.15.0+cpu` |
| faster-whisper | `1.2.1`, multilingual `small` model, revision `536b0662742c02347bc0e980a01041f333bce120` |
| Kokoro / Misaki | `0.9.4` / `0.9.4`, Kokoro-82M revision `f3ff3571791e39611d31c381e3a41a3af07b4987`, voice `af_heart`, `lang_code="a"`, speed `1.0` |
| spaCy | `3.8.18`, resource `en_core_web_sm 3.8.0` |
| transformers | `5.22.1` (risk recorded in task 4: **the Kokoro weights loaded and synthesized without error**) |
| Models directory | `C:\Users\Utilizador Exemplo\AppData\Local\language-assistant-tutor\Modelos de Voz` (outside the repository) |
| Manifest | `...\Modelos de Voz\models.json`, `preparedAt` `2026-11-02T01:16:48Z` |

Real setup (`python -m audio_worker.prepare --models-dir ...`): **34.3 s**
on the first run (download + spaCy + verification synthesis) and **10.2 s** on the
repeat, with 25 files and **814,144,526 bytes (776.4 MiB)** —
`faster-whisper-small\model.bin` 461.1 MiB and `kokoro\kokoro-v1_0.pth` 312.1 MiB.
The verification synthesis produced 3,900 ms of audio at 24 kHz. No personal
recording was created or saved.

## 2. Automated verification performed

| Command | Result |
| --- | --- |
| `.\scripts\verify.ps1` | 8/8 checks with exit 0 (typecheck, 15 Vitest tests, Vite build, `cargo fmt --check`, `clippy -D warnings`, 77 Rust tests, 36 pytest tests without `models`, `pip check`) |
| `.\.venv\Scripts\python.exe -m pytest audio-worker/tests/test_offline_models.py -m models -q` | `2 passed` in 12.18 s (question synthesis + transcription; silence → `no_speech`); **it was not a skip** |
| Lock rebuild in a throwaway `.venv` in `%TEMP%` | all steps with exit 0 in 356.8 s: `torch==2.15.0` from the CPU index, `-r requirements.lock.txt --extra-index-url .../cpu`, editable and `pip check` (`No broken requirements found.`); final versions `torch 2.15.0+cpu`, `faster-whisper 1.2.1`, `kokoro 0.9.4`, `spacy 3.8.18`, `transformers 5.22.1`; the throwaway `.venv` was deleted |
| `.\.venv\Scripts\python.exe -m audio_worker.prepare --models-dir ...` | exit 0; manifest and verification synthesis |
| `npm.cmd run tauri -- build --no-bundle` | exit 0 in 29.2 s; binary `src-tauri\target\release\language-assistant-tutor.exe` |
| Startup of the release binary | «Tutor de entrevistas» window created, process responding, helper started with the `.venv` and the `models.json` with spaces; normal shutdown terminated the helper |
| Offline test with blocked sockets (`health` + synthesis + transcription) | exit 0: `health` `ready/ready`, correct synthesis and transcription, without any network connection |
| Manifest with a missing voice | `health` → synthesis `missing`; `synthesize` → `MODEL_MISSING` error with the path of the missing voice and setup instruction |
| Search for calls to AI APIs (`opencode`, `openai`, `api.deepseek`, `https://api`) in the Rust/TS/Python code | no occurrence |

### Measured times (automation, CPU, synthesized audio)

Measurements made with `EngineRegistry` over the prepared weights. They are an indication
of performance, they **do not** replace the user's validation or a human
recording.

| Operation | Loading (`loadMs`) | Processing (`processingMs`) |
| --- | --- | --- |
| `health` (without loading models) | — | 4 ms (wall) |
| Question synthesis — 1st call (cold) | 7,731 ms | 1,150 ms |
| Question synthesis — repeat | 0 ms (reused) | 1,169 ms |
| Synthesis of long text (26.1 s of audio) | 0 ms | 6,318 ms |
| Question transcription — 1st call (cold) | 4,181 ms | 1,838 ms |
| Question transcription — repeat | 0 ms | 1,504 ms |
| Transcription of 26.1 s of English audio | 0 ms | 3,057 ms |
| Repeat of the long-text transcription | 0 ms | 3,082 ms |

In an earlier run on this same day, the cold load was 6,989 ms
(Kokoro) and 919 ms (Whisper); the variation is real and is due to file cache and
antivirus. The 180 s limit per model was not approached.

The transcription returned exactly
`Tell me about a time you solved a difficult problem at work.` and the long text
was transcribed without translation or rewriting (only punctuation/commas from the engine).

## 3. Criteria matrix

Legend: **verified** = there is executed and reproducible evidence;
**partial** = part of the criterion has executed evidence and the rest
depends on user validation;
**pending** = requires user action (hardware, listening or decision);
**failed** = there was contrary evidence.

| CA | Criterion | Status | Evidence / what is missing |
| --- | --- | --- | --- |
| CA-01 | The application opens on Windows and shows the correct fixed question. | **verified** | Release binary started: `Tutor de entrevistas` window, `Responding=True`, helper started and normal shutdown. Test `shows_the_fixed_question` pins the `<h1 lang="en">` with `Tell me about a time you solved a difficult problem at work.` Only human visual confirmation is missing, which does not change the criterion. |
| CA-02 | Kokoro generates English voice for the question and the user can listen to it again. | **pending** | Real generation verified (2 `models` tests, measurements above; PCM16 mono 24 kHz). Listening and repeating through the window depend on the user; the `question-cache/<sha256>.wav` cache is covered by Rust tests. |
| CA-03 | Manual recording start and end, with visible state and duration. | **pending** | `manual_start_stop` (Vitest) and the Rust capture tests pass, but they use simulated audio. A real recording with a microphone is missing. |
| CA-04 | No playback during capture and no duplicate operations. | **verified** | `playback_is_blocked_while_recording`, `recording_stops_playback`, `starting_recording_stops_playback` and `playback_is_refused_while_recording` (Rust) prove the rule in the core; the interface allows starting the recording with audio playing and the core stops the output. |
| CA-05 | Finished audio saved and playable even with a transcription failure. | **pending** | `failed_stop_keeps_previous_attempt`, `save_publication_can_retry`, atomic write/publication and playback from the original are tested; what is missing is listening to the recorded answer on real hardware and with the transcriber unavailable. |
| CA-06 | faster-whisper transcribes a real answer in English, without translation or evaluation. | **pending** | Real transcription of synthesized English (question and 26.1 s text) verified, without translation/rewriting. A human recording is missing (the three recordings of the spec). |
| CA-07 | Automatic text preserved and review saved separately. | **verified** | `raw_transcript_is_readonly`, `save_failure_keeps_dirty_text`, storage tests (`append_does_not_overwrite_raw`, `failed_review_write_preserves_saved_text`) and `retranscribe_preserves_existing_review`. |
| CA-08 | Repeating a failed transcription does not require a new recording; later results do not erase revisions. | **verified** | `retry_transcription_reuses_audio`, `late_result_does_not_change_active_review`, `retranscribe_preserves_existing_review` (Rust) and the review flow in the interface. |
| CA-09 | A new recording and a restart do not erase the last answer or the saved review. | **verified** | `failed_publish_preserves_last`, `replaces_existing_json_atomically`, `corrupt_pointer_is_recoverable`, `restore_interrupted_job`, `allocate_never_reuses_a_directory`. Note: the restart was proven with controlled storage, not with a real recorded answer. |
| CA-10 | Empty audio, missing files, absence of a device and absence of speech have an explicit and recoverable behavior. | **pending** | Verified: `empty_audio_is_invalid`, `restore_reports_missing_original`, real `no_speech` with silence, `MODEL_MISSING` error with a missing voice, `failed_start_resamples_microphone` (availability is re-sampled after a failure) and `timed_out_stop_recovers_capture` (an expired audio command does not block recording or closing; the finished original is published as incomplete). Removing/restoring the real device is missing. |
| CA-11 | A helper failure or hang allows recovery and a repeat, without applying delayed results. | **verified** | `worker_crash_releases_job`, `timeout_rejects_old_generation`, `shutdown_kills_unresponsive_worker`, `non_json_stdout_is_protocol_error`, `failed_restart_discards_the_pending_health_handle` and `late_result_does_not_change_active_review`. Executed with the controlled stub, not with the real helper blocked by the interface. |
| CA-12 | Usable interface during loading and processing. | **verified** | `shows_loading_without_freezing_editor` keeps the editor editable and the incompatible buttons disabled; the worker is a separate process (measurements of 7.7 s of loading do not block the window). Real perception is left to the user. |
| CA-13 | With the models prepared, the path works without the network and does not call AI services. | **partial — verified in the engines/audio; the repeat through the window is missing** | Run with blocked sockets: `health` `ready`, correct synthesis and transcription; `HF_HUB_OFFLINE=1`/`TRANSFORMERS_OFFLINE=1` applied before the imports; no reference to AI APIs in the code. Still missing is repeating the complete path in the window with the network off (step 5 of section 4), which remains pending. |
| CA-14 | Real recordings and measured times recorded; the user considers the wait acceptable. | **pending** | The times above were measured with synthesized audio. The three real recordings are missing (short, with technical terms and problematic), the duration of each one and the user's decision about the wait. |
| CA-15 | Verified instructions to set up and run on the development Windows. | **verified** | [setup.md](setup.md) records the commands really executed, the observed versions, the path with spaces, `audio.json` outside Git and `.\scripts\verify.ps1` (8/8 checks). |

## 4. Manual validation still missing (user action)

With the application open via `npm.cmd run tauri -- dev` and the `audio.json` from
section 4 of [setup.md](setup.md):

1. Listen to the question and confirm that the English voice is intelligible; listen to it
   again without a new synthesis.
2. Record three answers — short, with technical terms and a problematic
   one (prolonged silence or device removed halfway). Record the duration and the
   result shown (transcription, `no_speech`, recovered incomplete audio).
3. Save a revision, close the application, reopen it and confirm that the answer and
   the review are still there. Repeat the transcription of an already transcribed answer and
   confirm that the saved review is not replaced.
4. Remove and restore the microphone; stop/restart the helper; confirm that the
   interface recovers and allows a repeat without recording again.
5. Repeat the path with the network off and confirm that nothing tries
   to download. Repeat with the missing voice (temporarily rename
   `voices\af_heart.pt`) and confirm the explicit error.
6. With a review not yet saved, start another recording and close the window;
   confirm the save/discard/cancel options and that a write failure
   keeps the text.
7. Decide whether the measured wait allows practicing comfortably (CA-14). Without
   that decision, the phase is not marked as complete.

The real recordings stay in the application's data folder
(`%APPDATA%\dev.thacio.languageassistanttutor\phase1`), never in the repository.

## 5. Recorded limits and risks

- `transformers 5.22.1` with Kokoro 0.9.4: the weights loaded and synthesized
  successfully on this machine, including the smoke test and two test runs. The risk
  from task 4 did not materialize, but it remains a combination outside
  Kokoro's original base.
- Third-party warnings during loading (`UserWarning` from PyTorch's LSTM,
  `FutureWarning` from `weight_norm`/`torch.jit.script`, `DeprecationWarning` from
  Misaki) do not prevent operation and are not handled in this phase.
- The initial cold load of Kokoro (~7 s) is the most visible wait before
  the first playback of the question; Whisper varied between ~0.9 s and ~4.2 s
  between runs.
- Rust tests that use the stub left orphan `python -m audio_worker`
  processes from earlier runs on this day; they were terminated manually. It is
  test hygiene to review, not an application behavior.
- Nothing in this validation proves pronunciation quality, fluency or confidence —
  outside the phase scope, as the spec requires.

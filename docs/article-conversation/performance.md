# Article conversation performance

Status on 2026-11-05: **instrumentation and measurement tool ready; real measurements still to do.** None of the values below was measured with real recordings; what exists is what the code now does and how to measure it.

## What is measured in each turn

The service records, in closed local diagnostics operations (see [diagnostics](../diagnostics/validation.md)), only the duration and a stable code — never text, title, paths or audio:

| Operation | What it measures |
|---|---|
| `article_transcribe` | from the request to the helper until the transcript result |
| `article_reply` | from the request to the provider until the tutor's reply is applied (opening and answers) |
| `article_first_audio` | from the start of the synthesis until the first block delivered to the player |
| `article_feedback` | from the report request until the result |

The test `turn_metrics_use_closed_operations_with_durations_and_no_content` (in `src-tauri/tests/article_service.rs`) checks that only these operations appear, with duration, and that the content never reaches the record.

## Measure the transcript and the voice

```powershell
.\scripts\benchmark-audio.ps1 -Recordings D:\Gravacoes\medicao
```

- The folder is indicated explicitly (only the top-level `*.wav`); the script refuses the application's personal sessions.
- It compares `beam_size=5` (current) with `beam_size=1` (candidate), keeping English, VAD, the `small` model and CPU/int8.
- Per configuration and recording: one cold pass (new model) and five warm ones. It shows each value, the median and the worst time. **p95 is not calculated** with five samples.
- With `-VoiceText` it also measures the first voice block and the full synthesis.
- The results only have durations, stay in a local folder and are not sent or versioned.

Text fidelity is compared by hand, with a human reference and technical terms, outside the script.

## Decision

**Keep the baseline (`beam_size=5`).** The `beamSize` parameter is optional in the `transcribe` request (accepts 1 or 5) and the engine default was not changed: real recordings and the fidelity comparison that condition it are missing. Target values are only defined after measuring.

## To do (requires real recordings and models)

- Run the script with short, long and technical-term recordings; record the individual values, the median and the worst time here.
- Compare the `beam_size=1` text with the `beam_size=5` text and with the human reference before any default change.
- Assess the voice by the time to the first sound and by the breaks between blocks (listening).

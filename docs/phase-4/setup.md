# Phase 4 setup and execution

Date: 2026-11-03. Phase 4 (improve and repeat) was implemented at the user's request («termine as tarefas») after the spec and the plan had been reviewed. **It is not accepted or complete**: the manual acceptances of phases 1, 2 and 3 remain pending and phase 4 has its own pending items in [validation.md](validation.md). Phase 5 was not started.

Documents: [spec](../superpowers/specs/2026-11-03-phase-4-improve-repeat-design.md) and [technical plan](../superpowers/plans/2026-11-03-phase-4-improve-repeat-implementation.md).

## Run

The prerequisites, models and AI configuration of [phase 2](../phase-2/setup.md) and [phase 3](../phase-3/setup.md) still apply. There are no new dependencies.

```powershell
npm.cmd install
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"   # if Cargo is not on the PATH
npm.cmd run tauri -- dev
```

Automated checks and build without installer:

```powershell
.\scripts\verify.ps1
npm.cmd run tauri -- build --no-bundle
```

## What changes in the flow

Since the approved change of 2026-11-04, the rewrite of the original is also available during an AI interview, after closing the question's follow-up, through **Avaliar e sugerir melhorias**. The request evaluates first and only then asks for the rewrite; the results stay separate and reusable. If the evaluation finishes and the rewrite fails, repeating asks only for what is missing. Opening or resuming the panel makes no paid calls. Practices still require a completed session. See [design](../superpowers/specs/2026-11-04-per-question-ai-coaching-design.md) and [validation](../ai-coaching/validation.md).

- **Later practice** (from a confirmed question of a completed session): see the origin (initial answer, follow-up or previous attempt), request a rewrite at B1, B2 or C1, listen to the suggestion with Kokoro, open a **practice** (new attempt) and compare. B1 is the initial value; the level is a language goal and not a CEFR diagnosis.
- A practice has its own recording, automatic transcript, review and confirmation. It does not reopen the original session, does not change `position` or the question counter and never creates a follow-up. Only one unconfirmed practice exists at a time and it can only be started when no interview is running. It can be resumed or abandoned (without deletion).
- **Practice evaluation** and **rewrite** are explicit requests, never automatic on confirmation, with one remote call at a time and no automatic paid retries. Valid results are immutable; a repeated request for the same origin/level/versions reuses the result.
- **Comparison** is local, between initial, follow-up, attempts and suggestion, with explicit versions and pending items. It does not claim improvement from textual difference alone and does not consume the API.
- **Stories** (local bank): title, factual STAR notes and skills, with validation; they do not require a complete STAR. They can be chosen as preparation support before a practice recording. The support is fixed by snapshot at the start of the capture; editing the story afterwards does not change the snapshot. Stories are exclusively local: no story field or identifier enters the HTTP payloads, the transcript, the editor or the confirmation.
- **Practice usage** presented separately from the original interview, by distinct IDs, with unknown labeled as unknown.

## Persistence

`tutor.sqlite3` moves to schema **v3** (`003_improve_repeat.sql`) in a transactional migration (empty base, v1 and v2; future versions refused). New tables: `practices`, `stories`, `story_versions`, `preparations`, `preparation_captures`, `practice_reports`, `evaluation_families`, `rewrites`, `rewrite_audio`, `practice_preferences`; `answer_slots` and `ai_requests` were rebuilt to support practice owner and new purposes. Previous data (sessions, originals, reports, receipts) is preserved.

Audio stays local. Only the necessary text and the criteria go to the API, and only after the confirmation and consent that already exist.

## Known limitations

- No real key and no paid inference during implementation: the transport was tested with loopback HTTP and fake credentials.
- `build --no-bundle` does not validate an installer or a packaged installation.
- Factual fidelity of the rewrites, usefulness, wait, real cost and Kokoro voice depend on human validation.

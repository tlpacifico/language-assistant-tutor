# Plan — local diagnostics

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to run task by task.

**Goal:** investigate failures during real use, preserving diagnostics between builds.

**Architecture:** Rust writer with a bounded queue, closed-field events and local export; React/IPC boundaries and the helper supervisor feed the writer. Build metadata produced by build.rs.

**Tech Stack:** existing React/TypeScript, Rust/Tauri 2 and Python; no new dependencies.

**Spec:** [design](../specs/2026-11-04-local-diagnostics-design.md).

## Global Constraints

European Portuguese. Data in `app_data_dir()/diagnostics`; stable Tauri identity; five files of 1 MiB, 30 days. No personal content and no remote sending. Preserve earlier work on the current branch, which contains changes still to be consolidated; direct execution requested by the user.

## Review Focus

- Restarting with a new version keeps the earlier events (Rust reopen test).
- Unavailable disk does not interrupt operations (Rust write failure test).
- External events with secrets do not enter the files (Rust and React closed-field tests).
- Export includes the earlier events and does not replace files (Rust barrier/export test).
- A React failure allows exporting without automatically restarting and losing drafts (error boundary test).

### Task 1: storage and builds

**Files:** create `src-tauri/src/diagnostics.rs`, `src-tauri/tests/diagnostics.rs`; change `src-tauri/build.rs`, `src-tauri/src/lib.rs`.

**Interfaces:** `Diagnostics::open(root, build)`, `record(event)`, `status()`, `export(directory)`; `BuildInfo::current()`; bounded writer, shared state and barrier for export.

- [x] Write tests for preservation between versions, privacy, rotation, retention, export and disk failure; run `cargo test --manifest-path src-tauri/Cargo.toml --test diagnostics`, observe the failure before the implementation.
- [x] Implement writer and metadata without new dependencies; run the same battery until it passes.

### Task 2: capture and interface

**Files:** change `src-tauri/src/{lib,domain}.rs`, `src-tauri/src/worker/mod.rs`, `audio-worker/src/audio_worker/__main__.py`, `src/{main,App}.tsx`, `src/lib/interview-client.ts`; create `src/lib/diagnostics.ts`, panel and error boundary in `src/features/diagnostics/`, and their tests.

**Interfaces:** Tauri commands `diagnostics_status`, `diagnostics_record`, `diagnostics_export`; events only allowed categories/codes. Export returns a local path; the panel allows indicating a directory.

- [x] Write React tests for capture without personal content, IPC failures and React recovery; run `npm.cmd test -- src/features/diagnostics src/lib/diagnostics.test.ts` and observe the failure.
- [x] Implement capture of Rust codes, panic without message, Python errors with their own frames, IPC wrapper and global capture; integrate panel and recovery; pass the tests.

### Task 3: validation and documentation

**Files:** create `docs/diagnostics/validation.md`; update `AGENTS.md` and `docs/tech-stack.md`.

- [x] Run `scripts/verify.ps1`, release build `--no-bundle`, isolated smoke test, check the exported JSON and local links; record real results and manual limits.
- [x] Review the diff of this change exclusively; preserve earlier changes and do not push without a request.

## Execution record

The checkboxes and the validation will be updated with evidence. The explicit request includes implementing, so no new approval pause is introduced. The current branch contains necessary work already authorized; a clean checkout that excludes it is not created.

- Tasks 1 and 2: focused tests failed before the implementation and passed afterwards. Aggregated verification: 8/8, 262 Rust, 117 React, 36 Python. IPC interfaces and writer checked together.
- Final independent review: four important problems fixed, with reproduction and regression tests (see validation). No minor problems deferred.
- Execution decision: keep the changes on the existing branch, without automatic commit or push, to preserve the earlier work still to be consolidated.
- Rehearsal decision: use `app.appDirectoriesOverride` in a temporary build configuration and compile the normal configuration again at the end. On Windows, the system's known directories are not necessarily isolated by changing only APPDATA/LOCALAPPDATA.

- Task 3: isolated build, three WebView2 rehearsals with exit code 0 and persistence between restarts, final normal build with exit code 0 (1m01s), absence of the rehearsal directory in the final binary and local links checked. Evidence in `docs/diagnostics/validation.md`.

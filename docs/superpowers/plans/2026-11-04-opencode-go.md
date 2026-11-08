# Plan — OpenCode Go

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to execute directly.

**Goal:** use the Go modality and the headers of the Marlo integration, with our own identity.

**Architecture:** Go default and one-time adaptation in SQLite; context persisted in the service passes to the HTTP transport; existing configuration/consent.

**Tech Stack:** existing Rust/Tauri, SQLite and React, no new dependencies.

**Spec:** [design](../specs/2026-11-04-opencode-go-design.md).

## Global constraints and review focus

Preserve the protected key, answers and history; invalidate consent when the destination is adapted; no automatic call. Do not copy credentials or Marlo's identity. Test custom settings and a later explicit choice, stability per conversation and usage without fixed prices.

### Task 1 — context and transport

Files: `src-tauri/src/ai/transport.rs`, `src-tauri/src/service/ai.rs`, `src-tauri/tests/ai_service.rs`.

Interfaces: add `conversation_id: String` to `TransportRequest`; `run_attempt` uses the practice when it exists, otherwise the session; the transport sends its own User-Agent and a stable session header, without changing the body.

- [x] RED tests: local HTTP receives path/headers, IDs equal in repetitions and distinct between conversations, zero Go cost preserved.
- [x] Implement and verify `cargo test --manifest-path src-tauri/Cargo.toml --lib` and the service tests.

### Task 2 — configuration and one-time adaptation

Files: `src-tauri/src/ai/mod.rs`, `src-tauri/src/db/ai.rs`, new tests `src-tauri/tests/opencode_go.rs`, `src-tauri/tests/ai_usage.rs`; `src/features/ai/AiSettings.tsx`, React fixtures/tests.

Interfaces: Go defaults without a fixed price list; `Database::ai_settings()` adapts the old reference configuration once, using a marker in `ai_receipts`, incrementing the version and clearing consent/price list. It does not touch the system key. Later configurations are respected.

- [x] RED tests for the default, idempotent adaptation, preservation of custom configuration and later choice, preserved credential and mandatory new authorization.
- [x] Implement, adjust formula tests to use an explicit test price list and verify the affected tests.

### Task 3 — delivery

- [x] Update the stack, the phase 3 setup and validation and the diagnostics; record the replacement of the earlier decision by the current authorization.
- [x] Run `scripts/verify.ps1`, check links/diff and generate the release `--no-bundle`. Preserve the earlier executable if it is open, without closing the user's session.

## Execution

Direct execution on the existing branch to preserve earlier changes. The user explicitly chose this integration, so the work proceeds without a new approval pause.

Result on 2026-11-04: the three tasks completed. The RED tests failed first for the predicted reasons (nonexistent context field, default by usage, no adaptation); after the implementation the focused transport, service, configuration and usage tests passed. `scripts/verify.ps1` finished with 8/8 checks and exit code 0 (270 Rust, 119 React, 36 Python); the release build `--no-bundle` finished with exit code 0 and the smoke test of the binary exited with code 0. Complete evidence in [Go validation](../../opencode-go/validation.md).

Only what depends on the provider and on real use remained unverified: acceptance of the tutor traffic in the Go modality, the real cost on the account and the semantic usefulness. No paid request was made. This change does not start phase 5.

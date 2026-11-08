# Phase 4 validation

Update of 2026-11-04: the rewrite of the original after closing a question in a running AI interview has its own evidence in the [coaching validation](../ai-coaching/validation.md). Practices still require a completed session; the manual acceptance of this phase remains pending.

Date: 2026-11-03. Status: **implemented and automatically verified; not accepted or complete.** The implementation was authorized by the user. All human validation is missing (F4-16), as are real paid inference and the semantic fidelity of the rewrites. The pending items of phases [1](../phase-1/validation.md), [2](../phase-2/validation.md) and [3](../phase-3/validation.md) remain.

Three levels of evidence, which do not replace one another:

1. **Automation** (this document): real SQLite and temporary files, simulated audio, fake credentials and controlled loopback HTTP.
2. **Real inference**: not run. No real key, provider call or real cost.
3. **Human acceptance**: not run. The user's favorable report of 2026-11-03 (phase 3) does not cover phase 4.

## Actual commands (2026-11-03)

- `.\scripts\verify.ps1`: **8/8 checks, exit 0 in 41.7 s** (final run). Counts: **246 Rust tests** (sum of the 25 `cargo test` suites/binaries, all `ok`), **97 Vitest in 11 files**, **36 pytest** (2 `models` deselected), clean `pip check`, `tsc`, Vite build, `cargo fmt --check` and `clippy -D warnings` clean.
- The first run failed in check 4 (`cargo fmt --check`: formatting of two assertions in `src-tauri/tests/practice_service.rs`). Fixed with `cargo fmt` (commit `fix:`) and the repeated full aggregate turned green.
- `npm.cmd run tauri -- build --no-bundle`: exit code 0, release profile in 1m11s (1m15s in total) and `language-assistant-tutor.exe` generated.
- Smoke of the release binary with empty temporary `APPDATA`/`LOCALAPPDATA` (no audio configuration, no personal data): window alive after 8 s, `CloseMainWindow=True`, exit within 15 s with **code 0**; temporary folders removed. This is not installer validation.
- `git diff --check` clean. The 36 `interview_*` commands invoked in `src/lib/interview-client.ts` are all registered in Rust.
- The `-m models` test (real weights) was not repeated in this phase; the Python helper did not change.

## Matrix F4-01 to F4-16

| Criterion | Automated evidence (suites) | Human / real pending item |
| --- | --- | --- |
| F4-01 | `phase4_migration` (empty base, v1, v2, rollback, future version, SQL invariants, practice-slot composite FK), `database`, `sessions`, `answers`, `ai_migration` regressions | Update a real personal database (not done) |
| F4-02 | `rewrite_validation`, `practice_ai`: fixed source/level, payloads by origin, exclusion of the previous answer when rewriting an attempt | Fidelity with real answers |
| F4-03 | `rewrite_validation`: schema, citations and invalid segments refused; malicious instructions in the answer stay in the data message | Human verification of refusal, uncertainty, team vs. individual, numbers and negative/qualitative result |
| F4-04 | `practice_ai`, `practice_outputs`, `Practice/RewritePanel.test.tsx`: B1 by default, B1/B2/C1, reuse without a new charge, immutable variants, local preference | Usefulness of each level |
| F4-05 | `practice_outputs`: synthesis text without gaps/citations/explanations; local synthesis without new inference; UI | Real Kokoro listening |
| F4-06 | `practice_service`: only a confirmed question of a completed session, without reopening, without changing the counter, without a follow-up | Flow through the window |
| F4-07 | `practice_service`, `stories`: capture/transcript/review/confirmation, snapshot write failure keeps manual capture, preserves audio | Real write failure and real restart |
| F4-08 | `practice_service`, `Practice.test.tsx`: one active practice, abandonment without deletion, resumption, without interfering with the interview | Real resumption |
| F4-09 | `practice_ai`: attempt role, single source, own report, retry without repeating valid results or a new recording | Real evaluation of the attempt |
| F4-10 | `practice_outputs`, `Comparison.test.tsx`: local comparison, different rubrics and pending items, without claiming improvement from textual difference | Usefulness of the comparison |
| F4-11 | `stories`, `Stories.test.tsx`: validation, editing, historical snapshot, use without a complete STAR | Use with real stories |
| F4-12 | `stories` (`story_marker_never_leaks_into_sources_or_answers`), `rewrite_validation` (body without `ONLY_IN_STORY_742`, without `storyId`/`snapshot`/`practiceId`/`sessionId`), `Stories.test.tsx`; later editing does not change the snapshot | Facts only in the notes never treated as said (human judgement) |
| F4-13 | `practice_ai`, `practice_service`, `RewritePanel.test.tsx`: generations, answer for another level/practice does not switch the selection, repeated commands and old clicks without duplicating effects | Real clicks |
| F4-14 | `practice_ai`, `ai_service`: timeout, 401/403/429/5xx, invalid JSON, unavailable credential, failed publication and retry without network, shutdown; no automatic resend | Real network/storage failures |
| F4-15 | `ai_usage`, `practice_outputs`, `TrainingUsagePanel`: usage by disjoint IDs, partial subtotals, unknown is not zero, practice separate from the interview | Compare with real billing |
| F4-16 | Not replaceable by automation | Script below; **not run** |

## Windows manual script (F4-16) — pending

Prerequisites: application in `tauri dev` or release binary, local models, and, for the steps with AI, a stored key and consent (explicit authorization from the user before any cost). Start with test text, without personal data at risk. Do not send keys through chat.

1. AI session with one main question and one follow-up; confirm both and end the session.
2. In the later practice, confirm that the origin (initial, follow-up) is identified and that the original session does not change (counter, position, report).
3. Use an answer without numbers, a negative one and one with a learning: the rewrite does not invent numbers, responsibilities or results and flags gaps.
4. Request B1, B2 and C1; check that the variants are kept, that repeating the same request does not charge again and that changing level during a request does not switch the selection.
5. Listen to the suggestion (Kokoro): only the suggested text, without gaps or explanations.
6. Open a practice, repeat with your own words, review and confirm; open a second practice on another question and confirm that only one is pending at a time.
7. Create a story with facts that were not said (use an exclusive term) and choose it as support: the term does not appear in the transcript, in the editor, in the confirmation or in the evaluation/rewrite.
8. Record again, exit and resume the practice; abandon another.
9. Disconnect the network: without credential/network, consultation, comparison and history continue; the remote request fails explicitly, without resending, and is repeated later without a new recording.
10. Evaluate the attempt and compare with initial/follow-up/suggestion; confirm that the comparison does not claim improvement from textual difference alone.
11. Measure wait (rewrite and evaluation), factual fidelity, usefulness and cost (compare with real billing; unknowns remain unknown).

Only this script, with authorization for paid inference, allows F4-16 to be accepted. Until then phase 4 is not marked as accepted or complete.

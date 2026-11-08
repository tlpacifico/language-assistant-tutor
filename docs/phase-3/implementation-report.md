# Phase 3 implementation report

Date: 2026-11-02. Branch `feature/phase-3-ai-tutor`. No commits, push, paid inference or reading of personal credentials by the tests.

## Internal decisions and deviations

- One agent implemented; the coordinator reviews the code and the evidence. The user's authorization to bring phase 3 forward preserves the previous manual acceptances.
- `answer_slots` is the authority for the answer target; the legacy internal column `session_question_id` in the audio tables keeps the name to reuse the pipeline, but the FK points to the slot. `answerId` and `currentAnswer` make the targets explicit in the public contracts. The main questions and the progress remain separate.
- `QuestionDetail.followUp` reuses the answer DTO, with `initialAnswerId` identifying the main one. There is no artificial main question for the follow-up.
- The AI tables/receipts are separate from the local jobs. The result awaiting publication stays in memory until commit or shutdown; after a crash the operation remains uncertain. It is not promised to persist data that was not written.
- Feedback is JSON validated in Rust with field and evidence whitelists. Literal validation does not prove relevance or semantic fidelity.
- `reqwest` 0.13.5 (JSON/Rustls) and `keyring` 3.6.3 Windows were added, without changing the existing engines. Prettier was used as a transient tool, without a new project dependency.

## Tests and process

TDD was partial. The initial migration test failed with version 1 before the implementation; a partial feedback test identified a race in the initial state of the batch and went green after the correction. Several HTTP, contract, cost and UI tests were added after the code and are not presented as complete red/green cycles. The comparison of the schema/DTO fixtures also found v1 assertions that were updated to v2, keeping exact contract equality.

Commands executed: specific Rust tests `ai_migration`, `ai_service`, `ai_validation`, `ai_usage`, `database`, `interview_contracts`; internal HTTP tests with `--lib ai::transport`; specific React tests `Ai.test.tsx` and `Interview.test.tsx`; `npm.cmd run typecheck`; `cargo fmt`; `cargo clippy --all-targets -- -D warnings`; full Rust regression. The final consolidated evidence is recorded in the [validation](validation.md).

Not exercised: real paid inference, real protected storage with a personal key, real microphone/playback, model semantics/usefulness and installed package. These are explicit pending items, not failures hidden by mocks.

Final evidence: 8/8 aggregate in 77.6 s (175 Rust, 57 React, 36 Python), `--no-bundle` build with exit 0 and isolated smoke with shutdown/exit 0. The restart tests, shutdown during batch and configuration/consent replay also passed.

The technical review identified a race between the shutdown and the reservation of the next report. The test closing_queued_before_batch_reservation_prevents_new_attempts reproduced the error (one external call when zero were expected). The reservation and the admission to the transport started checking the shutdown under transitions, released before the HTTP wait. The test went green and the later aggregate passed 8/8 in 77.6 s (175 Rust, 57 React, 36 Python).

After the correction: `--no-bundle` rebuild with exit 0 in 1m18s and final smoke with temporary APPDATA/LOCALAPPDATA; shutdown and exit 0, without personal data.

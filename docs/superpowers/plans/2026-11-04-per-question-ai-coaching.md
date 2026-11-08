# Implementation plan — per-question evaluation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans for direct execution, task by task, with tests before the implementation.

**Goal:** present evaluation, corrections and rewrite during the interview, after closing each question.

**Architecture:** reuse persisted reports and rewrites. Add a command to evaluate a single question and a panel that coordinates the two operations by explicit action. Keep the final feedback command and the completed-session restriction for creating practices.

**Stack:** existing React/TypeScript, Rust/Tauri 2 and SQLite; no new dependency.

**Spec:** [approved design](../specs/2026-11-04-per-question-ai-coaching-design.md), authorized by the user on 2026-11-04 («pode avancar»).

## Global constraints

- Confirmed text and closed follow-up before the evaluation/rewrite.
- Explicit action and existing consent before paid calls; no request when mounting or resuming a page.
- Preserve sources, evidence, costs, local mode and recovery; no automatic repetition.
- Execute directly on a dedicated branch in the current checkout, per the authorization to advance autonomously; without publishing and without real paid calls.

## Review focus

- A pending follow-up cannot originate an evaluation or a definitive rewrite.
- Advancing to the next question during the network request does not associate the result with the new question.
- A valid evaluation followed by a failed rewrite does not charge the evaluation again.
- Resuming a page only queries persisted results.
- A transcript received after creating an empty draft fills the editor without losing local edits.

### Task 1: evaluation and rewrite of a closed question

**Files:** `src-tauri/src/db/ai.rs`, `src-tauri/src/db/rewrites.rs`, `src-tauri/src/service/ai.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`, `src/lib/interview-client.ts`, `src/test/fake-interview-client.ts`, `src-tauri/tests/ai_service.rs`, `src-tauri/tests/practice_ai.rs`.

**Interfaces:** `evaluateQuestion(AiQuestionCommand): Promise<FeedbackView>` / `AppService::evaluate_question`; reuse `reserve_ai(..., "feedback", ...)` and `request_rewrite`.

- [x] Add tests for evaluation before the end, blocking before resolving the follow-up, reuse in the final report and rewrite of a closed question; run and observe the failures.
- [x] Implement eligibility in the core, individual command and Tauri/client registration; allow rewrite of closed AI originals without anticipating the creation of practices.
- [x] Run the relevant Rust tests and check sources and immutability.

### Task 2: evaluation panel and improved version

**Files:** create `src/features/ai/QuestionCoaching.tsx` and tests; adapt `FeedbackPanel.tsx`, `RewritePanel.tsx`, `src/features/interview/Interview.tsx`, contracts and fake client when necessary.

**Interfaces:** the panel receives `client`, `snapshot` and `onSnapshot`; it uses the individual command, existing results and `RewritePanel` for variants and voice.

- [x] Test explicit click, evaluation/rewrite sequence, partial failure, resumption without calls and question change during a request; observe the failures.
- [x] Implement the panel with the «Avaliar e sugerir melhorias» button, cost warning, results per question and independent recovery.
- [x] Integrate only when the follow-up is closed in AI mode; clarify the optional role of the transcript review.
- [x] Run Vitest and the TypeScript/Vite build.

### Task 3: editor and final verification

**Files:** `src/features/interview/useInterview.ts`, interview tests, validation documentation and documents of the affected phases.

- [x] Reproduce an empty draft followed by an automatic transcript/review; preserve the edit in progress and the saved review.
- [x] Fix reconciliation and origin message only where the reproduction demonstrates the defect.
- [x] Run `.\scripts\verify.ps1`, review the diff and the record of commands; document real results and manual pending items.

## Execution record

- Preparation: checkout initially clean except for the design of this task; direct execution authorized. Branch `feat/per-question-ai-coaching`.
- Decision: the approved design serves as the authority; the plan details the already authorized execution, without a new approval round.
- Shared interfaces: task 2 consumes the individual command of task 1; task 3 verifies the review used in task 2. No incompatible contracts identified in the preparation.

- Tasks 1–3: executed and verified automatically. Final run of verify.ps1: 8/8, 252 Rust, 112 React and 36 Python; exit code 0 in 41.8 s. See docs/ai-coaching/validation.md.
- Decision after regression: require a closed follow-up also in the feedback sources of completed AI sessions; fix the artificial stories scenario, which omitted that step, instead of weakening the rule.
- Integration: changes available on the local branch, without publishing and without changing the release executable in use.

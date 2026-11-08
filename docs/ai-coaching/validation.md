# Validation — per-question evaluation and corrections

Date: 2026-11-04. Implementation authorized by the user («pode avancar»).
The [design](../superpowers/specs/2026-11-04-per-question-ai-coaching-design.md) and the [plan](../superpowers/plans/2026-11-04-per-question-ai-coaching.md) describe this change to phases 3 and 4. Manual acceptance and paid inference remain pending.

## Change

After confirming the answer and closing the follow-up, the AI mode presents **Avaliar e sugerir melhorias**. The click requests an evaluation of the question and, if that evaluation is valid, a rewrite at the saved level (B1 initially). Explained corrections, sources and the improved version stay separate from the original. The consultation does not call the provider; the final feedback reuses the valid reports. Creating practices still requires a completed session.

The core adds `interview_ai_evaluate_question`, reusing the `feedback` purpose, the existing tables and the consent, source and usage checks. There is no new migration and no new application dependency. The evaluation task still publishes the result if the frontend stops waiting. On leaving the question, the interface does not later start the rewrite from that click.

The editor reconciliation was also fixed: an automatic revision received after an empty draft exists fills in the text, the version and the origin. A local unsaved edit is preserved. The regression test reproduced the empty editor and the manual warning observed in the image.

## Automated evidence

- `.\scripts\verify.ps1`: **8/8 checks, exit 0, 41.8 s** in the final run. **252 Rust tests**, **112 React tests in 13 files**, **36 Python tests** without the `models` marker (2 deselected), `pip check` clean, TypeScript, Vite build, `cargo fmt --check` and Clippy with `-D warnings` approved.
- `git diff --check`: clean. Check of the local links of the seven updated functional documents: valid; the **56 Tauri commands** invoked by the client are registered in Rust.
- In the first aggregate run, `story_marker_never_leaks_into_sources_or_answers` failed because the scenario seeded a completed AI session without a closed follow-up step. The scenario now records the explicit dismissal, keeping the core protection and the check that notes do not enter the sources. The isolated test and the following aggregate run passed.
- The initial attempts to run several Cargo commands while other tests still used the same executables hit LNK1104/Windows error 32. The following commands ran sequentially; the final run did not show those errors.

- New core tests in `ai_service`: closed question eligible before the end; follow-up not yet confirmed blocks definitive sources; rewrite stored and reused; evaluation reused in the final report; ID used in the dismissal refused; late result keeps its origin and does not block the next capture.
- React tests in `QuestionCoaching`: explicit click, corrections and suggestion kept separate, resume without sending, partial failure without repeating the evaluation, leaving the question without a delayed rewrite, pending follow-up and partial usage after a failure.
- Regression in `Interview`: a transcript received after an empty draft fills the editor and confirms with the correct origin/transcript. The existing tests preserve local edits during writes and transcripts.
- Static review of the complete change: no concrete problems identified; it does not replace tests or manual acceptance.

The simulations use the existing controlled adapters: real SQLite and temporary artifacts, suspended/scripted transport and fake credentials. The existing HTTP tests still verify the transport. No personal credentials were consulted and no paid calls were made.

## Manual script — pending

1. Run the updated checkout with `npm.cmd run tauri -- dev`. A release binary already open keeps the implementation it started with; this verification does not replace it.
2. Configure the protected key and the consent through the application. Start a **Tutor com IA** interview with two questions; record, check the transcript without correcting the English errors actually spoken and confirm.
3. Resolve the follow-up (answer/confirm when it exists). Request **Avaliar e sugerir melhorias**; check the citations, the explained corrections, the separation of style and the factual fidelity of the improved version.
4. Listen to the suggestion; switch B1/B2/C1 through explicit requests. Confirm that consulting stored versions is not charged again.
5. Advance and finish the interview; generate the missing feedback and check that the first evaluation was reused.
6. Simulate network unavailability and resume: the originals and the valid results are kept, a failed rewrite does not require evaluating again and there are no automatic resends when opening the panel.
7. Measure the wait, the usefulness and the real usage. Confirm an unknown cost as unknown and the comparison with the billing. The automated tests do not validate the semantic usefulness of the provider.

This change does not accept phases 1–4, does not validate real audio or distribution and does not start phase 5.

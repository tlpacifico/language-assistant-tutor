# Phase 2 validation — matrix F2-01 to F2-16

Date: 2026-11-02 · Branch: `feature/phase-2-guided-interview` · HEAD at the start
of this verification: `f2eeb20` · Environment described in [setup.md](setup.md).

This document distinguishes **what was verified by automation** (actual command,
test and result) from what **only the user can validate** (sessions with real
voice, listening, waiting, the offline journey through the window and a real
restart). Phase 2 **is not complete or accepted**: the implementation and the
automated verification are done, but the manual validation of section 4 remains
pending. The existence of this matrix does not replace that validation.

Legend of states:

| State | Meaning |
| --- | --- |
| **verified** | There is executed and reproducible evidence covering the criterion in full. |
| **partial** | Part of the criterion has executed evidence; the remainder requires user action. |
| **pending** | It can only be demonstrated by the user (real voice, listening, waiting or decision). |
| **failed** | There was contrary evidence. No criterion ended in this state. |

## 1. Environment and scope

The environment is that of phase 1, on the same machine (see
[`docs/phase-1/setup.md`](../phase-1/setup.md)), with a single new dependency:

| Element | Observed value |
| --- | --- |
| Windows / Rust / Node / Python | Windows 11 Enterprise `10.0.26300`; `rustc 1.99.1`; Node `24.11.1` (npm `11.6.2`); Python `3.11.9` in the `.venv` |
| SQLite in Rust | `rusqlite 0.41.1` with `bundled`, `libsqlite3-sys 0.39.1` compiled in the release build |
| Models | Whisper `small` and Kokoro-82M prepared outside the repository (phase 1) |
| Personal configuration | `audio.json` in `%APPDATA%\dev.thacio.languageassistanttutor\` (not versioned) |
| Database | `%APPDATA%\dev.thacio.languageassistanttutor\tutor.sqlite3` |

No personal recording was created in this verification. The database and the
directories were created by the first startup of the phase 2 version and
inspected in read-only mode.

## 2. Automated verification executed

| Command | Exit | Result |
| --- | --- | --- |
| `.\scripts\verify.ps1` | 0 | 8/8 checks in 18.5 s (147 Rust tests, 43 Vitest, 36 pytest without `models`, clean `pip check`) |
| `.\.venv\Scripts\python.exe -m pytest audio-worker/tests/test_offline_models.py -m models -q` | 0 | `2 passed` in 48.14 s — **it was not a skip** (with `LANGUAGE_TUTOR_MODELS_MANIFEST` for the prepared manifest) |
| `npm.cmd run tauri -- build --no-bundle` | 0 | Release build in 1m 17s; binary `src-tauri\target\release\language-assistant-tutor.exe` |
| Startup and shutdown of the release binary | 0 | «Tutor de entrevistas» window, process responding, helper started and finished, process exit code 0 |
| Inspection of `tutor.sqlite3` after the startups | 0 | `user_version = 1`; 20 questions (4/3/3/4/3/3); one `questions-v1` marker; 0 sessions |
| Search for `opencode`, `openai`, `api.deepseek`, `https://api` in Rust/TS/Python | 0 occurrences | No AI client or call in the code of this phase |

### 2.1 `.\scripts\verify.ps1`

Real output (excerpts; the complete output is in the task report at
`.superpowers/sdd/2026-11-02-phase-2-guided-interview-implementation/task-10-report.md`):

```text
[1/8] TypeScript (tsc --noEmit)                OK (0,6 s)
[2/8] Testes React (Vitest)                     OK (8,6 s)
      Test Files  6 passed (6)
           Tests  43 passed (43)
[3/8] Build do frontend (typecheck + Vite)      OK (1,2 s)
[4/8] Formatação Rust (cargo fmt --check)       OK (0,4 s)
[5/8] Lint Rust (cargo clippy -D warnings)      OK (0,7 s)
[6/8] Testes Rust (cargo test)                  OK (5,0 s)
[7/8] Testes Python (pytest sem a marca models) OK (1,3 s)
      36 passed, 2 deselected in 0.67s
[8/8] Dependências Python (pip check)           OK (0,8 s)
      No broken requirements found.

Verificação concluída: 8/8 checks com saída 0 em 18,5 s.
```

Rust suites of this run (147 tests, 0 failures): `lib` 20, `answers` 16,
`artifacts` 10, `audio` 11, `database` 7, `domain` 8, `interview_contracts` 4,
`legacy` 4, `questions` 6, `service` 34, `sessions` 8, `storage` 12, `worker` 7.
The two deselected Python tests are those of the `models` marker, run separately
in section 2.2.

### 2.2 Real engine test (`-m models`)

Real output:

```text
..                                                                       [100%]
2 passed, 5 warnings in 48.14s
```

The warnings are third-party and already known (`UserWarning` from PyTorch's
LSTM, `FutureWarning` from `weight_norm`/`torch.jit.script`, `DeprecationWarning`
from Misaki). Without the prepared manifest the same command explicitly skips;
this result is `passed`, not `skipped`.

### 2.3 Release build without installer

Real output (excerpts):

```text
   Compiling libsqlite3-sys v0.39.1
   Compiling rusqlite v0.41.1
    Finished `release` profile [optimized] target(s) in 1m 17s
       Built application at: C:\Repos\Thacio\language-assistant-tutor\src-tauri\target\release\language-assistant-tutor.exe
EXIT=0
```

This build is **not** a standalone installer and does not validate the phase 5
distribution: the binary still depends on the `.venv`, on `audio.json` and on the
models prepared on this machine.

### 2.4 Startup and shutdown of the binary

Real output of the second startup (the first one created the database):

```text
auxiliares antes: 0
base antes: True
pid: 43048
janela: [Tutor de entrevistas]
a responder: True
auxiliares durante a execucao: 2
fecho pedido: True
processo terminou: True
auxiliares depois do fecho: 0
codigo de saida do processo: 0
```

Normal shutdown finalizes the pending capture, terminates the helper and exits
with code 0; no `python -m audio_worker` process was left orphaned. The two
processes during the run are the `.venv` launcher and the Python that runs the
module.

Inspection of the database after the first startup and again after the second
(read only):

```text
user_version: 1
tabelas: command_receipts, confirmations, content_seeds, operations, questions,
         recordings, reviews, session_questions, sessions, transcripts
perguntas: 20
categorias: [('change_adaptation', 3), ('conflict_collaboration', 4),
             ('failure_learning', 3), ('leadership_initiative', 3),
             ('priorities_deadlines', 3), ('problem_solving', 4)]
sessoes: 0
seed: [('questions-v1',)]
```

Repeating the startup kept 20 questions and a single seed marker — the initial
bank was neither duplicated nor reset over changes.

### 2.5 Re-run after fixing the findings of the branch review

The final review of the branch found three important problems and two
documentation inconsistencies; this section records the re-run after the
corrections. Real output (excerpts):

```text
[1/8] TypeScript (tsc --noEmit)                 OK (1,3 s)
[2/8] Testes React (Vitest)                     OK (19,3 s)
      Tests  50 passed (50)
[3/8] Build do frontend (typecheck + Vite)      OK (4,7 s)
[4/8] Formatação Rust (cargo fmt --check)       OK (1,5 s)
[5/8] Lint Rust (cargo clippy -D warnings)      OK (5,9 s)
[6/8] Testes Rust (cargo test)                  OK (8,6 s)
[7/8] Testes Python (pytest sem a marca models) OK (3,2 s)
[8/8] Dependências Python (pip check)           OK (1,5 s)

Verificação concluída: 8/8 checks com saída 0 em 46,1 s.
```

Rust suites of this re-run (151 tests, 0 failures): `lib` 20, `answers` 18,
`artifacts` 10, `audio` 11, `database` 7, `domain` 8, `interview_contracts` 4,
`legacy` 4, `questions` 6, `service` 36, `sessions` 8, `storage` 12, `worker` 7.
The two new `answers` tests cover the operation recorded as `running` and closed
with times/error; the two new `service` tests cover the recovery of an operation
interrupted at startup and the closing of a transcription with its times. The
tables in section 2 describe the previous run; this re-run is the most recent
evidence.

## 3. Criteria matrix

Link to the tasks of the [technical plan](../superpowers/plans/2026-11-02-phase-2-guided-interview-implementation.md)
and to the tests actually executed. The test names are those of the Rust suites
(`src-tauri/tests/*.rs`), Vitest (`*.test.tsx`) or internal unit tests.

| F2 | Criterion | Tasks | State | Evidence / what is missing |
| --- | --- | --- | --- | --- |
| F2-01 | The initial bank contains 20 questions in the six categories; repeating the startup neither duplicates nor restores edits. | 1, 2 | **verified** (automation) | `questions::seed_is_once_and_preserves_edits` (20 questions, stable IDs, reopening does not duplicate, edit preserved, single marker), `questions::bank_covers_six_categories` (4/3/3/4/3/3), `questions::incomplete_seed_rolls_back_without_marker`, unit test `db::questions::tests::embedded_seed_is_valid_and_unpadded`. Real startup twice: 20 questions, one `questions-v1` marker. |
| F2-02 | Adding and editing text, category, competencies and criteria persists after a restart, with validation of the required fields. | 2, 7 | **verified** (automation) | `questions::blank_fields_are_rejected`, `question_trims_only_outer_whitespace` (persistence after reopening), `stale_edit_does_not_overwrite`; `interview_contracts::question_validation_trims_outer_spaces_and_requires_content`; Vitest `QuestionBank.test.tsx`: `bank_edit_survives_reload`, `bank_rejects_empty_fields`, `creates_a_question_with_competencies_and_criteria`, `keeps_the_editor_open_when_the_save_fails`. |
| F2-03 | Initial quantity three; sessions with one, three and another eligible quantity; explicit insufficiency, without silent reduction or repetition. | 3, 7, 10 | **partial** | Rule and interface covered: `sessions::starts_one_three_and_available_count`, `insufficient_category_creates_nothing`, `interview_contracts::count_validation_rejects_zero_and_more_than_eligible` (`count == 0` returns `INSUFFICIENT_QUESTIONS`), Vitest `defaults_to_three_and_all_categories`, `insufficient_questions_require_explicit_adjustment`. Still to complete with the user: a session of one question and another eligible quantity (section 4, items 1 and 6). |
| F2-04 | Selection and order without repetition persist; editing the bank does not change any existing session copy. | 3 | **verified** (automation) | `sessions::starts_one_three_and_available_count` (positions 1..N, without repetition, order stored), `snapshots_survive_bank_edits` (text, category, competencies and criteria of the session do not change after editing the bank), `repeated_start_returns_original_session`, `failed_second_question_rolls_back_session`. |
| F2-05 | The user completes three questions with voice, manual capture and correct progress; a synthesis failure still allows answering by text. | 6, 8, 10 | **partial** | Automation: `service::question_audio_is_synthesized_and_validated`, `question_cache_distinguishes_session_texts`, `synthesis_failure_still_allows_recording`, `start_confirm_and_advance_persist_the_session`, `finishing_the_last_question_completes_the_session`; manual capture already with evidence from phase 1 (CA-03 pending on the hardware). Still missing: the real session of three questions with voice, including one with technical terms (section 4, item 1). |
| F2-06 | Original audio, derived audio, automatic results, review and confirmation remain distinct and traceable. | 1, 4, 5, 9 | **verified** (automation) | Schema with separate tables and FKs and immutability triggers: `database::triggers_protect_confirmed_text_and_transcripts`, `foreign_keys_reject_cross_recording_sources`; `answers::normalized_audio_is_linked_to_a_ready_recording`, `transcript_append_creates_and_closes_its_operation`, `transcript_append_does_not_replace_review`, `confirmation_freezes_exact_seen_text`; Vitest `history_separates_raw_review_and_confirmed`. |
| F2-07 | Saving does not confirm; confirming fixes exactly the text seen; a failure, an old version or a repeated request does not create an undue confirmation. | 5, 8 | **verified** (automation) | `answers::save_does_not_confirm`, `confirmation_freezes_exact_seen_text`, `stale_review_cannot_confirm`, `same_request_with_other_text_is_rejected`, `confirmation_failure_rolls_back_review_and_receipt`; Vitest `confirmation_locks_exact_editor_text`, `confirm_failure_preserves_text`, `save_does_not_enable_next_until_confirmation`. |
| F2-08 | It is not possible to advance without confirming, to skip by double click or to finish before all confirmations; repeating the advance is safe. | 5, 8 | **verified** (automation) | `answers::advance_requires_confirmation`, `replayed_advance_does_not_skip`, `last_advance_completes_only_full_session`; `service::confirmation_and_advance_are_refused_during_capture`, `finishing_the_last_question_completes_the_session`; Vitest `double_next_click_sends_one_request`, `last_question_uses_finish_label`. |
| F2-09 | A new capture before confirmation preserves the previous ones; a failure of the new capture does not lose the valid selection. A confirmed answer is not reopened. | 4, 6, 8 | **verified** (automation) | `artifacts::new_capture_failure_keeps_previous_selection`, `older_publication_cannot_replace_newer_selection`; `answers::confirmed_question_keeps_selection_and_refuses_new_captures`; `service::failed_stop_keeps_previous_recording`, `confirmed_question_rejects_capture_and_transcription`; Vitest `prior_recordings_remain_readable`. |
| F2-10 | Shutdown and navigation with changes resolve save/discard/cancel and do not lose an edit made during a write. | 8 | **partial** | The decision covers the answer review and the question bank draft, including `beginEdit`/`beginNew`. `NavigationGuard.test.tsx`: `save_and_leave_keeps_newer_edit`, `failed_save_does_not_destroy_window`, `cancel_keeps_editor`, `discard_requires_explicit_choice`, `capture_blocks_navigation`; `App.test.tsx`: resolution of the review before closing, finalization of the capture before destroying, window kept when the shutdown fails, text written during the preparation of the shutdown, blocking of a page change during capture/finalization, `pede decisão antes de sair do banco com um rascunho por guardar`, `guarda o rascunho do banco antes de mudar de página`, `não troca de pergunta no editor sem resolver o rascunho atual`, `não fecha com um rascunho do banco por guardar quando a escrita falha`. Still missing: the real experience of shutdown and navigation with unsaved changes, including a failed write that keeps the window open (section 4, item 10). |
| F2-11 | Restarting during transcription, after saving a review, after confirming and after advancing recovers the persisted step, without duplications. | 4, 5, 6, 9 | **partial** | Real reopening of the database in tests: `answers::advance_after_reopen_keeps_the_confirmation_pending_to_advance`, `sessions::old_start_replay_after_completion_does_not_create_session`, `artifacts::reopen_reconciles_once`, `finalized_audio_survives_database_failure`, `storage::restore_interrupted_job`; `service::interrupted_operation_is_recovered_on_the_next_start` (an operation left in `running` by the previous process becomes `interrupted` and is reported); Vitest `resume_keeps_confirmed_question_awaiting_next` and the integrated journey `three_question_session_can_resume_and_finish`. Still missing: restarting the real application in the middle of a session (section 4, item 4). |
| F2-12 | Write/publication failures allow recovery without a new recording; missing references do not delete texts nor block other items. | 4 | **verified** (automation) | `artifacts::finalized_audio_survives_database_failure`, `part_file_is_not_a_saved_answer`, `missing_audio_keeps_text`, `locked_original_stays_publishable_for_the_next_start`, `corrupt_original_is_interrupted_not_retried`, `paths_cannot_escape_artifact_root`; `service::save_publication_can_retry` (the snapshot exposes `pendingRecordingId` and the reservation on the current question, with the retry reachable); `storage::failed_publish_preserves_last`; Vitest `failed_publication_keeps_the_take_retryable`, `missing_audio_keeps_confirmed_text`. |
| F2-13 | Repeated requests do not block the service; old events and results do not affect new recordings or confirmations. | 5, 6 | **verified** (automation) | `service::repeated_start_returns_busy_instead_of_blocking`, `late_interruption_from_an_older_capture_is_ignored`, `timed_out_stop_recovers_capture`, `late_worker_result_cannot_target_next_question`, `history_read_does_not_retarget_model_job`, `playback_is_allowed_while_transcribing`, `close_finalizes_capture_and_ignores_late_results`; `answers::late_transcript_cannot_change_confirmation`; Vitest `old_snapshot_does_not_erase_draft`. |
| F2-14 | History allows consulting sessions and listening to originals; phase 1 practice sessions remain consultable without changing their files. | 9, 10 | **partial** | Reading and preservation: `legacy::legacy_list_ignores_last_pointer`, `corrupt_legacy_item_does_not_hide_valid_items`, `legacy_read_changes_no_bytes`, `legacy_rejects_path_escape`; `service::legacy_attempt_audio_is_read_only`; `sessions::list_sessions_reports_completed_sessions_and_order`; Vitest `History.test.tsx` (5 tests) and `history_separates_raw_review_and_confirmed`. Still missing: listening to the originals through the window with real hardware (section 4, items 2 and 5); real audio playback also remains pending in phase 1 (CA-02/CA-05). |
| F2-15 | A failed migration preserves the database; a future version is not written; only one session remains in progress. | 1, 3 | **verified** (automation) | `database::failed_migration_rolls_back`, `future_version_is_not_modified`, `fresh_database_has_version_one`, `only_one_session_can_be_in_progress`; `sessions::only_one_session_can_be_active`, `insufficient_category_creates_nothing`; partial index `one_active_session` in the schema. |
| F2-16 | With the models prepared, the complete session works offline, without AI calls, fictitious dismissals or invented feedback. | 6, 9, 10 | **partial** | Code without AI calls: 0 occurrences of `opencode`, `openai`, `api.deepseek`, `https://api` in Rust/TS/Python; the client only uses `interview_*` commands and the `interview://snapshot` event; `last_advance_completes_only_full_session` fixes `mode = local_v2` without pending feedback; the history shows a summary without evaluation. The engines were re-run with the prepared weights (2 passed). Still missing: repeating a complete session through the window with the network disconnected (section 4, item 8); that repetition also remains pending for phase 1 (CA-13). |

No criterion was marked **failed**: no contrary evidence appeared in this
verification.

## 4. Manual validation still missing (user action)

Detailed list in [setup.md](setup.md), section 6. Summary of what spec §10
requires and that **is not** demonstrated by this automation:

- [ ] Real session of three questions, with spoken answers, including one with
      technical terms; record the duration of the audios and the loading,
      synthesis and transcription times, and the perception of the wait.
- [ ] Listen to the synthesized questions and to the recorded originals, and
      repeat a question without new synthesis.
- [ ] Review, save and confirm each text; confirm that saving does not confirm
      and that confirming fixes the text shown.
- [ ] Leave after confirming and before advancing, reopen and resume the
      persisted step without duplicating the confirmation or advancing on its
      own.
- [ ] Finish the session and consult the history, including the summary without
      evaluation.
- [ ] Complete a session of one question and another eligible quantity.
- [ ] Confirm the insufficiency in a category and that it only starts after an
      explicit adjustment of the quantity.
- [ ] Edit a question after starting the session and confirm that the session
      copy does not change.
- [ ] Repeat the journey with the network disconnected.
- [ ] With the transcript unavailable (helper stopped) or `no_speech`, use the
      explicit manual review and confirm that nothing is treated as a
      pronunciation problem.
- [ ] Provoke a write failure and confirm the recovery; identify any simulated
      failure as a simulation, not as real hardware.

While these steps do not have evidence recorded by the user, phase 2 is not
marked as complete.

## 5. Phase 1: manual criteria that remain pending

This phase 2 verification **does not** close or change the phase 1 record. In
[`docs/phase-1/validation.md`](../phase-1/validation.md), they continue as before:

| CA | State in phase 1 | Why it remains pending |
| --- | --- | --- |
| CA-02 | pending | Listen to the Kokoro voice through the window and repeat it. |
| CA-03 | pending | Manual recording with a real microphone and visible duration. |
| CA-05 | pending | Listen to the stored answer and recover it with the transcriber unavailable. |
| CA-06 | pending | Transcribe a human recording in English. |
| CA-10 | pending | Remove/restore the real microphone. |
| CA-13 | partial | Repeat the complete journey through the window with the network disconnected. |
| CA-14 | pending | Three real recordings and the user's decision on the wait. |

The real engine tests re-run in this verification (2 `passed`) are the same engine
evidence already recorded in phase 1; they do not replace human recordings or
listening, and they do not change any of these states.

## 6. Decisions taken during the implementation (ADR-type record)

1. **rusqlite 0.41.1 with `bundled`, as planned.** The resolution worked without
   changes; `Cargo.lock` pins `rusqlite 0.41.1` and `libsqlite3-sys 0.39.1`, both
   compiled in the release build. Advantage: no dependency on a SQLite installed
   on the system. Cost: longer first compilation (~1m 17s in the release build of
   this machine). A single `Connection` inside `Arc<Mutex<…>>`, always in
   `spawn_blocking`; the guard never crosses an `await`.
2. **The phase 1 `practice` interface was removed in task 6**, when the service
   became the guided interview one. The protections were ported, not lost: the
   `saved/pending/failed` distinction when saving a review, blocking of editing
   during the request and the save/discard/cancel decision before navigating or
   closing now live in `useInterview`/`NavigationGuard`. Only `practice.css`
   remains, reused by the new interface. The JSON format of `phase1/` is kept only
   for reading the old practice sessions.
3. **`ReviewDraft.savedAt` is null while the revision is not saved**
   (`version = 0` represents the draft). The interface thus distinguishes «never
   saved» from «saved on a date», without inventing a date for the draft and
   without using `null`/`0` as if it were a persisted revision. The confirmation
   stores at most one exact version per question.
4. **`validate_count(0)` returns `INSUFFICIENT_QUESTIONS`**, the same code as a
   quantity above the eligible total. Zero is not a valid session request nor a
   separate case: the message asks for a quantity between 1 and the eligible
   total, and nothing is created (the validation runs before the transaction).
5. **Reconciliation only considers the transient Windows I/O errors as
   repeatable: 32 (sharing), 5 (access denied) and 33 (lock).** All other I/O
   errors — including Hound's synthetic end-of-file error, which has no system
   code — are permanent and mark the recording as interrupted, instead of
   repeating it indefinitely. A file locked by antivirus remains publishable on
   the next startup (`locked_original_stays_publishable_for_the_next_start`).
6. **R8: the only write in `phase1/` is the synthesis cache**
   (`phase1/question-cache/`), foreseen in the technical plan. The phase 1
   practice sessions remain read-only; the cache is shared because the key
   includes the text, the voice and the speed. «No writes to `phase1/`» means,
   therefore, «no writes to the practice sessions», not «no writes in the whole
   tree».
7. **The helper operations are recorded as `running` before the request and
   closed with the result.** A transcription closes the row in the same
   transaction as the text; a synthesis closes it as `completed`; a failure stores
   the error; a shutdown or abrupt termination leaves the row for reconciliation
   to mark it as `interrupted`. `RecoveryReport.interruptedOperations` therefore
   stops being always empty.

## 7. Recorded limits and risks

- The automated verification uses synthesized audio and controlled helpers. It
  does not prove voice quality, intelligibility on the user's hardware, perceived
  latency nor the behavior with the microphone removed midway.
- The `--no-bundle` build is not an installer: it does not include Python, the
  models nor the `.venv`. Phase 5 remains responsible for distribution.
- The database created on this machine contains no sessions; the session journeys
  were exercised by tests with temporary directories and by a controlled client,
  not by a spoken interview.
- The search for AI calls is a textual verification of the current code
  (0 occurrences), not a network traffic capture. The complete offline proof
  requires repeating the journey through the window with the network
  disconnected.
- Third-party warnings during model loading (`PyTorch`, `weight_norm`, Misaki) do
  not prevent operation and were already recorded in phase 1.

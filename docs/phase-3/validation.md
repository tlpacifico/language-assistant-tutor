# Phase 3 validation

Current modality, authorized on 2026-11-04: [OpenCode Go](../opencode-go/validation.md), with its own identity and a stable session. It replaces the per-usage default of the original record, preserving historical requests; it does not constitute manual acceptance of the phase.

Update of 2026-11-04: the per-question evaluation during the interview and its reuse in the final report have their own evidence in the [coaching validation](../ai-coaching/validation.md). This extension does not change the manual acceptance pending items of this phase.

Date: 2026-11-02. State: implementation verified automatically; technical review by the coordinator completed; manual acceptance pending. The bringing forward of phase 3 was authorized by the user; it does not accept nor complete phases 1 and 2.

## Automated evidence

The tests use real SQLite and temporary files, simulated audio, false instance credentials and controlled loopback HTTP. They do not use a paid key, a personal microphone nor real professional answers.

| Criterion | Evidence | Human pending item |
| --- | --- | --- |
| F3-01 | `ai_migration`, `database`, `sessions/answers/service/artifacts` regressions, v2 contracts | Real local journey after the update |
| F3-02 | `ai_service::settings_remove_secret_and_consent_on_destination_change`, UI clears the field on failure; Windows adapter without fallback | Exercise a protected credential on the machine |
| F3-03 | Test without confirmation/consent does not call the network; HTTP test inspects body/auth | Check the warning and the real account |
| F3-04 | Tests of a single follow-up, its own recording, the counter and the blocking of a second request | Usefulness of the question |
| F3-05 | `ai_service` records and confirms the initial answer and the follow-up separately; audio/transcription/review regression | Real recordings and Kokoro playback |
| F3-06 | HTTP barrier allows dismissing, advancing and recording the next one; late content discarded | Repeat through the window |
| F3-07 | Repeated receipt during the network does not duplicate the call; confirmation and advance preserve the existing rules | Journey with real clicks and resume |
| F3-08 | Restart turns the request into interrupted without resending; shutdown neither waits for HTTP nor continues the batch | Close/reopen the window at each step |
| F3-09 | `ai_validation` verifies literal Unicode, role and source; UI presents distinct sources | Semantic fidelity of the feedback |
| F3-10 | Absence requires all sources; altered citations/orphan priorities/extra fields are refused | Assess the suggested absences |
| F3-11 | Fixtures with failure/learning; the answer's instructions stay in the data message; no global score/acoustic fields | Qualitative/negative answers and real malicious instructions |
| F3-12 | Session of three: report 1 valid, 2 fails; retry requests only 2 and 3; partial/ready state | Consult and resume in the history |
| F3-13 | `ai_usage`: cache, unknown, explicit zero, different model and partial subtotal; late usage recorded | Compare with the real billing |
| F3-14 | HTTP 401/403/429/500, timeout, invalid JSON, excess without Content-Length; injected publication failure and retry without new network; migration rollback | Real network/storage failures |
| F3-15 | Test with suspended HTTP keeps dismissal/next capture; integrated React test of the follow-up | Perceived responsiveness |
| F3-16 | Not replaceable by automation | Real session of three main questions and cost/wait/usefulness |

## Commands

On 2026-11-02, `scripts/verify.ps1` finished with **8/8 checks and exit code 0 in 77.6 s**: **175 Rust tests, 57 Vitest, 36 pytest without models**, frontend build, formatting, clippy with -D warnings and clean pip check. A previous run gave 8/8 in 96.2 s.

`npm.cmd run tauri -- build --no-bundle` finished with exit code 0; initial Cargo release in 2m00s and final rebuild after the shutdown correction in 1m18s. The binary `src-tauri/target/release/language-assistant-tutor.exe` was started with temporary APPDATA/LOCALAPPDATA and a nonexistent audio configuration; `CloseMainWindow=True`, exit within the 15 s deadline and **exit code 0**. No personal data was migrated by the smoke. It is not an installer validation.

The specific commands and limitations are in the [report](implementation-report.md).

## Manual result reported by the user — 2026-11-03

The user reported: «I did the manual test, everything is fine» and clarified that they did not use the paid API. The favorable result of the tested journey is recorded, without real paid inference. The integration with the provider, the usefulness of the follow-ups, the semantic fidelity of the feedback and the real usage/cost remain to be validated. The report does not distinguish the remaining scenarios of the scripts of the three phases, including failures and recovery; no measurements, effective credential protection or individual coverage of those criteria are inferred. The complete manual acceptance remains pending.

## Pending manual script

1. Complete the manual validation of phases [1](../phase-1/validation.md) and [2](../phase-2/validation.md), which remain pending.
2. Configure the key/balance in the window, confirm the protection of the secret and the authorized destination. Use test text before personal data. Do not send keys by chat.
3. AI session of three main questions: one complete answer, another incomplete and another with technical terms and a qualitative/negative result. Listen to the questions, the initial originals and the follow-ups. Review and confirm each target.
4. Check that there is at most one follow-up per main question, that it does not repeat information already answered and that it does not demand numbers or success. Measure the wait and assess acceptability.
5. Fail the network after confirmation: repeat once and, in another main question, dismiss explicitly. Restore the connection and verify the absence of a late follow-up.
6. Leave and resume during the decision, the follow-up and the feedback; confirm that there is no automatic resending nor a new recording required.
7. Generate feedback, verify the citations and the initial/follow-up origin, the absences and the English separate from STAR. Interrupt the second report and repeat only the missing ones.
8. Consult the history during another session. Compare the model, the known usage and the estimates with the real account; keep the unknowns as such.
9. Exercise a write failure after the answer is received and the local publication without a new request. The automated test covers this condition; the real exercise requires a controlled scenario without putting personal data at risk.

Do not mark the phase as complete/accepted before this evidence. The `--no-bundle` build does not validate an installer nor a packaged installation.

Final review by the coordinator: shutdown race correction checked; final independent run of scripts/verify.ps1 with 8/8 checks, exit 0 in 42.7 s (175 Rust, 57 React, 36 Python). The final build after the correction finished with exit 0 in 1m18s; the isolated startup/shutdown finished with code 0. Manual acceptance and paid inference remain pending.

Local configuration on 2026-11-03: at the user's request, the deepseek-v4.1-flash model and the OpenCode HTTPS base were configured. The key was stored in the Windows Credential Manager under the target used by keyring and the read was verified in memory, without showing the secret. The public configuration was persisted without the key. There was no network call, paid inference or balance/authentication test at the provider. The authorization to send text remains to be confirmed in the application window.

On 2026-11-04 the presentation of request errors and the HTTP 402 (billing) / 403 (access) distinction were corrected. The real incident was confirmed by the user as `Account.InsufficientFunds`; no new paid calls were made. See [diagnostics and validation](../diagnostics/validation.md#billing-corrections-and-hidden-errors--2026-11-04).

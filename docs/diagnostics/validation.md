# Local diagnostics — preparation and validation

Date: 2026-11-04. Change authorized by the user: «crie a spec e o plano e faça a implementacao».

[Design](../superpowers/specs/2026-11-04-local-diagnostics-design.md) and [plan](../superpowers/plans/2026-11-04-local-diagnostics.md).

## Usage

Open **Diagnóstico local** on any page. Check version, commit, indication of local changes and build date; refresh the state after a failure. The panel also appears in the recovery from a React rendering failure.

**Exportar diagnóstico** writes a JSON with a unique name in the indicated folder, Downloads by default. The folder must exist and allow writing. The full path appears after the export; the user can share that file for investigation. There is no automatic sending and there are no paid calls.

The executable keeps the semantic version **0.1.0**. The Git commit and the build instant distinguish builds; `com alterações locais` reports that the executable does not correspond only to the commit. Without Git, the revision is unknown.

## Persistence and privacy

The path shown by the panel is resolved by Tauri through `app_data_dir()` with the stable identifier `dev.thacio.languageassistanttutor`. The five reserved files are `events.jsonl` and `events.1.jsonl` to `events.4.jsonl`, up to 1 MiB each. Retention uses the age of the events (30 days), including in recently written files: it checks at startup, on the first write after the expected expiry and on every export. Rotation depends on size, never on the version; exports are not cleaned automatically. An operating system file lock coordinates writers from several instances. Do not change the identity/folder when releasing new versions.

The events contain technical categories, closed codes, event/run ID, duration when known and build metadata. Python exceptions add only the allowed name of their own file and the line number. Error messages, IPC arguments, raw stack, raw stderr, keys, configuration, SQLite, audio and interview contents do not enter the persisted records or the export. The helper's pre-existing stderr continues to be drained in memory, without becoming a diagnostics file.

The writer uses a queue of 256 events and never waits for the disk to record an error. The panel reports write failures and losses in this run; the exported JSON also includes that state and the identification of the current run. The export waits for the events already queued; with a full queue or unavailable disk, it can fail and be repeated. The query/export calls run in `spawn_blocking`.

## Automated evidence

- `.\scripts\verify.ps1`: **8/8 checks passed, exit 0, 146.7 s**. **262 Rust tests** (the helper scenario still runs a child process, without counting that subtest twice), **117 React tests in 15 files**, **36 Python tests** without the `models` marker (2 deselected), TypeScript/Vite build, formatting, Clippy with `-D warnings` and `pip check` passed.
- Nine storage tests exercise reopening with another version, export after queued events, privacy, rotation, retention per file and per event, write failure/recovery, known losses, concurrent writers and respect for the operating system lock. A supervisor integration test proves the persistence of an asynchronous rejection without a private message.
- Five React tests cover IPC rejections without arguments/message, failure of the diagnostics itself, global capture, export to the indicated folder and React rendering recovery. The Python test of an unexpected failure also checks the structured channel without private text.
- An independent review found four concrete problems (event age, concurrent rotation, asynchronous rejections of the helper and losses omitted in the export). They were fixed; tests reproduced the retention, lock, rejection and metadata failures before the fixes and passed afterwards. The concurrent rehearsal also checks the result after the fix. An intermediate run of the aggregate stopped on a boolean test comparison refused by Clippy; fixed to `assert!`, followed by the green aggregate above.
- `git diff --check` clean and local links of the affected documents checked. No new dependencies and no change to version 0.1.0 or the Tauri identity.
- Isolated release build with `--no-bundle --config tmp/diagnostics-work/smoke-config.json`: exit 0, Cargo in 1m38s. Rehearsal in a **real WebView2** through CDP/Playwright: the panel opened, showed the build, captured a JavaScript error and exported through the interface; the message `PRIVATE_SMOKE_RESPONSE` was absent from the JSON. Three consecutive runs kept 3, 6 and 9 events, with 1, 2 and 3 distinct run IDs, write state available and zero losses. Each process closed with `CloseMainWindow=True`, **exit 0**. The versioned script was run on the third pass. Screenshot inspected at `tmp/diagnostics-work/webview/diagnostics-webview.png`.
- The rehearsal configuration explicitly replaces the application folders, keeping the identity; the data and WebView profile stay under `tmp/diagnostics-smoke-data`. There were no recordings and no paid inference. The normal final build is generated again without that override; no installer was created.
- **Normal final build**: `npm.cmd run tauri -- build --no-bundle`, exit 0, Cargo in **1m01s**. The absence of the rehearsal data path in the final executable was confirmed (present in the isolated executable used as control). Artifact: `src-tauri/target/release/language-assistant-tutor.exe`. The versioned configuration keeps version 0.1.0 and the original identity. After strengthening the removal assertion on disk, the focused diagnostics battery passed again: 9/9, exit 0.

## Reproduce the isolated rehearsal

Run from the root, with Cargo on the PATH and Python/Playwright available (the same optional preparation as the [layout rehearsal](../layout/validation.md)). These scripts are development tools; they are not dependencies of the audio helper.

```powershell
$smokeRoot = [System.IO.Path]::GetFullPath((Join-Path (Get-Location) 'tmp/diagnostics-smoke-data'))
New-Item -ItemType Directory -Force tmp/diagnostics-work | Out-Null
@{ app = @{ appDirectoriesOverride = $smokeRoot } } |
  ConvertTo-Json -Depth 5 |
  Set-Content tmp/diagnostics-work/smoke-config.json -Encoding utf8
npm.cmd run tauri -- build --no-bundle --config tmp/diagnostics-work/smoke-config.json
# Continue only if the isolated build exits with code 0.
Copy-Item src-tauri/target/release/language-assistant-tutor.exe tmp/diagnostics-work/diagnostics-smoke.exe
.\scripts\diagnostics-smoke.ps1 -Executable tmp/diagnostics-work/diagnostics-smoke.exe
# Repeat the previous command to check preservation between runs.
# Always restore the normal artifact before using/distributing the application:
npm.cmd run tauri -- build --no-bundle
```

The [PowerShell orchestrator](../../scripts/diagnostics-smoke.ps1) starts only the indicated executable, with a hidden window and debugging on a temporary local port, calls the [Python rehearsal](../../scripts/diagnostics-smoke.py) and closes the process. The Python checks the data path before causing the error. Use this orchestrator only with the build explicitly isolated above. The rehearsal files stay in `tmp/diagnostics-work`; the normal versioned configuration is not changed.

## Pending manual validation

1. Run the updated build and export to an existing folder. Confirm the path and the content of the JSON.
2. Use a real microphone, transcript and AI; cause a recoverable unavailability and check the codes and the preservation of the originals.
3. Close, replace the executable with a later version and open it again: check old and new events in the export. The automated reopening test does not validate the installer.
4. In the future distribution, install a real update and check identity, directory, sessions and records. Uninstallation, manual cleanup of the profile and explicit removal of the data are outside the update guarantee.

Diagnostics is best effort: forced terminations, native failures and events still in the queue may not be recorded. It does not accept phases 1–4 and does not start phase 5.

## Billing corrections and hidden errors — 2026-11-04

Later decision: the user asked for the Go modality used in Marlo Expenses.
The `InsufficientFunds` error observed corresponded to the per-usage route, which was
different from Marlo's `/inference/go/...` route. The current integration and the adaptation
of the configuration are described in [OpenCode Go](../opencode-go/validation.md).

On first use, three follow-up attempts became `failed`, with unknown usage. Reading only the request metadata and the diagnostics confirmed `AI_ACCESS`, with the message «Acesso recusado ou saldo insuficiente». The transport grouped HTTP 402 and 403; the old records do not allow recovering the exact HTTP status. The user confirmed in the console **HTTP 402, `Account.InsufficientFunds` / `Insufficient account funds`**. The official documentation confirms [HTTP 402 for lack of credit or billing problems](https://opencode.ai/v2/docs/console/billing/).

Two diagnostics problems in the application were fixed:

- HTTP 402 becomes `AI_BILLING`, with an instruction to check balance and billing. HTTP 403 keeps `AI_ACCESS`, with an instruction to check the permissions of the key/model. The messages identify the HTTP status without including the body returned by the provider. `AI_BILLING` is part of the diagnostics closed list.
- The final panel presents the stored reason of the last failed attempt, including old records, and translates the attempt states. The details of each attempt keep their error; unreadable errors have a generic message, without raw content. The query does not repeat requests.

Historical requests are not reclassified or resent; the unknown cost remains unknown. Resolving the balance is a user action with the provider. Afterwards, the explicit repetition uses the confirmed answer already stored, without requiring a new recording. No protected credentials were read, no paid calls were made and no personal data was changed.

The regression tests first reproduced the failures: two React checks did not find the reason; the transport returned `AI_ACCESS` for HTTP 402; the diagnostics discarded `AI_BILLING`. After the fix, the five tests of the React file and the HTTP 402/403 simulation passed. The existing local HTTP server also checks the absence of the provider's sensitive content. Final aggregate verification: `scripts/verify.ps1`, **8/8 checks, exit 0, 135.8 s**; **263 Rust tests, 119 React and 36 Python**, TypeScript/Vite build, formatting/Clippy and pip check passed. Updated release build completed with exit 0 (1m10s). The previous version was preserved under another name in the release folder, which allowed generating the new executable without closing the user's window or losing drafts. The later change to OpenCode Go rebuilt the same binary; see [Go validation](../opencode-go/validation.md).

## Article conversation — 2026-11-05

The operations `article_transcribe`, `article_reply`, `article_first_audio` and `article_feedback` were added to the closed list (only `durationMs` and a stable code), as were the `article_*` commands and the codes `ARTICLE_DELETED`, `ARTICLE_REGENERATE` and `AI_INTERRUPTED`. A service test checks that only these operations appear, with duration, and that the article, the answers, the paths and the file names never reach the record. Distribution through an installer remains to be validated.

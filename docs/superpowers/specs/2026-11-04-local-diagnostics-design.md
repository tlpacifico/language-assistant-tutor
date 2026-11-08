# Local diagnostics and build identification

## Intention and authorization

Prepare real use and allow investigating later failures. On 2026-11-04 the user asked for the spec, the plan and the implementation, with preservation of the records between versions. The local option proposed in the conversation is assumed; there is no automatic sending.

## Approaches

- Local diagnostics with manual export: chosen for this implementation; it works offline, without an account or additional service.
- Remote service: would allow receiving failures automatically, but adds dependencies, configuration and data transfer. Outside the scope.

## Contract

- Preserve `identifier = dev.thacio.languageassistanttutor`. Store JSONL in `app_data_dir()/diagnostics`, never in the installation directory, repository or temporary folders. Open in append; restarting or changing version does not delete the history.
- Retention: 30 days, up to five files of 1 MiB each, with rotation by size. Retention acts on startup and on writes; only on the reserved diagnostics names. The version does not participate in the cleanup. Exports are independent and are not cleaned by retention.
- Each event contains schema, ID, Unix instant in milliseconds, run, version, Git revision, local changes indicator and build instant. When Git is not available, indicate unknown revision.
- Record Rust error codes, IPC command failures with name and duration, global exceptions/unhandled promises and React rendering failures; record own frames of unexpected Python helper exceptions, without the error text. Diagnostics do not guarantee capturing forced termination, native WebView failures or process aborts.
- No keys, IPC parameters, free messages, raw stack traces, audios, transcripts, stories, answers or provider content. External fields go through closed lists; unknowns become generic categories. Do not persist raw stderr.
- Writing on a dedicated thread, bounded queue and non-blocking send; diagnostics failures must not prevent interviews. Expose write state and lost events; the export waits for previously queued events. The queue may lose the last events in an abrupt termination.
- «Diagnóstico local» panel accessible on all pages and in the React recovery, with build, state and export. Export JSON into a folder indicated by the user (by default Downloads), with a unique name and without replacing files. Use disk operations outside the interface thread. Do not export SQLite, personal configuration or session files.
- Do not modify interview rules, SQLite schema, engines, credentials or the semantic version 0.1.0. Identify different builds by the commit, local changes and build instant.

## Verification and limits

Test persistence after reopening with another version, rotation, age, write failure, privacy, export, IPC boundary and React recovery. Run `scripts/verify.ps1`, release build without bundle and smoke with temporary data. Updating through the installer, real audio and acceptance of the phases remain pending; this work does not start phase 5.

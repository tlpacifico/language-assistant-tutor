# Article conversation — validation

Status on 2026-11-05: **implementation and automated verification complete; manual acceptance, real audio, real voice and paid inference still to do.** Automated verification is not manual acceptance: the mode is **not** accepted.

## Observed automated verification

- `.\scripts\verify.ps1`: **8/8 checks, exit 0, 190.6 s** — TypeScript, **147 React tests** (20 files), Vite build, formatting and Clippy `-D warnings`, **354 Rust tests**, **56 Python tests** without the `models` marker, clean `pip check`.
- `npm run tauri -- build --no-bundle`: exit 0 (release, 1 min 27 s).
- Rehearsal with temporary data (`scripts/diagnostics-smoke.ps1` on an isolated build): window alive, diagnostics exported, shutdown with exit code 0. It does not touch personal sessions.
- The real Python tests with models (`-m models`) **were not run in this verification**.

None of this used a real microphone, real Kokoro voice, real protected credential or paid requests: audio, helper and provider are controlled adapters.

## Design matrix

| Design requirement | Automated evidence | Manual acceptance |
|---|---|---|
| Pasted article, fixed copy, limits without cutting | `article_domain`, `article_repository`, `ArticleSetup.test` | pending |
| Authorization per session, never pre-selected; revoking/changing the destination blocks sends | `ArticleSetup.test`, `article_service` (revocation, changed destination), `article_domain` | pending |
| Recording ends only by user action; audio stored before sending | `article_service` (`audio_is_saved_first…`), `article_domain` | pending (real microphone) |
| Automatic sending only with a stored transcript; empty/failed/excessive is not sent | `article_service`, `article_repository` | pending |
| Immutable sources; sent vs. revised version distinct | `article_repository`, `article_migration` | — |
| Article as untrusted data; quotes only from the user | `article_ai` | pending (semantic resistance) |
| Comment + question, without corrections between turns; suggest ending on the 5th; limit | `article_ai`, `ArticleConversation.test`, `article_repository` | pending (usefulness) |
| Feedback: 4 dimensions, explicit insufficient evidence, no overall score | `article_ai`, `ArticleReport.test` | pending (usefulness) |
| Correction in a new branch, old branch/report intact, explicit regeneration | `article_recovery`, `article_service` | pending |
| Late results, double click, ending/deleting during a request | `article_service`, `article_repository`, `ArticleConversation.test` | — |
| Local recovery without sends at startup; resumable deletion | `article_recovery`, `article_service` | pending (real abrupt shutdown) |
| STAR isolation (microphone/helper) | `article_service` (`star_and_article…`) | pending |
| Incremental voice: order, cancellation, partial failure | `article_voice`, `article_service` (voice), `test_stream` | **pending (real listening; Kokoro in blocks)** |
| Diagnostics without content; closed operations | `article_service` (metrics), `diagnostics` | — |
| Integrated scenario (authorize → 5 turns → correct → regenerate → conclude → reopen) | `article_service::full_conversation_…` | — |

## Known deviations and limits

- **No `synthesisChunks` capability in `health`**: adding a field to `HealthResult` would break the existing literals; the helper is distributed with the application. No automatic fallback to full synthesis if the helper does not know `synthesize_stream`.
- **Voice cancellation**: it asks the helper to stop between blocks; a block already being generated finishes and is discarded. The following transcript waits up to 2 s for the helper to become free.
- **Article limit**: 30,000 bytes (the spec said 32,000) so that title, opening, answer and reply fit in 48,000 bytes of context.
- **Old branches** do not appear in the interface (only the active branch); they are stored and reconstructible (`project_article_branch`).
- **Repeating an answer as a new branch** (recording again) is not implemented; only text correction.
- The Articles page **had no visual validation**.
- Performance measurements and the `beam_size=5/1` comparison: see [performance](performance.md) — to do.

## Pending manual script

1. Real conversation about an article, with at least five answers, including disagreement and elaboration (continuity, dialogue cohesion, perceived wait).
2. Recovery from a recognition error (manual text) and from a voice interruption in the middle.
3. Network failure and explicit repetition; resumption after closing the application in the middle of an answer.
4. Usefulness and fidelity of the feedback, and real cost (paid requests require your authorization).
5. Correction of an old answer and regeneration of the report.
6. Visual validation of the Articles page (desktop and mobile).

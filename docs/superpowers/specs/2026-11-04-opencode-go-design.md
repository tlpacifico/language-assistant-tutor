# OpenCode Go integration as in Marlo Expenses

The user authorized on 2026-11-04: «utilize o msm forma do Marlo Expense», after the comparison of the running container. The modality goes from pay-per-usage to OpenCode Go, keeping `deepseek-v4.1-flash`.

- Reference endpoint: `https://opencode.ai/inference/go/openai/v1`. Bearer authentication in Rust, with the key that already exists in protected storage.
- `User-Agent: LanguageAssistantTutor/<version>` and `x-opencode-session: language-tutor-<id>`; use the persisted ID of the interview, or of the practice when one exists, stable across requests, repetitions and restarts. Never copy the session ID, the User-Agent or the key from Marlo. Do not simulate programming traffic.
- Adapt the old reference configuration a single time (exact paid endpoint and reference DeepSeek model). Increment the version, clear consent and the current price list; keep the protected credential and all snapshots/history. Record the adaptation in `ai_receipts`, without a new schema version. Custom configurations remain intact; a later explicit choice is not migrated again.
- The change requires a new text authorization in the interface for the Go destination; there are no automatic resendings. Already confirmed data can be reused by an explicit request.
- The Go default has no fixed price list. Keep usage and cost returned by the provider, including explicit zero and origin `go`; absence of cost remains unknown. Do not convert the monthly fee or the included usage into an invented cost per request.
- No new dependencies, change of local engines or start of distribution/phase 5.

Only changing the URL was considered, but that omits the required headers. The same HTTP structure as Marlo was chosen, with its own identity and real IDs per conversation. The Go documentation describes the product for programming agents; this limitation was communicated to the user before the authorization. The integration honestly identifies the tutor.

Verify headers and path in real local HTTP, ID stability in the coordination, idempotent migration/versions/consent/preservation of configurations, Go usage zero and unknown, the complete battery of checks and the release build. Provider acceptance and real inference remain to be validated in the user's use; the tests do not make paid requests.

Sources consulted: [Go](https://opencode.ai/v2/docs/console/go/) and [API](https://opencode.ai/v2/docs/console/api/inference/); `ConfiguredAssistantModelClient.cs` and public configuration of the running `marlo-expenses-app` container (only endpoint, model and headers, without extracting keys).

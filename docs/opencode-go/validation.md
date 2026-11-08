# OpenCode Go — setup and validation

Date: 2026-11-04. Authorization: «utilize o msm forma do Marlo Expense».
[Design](../superpowers/specs/2026-11-04-opencode-go-design.md) and
[plan](../superpowers/plans/2026-11-04-opencode-go.md).

## What changed

The comparison with the `marlo-expenses-app` container in execution confirmed
`Assistant__BaseUrl=https://opencode.ai/inference/go/openai/v1`, the model
`deepseek-v4.1-flash`, its own User-Agent and a stable HTTP session. Only those
public configuration fields were consulted; the key was not extracted or copied.
The tutor now uses the same modality and transport structure, with its own
identity: `LanguageAssistantTutor/<versão>` and
`x-opencode-session: language-tutor-<id persistido da entrevista ou prática>`.

The URL remains configurable. The Go default keeps DeepSeek V4.1 Flash and has no
fixed rate card. The API may return zero cost for included usage; an explicit
zero is preserved, the absence of a cost remains unknown. The existing code
preserves the billing origin when it is returned. No plan limits or remaining
balance are presented that the application has not received.

On the first access to the configuration, the old reference (endpoint per exact
usage, model `deepseek-v4.1-flash`) is adapted transactionally to Go. The version is
incremented, the consent is withdrawn and the current rate card is cleared. The Windows
key is not changed. The marker `migration:opencode-go:v1` in `ai_receipts` makes
the operation single and allows later explicit choices to be respected. Configurations with other
destinations or models are not changed. There is no change to the SQLite schema.
Attempts, usage, configurations pinned in the requests, reports and audio are not
rewritten.

## Using it

1. Save pending revisions and reopen the updated executable.
2. Open **Configuração IA**. In the existing reference configuration, the URL changes
   to `https://opencode.ai/inference/go/openai/v1` and the key remains configured
   (the adaptation does not touch the Windows Credential Manager).
3. Confirm that the key is the one of the account with the Go subscription. If it is
   another key, use **Guardar chave protegida** to replace it (replacing the key does
   not by itself invalidate the consent; the next step asks for the authorization of
   the Go destination).
4. Check the destination and click **Autorizar envio de texto para este destino**.
5. Return to the interview and explicitly repeat the failed follow-up, or request the
   eligible evaluation. The confirmed answer is already stored; there is no need to
   record it again. The application does not do this resend automatically.

If the provider answers 402 or 403 on the repeated request, the error identifies the HTTP
status and the type of problem (billing or permissions) without presenting the returned body;
in that case, confirm in the account whether the key belongs to the Go subscription. Changing
the URL manually in the configuration still deletes the stored key and invalidates
the consent — it is the previous behavior, deliberately kept.

## Credential diagnosis (2026-11-04)

The user reported `AI_CREDENTIAL` («Credencial inválida») again and the block
«Resolva o seguimento… antes de avançar». The investigation separated the two facts:

- The key stored in the Windows Credential Manager has the format `sk_<uuid>` (39
  characters, ASCII). OpenCode Console keys have the format `oc_sk_...`
  ([API Overview](https://opencode.ai/v2/docs/console/api/)). A direct `GET` call
  to `https://opencode.ai/inference/go/v1/models` and to
  `https://opencode.ai/inference/v1/models`, with the header exactly as the one
  the tutor sends, returned **401** with `Invalid credential` on both destinations.
  Therefore, the cause is the key value, not the tutor request, the model, the
  `x-opencode-session` header nor the `/go/` segment.
- `AI_CREDENTIAL` appears in the error panel as the last persisted error of the
  session; when advancing is requested before resolving the follow-up,
  `AI_INVALID_RESULT` appears. They are not two independent defects: the follow-up
  failure blocks advancing until it is repeated or dismissed.

Changes applied to prevent recurrence:

- A key stored for the official destination (`opencode.ai`) is now validated
  in the `oc_sk_…` format; values such as `sk_<uuid>` are refused when saving and
  detected locally before any paid request, without `HTTP 401` and without
  cost. Configurable destinations keep the previous behavior.
- The `HTTP 401` error now says that the key was refused and where to
  replace it; Configuração IA indicates the format and the origin of the key.
- The refusal to advance now points to the «Seguimento IA» section, where
  «Repetir pedido de seguimento» and «Dispensar seguimento explicitamente» are; the
  follow-up panel explains that dismissing unblocks advancing when the
  cause is the key.

The manual validation with a real `oc_sk_` key remains to be done: obtain the key
in Account › Personal API keys, replace it in Configuração IA, authorize the
Go destination and repeat the follow-up. No paid request was made in this
investigation.

The [Go](https://opencode.ai/v2/docs/console/go/) documentation presents the plan
for coding agents. That condition was communicated before the authorization.
The tutor identifies itself honestly and does not imitate the traffic or identity of another
client. The provider's acceptance of tutor requests remains to be verified.

## Automated evidence

- Focused Rust tests: real HTTP transport over a local server, default,
  adaptation/idempotency/reopening, later explicit choice, preservation of
  custom destinations/models, key preserved and mandatory new authorization,
  session constant in a repetition and practice with its own identifier. The
  focused tests of the library, service, configuration and usage passed. The header
  selection and the migration assertions failed before the implementation (the new context
  field did not exist yet, the default was per usage and there was no adaptation).
- The usage tests now use an explicit test rate card to preserve the
  verification of the formulas, without depending on prices in the default
  configuration.
- Independent review: no concrete defects identified in the Go change.
  It was a static review; it does not validate the provider.
- Final aggregate verification on 2026-11-04: `scripts/verify.ps1` with **8/8 checks,
  exit 0 in 50.7 s** (270 Rust, 119 React, 36 Python), with TypeScript, Vite build,
  `cargo fmt --check` and Clippy `-D warnings` approved. Repeated after the last
  change in the usage tests.
- Release build `npm.cmd run tauri -- build --no-bundle`: exit 0 in 1m20s,
  `language-assistant-tutor.exe` generated at 14:52:32. The previous version was
  preserved as `language-assistant-tutor.previous-*.exe`; no process was
  terminated.
- Smoke of the release binary with temporary and empty `APPDATA`/`LOCALAPPDATA`:
  window alive after 8 s, `CloseMainWindow=True`, exit within 15 s with **code 0**;
  temporary folders removed. This is not installer validation.
- `git diff --check` clean and the relative links of the changed documents checked.
- What remains to be verified is external to the application: whether the provider accepts requests
  from the tutor in the Go modality, the real cost presented in the account and the semantic
  usefulness of the answers. No paid request was made in this verification.

The HTTP tests use the existing local server and a fake key; they exercise
`reqwest`, serialization and real headers. They do not make paid inference or queries
to the user's account. Real audio, semantic acceptance and previous manual validations
remain pending. This change does not start phase 5.

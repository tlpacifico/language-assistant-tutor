# Phase 3 setup and execution

**Current modality, 2026-11-04:** at the user's explicit request, the integration moves to OpenCode Go, with the endpoint `/inference/go/openai/v1`, its own identity and a stable HTTP session. The old reference configuration is adapted once, without deleting the key or the history; open Configuração IA and authorize the Go destination before repeating a request. The per-usage examples below are historical. See [Go setup/validation](../opencode-go/validation.md).

Date: 2026-11-02. Phase 3 was brought forward by explicit authorization of the user. The manual acceptances of phases 1 and 2 remain pending.

## Run

The prerequisites and models described in [phase 2](../phase-2/setup.md) are kept. The application calls the API directly; it does not require installing the OpenCode CLI.

```powershell
npm.cmd install
npm.cmd run tauri -- dev
```

If Cargo is not in the PATH, before the Tauri command:

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
```

In the setup, the default mode remains **Local, sem IA**. Choosing **Tutor com IA** creates a new `ai_v3` session. Previous sessions remain local; they are not converted into retrospective evaluations.

## Configure AI

1. Open **Configuração IA**. The current base is `https://opencode.ai/inference/go/openai/v1` (OpenCode Go) and the model `deepseek-v4.1-flash`. If the stored configuration still has the per-usage base, the application adapts it once when opening the configuration.
2. Save the configuration and confirm the model. In the Go default there is no fixed pricing: a manual rate in USD per million remains possible and must record date and source; the usage and cost returned by the provider prevail. The price examples below are historical.
3. Enter the key in the protected field and choose **Guardar chave protegida**. The core uses the Windows Credential Manager through `keyring` 3.6.3, service `language-assistant-tutor`, user `opencode-api`. The key is neither returned nor stored in SQLite.
4. Read the warning and authorize sending to the stored destination. Changing the base removes the previous credential and invalidates the consent. There is no sending when saving the configuration.

A protected storage failure is explicit; there is no plain-text alternative. Outside Windows, the credentials adapter declares unavailability.

## Journey

Confirming an answer stores it locally. **Pedir seguimento** is a distinct action of sending. The AI recommends a single follow-up or advancing; a follow-up has its own recording, transcription, review and confirmation. The counter keeps counting only the main questions.

If the API fails, repeat or explicitly dismiss the follow-up before advancing. Dismissal does not mean the AI considered the answer sufficient. The service does not automatically repeat paid requests. A late result of a dismissed step can only add known usage.

Since the approved change of 2026-11-04, after confirming the answer and resolving the follow-up, **Avaliar e sugerir melhorias** allows obtaining an evaluation, explained corrections and a rewrite before advancing. The review of the transcript is optional when there is usable text: it serves to correct recognition, preserving the English said for evaluation. Confirming remains only a local write. The evaluation button is an explicit send action, subject to the configured consent and to cost.

At the end, **Gerar ou repetir feedback em falta** evaluates only the questions without a valid report, also reusing the reports requested during the interview. The history presents partial/ready feedback, confirmed sources and usage per attempt. **Guardar resultado recebido sem novo pedido** repeats only the local publication when the remote answer has already arrived and the write failed. After a crash in that interval, the result may be unknown; recovery of lost memory is not promised. See [design](../superpowers/specs/2026-11-04-per-question-ai-coaching-design.md) and [validation](../ai-coaching/validation.md).

The audios remain local; only the question/criteria and the confirmed text needed go to the API. The entire bank, drafts, files or an automatic transcript different from the confirmed version are not sent.

## Persistence and limits

`tutor.sqlite3` moves to schema v2 in a transactional migration. `answer_slots` separates the initial answer and the follow-up; the initial IDs are the old IDs of the main question, preserving paths. In the existing audio modules, the legacy internal column `session_question_id` now references a slot; the new DTOs expose `answerId` and the main context. The main questions remain exclusively in `session_questions`.

Requests store immutable sources, configuration/rate, requested/returned model and prompt/rubric/schema versions. Valid reports are immutable. At startup, requests in progress become interrupted, without automatic resending.

Limits: 64,000 bytes UTF-8 per answer, 16,000 for question/criteria, 160,000 for the send body, 1 MiB of HTTP response and 2,000 for the follow-up question. Total timeout 90 s; connection 10 s. There is no silent truncation, redirects, automatic retries, tools or streaming.

## Verifications

```powershell
.\scripts\verify.ps1
npm.cmd run tauri -- build --no-bundle
```

The real results and pending items are in the [validation](validation.md). No paid inference was performed nor personal credentials accessed during the automated tests.

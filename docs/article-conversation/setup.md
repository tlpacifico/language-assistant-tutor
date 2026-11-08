# Article conversation — setup and use

New mode, distinct from the STAR interview, approved on 2026-11-04 ([design](../superpowers/specs/2026-11-04-article-conversation-design.md), [plan](../superpowers/plans/2026-11-04-article-conversation.md)). It reuses the microphone, the local models, the credential and the OpenCode Go destination already configured in «Configuração IA»; it adds no dependencies.

## Use

1. **Configuração IA**: store the key and confirm the destination (`/inference/go/openai/v1`). Without a key, the authorization is refused.
2. **Artigos → Nova conversa**: paste the English text (up to 12,000 characters and 30,000 bytes; optional title). Nothing is cut: above the limit the button is disabled.
3. **Authorization per session** (never pre-selected): authorizes the automatic sending of the article and of the transcript of each answer to the destination shown. The audio stays on this computer. Changing the destination or revoking prevents new sends until a new authorization.
4. The tutor asks an open question (the voice plays by itself only on new interventions; «Parar voz» interrupts). **Gravar resposta** and **Parar gravação** are your actions: the recording never ends by itself.
5. On stopping, the audio and the turn are stored and the transcript goes to the tutor **without a confirmation step** (unlike the STAR interview; the version sent is marked «transcrição enviada automaticamente»). A transcript that is empty, failed or above 8,000 characters is not sent: it asks for manual text (a distinct, «revista» version).
6. The tutor replies with a short comment and a question. No grammar corrections between turns. On the fifth answer it suggests ending; at 20 turns (or without context margin) recording becomes blocked and only ending is possible.
7. **Terminar e ver feedback**: coherence, cohesion, development and interaction, with quotes from your answers, «evidência insuficiente» when there is no text and up to three improvements. Without answers, it concludes without a report. No overall score and no assessment of pronunciation.
8. **Corrigir esta resposta** (only after it is completed): creates a new branch; the old branch, the versions and the previous report are kept. The new report is only requested when **Repetir** is pressed and may consume the API.

## Where the data stays

- SQLite (schema v5): `article_*` tables, with sources, turns and immutable versions, receipts per request, branches and reports.
- Audio and temporary voice: `articles\<sessionId>\<turnId>\original.wav` (and `normalized.wav`) and `articles\<sessionId>\voice-<id>\chunk-NNNN.wav`, next to the database. Deleting the conversation removes the folder.
- Only the necessary text goes to the provider; the key stays in protected storage and never in the logs.

## Recovery

- When the application opens, the recovery is **local and without sends**: pending requests become «interrompidos» (unknown usage), a recording in the middle is flagged and an unfinished transcript asks for review. Repeating is your action.
- **Retomar** in the list performs the same recovery for a conversation; opening, resuming or returning to the page never starts sends, recording or sound.

## Development commands

```powershell
.\scripts\verify.ps1                                   # all automated checks
.\scripts\benchmark-audio.ps1 -Recordings <folder>     # measurement (see performance.md)
```

See [validation](validation.md) and [performance](performance.md).

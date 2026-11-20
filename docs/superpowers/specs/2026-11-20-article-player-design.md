# Article conversation player — R1 design

Date: 2026-11-20. Status: **R1 implemented; manual acceptance pending**.

The user requested a specification after the [roadmap](../../article-conversation/roadmap.md).
This document specifies R1, the first implementation slice. See the
[validation record](../../article-conversation/validation.md#r1--article-player-and-chat)
for its checks and pending acceptance. Later milestones remain proposals.

## 1. Purpose and scope

Make a turn-based spoken conversation comfortable to follow: the user always knows
whether to listen, answer or wait, can revisit audio without losing their place,
and can start answering with one explicit action while the tutor is speaking.

Include the persistent control bar, per-message playback, honest audio progress,
recording feedback, transcript placeholders, scroll behaviour, article panel,
local preferences, keyboard access and operation-specific recovery.

Exclude debate progression/difficulty (R2), mid-conversation transcript correction
and branch browsing (R3), spoken practice/comparison (R4), buffer/decoding tuning
(R5), and hints/progress tracking (R6). Existing post-session correction remains.
No automatic speech-end detection, simultaneous speaking/listening, live
transcription, word-aligned captions, arbitrary seeking or new audio engine.

## 2. Existing implementation and approach

`ArticleConversation.tsx` renders the full conversation with controls below it.
`ArticleAudioView` exposes only idle/playing/failed and a turn ID. The current
service labels synthesis as playing before the first sound. The shared native
audio port supports play, append and stop, but not pause, position or metering.
Starting an article recording already stops article speech; this design makes
the handoff explicit and robust in the public interaction model.

Alternatives considered:

1. Reposition existing buttons only: inexpensive, but cannot provide real pause,
   progress, microphone feedback or a reliable visible audio lifecycle.
2. Extend the existing native player and expose scoped runtime state: recommended;
   preserves the single audio owner and existing local recording architecture.
3. Add a separate browser player: introduces competing playback ownership and
   more coordination with native capture; not selected.

Use approach 2 as the proposed design. Retain React/TypeScript, Rust/Tauri,
SQLite, Rodio/CPAL, faster-whisper and Kokoro. No new dependency is proposed.
The technical plan must verify capabilities against installed versions before
choosing concrete APIs. This is not a frontend-only layout change.

## 3. Layout and navigation

Use the existing tokens, typefaces, focus treatment and reduced-motion behaviour.
The conversation is the primary reading area. A bottom bar stays visible within
the article view, with reserved space so it never covers messages, errors or focus.
It contains current state, the selected audio item, media controls when relevant,
and the primary conversation action. Finish and session options are secondary.

The latest tutor question has a labelled jump link. On wide layouts, the article
can open alongside the chat. On narrow layouts it is a collapsible reading panel;
opening it must not hide recording controls or move the conversation's position.
Preserve the original article text and its language. Panel visibility is view-local.

Scroll to new content only when already following the end (proposed threshold:
within 80 CSS pixels). Otherwise preserve position and show «Nova resposta ↓».
Clicking it moves to the latest turn and resumes following. Opening a session
positions at its latest turn without playing sound; focus goes to the view heading.
Ordinary arriving messages never steal focus. User-originated recording and error
actions preserve an actionable focus target. Respect reduced motion for scrolling.

## 4. Conversation action matrix

Audio playback is independent of the persisted conversation state. Pausing audio
does not pause the session, revoke consent or change the answer count.

| Conversation condition | Primary presentation/action | Secondary behaviour |
| --- | --- | --- |
| Ready; no audio active | «Gravar resposta» | Play any available message; finish |
| Ready; tutor voice preparing, playing or paused | «Responder agora» | Pause/resume/stop voice as applicable |
| Ready; user recording being replayed | «Gravar resposta» | The same safe handoff stops playback first |
| Recording | «Parar e enviar», recording duration and live level | Disable playback, finish and navigation away |
| Transcribing | «A transcrever a resposta…» | No new recording or playback |
| Awaiting tutor | «O tutor está a preparar a resposta…» | No new recording; finish remains possible |
| Needs review | Existing manual editor and explicit send | «Gravar novamente» uses existing replacement/recovery rules |
| Recoverable error | Specific retry action when retryable | Keep saved content and allowed finish action |
| Finishing | «A gerar o feedback…» | No new answer or voice request |
| Completed | Report and per-message playback | Existing post-session correction |
| Draft / authorization revoked | Authorization controls | No recording or automatic sends |
| Paused session | Explicit resume | Reopening/resuming alone never plays or sends |

If the turn/context limit has been reached, replace any recording action with
«Terminar e ver feedback». Respect the existing authorization and destination checks.
Do not expose recording merely because audio is idle. Historical playback is
available in ready, needs-review and completed states; stop it before external
retry/finish. During awaiting-tutor, draft or paused states, retain readable text
without starting historical playback. This conservative policy avoids an arriving
tutor response competing with an explicit replay.

## 5. Respond now and recording lifecycle

Treat a click as one core-owned handoff, not independent frontend stop/start calls.
Validate session, branch/version, consent, limits and request identity before
changing devices. Disable duplicate actions while acknowledging the request.

1. Invalidate the old playback/synthesis generation so late chunks cannot play.
2. Stop native output and confirm it stopped. Request cooperative synthesis
   cancellation without waiting for all discarded synthesis work to finish.
3. Start native capture and acknowledge recording only when capture is active.

If output cannot be stopped, do not start capture. If capture fails, show the
failure with an explicit retry; do not restore tutor playback automatically.
No arbitrary acoustic-delay guarantee is made: validate speaker bleed with real
hardware. The microphone opens only as the direct result of the click.

Recording duration starts at native capture acknowledgement, not button press.
Show an elapsed mm:ss value and a normalized level derived from actual captured
samples. Meter values are transient: never persist or log samples/levels, and never
describe them as a pronunciation or confidence assessment. Rate-limit UI updates;
exact sampling/smoothing is left to the builder. Unavailable metering is labelled,
not replaced with a simulated animation. Existing microphone-loss recovery applies.

Stopping records audio before transcription/sending. Keep the current automatic
send authorization, with nearby copy explaining «Parar e enviar». A saved turn
appears as «A transcrever…» until a real transcript arrives; failed transcription
shows the existing review path. Never fabricate a user transcript optimistically.
If residual helper work delays transcription, show that local audio is becoming
available and preserve the recording; do not require recording again.

## 6. Playback model and controls

Expose runtime states: idle, preparing, playing, paused, buffering, ended and
failed. Each state identifies the session, source turn/version, playback generation
and ordered event sequence. Frontend state must not turn preparing into playing
until actual output begins. Late completion/progress events from old generations
must not stop or update a newer item, including across STAR and Articles.

Each tutor message offers «Ouvir», «Pausar», «Continuar» and «Ouvir do início»
as applicable. User messages offer the same controls for the original recording.
The selected message and bottom bar reflect the same player state. Missing original
audio has an unavailable label; there is no substitute recording or API retry.

Only one playback owner exists across the app. Selecting a different message stops
the old item and starts the new one from its beginning. Pausing preserves position
and queued chunks; generation may finish locally while paused, but cannot resume
output. Buffer depletion is buffering, not ended, until synthesis is complete.
Restart returns to the beginning of the current source; it is not a new tutor turn.
Stop clears the active queue/position and cancels unfinished synthesis.

Show elapsed source-audio position based on the player, excluding paused/buffering
time. For complete audio show its known total duration and a read-only progress
bar. While generating, show elapsed position and «A preparar o restante áudio…»;
any buffered-duration indication is explicitly labelled and not a final total.
No draggable seek control in R1. Highlight the active message, not individual words.

### Local voice reuse

Cache completed validated tutor audio by exact text-version identity, voice,
synthesis speed and engine/configuration identity. Keep ordered chunk metadata
and completion status. Reuse matching complete audio after reopening without an
external text request. Missing or incomplete cache can be regenerated explicitly
using the already saved tutor text; it must not alter conversation content or usage
of the text provider. Do not treat partial output as a completed cache entry.

Restart during an active generation reuses its ordered generated prefix and accepts
subsequent blocks once, from the same generation. After cancellation/failure, a
retry regenerates the incomplete rendition from the beginning. Corrupt/missing
cached output follows the same explicit voice-retry path. Session deletion removes
its cached files; stale cache references cannot escape the session directory.

## 7. Preferences and keyboard

Proposed defaults: autoplay enabled (preserves current behaviour), tutor synthesis
speed 1.0×, with 0.8× and 1.2× options. These are app-local article preferences,
persisted through the core; no account, external call or content-bearing log.
Original recordings always play at their recorded speed.

Speed changes apply to the next playback request, not midway through current
audio. A changed speed selects a matching cache or synthesizes locally; show the
preparation state. This avoids claiming pitch-preserving native time stretching.
Disabling autoplay immediately stops/cancels automatic voice, including a pending
autoplay intent. It does not stop an explicitly selected replay. Enabling it does
not play an existing message retroactively. With autoplay off, do not synthesize
new tutor audio until explicitly requested.

Autoplay is allowed only for a newly validated tutor turn produced by the active
session's current operation, once, while its article view is active. Returning,
reopening, subscribing again or receiving a duplicate event never triggers it.
Navigating away stops/cancels playback and removes autoplay intent. Background
text completion is saved, but returning shows it silently. Recording/transcription
navigation restrictions remain unchanged.

All controls are keyboard reachable with visible focus and accessible names.
Proposed shortcuts: Ctrl+Enter invokes the currently enabled recording action;
Alt+P toggles the selected item's pause/resume. Suppress shortcuts in inputs,
textareas, editable content, modal dialogs, inactive views, key-repeat events and
IME composition. Neither shortcut triggers finish, consent, retries or sending
manual text. Announce state changes politely; do not announce every meter/timer
update. The implementation plan verifies shortcut conflicts in the Tauri window.

## 8. Failure, persistence and contract boundaries

Rust remains authoritative for capture, playback ownership, transitions and
preferences. Keep persisted session versions separate from high-frequency runtime
events; progress does not write SQLite or increment a conversation version.
Runtime subscriptions need a snapshot/resubscription strategy and generation plus
sequence filtering so equal conversation versions cannot reorder player state.

Playback commands are scoped to an item/generation; a stale stop/pause must not
stop a replacement item. Recording handoff keeps the existing idempotent command
identity. Do not hold the UI or a database lock while waiting on devices/helpers.
Public contracts expose identities and bounded metadata, not arbitrary file paths.

| Failure | Required outcome |
| --- | --- |
| Text request fails | Keep saved audio/text; «Tentar enviar novamente» uses existing retry semantics |
| Voice synthesis/output fails | Keep readable tutor text; «Gerar voz novamente» or «Tentar reproduzir novamente» targets only failed audio work |
| Feedback request fails | «Gerar feedback novamente»; do not resend/re-record an answer |
| Output failure during handoff | No capture acknowledgement; explicit recovery |
| Microphone loss | Existing recoverable recording behaviour; no automatic new take |
| App closes | Existing session recovery; runtime player starts idle on reopen |
| Delete/finish/revoke during pending work | Invalidate applicable playback/autoplay; reject stale results; preserve existing domain guarantees |

Expose the failed operation sufficiently to select truthful retry labels; do not
infer it from free-form error messages. If a voice block fails after some playback,
label the rendition incomplete. Never retry external text automatically.

Extend the shared audio port only as needed; add regression coverage for STAR,
practice and article ownership. Keep Tauri identity and diagnostics directory.
New diagnostic operations/codes require updating closed lists. No content, audio,
credentials, paths or continuous meter readings in diagnostics. Preferences/cache
schema and migration details belong to the next implementation plan; older
sessions must load with defaults and no automatic playback or inference.

## 9. Acceptance matrix

Manual acceptance remains **pending** for every criterion. The validation
record holds the implementation evidence.

| ID | Criterion | Verification |
| --- | --- | --- |
| R1-01 | Bar remains visible without hiding content or focus | UI tests and visual checks at 800×600, 1024×768, 1440×900; frontend adaptation at 390×844 |
| R1-02 | Allowed actions follow conversation/limit/consent states | State matrix tests, including needs-review and paused sessions |
| R1-03 | One click stops output before capture; duplicate/stale actions are harmless | Controlled Rust events plus real microphone/speaker handoff |
| R1-04 | Pause/resume/restart preserve correct position and exclusive ownership | Native adapter tests and listening, including paused generation and empty buffer |
| R1-05 | Preparing/buffering/ended and known/unknown duration are truthful | Controlled events and real chunked playback |
| R1-06 | Original replay and complete tutor cache work without text inference | Transport counters; reopen, missing/corrupt/incomplete cache scenarios |
| R1-07 | Duration and meter represent active capture; failures preserve recovery | Controlled capture events plus real silence/speech/device removal |
| R1-08 | Saved placeholder becomes actual transcript; text does not wait for voice | UI/service tests with delayed transcription/synthesis |
| R1-09 | New messages preserve reading position and focus | Long-chat tests; keyboard and reduced-motion manual checks |
| R1-10 | Autoplay/speed defaults persist and apply only as specified | Restart/navigation/duplicate-event tests; real listening at all three speeds |
| R1-11 | Shortcuts never activate in editors, dialogs or inactive views | Keyboard tests and Tauri manual check |
| R1-12 | Audio/network/report failures have precise, isolated recovery | Inject failures and verify no unintended external calls or new recordings |
| R1-13 | Late audio events cannot affect another item/session/mode | Cross-mode ownership and generation/sequence tests |
| R1-14 | Finish/revoke/delete/reopen cannot restart cancelled output | Recovery/idempotency tests and abrupt-close rehearsal |
| R1-15 | Existing versions, branch/report sources and privacy are preserved | Persistence/regression tests and diagnostics-content checks |
| R1-16 | Five-answer conversation is usable end to end | Real microphone, Kokoro, authorized provider; replay, handoff, reading back, recovery and reopen |

Run existing relevant verification commands from the README for implementation;
record actual results in article validation. The shared audio changes require Rust
and affected STAR/practice regressions, not only React tests. No installer work is
included; if packaging changes, validate the packaged application as well.

Use the R0 baseline to assess stop-to-first-sound and inter-block waits, cold and
warm. R1 does not promise lower inference latency. A usable visual state does not
count as measured audio improvement. Missing real-use prerequisites remain pending
and do not prevent independent contract/UI work.

## 10. Next step and references

Specification review on 2026-11-20: an independent builder check found no blocking
product questions for planning. Local Markdown links and whitespace were checked.
This is document verification, not implementation verification or user approval.

The technical plan specifies the event fields, audio APIs, cache, migration and
test placement for R1.

- [Roadmap](../../article-conversation/roadmap.md)
- [R1 technical implementation plan](../plans/2026-11-20-article-player-implementation.md)
- [Article setup](../../article-conversation/setup.md), [validation](../../article-conversation/validation.md), [performance](../../article-conversation/performance.md)
- [Existing article design](2026-11-04-article-conversation-design.md)
- [Stack](../../tech-stack.md) and [layout validation](../../layout/validation.md)
- [Diagnostics validation](../../diagnostics/validation.md)

# Per-question evaluation and corrections

Date: 2026-11-04. Status: approved by the user («pode avancar»); execution and evidence in the [plan](../plans/2026-11-04-per-question-ai-coaching.md) and in the [validation](../../ai-coaching/validation.md). Manual acceptance pending.

## Intention

The user wants the AI provider to evaluate the answer and propose the
corrections. They agreed with the path «gravar → confirmar transcrição → receber
avaliação, correções explicadas e uma versão melhorada», preserving the original.
They did not expressly choose between evaluation per answer and at the end of the interview.
The proposal is to present it per question, before advancing to the next one.

## Approaches

1. Keep evaluation and rewrite at the end of the interview: reuses the current
   path, but postpones the corrections and does not allow applying them in the next question.
2. Make evaluation and rewrite available after each question: reuses the
   existing contracts, adapting the conditions that currently require a completed
   session. Approach chosen through the approval of this document.

## Proposed behavior

- Review of the transcript is optional when there is usable automatic text.
  It serves to correct misrecognized words, preserving the English errors
  actually said for the AI to evaluate them. The text presented for confirmation
  starts with the automatic transcript, unless a review already exists.
- Confirmation stores an immutable version locally. It does not, by itself,
  imply a paid request. Local mode keeps working without network.
- In AI mode, the follow-up decision and the limit of one follow-up per
  main question are kept. Evaluation becomes available after the follow-up is
  answered, dismissed or considered unnecessary. Thus an incomplete
  report is not fixed before knowing all the sources of the question.
- An explicit action «Avaliar e sugerir melhorias» starts the evaluation first
  and then the rewrite. The interface explains that it uses the configured API and
  may generate cost; consent for the destination remains mandatory.
- Show content/STAR, necessary corrections with the original excerpt, proposal and
  explanation, and optional style suggestions separated.
- Show an improved version in English, separate from the original, with level B1
  by default and reuse of the existing B1/B2/C1 variants. The level is a
  language goal, not a classification of the user.
- The rewrite preserves facts and flags gaps without inventing responsibilities,
  numbers or results. It does not replace the confirmation, the audio or the transcript.
- Evaluation is an optional help before advancing: a provider failure does not
  oblige re-recording nor creates a new condition to end the interview.
- The final report reuses valid per-question evaluations and asks only for the
  missing ones; it does not charge again for reports already available.

## Persistence and recovery

Reuse the provider, the evidence, request, usage and rewrite
contracts of phases 3 and 4. Widen eligibility only to questions with
confirmed sources and a closed follow-up stage, keeping the practice
restrictions and the checks in the Rust core. A late response applies only
to the origin that started the request, even if the interface already shows another question.

Evaluation and rewrite are separate operations: if the first finishes and the
second fails, keep the evaluation and allow repeating only the missing
operation. Do not resend automatically after a restart or failure. Keep unknown usage
as unknown and publication recovery without a new call.

## Required verification

Test eligibility per question, confirmation and follow-up, preservation of the
sources, late results, reuse in the final report, partial failure between
evaluation and rewrite, absence of automatic requests when opening a page and
the local path. Also verify the editor initialization with a usable transcript
and the manual-origin notices, given the inconsistency observed in the image.

Update the affected documentation with the real results. Paid inference,
semantic usefulness and real audio remain subject to specific validation;
this change does not accept the previous phases nor start phase 5.

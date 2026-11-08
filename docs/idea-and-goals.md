# Project idea and goals

## The idea

Create a desktop tutor to practice behavioral interviews in English by voice, using the user's own professional experiences and the STAR structure.

STAR organizes an answer in four parts:

- **Situation:** the context in which the experience happened.
- **Task:** the user's responsibility or challenge.
- **Action:** what the user actually did.
- **Result:** what happened and what was learned, including qualitative or negative results.

The application should help explain experiences with clarity, identify information that was left unsaid and improve the way of communicating in English. The goal is to develop autonomy, instead of memorizing artificial answers.

## For whom

The first version is intended for personal use on the computer, starting with Windows. The practice will be in English, with an interface and explanations in Portuguese.

In the future, the architecture may accommodate other languages. That expansion is outside the MVP and requires validating the voice, transcription and evaluation models for each language.

## Problem we want to solve

It is possible to know a professional experience and still have difficulty telling it in an interview: omitting context, talking only about the team's work, not explaining the individual actions or leaving the result unclear.

Answering in English adds difficulties of vocabulary, grammar and organization. The tutor offers a space to practice out loud, receive follow-up questions and try again with specific guidance.

## Main cycle

1. Choose the category and the number of main questions.
2. Listen to and read a question in English.
3. Answer through the microphone, starting and ending the recording manually.
4. Review the transcript to faithfully recover what was said.
5. Answer a follow-up when a gap needs to be clarified.
6. Consult the feedback at the end of the interview.
7. Listen to a suggested version and record a new attempt with your own words.

Follow-ups are additional and limited to one per main question. The suggested initial configuration is three main questions, with the possibility of practicing only one or choosing another available quantity.

## MVP goals

- Allow completing a short behavioral interview in English.
- Help structure answers according to STAR without imposing a single answer.
- Identify relevant gaps, especially the individual contribution and the result.
- Improve grammar, vocabulary, clarity and naturalness in the answer text.
- Produce observations supported by excerpts actually answered.
- Allow repeating questions and comparing answers without losing the previous versions.
- Store an editable bank of questions and factual professional stories.
- Keep audio and history local, with low and visible variable AI costs.

## Feedback principles

The feedback separates **content and STAR structure** from **English in the confirmed transcript**. It uses simple classifications, such as clear, partial and not identified, accompanied by explanations, without an overall score that would suggest unvalidated precision.

An answer can be good without containing numbers or a positive result. The tutor should recognize learnings, limitations and individual responsibility, without encouraging inventing facts.

Rewrites preserve the meaning and the level of certainty of what was said. Missing information is flagged, instead of being filled in by the model. Necessary corrections and optional style suggestions are presented separately.

The notes of personal stories serve as support for preparation. They do not count as information explained orally. The feedback also distinguishes what was clear in the initial answer from what was only clarified in the follow-up.

## Privacy and costs

Recording, playback and transcription are local. The confirmed transcripts and the necessary textual context are sent to OpenCode to produce follow-ups and evaluations. Local storage does not mean that all processing is local.

The recording will not be sent to the LLM in this flow. The application will allow deleting sessions and their audio, protecting the API key and consulting the usage and the estimated cost.

The goal is to take advantage of local models for audio and an economical model in the cloud for text, measuring the real cost instead of depending on a free offer.

## Outside the MVP

- Automatic evaluation of pronunciation or scoring by phoneme.
- Interruptions while the user speaks.
- Simultaneous real-time conversation or live transcription.
- Diagnosis of emotional confidence or proficiency certification.
- Other languages, cloud synchronization, accounts or collaboration.
- Training custom models.

Whisper turns speech into text. A correct transcription does not prove good pronunciation, and the LLM that receives only text cannot evaluate oral delivery. Listening to a suggested version is a practice tool, not a pronunciation diagnosis.

## How we will know the MVP is useful

The user can complete a short interview, understand what can be improved and repeat an answer without losing the previous work.

The validation includes about ten real answers, with different levels of completeness, technical terms and problematic recordings. We will evaluate the fidelity of the transcription, the wait between turns, the usefulness of the follow-ups, the preservation of facts and the cost of real sessions.

## Evolution by phases

1. Validate local audio with a fixed question.
2. Create guided interviews with questions and persistent sessions.
3. Add follow-ups and feedback with AI.
4. Allow rewriting, repeating and comparing answers, with the support of personal stories.
5. Consolidate installation, failure recovery and validation of the whole.

The deliveries and completion criteria are in the [phase plan](superpowers/plans/2026-11-01-star-interview-tutor-phases.md). The architecture is in the [technical stack](tech-stack.md) and the complete rules in the [approved design](superpowers/specs/2026-11-01-star-interview-tutor-design.md).

## Extension: article conversation

On 2026-11-04 a second mode was approved, outside the five phases: conversing in English, by voice, about a pasted article, with the tutor commenting on and deepening each answer and a final feedback on coherence, cohesion, development and interaction. It does not apply STAR, does not evaluate pronunciation and does not replace the interview. See [setup](article-conversation/setup.md) and [validation](article-conversation/validation.md).

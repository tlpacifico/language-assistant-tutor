# Applying the tutor layout

**Goal:** transfer the design provided in `docs/tutor-layout` into the existing React application.

**Architecture:** keep the Tauri clients, hooks, rules and persistence. Extract the tokens from the reference HTML into local CSS and reorganize the presentation into side navigation, per-view header, session panel and content modules. Adapt the static examples to real data; do not transfer the in-browser storage or the prototype's audio simulations.

**Stack:** React, TypeScript and CSS; no new runtime dependencies.

**Reference:** [handoff](../../tutor-layout/DESIGN-HANDOFF.md) and [HTML](../../tutor-layout/index.html).

## Constraints

- Interface in European Portuguese; practice questions and answers in English.
- Preserve immutable confirmation, revisions, consent, recovery and navigation/recording blocks.
- Availability, progress and usage come from the existing contracts; do not invent statistics.
- Local fonts for offline operation; keep the serif/sans-serif fallback.
- Do not start phase 5 and do not declare manual acceptance of the earlier phases.

## Tasks

- [x] Structure and tokens: `src/App.tsx`, `src/features/layout/*` and CSS. Side navigation of 264 px, central area with a maximum of 1060 px, adaptation at 1023/899/639 px and visible focus.
- [x] Setup and interview: adapt `SessionSetup.tsx` and `Interview.tsx`; green panel with real progress, new session form, environment and database. Preserve the existing handlers and states.
- [x] Question bank, stories and history: subagent with exclusive ownership of the three components and its own CSS. Apply the design's headers, lists and filters without changing contracts.
- [x] Integration: AI configuration, practice, comparison and warnings share the tokens. Check blocks, long texts, empty state, errors and loading states.
- [x] Verify: existing React tests, typecheck/build and visual comparison in the browser across the nine dimensions of the manifest. Document real commands/results and the limits of the validation.

## Verification focus

Navigation with pending review; capture in progress; missing/failed database; lists and long texts on narrow screens; absence of session and unknown availability. The existing tests cover the rules; the screenshots verify geometry and overflow. Add tests only for new behavior that is not covered.

## Result

See [layout validation](../../layout/validation.md): 106 React tests, build with exit code 0 and 72 checks in the browser without horizontal overflow or JavaScript errors. Manual acceptance in the desktop application remains pending.

Review before commit: compact audio recovery outside the setup, field margin limited to the forms, focus on page change, single tokens/CSS layers, WOFF2 fonts and reproducible matrix with Practice and long text.

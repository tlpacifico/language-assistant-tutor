# Tutor layout — 2026-11-04

Later change on the same day: the interview now includes the «Avaliação e correções da IA» panel, using the existing classes and components. The transcript review clarifies its optional purpose. See [functional validation](../ai-coaching/validation.md); the visual matrix below refers to the layout before this panel and does not constitute visual validation of this extension.

The layout was adapted from the [supplied HTML](../tutor-layout/index.html), following the [handoff](../tutor-layout/DESIGN-HANDOFF.md) and the [manifest](../tutor-layout/DESIGN-MANIFEST.json). The implementation keeps React/TypeScript, the Tauri contracts and the existing persistence. A subagent handled the collections (bank, stories and history); the structure, preparation, interview and integration were executed in the main agent.

## Visual system

| Element | Value |
| --- | --- |
| Background | `#F2F5F3` |
| Surfaces | `#FFFFFF`, `#F7FAF8` |
| Text / secondary | `#14231D`, `#586B62` |
| Borders | `#DCE5DF`, `#C6D4CC` |
| Accent / soft | `#0E6B52`, `#E4F1EB` |
| Session panel | `#0F3D30` |
| Headings / text | Newsreader / Public Sans |
| Navigation / content | 264 px / maximum of 1060 px |
| Radii | 14 px on cards, 10 px on controls, 18 px on the session panel |
| Transitions | 180 ms on controls, 350 ms on progress; respects reduced motion |

The WOFF2 variable fonts and OFL licenses are in `public/fonts`, without external requests during use. The two files total 158,832 bytes (155.1 KiB), replacing seven TTF files that totaled 544.6 KiB. `fonts.css` is imported directly by the application. The light theme follows the reference regardless of the Windows theme. `tokens.css` is the single source of the tokens; the sheets declare the order `tokens, legacy, layout, collections` through CSS layers, which makes precedence independent of the import order. The typographic base is 16 px; radii, text colors over accent/panel and durations have their own tokens. Below 1024 px, the navigation moves to the top with its own horizontal offset; below 900 px, the modules go to a single column. The small dimensions are frontend adaptation checks; the desktop window keeps the minimum of 800 × 600 defined in Tauri.

## Adaptation to the real behaviors

- Preparation: panel to resume the session with the real position and count; when there is no session, a form with local/AI mode, category and quantity. The complete environment panel and the database are in this view. On the other pages, «Áudio e recuperação» keeps Verificar disponibilidade and Reiniciar áudio local accessible without navigating or losing unsaved revisions, including when the navigation is blocked. The core keeps its restrictions: it does not restart the helper during transcription/synthesis and does not interrupt the native recording in progress.
- Bank and stories: toolbars, labels and cards; the existing filters, editors and validations are preserved. The additional free search and the prototype's skill chips were not introduced: the bank still filters by category and the stories by skill.
- Interview: question, recording, takes, automatic transcript, review and confirmation keep separate identities and actions. The side column shows real progress and STAR guidance. This replaces the prototype's «Roteiro da sessão» because `InterviewSnapshot` exposes the current question, not the list of the session's fixed questions. No list is invented and no session read is added for this layout change. The prototype's shortcuts to skip questions, the free notes or the simulated recording were not introduced.
- History: dates and summaries in rows, with the existing consultation, resume and later practice.
- Settings: uses the real destination, model, rate card, protected key and consent; it does not carry over the fictitious provider/voice/language options of the HTML.
- Practice, comparison, feedback and navigation decisions use the same visual system. The Prática page has its own current item in the navigation; Histórico is not marked as the current page during a practice. The content receives focus when changing page, but not at startup nor when the navigation is canceled. The Rust commands, the SQLite data and the audio engines were not changed.

The text was adapted to European Portuguese and corrected where the reference described browser storage or behaviors that the application does not offer. Demonstration statistics are not presented as real data.

## Verification performed

| Command / method | Result |
| --- | --- |
| `npm test` | 12 files, **106 tests passed** |
| `npm run build` | TypeScript and Vite with exit 0 |
| `git diff --check` | No whitespace errors |
| Chromium through Playwright, with controlled Tauri IPC | **72 checks**, with no horizontal overflow and no JavaScript errors |

The integration tests were updated for the per-view title, for availability in the preparation and for semantic progress. Three regression tests in `src/features/layout/Layout.test.tsx` cover recovery with the navigation blocked, focus after navigation and recovery in a Prática without losing the unsaved review. The checks for recording, confirmation, recovery, single subscription and decisions about unsaved changes are kept.

The visual matrix covers **the seven pages of the application** (Preparação, Banco de perguntas, Histórias, Entrevista, Histórico, Configuração IA and Prática), plus the new session form: eight states at **360×800, 390×844, 430×932, 600×960, 820×1180, 1024×768, 1366×768, 1440×900 and 1920×1080**. The Prática page uses long prose and a reference of 240 characters without spaces. The rehearsal also checks the 16 px between review fields of the interview/practice and between two transcripts/revisions of the history, and the equality of the base/button styles when inverting the CSS sheets, with the system preference for a dark theme. See [matrix results](responsive-results.json).

The rehearsal is reproducible through [scripts/layout-matrix.py](../../scripts/layout-matrix.py), with controlled Tauri IPC and synthetic data derived from `contracts/interview-v2.example.json`. Commands from the root, on Windows:

```powershell
# Optional preparation isolated from the visual checker (does not change the audio .venv)
py -3.13 -m venv tmp/layout-venv
.\tmp\layout-venv\Scripts\python.exe -m pip install playwright==1.57.0
.\tmp\layout-venv\Scripts\python.exe -m playwright install chromium
npm.cmd ci

# Terminal 1
npm.cmd run dev -- --host 127.0.0.1

# Terminal 2
.\tmp\layout-venv\Scripts\python.exe scripts/layout-matrix.py
```

The command actually executed on this machine was `python scripts/layout-matrix.py`, with Python 3.13 and Playwright 1.57.0 already available. The script stores screenshots and `results.json` in `tmp/layout-matrix` and exits with an error if there is overflow, a JavaScript error, insufficient spacing or a divergence in the sheet order. It accepts `--url` and `--output` to change the server/destination. It does not make paid calls, recordings or changes to the personal database.

Desktop and mobile screenshots were inspected and compared with the HTML exported at 1440×900. Screenshots of the implementation:

- [Preparação desktop](screenshots/preparacao-desktop.png)
- [Preparação mobile](screenshots/preparacao-mobile.png)
- [Nova sessão](screenshots/nova-sessao-desktop.png)
- [Entrevista desktop](screenshots/entrevista-desktop.png)
- [Prática desktop with long content](screenshots/pratica-desktop.png)
- [Prática mobile with long content](screenshots/pratica-mobile.png)

## Limits of the validation

This verification establishes the presentation in the browser and the absence of regressions detected by the React tests. It does not replace use in the Tauri window with real audio, a protected credential and a paid API. No installer was generated and the Rust/Python suite was not repeated, since those files did not change. The manual acceptances of phases 1 to 4 remain pending; phase 5 has not been started.

## Artigos page (2026-11-05)

The **Artigos** page was added to the menu and to the navigation (blocked during the recording or transcription of an article). The React tests cover the presence in the menu and the navigation block. **No visual validation was done** (desktop/mobile screenshots, contrast, focus) of this page: the seven-page matrix was not repeated and remains pending.

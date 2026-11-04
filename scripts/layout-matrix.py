"""Matriz visual local com IPC controlado; não lê nem altera dados pessoais.

Arrancar Vite e executar: python scripts/layout-matrix.py
Pré-requisitos e comandos completos: docs/layout/validation.md.
"""

import argparse
import copy
import json
from pathlib import Path

from playwright.sync_api import sync_playwright

ROOT = Path(__file__).resolve().parents[1]
VIEWPORTS = [(360, 800), (390, 844), (430, 932), (600, 960), (820, 1180),
             (1024, 768), (1366, 768), (1440, 900), (1920, 1080)]
VIEWS = [("Preparação", "setup", ".tutor-hero"),
         ("Banco de perguntas", "bank", ".collection__card"),
         ("Histórias", "stories", ".collection__story-notes"),
         ("Entrevista", "interview", "#review-title"),
         ("Histórico", "history", ".collection__history-row"),
         ("Configuração IA", "ai", ".tutor-ai-settings")]


def fixtures():
    contract = json.loads((ROOT / "contracts/interview-v2.example.json").read_text(encoding="utf-8"))
    snapshot = copy.deepcopy(contract["snapshot"])
    snapshot.update(database="ready", activeSession=contract["session"], current=contract["current"])
    snapshot["availability"] = {key: {"status": "ready", "message": None} for key in snapshot["availability"]}
    question = {**contract["current"]["question"], "id": "q-1", "version": 1,
                "updatedAt": "2026-11-04T10:00:00Z"}
    preparation = dict(answerId="practice-answer", version=1, selectedStoryId=None,
                       selectedStoryVersion=None, lockState="locked", support="none",
                       snapshot=None, lockedCaptureId=None)
    # Prosa longa e uma sequência sem espaços exercitam wrapping e revisão.
    text = ("I documented the deployment blockers, discussed the trade-offs with the team, "
            "and shared what we learned from the unsuccessful first attempt. " * 12)
    text += " IncidentReference-" + "abcdefghij" * 24
    recording = copy.deepcopy(contract["current"]["recordings"][0])
    recording.update(id="practice-recording", sessionQuestionId="practice-answer", answerId="practice-answer")
    recording["transcripts"][0]["text"] = text
    recording["review"].update(recordingId="practice-recording", text=text)
    practice = dict(id="practice-1", sourceSessionId=contract["session"]["id"],
                    sessionQuestionId=question["id"], ordinal=1, previousPracticeId=None,
                    status="answering", version=2, createdAt="2026-11-04T10:00:00Z",
                    confirmedAt=None, abandonedAt=None, question=question,
                    answer=dict(id="practice-answer", sessionQuestionId=question["id"],
                                role="attempt", text="", selectedRecordingId=recording["id"],
                                selectionVersion=1, practiceId="practice-1"),
                    recordings=[recording], confirmation=None, preparation=preparation)
    story = dict(id="s-1", version=1, missingFields=[], title="Melhorar a colaboração",
                 situation="Os lançamentos estavam a sofrer atrasos.", task="Identificar bloqueios.",
                 actions="Documentei os problemas e organizei uma revisão com a equipa.",
                 outcome="A equipa passou a partilhar os bloqueios mais cedo.",
                 competencies=["Colaboração"], createdAt="2026-11-04T09:00:00Z",
                 updatedAt="2026-11-04T09:00:00Z")
    settings = dict(version=1, baseUrl="https://opencode.ai/inference/openai/v1",
                    model="deepseek-v4.1-flash", tariff=None, credentialConfigured=False, consent=None)
    history_question = copy.deepcopy(contract["current"])
    second_take = copy.deepcopy(history_question["recordings"][0])
    second_take.update(id="history-second-take", ordinal=2)
    second_take["transcripts"][0]["id"] = "history-second-transcript"
    second_take["review"]["recordingId"] = second_take["id"]
    history_question["recordings"].append(second_take)
    return dict(snapshot=snapshot, questions=[{**question, "id": f"q-{i}"} for i in range(4)],
                story=story, settings=settings, preparation=preparation, practice=practice,
                sessionDetail=dict(summary=contract["session"], questions=[history_question]))


def init_script(data):
    return "const fixture = " + json.dumps(data, ensure_ascii=False) + ";" + """
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {unregisterListener: () => {}};
    window.__TAURI_INTERNALS__ = {
      metadata: {currentWindow: {label: 'main'}, currentWebview: {label: 'main'}},
      transformCallback: () => 1, unregisterCallback: () => {},
      invoke: async (cmd) => {
        if (cmd === 'plugin:event|listen') return 1;
        if (cmd === 'plugin:event|unlisten') return null;
        if (cmd === 'interview_snapshot') return fixture.snapshot;
        if (cmd === 'interview_list_questions') return fixture.questions;
        if (cmd === 'interview_list_stories') return [fixture.story];
        if (cmd === 'interview_list_sessions') return [fixture.snapshot.activeSession].filter(Boolean);
        if (cmd === 'interview_load_session') return fixture.sessionDetail;
        if (cmd === 'interview_list_legacy') return [];
        if (cmd === 'interview_ai_load_ai_settings') return fixture.settings;
        if (cmd === 'interview_load_preparation') return fixture.preparation;
        if (cmd === 'interview_load_practice') return fixture.practice;
        throw new Error('Comando não previsto no ensaio visual: ' + cmd);
      }
    };
    """


def field_gap(page, selector):
    return page.locator(selector).evaluate("""panel => {
      const fields = [...panel.children].filter(e => e.classList.contains('practice__field'));
      if (fields.length < 2) throw new Error('Não há dois campos para verificar o espaçamento');
      return parseFloat(getComputedStyle(fields[1]).marginTop);
    }""")


def check_cascade(page):
    paths = ["src/features/layout/tokens.css", "src/features/practice/practice.css",
             "src/features/layout/tutor-layout.css", "src/features/layout/collections.css"]
    sheets = [(ROOT / path).read_text(encoding="utf-8") for path in paths]
    values = []
    for ordered in [sheets, list(reversed(sheets))]:
        values.append(page.evaluate("""css => {
          const frame = document.createElement('iframe'); document.body.append(frame);
          frame.contentDocument.write('<style>' + css + '</style><div class="practice"><button>Teste</button></div>');
          const doc = frame.contentDocument;
          const button = getComputedStyle(doc.querySelector('button'));
          const body = getComputedStyle(doc.body);
          const result = {background: body.backgroundColor, font: body.fontSize,
                          button: button.backgroundColor, radius: button.borderRadius,
                          padding: button.padding, ink: body.color};
          frame.remove(); return result;
        }""", "\n".join(ordered)))
    assert values[0] == values[1], f"Estilos dependem da ordem: {values}"
    assert values[0]["font"] == "16px" and values[0]["background"] == "rgb(242, 245, 243)"
    return values[0]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--url", default="http://127.0.0.1:1420")
    parser.add_argument("--output", type=Path, default=ROOT / "tmp/layout-matrix")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    data = fixtures()
    results, errors, spacing = [], [], []
    with sync_playwright() as pw:
        browser = pw.chromium.launch()

        def new_page(fixture):
            page = browser.new_page(viewport={"width": 1440, "height": 900}, color_scheme="dark")
            page.on("pageerror", lambda error: errors.append(str(error)))
            page.add_init_script(init_script(fixture))
            page.goto(args.url)
            return page

        def record(page, key, width, height):
            page.evaluate("document.fonts.ready")
            overflow = page.evaluate("document.documentElement.scrollWidth > innerWidth")
            results.append(dict(width=width, height=height, view=key, overflow=overflow))
            if width in [390, 1440]:
                page.screenshot(path=str(args.output / f"{key}-{width}.png"), full_page=True)

        page = new_page(data)
        for width, height in VIEWPORTS:
            page.set_viewport_size(dict(width=width, height=height))
            for label, key, ready in VIEWS:
                page.get_by_role("navigation").get_by_role("button", name=label, exact=True).click()
                page.wait_for_selector(ready)
                if key == "history":
                    page.get_by_role("button", name="Abrir sessão", exact=True).click()
                    page.wait_for_selector('section[aria-labelledby^="history-review-"]')
                    for kind in ["raw", "review"]:
                        gap = field_gap(page, f'section[aria-labelledby^="history-{kind}-"]')
                        spacing.append(dict(view=f"history-{kind}", width=width, gap=gap))
                        assert gap >= 16, f"Campos do histórico colados: {gap}"
                record(page, key, width, height)
                if key == "interview":
                    gap = field_gap(page, '[aria-labelledby="review-title"]')
                    spacing.append(dict(view=key, width=width, gap=gap))
                    assert gap >= 16, f"Campos da entrevista colados: {gap}"
        cascade = check_cascade(page)
        page.close()

        empty = copy.deepcopy(data)
        empty["snapshot"].update(activeSession=None, current=None)
        page = new_page(empty)
        page.wait_for_selector(".tutor-new-session")
        for width, height in VIEWPORTS:
            page.set_viewport_size(dict(width=width, height=height))
            record(page, "new-session", width, height)
        page.close()

        practice = copy.deepcopy(empty)
        practice["snapshot"]["practice"].update(activePractice=practice["practice"],
            activeOwner=dict(type="practice", practiceId="practice-1", answerId="practice-answer"))
        page = new_page(practice)
        page.get_by_role("button", name="Retomar prática pendente").click()
        page.get_by_label("Revisão editável").wait_for()
        assert page.get_by_role("navigation").get_by_role("button", name="Prática", exact=True).get_attribute("aria-current") == "page"
        for width, height in VIEWPORTS:
            page.set_viewport_size(dict(width=width, height=height))
            record(page, "practice", width, height)
            gap = field_gap(page, 'section[aria-labelledby="practice-review"]')
            spacing.append(dict(view="practice", width=width, gap=gap))
            assert gap >= 16, f"Campos da prática colados: {gap}"
        browser.close()
    report = dict(checks=results, errors=errors, spacing=spacing, cascade=cascade,
                  systemColorScheme="dark", command="python scripts/layout-matrix.py")
    (args.output / "results.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    failures = [row for row in results if row["overflow"]]
    print(json.dumps(dict(checks=len(results), overflows=failures, errors=errors), ensure_ascii=False))
    assert not failures and not errors, "A matriz encontrou falhas; consultar results.json"


if __name__ == "__main__":
    main()

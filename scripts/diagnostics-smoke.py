"""Ensaio do painel num WebView2 real com dados explicitamente isolados.

Requer Playwright e um build Tauri de ensaio com appDirectoriesOverride;
nunca usar a depuração remota com os dados pessoais da aplicação.
"""
import argparse
import json
from pathlib import Path
from playwright.sync_api import sync_playwright


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--cdp", required=True)
    parser.add_argument("--data-root", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    root = args.data_root.resolve()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    with sync_playwright() as playwright:
        browser = playwright.chromium.connect_over_cdp(args.cdp)
        pages = [page for context in browser.contexts for page in context.pages if "tauri.localhost" in page.url or page.url.startswith("tauri://")]
        assert len(pages) == 1, "WebView da aplicação não encontrado"
        page = pages[0]
        page.wait_for_function("typeof window.__TAURI_INTERNALS__?.invoke === 'function'")
        status = page.evaluate("window.__TAURI_INTERNALS__.invoke('diagnostics_status')")
        assert Path(status["directory"]).resolve() == root / "diagnostics", "O ensaio exige dados isolados"
        page.get_by_text("Diagnóstico local", exact=True).click()
        page.get_by_role("button", name="Exportar diagnóstico", exact=True).wait_for()
        page.get_by_label("Pasta de destino", exact=True).fill(str(output))
        page.evaluate("setTimeout(() => { throw new Error('PRIVATE_SMOKE_RESPONSE'); }, 0)")
        page.wait_for_timeout(150)
        page.get_by_role("button", name="Exportar diagnóstico", exact=True).click()
        page.get_by_role("status").filter(has_text="Diagnóstico guardado em").wait_for()
        exports = sorted(output.glob("tutor-diagnostic-*.json"), key=lambda path: path.stat().st_mtime_ns)
        assert exports
        text = exports[-1].read_text(encoding="utf-8")
        value = json.loads(text)
        assert any(event["operation"] == "window_error" for event in value["events"])
        assert "PRIVATE_SMOKE_RESPONSE" not in text
        assert value["diagnosticStatus"]["build"]["version"] == status["build"]["version"]
        assert value["diagnosticStatus"]["writable"] is True
        assert value["diagnosticStatus"]["dropped"] == 0
        page.screenshot(path=str(output / "diagnostics-webview.png"), full_page=True)
        result = {"build": status["build"], "events": len(value["events"]), "runs": len({event["runId"] for event in value["events"]}), "export": exports[-1].name, "passed": True}
        (output / "result.json").write_text(json.dumps(result, indent=2), encoding="utf-8")
        print(json.dumps(result))
        browser.close()


if __name__ == "__main__":
    main()

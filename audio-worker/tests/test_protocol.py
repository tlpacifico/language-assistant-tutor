"""Contrato do protocolo v1 entre a app Tauri e o auxiliar Python.

Estes testes nao carregam modelos: verificam apenas o parser, a codificacao das
mensagens, os exemplos partilhados e o laco de servidor com um registo falso.
"""

from __future__ import annotations

import io
import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

from audio_worker import __main__ as worker_main
from audio_worker.protocol import (
    COMPONENT_STATUSES,
    ERROR_CODES,
    PROGRESS_STAGES,
    PROTOCOL_VERSION,
    ProtocolError,
    WorkerRequest,
    component_state,
    encode_message,
    error_message,
    parse_request,
    progress_message,
    result_message,
)

REPO_ROOT = Path(__file__).resolve().parents[2]
EXAMPLES_PATH = REPO_ROOT / "contracts" / "worker-v1.examples.json"
PACKAGE_ROOT = Path(__file__).resolve().parents[1]


def load_examples() -> dict:
    return json.loads(EXAMPLES_PATH.read_text(encoding="utf-8"))


def test_protocol_examples():
    examples = load_examples()
    assert examples["protocolVersion"] == PROTOCOL_VERSION
    assert set(examples["requests"]) == {
        "health",
        "transcribe",
        "synthesize",
        "preload",
        "shutdown",
    }

    parsed = {}
    for op, message in examples["requests"].items():
        request = parse_request(json.dumps(message, ensure_ascii=False))
        assert isinstance(request, WorkerRequest)
        assert request.protocol_version == PROTOCOL_VERSION
        assert request.op == op
        assert request.id == message["id"]
        assert isinstance(request.params, dict)
        parsed[op] = request

    request = parsed["transcribe"]
    assert request.op == "transcribe"
    assert request.params["language"] == "en"
    assert request.params["device"] == "cpu"
    assert request.params["computeType"] == "int8"
    # Os caminhos dos exemplos sao absolutos e contem espacos (Windows real).
    assert " " in request.params["audioPath"]
    assert parsed["synthesize"].params["voice"] == "af_heart"
    assert parsed["synthesize"].params["speed"] == 1.0
    # O carregamento antecipado não leva parâmetros.
    assert parsed["preload"].params == {}

    for name, message in examples["responses"].items():
        assert message["protocolVersion"] == PROTOCOL_VERSION, name
        assert message["event"] in {"result", "progress"}, name
        encoded = encode_message(message)
        assert "\n" not in encoded, name
        assert json.loads(encoded) == message, name
        if message["event"] == "progress":
            assert message["stage"] in PROGRESS_STAGES, name
            assert "result" not in message and "error" not in message, name
        else:
            assert isinstance(message["ok"], bool), name
            if message["ok"]:
                assert "result" in message and "error" not in message, name
            else:
                error = message["error"]
                assert error["code"] in ERROR_CODES, name
                assert isinstance(error["message"], str) and error["message"], name
                assert isinstance(error["retryable"], bool), name

    health = examples["responses"]["healthResult"]["result"]
    assert set(health) == {"transcription", "synthesis"}
    for state in health.values():
        assert state["status"] in COMPONENT_STATUSES
        assert state["message"] is None or isinstance(state["message"], str)
    assert health["synthesis"]["status"] == "missing"

    transcribe = examples["responses"]["transcribeResult"]["result"]
    assert set(transcribe) == {
        "status",
        "text",
        "segments",
        "model",
        "language",
        "loadMs",
        "processingMs",
    }
    assert transcribe["status"] in {"completed", "no_speech"}
    assert transcribe["model"] == "small"
    assert transcribe["language"] == "en"
    for segment in transcribe["segments"]:
        assert set(segment) == {"startMs", "endMs", "text"}
        assert isinstance(segment["startMs"], int) and isinstance(segment["endMs"], int)
    assert examples["responses"]["transcribeNoSpeech"]["result"]["segments"] == []

    synthesize = examples["responses"]["synthesizeResult"]["result"]
    assert set(synthesize) == {
        "audioPath",
        "sampleRate",
        "durationMs",
        "loadMs",
        "processingMs",
    }
    assert synthesize["sampleRate"] == 24000
    assert isinstance(synthesize["durationMs"], int)

    assert examples["responses"]["shutdownResult"]["result"] == {"stopped": True}

    for invalid in examples["invalidRequests"]:
        with pytest.raises(ProtocolError) as raised:
            parse_request(invalid["line"])
        assert raised.value.code == invalid["errorCode"]
        assert raised.value.to_error()["retryable"] is False


def test_stdout_contains_only_json():
    class NoisyRegistry:
        """Registo falso que imita uma biblioteca a escrever para stdout."""

        def health(self):
            print("aviso de uma biblioteca qualquer")
            return {
                "transcription": component_state("ready"),
                "synthesis": component_state("missing", "Falta a voz af_heart."),
            }

        def transcribe(self, params, progress):
            progress("loading")
            print("barulho durante o carregamento")
            progress("processing")
            return {
                "status": "no_speech",
                "text": "",
                "segments": [],
                "model": "small",
                "language": "en",
                "loadMs": 1,
                "processingMs": 2,
            }

        def synthesize(self, params, progress):
            progress("loading")
            progress("processing")
            return {
                "audioPath": params["outputPath"],
                "sampleRate": 24000,
                "durationMs": 10,
                "loadMs": 1,
                "processingMs": 2,
            }

    stdin = io.StringIO(
        "\n".join(
            [
                json.dumps(load_examples()["requests"]["health"]),
                json.dumps(
                    {
                        "protocolVersion": 1,
                        "id": "req-transcribe-1",
                        "op": "transcribe",
                        "params": {"audioPath": "C:\\Audio Local\\a.wav", "modelPath": "D:\\M\\small"},
                    }
                ),
                json.dumps(load_examples()["requests"]["shutdown"]),
                "",
            ]
        )
    )
    capture = io.StringIO()
    diagnostics = io.StringIO()

    code = worker_main.serve(stdin, capture, diagnostics, registry=NoisyRegistry())

    stdout = capture.getvalue()
    assert code == 0
    assert stdout
    assert all(json.loads(line)["protocolVersion"] == 1 for line in stdout.splitlines())
    messages = [json.loads(line) for line in stdout.splitlines()]
    assert [message["event"] for message in messages] == [
        "result",
        "progress",
        "progress",
        "result",
        "result",
    ]
    assert [message["id"] for message in messages] == [
        "req-health-1",
        "req-transcribe-1",
        "req-transcribe-1",
        "req-transcribe-1",
        "req-shutdown-1",
    ]
    assert messages[0]["result"]["synthesis"]["status"] == "missing"
    assert messages[1]["stage"] == "loading"
    assert messages[2]["stage"] == "processing"
    assert messages[3]["result"]["status"] == "no_speech"
    assert messages[4]["result"] == {"stopped": True}
    # O ruido das bibliotecas foi desviado para stderr.
    assert "aviso de uma biblioteca qualquer" in diagnostics.getvalue()
    assert "barulho durante o carregamento" in diagnostics.getvalue()


def test_preload_is_served_with_component_states():
    class Registry:
        def preload(self, progress):
            progress("loading")
            return {
                "transcription": component_state("ready"),
                "synthesis": component_state("missing", "Falta a voz af_heart."),
            }

    stdin = io.StringIO(
        "\n".join(
            [
                json.dumps(
                    {"protocolVersion": 1, "id": "req-preload-1", "op": "preload", "params": {}}
                ),
                json.dumps(load_examples()["requests"]["shutdown"]),
                "",
            ]
        )
    )
    capture = io.StringIO()

    code = worker_main.serve(stdin, capture, io.StringIO(), registry=Registry())

    messages = [json.loads(line) for line in capture.getvalue().splitlines()]
    assert code == 0
    assert [message["event"] for message in messages] == ["progress", "result", "result"]
    assert messages[1]["result"] == {
        "transcription": {"status": "ready", "message": None},
        "synthesis": {"status": "missing", "message": "Falta a voz af_heart."},
    }


def test_blank_lines_and_eof_produce_no_output(tmp_path: Path):
    capture = io.StringIO()
    code = worker_main.serve(io.StringIO("\n   \n"), capture, io.StringIO(), registry=object())
    assert code == 0
    assert capture.getvalue() == ""


def test_invalid_request_yields_a_single_error_result():
    capture = io.StringIO()
    code = worker_main.serve(
        io.StringIO("isto nao e um pedido\n"),
        capture,
        io.StringIO(),
        registry=object(),
    )
    lines = capture.getvalue().splitlines()
    assert code == 0
    assert len(lines) == 1
    message = json.loads(lines[0])
    assert message["event"] == "result"
    assert message["ok"] is False
    assert message["error"]["code"] == "WORKER_PROTOCOL"
    assert message["error"]["retryable"] is False


def test_unexpected_engine_failure_stays_inside_the_protocol():
    class BrokenRegistry:
        def health(self):
            raise RuntimeError("falha inesperada do motor")

    capture = io.StringIO()
    diagnostics = io.StringIO()
    worker_main.serve(
        io.StringIO(json.dumps({"protocolVersion": 1, "id": "req-1", "op": "health"}) + "\n"),
        capture,
        diagnostics,
        registry=BrokenRegistry(),
    )
    message = json.loads(capture.getvalue().splitlines()[0])
    assert message["ok"] is False
    assert message["error"]["code"] == "WORKER_UNAVAILABLE"
    assert message["error"]["retryable"] is True
    # O rasto completo fica no diagnostico, nunca no stdout.
    assert "falha inesperada do motor" in diagnostics.getvalue()
    structured = [line.removeprefix("TUTOR_DIAGNOSTIC ") for line in diagnostics.getvalue().splitlines() if line.startswith("TUTOR_DIAGNOSTIC ")]
    assert structured
    event = json.loads(structured[0])
    assert event["component"] == "worker"
    assert event["operation"] == "health"
    assert event["code"] == "UNEXPECTED"
    assert "falha inesperada do motor" not in json.dumps(event)
    assert "test_protocol.py" not in json.dumps(event)


def test_progress_and_error_helpers_shapes():
    assert progress_message("req-1", "loading") == {
        "protocolVersion": PROTOCOL_VERSION,
        "id": "req-1",
        "event": "progress",
        "stage": "loading",
    }
    with pytest.raises(ValueError):
        progress_message("req-1", "downloading")

    assert result_message("req-1", {"stopped": True}) == {
        "protocolVersion": PROTOCOL_VERSION,
        "id": "req-1",
        "event": "result",
        "ok": True,
        "result": {"stopped": True},
    }

    error = ProtocolError("Pedido invalido.")
    assert error_message("req-1", error) == {
        "protocolVersion": PROTOCOL_VERSION,
        "id": "req-1",
        "event": "result",
        "ok": False,
        "error": {"code": "WORKER_PROTOCOL", "message": "Pedido invalido.", "retryable": False},
    }


@pytest.mark.parametrize(
    "line",
    [
        "",
        "   ",
        "{",
        "[]",
        "null",
        json.dumps({"protocolVersion": 1, "op": "health"}),
        json.dumps({"protocolVersion": 1, "id": "", "op": "health"}),
        json.dumps({"protocolVersion": 1, "id": 7, "op": "health"}),
        json.dumps({"protocolVersion": 1.5, "id": "req-1", "op": "health"}),
        json.dumps({"protocolVersion": 1, "id": "req-1", "op": "translate"}),
        json.dumps({"protocolVersion": 1, "id": "req-1", "op": "health", "params": []}),
    ],
)
def test_parse_request_rejects_invalid_lines(line: str):
    with pytest.raises(ProtocolError) as raised:
        parse_request(line)
    assert raised.value.code == "WORKER_PROTOCOL"
    assert raised.value.to_error()["message"]
    assert raised.value.to_error()["retryable"] is False


def test_parse_request_defaults_missing_params():
    request = parse_request(json.dumps({"protocolVersion": 1, "id": "req-1", "op": "shutdown"}))
    assert request.op == "shutdown"
    assert request.params == {}


def test_module_serves_health_and_shutdown_without_a_manifest(tmp_path: Path):
    environment = dict(os.environ)
    environment["PYTHONPATH"] = str(PACKAGE_ROOT / "src")
    environment.pop("PYTHONIOENCODING", None)

    completed = subprocess.run(
        [sys.executable, "-u", "-m", "audio_worker"],
        input="\n".join(
            [
                json.dumps({"protocolVersion": 1, "id": "req-health-1", "op": "health", "params": {}}),
                json.dumps({"protocolVersion": 1, "id": "req-shutdown-1", "op": "shutdown", "params": {}}),
            ]
        )
        + "\n",
        capture_output=True,
        text=True,
        encoding="utf-8",
        cwd=str(tmp_path),
        env=environment,
        timeout=180,
    )

    assert completed.returncode == 0, completed.stderr
    lines = completed.stdout.splitlines()
    assert len(lines) == 2, completed.stdout
    assert all(json.loads(line)["protocolVersion"] == 1 for line in lines)
    health = json.loads(lines[0])
    assert health["ok"] is True
    assert set(health["result"]) == {"transcription", "synthesis"}
    assert health["result"]["transcription"]["status"] in COMPONENT_STATUSES
    assert health["result"]["synthesis"]["status"] in COMPONENT_STATUSES
    assert json.loads(lines[1])["result"] == {"stopped": True}

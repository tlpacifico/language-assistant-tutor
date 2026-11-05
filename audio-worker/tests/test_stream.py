"""Síntese incremental (``synthesize_stream``): blocos ordenados, cancelamento e
compatibilidade com os pedidos antigos."""

from __future__ import annotations

import io
import json
import time
from pathlib import Path

import pytest

from audio_worker import __main__ as worker_main
from audio_worker import engines
from audio_worker.engines import EngineRegistry
from audio_worker.protocol import (
    OPERATIONS,
    ProtocolError,
    WorkerError,
    chunk_message,
    parse_request,
)
from test_engines import create_kokoro_assets, create_whisper_assets, write_manifest


class _Chunk:
    def __init__(self, audio):
        self.audio = audio


def _ready_registry(tmp_path: Path, monkeypatch, pipeline) -> EngineRegistry:
    registry = EngineRegistry(write_manifest(tmp_path))
    paths = registry.paths()
    create_whisper_assets(paths)
    create_kokoro_assets(paths)
    monkeypatch.setattr(engines, "missing_synthesis_assets", lambda paths: [])
    monkeypatch.setattr(engines, "load_kokoro_pipeline", lambda paths, **kwargs: pipeline)
    return registry


def _params(directory: Path, text: str = "One. Two. Three.") -> dict:
    return {"text": text, "outputDir": str(directory), "voice": "af_heart", "speed": 1.0}


def test_protocol_accepts_stream_and_cancel_and_builds_chunk_events():
    assert "synthesize_stream" in OPERATIONS and "cancel" in OPERATIONS
    request = parse_request(
        json.dumps(
            {
                "protocolVersion": 1,
                "id": "r1",
                "op": "synthesize_stream",
                "params": {"text": "Hi."},
            }
        )
    )
    assert request.op == "synthesize_stream"
    assert chunk_message("r1", 0, "C:\\x\\chunk-0000.wav", 24000, 500) == {
        "protocolVersion": 1,
        "id": "r1",
        "event": "audio_chunk",
        "index": 0,
        "audioPath": "C:\\x\\chunk-0000.wav",
        "sampleRate": 24000,
        "durationMs": 500,
    }
    with pytest.raises(ProtocolError):
        parse_request('{"protocolVersion":1,"id":"r","op":"synthesize_streams"}')


def test_first_chunk_is_delivered_before_the_next_is_generated(tmp_path: Path, monkeypatch):
    import numpy

    events: list[tuple[str, int]] = []
    emitted: list[dict] = []

    def pipeline(text, voice, speed):
        for index in range(3):
            # Quando o 2.º bloco é pedido, o 1.º já foi entregue.
            events.append(("generate", len(emitted)))
            yield _Chunk(numpy.full(2400, 0.1, dtype=numpy.float32))

    registry = _ready_registry(tmp_path, monkeypatch, pipeline)
    out = tmp_path / "out"

    def emit(index, path, sample_rate, duration_ms):
        assert Path(path).is_file(), "o WAV completo existe quando o bloco é entregue"
        emitted.append(
            {"index": index, "path": path, "sampleRate": sample_rate, "durationMs": duration_ms}
        )

    result = registry.synthesize_stream(_params(out), lambda stage: None, emit)

    assert [item["index"] for item in emitted] == [0, 1, 2]
    assert events == [("generate", 0), ("generate", 1), ("generate", 2)]
    assert [Path(item["path"]).name for item in emitted] == [
        "chunk-0000.wav",
        "chunk-0001.wav",
        "chunk-0002.wav",
    ]
    assert all(item["sampleRate"] == 24000 and item["durationMs"] == 100 for item in emitted)
    assert result["chunks"] == 3 and result["cancelled"] is False
    # Nunca fica um ficheiro em escrita.
    assert sorted(p.name for p in out.iterdir()) == [
        "chunk-0000.wav",
        "chunk-0001.wav",
        "chunk-0002.wav",
    ]


def test_error_after_the_first_chunk_keeps_the_delivered_chunk(tmp_path: Path, monkeypatch):
    import numpy

    def pipeline(text, voice, speed):
        yield _Chunk(numpy.zeros(2400, dtype=numpy.float32))
        raise RuntimeError("falha do motor")

    registry = _ready_registry(tmp_path, monkeypatch, pipeline)
    emitted: list[int] = []
    with pytest.raises(WorkerError) as error:
        registry.synthesize_stream(
            _params(tmp_path / "out"), lambda stage: None, lambda i, *a: emitted.append(i)
        )
    assert emitted == [0]
    assert error.value.retryable is True
    assert (tmp_path / "out" / "chunk-0000.wav").is_file()


def test_cancel_stops_between_chunks_without_error(tmp_path: Path, monkeypatch):
    import numpy

    produced = []

    def pipeline(text, voice, speed):
        for _ in range(5):
            produced.append(1)
            yield _Chunk(numpy.zeros(2400, dtype=numpy.float32))

    registry = _ready_registry(tmp_path, monkeypatch, pipeline)
    emitted: list[int] = []
    result = registry.synthesize_stream(
        _params(tmp_path / "out"),
        lambda stage: None,
        lambda index, *a: emitted.append(index),
        should_cancel=lambda: len(emitted) >= 1,
    )
    assert emitted == [0]
    assert result["cancelled"] is True and result["chunks"] == 1
    assert len(produced) == 1, "não gera mais blocos depois de cancelado"


@pytest.mark.parametrize(
    "params",
    [
        {"text": "  ", "outputDir": "C:\\abs"},
        {"text": "Hi.", "outputDir": "relativo"},
        {"text": "Hi.", "outputDir": "C:\\abs", "voice": "other"},
        {"text": "Hi.", "outputDir": "C:\\abs", "speed": 0},
    ],
)
def test_invalid_stream_params_are_protocol_errors(tmp_path: Path, monkeypatch, params):
    registry = _ready_registry(tmp_path, monkeypatch, lambda *a, **k: iter(()))
    if params["outputDir"] == "C:\\abs":
        params = {**params, "outputDir": str(tmp_path / "abs")}
    with pytest.raises(WorkerError) as error:
        registry.synthesize_stream(params, lambda stage: None, lambda *a: None)
    assert error.value.code == "WORKER_PROTOCOL"


class _StreamRegistry:
    def synthesize_stream(self, params, progress, emit, should_cancel=lambda: False):
        count = 0
        for index in range(3):
            emit(index, f"C:\\out\\chunk-{index:04d}.wav", 24000, 100)
            count += 1
            time.sleep(0.05)  # dá tempo à thread de leitura para ver o cancel
            if should_cancel():
                return {"chunks": count, "sampleRate": 24000, "cancelled": True}
        return {"chunks": count, "sampleRate": 24000, "cancelled": False}

    def synthesize(self, params, progress):
        return {"audioPath": "x.wav", "sampleRate": 24000, "durationMs": 1, "loadMs": 0, "processingMs": 0}


def _serve(lines: list[dict]) -> list[dict]:
    stdin = io.StringIO("".join(json.dumps(line) + "\n" for line in lines))
    stdout, stderr = io.StringIO(), io.StringIO()
    worker_main.serve(stdin, stdout, stderr, registry=_StreamRegistry())
    return [json.loads(line) for line in stdout.getvalue().splitlines()]


def _request(op: str, request_id: str, params: dict | None = None) -> dict:
    return {"protocolVersion": 1, "id": request_id, "op": op, "params": params or {}}


def test_serve_streams_chunks_then_exactly_one_result():
    messages = _serve([_request("synthesize_stream", "s1", {"text": "Hi."})])
    assert [m["event"] for m in messages] == ["audio_chunk"] * 3 + ["result"]
    assert [m["index"] for m in messages[:3]] == [0, 1, 2]
    assert messages[-1]["ok"] is True and messages[-1]["result"]["chunks"] == 3
    assert {m["id"] for m in messages} == {"s1"}


def test_serve_cancel_is_silent_and_stops_the_active_stream():
    messages = _serve(
        [
            _request("synthesize_stream", "s1", {"text": "Hi."}),
            _request("cancel", "c1", {"target": "s1"}),
            _request("synthesize", "old-1", {"text": "Hi.", "outputPath": "C:\\x.wav"}),
        ]
    )
    stream = [m for m in messages if m["id"] == "s1"]
    assert stream[-1]["event"] == "result" and stream[-1]["result"]["cancelled"] is True
    assert sum(1 for m in stream if m["event"] == "audio_chunk") == 1
    # O cancelamento não produz mensagem própria e o pedido antigo continua igual.
    assert not [m for m in messages if m["id"] == "c1"]
    old = [m for m in messages if m["id"] == "old-1"]
    assert len(old) == 1 and old[0]["result"]["audioPath"] == "x.wav"


def test_serve_ignores_a_cancel_when_nothing_is_streaming():
    messages = _serve([_request("cancel", "c1", {"target": "nada"})])
    assert messages == []


def test_shared_stream_examples_are_valid_protocol_messages():
    path = Path(__file__).resolve().parents[2] / "contracts" / "worker-v1.stream.examples.json"
    examples = json.loads(path.read_text(encoding="utf-8"))
    for message in examples["requests"].values():
        request = parse_request(json.dumps(message, ensure_ascii=False))
        assert request.op == message["op"]
    chunk = examples["responses"]["audioChunk"]
    assert chunk == chunk_message(
        chunk["id"], chunk["index"], chunk["audioPath"], chunk["sampleRate"], chunk["durationMs"]
    )

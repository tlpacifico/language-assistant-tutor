"""Motores locais: disponibilidade, preparacao exigida e mapeamento de resultados.

Nenhum teste carrega pesos reais. As quatro verificacoes fixadas pelo plano sao
as duas primeiras funcoes: ausencia de recursos nao provoca ligacoes de rede e a
chave da cache de sintese depende da voz e do texto.
"""

from __future__ import annotations

import contextlib
import importlib.util
import json
import os
import socket
import subprocess
import sys
import types
from pathlib import Path

import pytest

from audio_worker import engines
from audio_worker.engines import (
    SAMPLE_RATE,
    EngineRegistry,
    ModelPaths,
    synthesis_cache_key,
)
from audio_worker.protocol import COMPONENT_STATUSES, WorkerError

PACKAGE_ROOT = Path(__file__).resolve().parents[1]
QUESTION = "Tell me about a time you solved a difficult problem at work."


@contextlib.contextmanager
def blocked_network(monkeypatch):
    """Falha qualquer acesso a rede ou subprocesso de download."""

    def refuse(*args, **kwargs):
        raise AssertionError("o auxiliar tentou aceder a rede ou a um subprocesso")

    monkeypatch.setattr(socket.socket, "connect", refuse)
    monkeypatch.setattr(socket.socket, "connect_ex", refuse)
    monkeypatch.setattr(socket, "create_connection", refuse)
    monkeypatch.setattr(socket, "getaddrinfo", refuse)
    monkeypatch.setattr(subprocess, "Popen", refuse)
    monkeypatch.setattr(subprocess, "run", refuse)
    monkeypatch.setattr(os, "system", refuse)
    yield


def write_manifest(tmp_path: Path, **overrides) -> Path:
    """Manifesto de modelos cujos caminhos nao existem por omissao."""
    base = tmp_path / "Modelos de Voz"
    manifest = {
        "schemaVersion": 1,
        "preparedAt": "2026-11-01T10:00:00Z",
        "whisperDir": str(base / "faster-whisper-small"),
        "whisperModel": "small",
        "kokoroConfig": str(base / "kokoro" / "config.json"),
        "kokoroWeights": str(base / "kokoro" / "kokoro-v1_0.pth"),
        "kokoroVoice": str(base / "kokoro" / "voices" / "af_heart.pt"),
        "revisions": {},
        "resources": {"spacyModel": "en_core_web_sm"},
    }
    manifest.update(overrides)
    path = tmp_path / "models.json"
    path.write_text(json.dumps(manifest, ensure_ascii=False), encoding="utf-8")
    return path


def create_whisper_assets(paths: ModelPaths) -> None:
    directory = Path(paths.whisper_dir)
    directory.mkdir(parents=True, exist_ok=True)
    for name in ("model.bin", "config.json", "tokenizer.json", "vocabulary.txt"):
        (directory / name).write_bytes(b"0")


def create_kokoro_assets(paths: ModelPaths) -> None:
    for value in (paths.kokoro_config, paths.kokoro_weights, paths.kokoro_voice):
        path = Path(value)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(b"0")


def test_missing_assets_does_not_connect(tmp_path: Path, monkeypatch):
    registry = EngineRegistry(write_manifest(tmp_path))

    with blocked_network(monkeypatch):
        health = registry.health()

    assert health["synthesis"]["status"] == "missing"
    assert health["transcription"]["status"] == "missing"
    assert "af_heart" in health["synthesis"]["message"]
    assert "faster-whisper-small" in health["transcription"]["message"]
    assert all(state["status"] in COMPONENT_STATUSES for state in health.values())


def test_voice_and_text_cache_key():
    first_key = synthesis_cache_key(QUESTION, "af_heart", 1.0)
    changed_voice_key = synthesis_cache_key(QUESTION, "am_michael", 1.0)

    assert first_key != changed_voice_key
    assert synthesis_cache_key(QUESTION, "af_heart", 1.0) == first_key
    assert synthesis_cache_key(QUESTION, "af_heart", 0.9) != first_key
    assert synthesis_cache_key(QUESTION.upper(), "af_heart", 1.0) != first_key
    assert synthesis_cache_key(f"  {QUESTION}\n", "af_heart", 1.0) == first_key
    assert len(first_key) == 64
    assert set(first_key) <= set("0123456789abcdef")


def test_missing_manifest_reports_both_components(tmp_path: Path, monkeypatch):
    registry = EngineRegistry(None)

    with blocked_network(monkeypatch):
        health = registry.health()

    assert health["transcription"]["status"] == "missing"
    assert health["synthesis"]["status"] == "missing"
    assert "manifesto" in health["transcription"]["message"].lower()


def test_invalid_manifest_is_an_error_not_a_download(tmp_path: Path, monkeypatch):
    manifest = tmp_path / "models.json"
    manifest.write_text("{ isto nao e json", encoding="utf-8")
    registry = EngineRegistry(manifest)

    with blocked_network(monkeypatch):
        health = registry.health()

    assert health["transcription"]["status"] == "error"
    assert health["synthesis"]["status"] == "error"
    assert health["transcription"]["message"]


def test_relative_manifest_paths_are_rejected(tmp_path: Path):
    registry = EngineRegistry(write_manifest(tmp_path, whisperDir="modelos\\small"))

    health = registry.health()

    assert health["transcription"]["status"] == "error"
    assert "absoluto" in health["transcription"]["message"].lower()


def test_health_ready_when_assets_and_dependencies_are_present(tmp_path: Path, monkeypatch):
    manifest = write_manifest(tmp_path)
    registry = EngineRegistry(manifest)
    paths = registry.paths()
    create_whisper_assets(paths)
    create_kokoro_assets(paths)
    monkeypatch.setattr(engines, "missing_synthesis_assets", lambda paths: [])

    health = registry.health()

    assert health["transcription"] == {"status": "ready", "message": None}
    assert health["synthesis"] == {"status": "ready", "message": None}


def test_missing_synthesis_assets_name_the_english_resource(tmp_path: Path):
    paths = engines.ModelPaths(
        whisper_dir=Path("C:/M/small"),
        whisper_model_name="small",
        kokoro_config=tmp_path / "config.json",
        kokoro_weights=tmp_path / "kokoro-v1_0.pth",
        kokoro_voice=tmp_path / "af_heart.pt",
        spacy_model="en_core_web_sm",
    )
    create_kokoro_assets(paths)

    missing = engines.missing_synthesis_assets(paths)

    if importlib.util.find_spec("en_core_web_sm") is None:
        assert any("en_core_web_sm" in item for item in missing)
    # A voz do exemplo existe; a configuracao e os pesos tambem.
    assert not any("af_heart.pt" in item for item in missing)


def test_normal_use_with_missing_assets_never_downloads(tmp_path: Path, monkeypatch):
    registry = EngineRegistry(write_manifest(tmp_path))
    audio = tmp_path / "Gravacoes da pratica" / "normalized.wav"
    audio.parent.mkdir(parents=True)
    audio.write_bytes(b"RIFF")

    with blocked_network(monkeypatch):
        with pytest.raises(WorkerError) as transcription:
            registry.transcribe(
                {
                    "audioPath": str(audio),
                    "modelPath": str(tmp_path / "Modelos de Voz" / "faster-whisper-small"),
                    "device": "cpu",
                    "computeType": "int8",
                    "language": "en",
                },
                lambda stage: None,
            )
        with pytest.raises(WorkerError) as synthesis:
            registry.synthesize(
                {
                    "text": QUESTION,
                    "outputPath": str(tmp_path / "question-cache" / "abc.wav"),
                    "voice": "af_heart",
                    "speed": 1.0,
                },
                lambda stage: None,
            )

    assert transcription.value.code == "MODEL_MISSING"
    assert transcription.value.retryable is False
    assert "prepara" in transcription.value.message.lower()
    assert synthesis.value.code == "MODEL_MISSING"
    assert "af_heart" in synthesis.value.message
    assert not (tmp_path / "question-cache").exists()


def test_synthesis_never_starts_misaki_without_its_english_resource(tmp_path: Path, monkeypatch):
    """O Misaki descarrega ``en_core_web_sm`` sozinho; isso nunca pode acontecer."""
    manifest = write_manifest(tmp_path)
    registry = EngineRegistry(manifest)
    paths = registry.paths()
    create_kokoro_assets(paths)
    problem = "Falta o recurso inglês do spaCy «en_core_web_sm»; execute a preparação offline."
    monkeypatch.setattr(engines, "_missing_spacy_model", lambda name: [problem])

    def refuse_pipeline(*args, **kwargs):
        raise AssertionError("o pipeline do Kokoro não devia ser construído")

    monkeypatch.setattr(engines, "load_kokoro_pipeline", refuse_pipeline)

    with blocked_network(monkeypatch):
        with pytest.raises(WorkerError) as raised:
            registry.synthesize(
                {"text": QUESTION, "outputPath": str(tmp_path / "question-cache" / "a.wav")},
                lambda stage: None,
            )

    assert raised.value.code == "MODEL_MISSING"
    assert "en_core_web_sm" in raised.value.message


def test_transcribe_rejects_unsupported_options(tmp_path: Path):
    registry = EngineRegistry(write_manifest(tmp_path))
    audio = tmp_path / "a.wav"
    audio.write_bytes(b"RIFF")

    with pytest.raises(WorkerError) as device:
        registry.transcribe({"audioPath": str(audio), "device": "cuda"}, lambda stage: None)
    assert device.value.code == "WORKER_PROTOCOL"
    assert device.value.message

    with pytest.raises(WorkerError) as language:
        registry.transcribe({"audioPath": str(audio), "language": "pt"}, lambda stage: None)
    assert language.value.code == "WORKER_PROTOCOL"

    with pytest.raises(WorkerError) as missing_audio:
        registry.transcribe({"audioPath": str(tmp_path / "ausente.wav")}, lambda stage: None)
    assert missing_audio.value.code == "ARTIFACT_MISSING"


def test_transcribe_maps_engine_output_without_rewriting(tmp_path: Path, monkeypatch):
    paths = engines.ModelPaths(
        whisper_dir=tmp_path / "Modelos de Voz" / "faster-whisper-small",
        whisper_model_name="small",
        kokoro_config=tmp_path / "config.json",
        kokoro_weights=tmp_path / "kokoro-v1_0.pth",
        kokoro_voice=tmp_path / "af_heart.pt",
        spacy_model="en_core_web_sm",
    )
    create_whisper_assets(paths)
    audio = tmp_path / "Gravacoes da pratica" / "normalized.wav"
    audio.parent.mkdir(parents=True)
    audio.write_bytes(b"RIFF")
    stages: list[str] = []

    segment = types.SimpleNamespace(start=0.0, end=2.4, text=" I solved it with a checklist,")
    trailing = types.SimpleNamespace(start=2.4, end=5.1, text=" and it worked.")
    calls: dict = {}

    class FakeWhisper:
        def transcribe(self, path, **kwargs):
            calls.update(kwargs)
            calls["path"] = path
            return iter([segment, trailing]), types.SimpleNamespace(language="en")

    def fake_loader(model_path, **kwargs):
        calls["model_path"] = model_path
        calls["loader"] = kwargs
        return FakeWhisper()

    monkeypatch.setattr(engines, "load_whisper_model", fake_loader)
    registry = EngineRegistry(write_manifest(tmp_path))

    result = registry.transcribe(
        {
            "audioPath": str(audio),
            "modelPath": str(paths.whisper_dir),
            "device": "cpu",
            "computeType": "int8",
            "language": "en",
        },
        stages.append,
    )

    assert stages == ["loading", "processing"]
    assert calls["model_path"] == str(paths.whisper_dir)
    assert calls["loader"] == {"device": "cpu", "compute_type": "int8"}
    assert calls["language"] == "en"
    assert calls["task"] == "transcribe"
    assert calls["vad_filter"] is True
    assert calls["path"] == str(audio)
    assert result["status"] == "completed"
    # Texto e segmentos preservados tal como o motor os devolveu.
    assert result["text"] == "I solved it with a checklist, and it worked."
    assert result["segments"] == [
        {"startMs": 0, "endMs": 2400, "text": " I solved it with a checklist,"},
        {"startMs": 2400, "endMs": 5100, "text": " and it worked."},
    ]
    assert result["model"] == "small"
    assert result["language"] == "en"
    assert isinstance(result["loadMs"], int) and result["loadMs"] >= 0
    assert isinstance(result["processingMs"], int) and result["processingMs"] >= 0


def test_transcribe_empty_result_is_no_speech(tmp_path: Path, monkeypatch):
    paths = engines.ModelPaths(
        whisper_dir=tmp_path / "small",
        whisper_model_name="small",
        kokoro_config=tmp_path / "config.json",
        kokoro_weights=tmp_path / "kokoro-v1_0.pth",
        kokoro_voice=tmp_path / "af_heart.pt",
        spacy_model="en_core_web_sm",
    )
    create_whisper_assets(paths)
    audio = tmp_path / "a.wav"
    audio.write_bytes(b"RIFF")

    class SilentWhisper:
        def transcribe(self, path, **kwargs):
            return iter([]), types.SimpleNamespace(language="en")

    monkeypatch.setattr(engines, "load_whisper_model", lambda model_path, **kwargs: SilentWhisper())
    registry = EngineRegistry(write_manifest(tmp_path))

    result = registry.transcribe({"audioPath": str(audio), "modelPath": str(paths.whisper_dir)}, lambda stage: None)

    assert result["status"] == "no_speech"
    assert result["text"] == ""
    assert result["segments"] == []
    assert result["language"] == "en"


def test_synthesize_concatenates_every_segment_at_24k(tmp_path: Path, monkeypatch):
    import numpy
    import soundfile

    manifest = write_manifest(tmp_path)
    registry = EngineRegistry(manifest)
    paths = registry.paths()
    create_kokoro_assets(paths)
    monkeypatch.setattr(engines, "missing_synthesis_assets", lambda paths: [])
    stages: list[str] = []

    chunks = [numpy.full(2400, 0.1, dtype=numpy.float32), numpy.full(4800, -0.1, dtype=numpy.float32)]
    calls: dict = {}

    class FakePipeline:
        def __call__(self, text, voice=None, speed=None):
            calls["text"] = text
            calls["voice"] = voice
            calls["speed"] = speed
            for chunk in chunks:
                yield types.SimpleNamespace(audio=chunk)

    def fake_loader(paths, **kwargs):
        return FakePipeline()

    monkeypatch.setattr(engines, "load_kokoro_pipeline", fake_loader)
    output = tmp_path / "question-cache" / "3c1f.wav"

    result = registry.synthesize(
        {"text": QUESTION, "outputPath": str(output), "voice": "af_heart", "speed": 1.0},
        stages.append,
    )

    assert stages == ["loading", "processing"]
    assert calls["text"] == QUESTION
    assert calls["voice"] == str(paths.kokoro_voice)
    assert calls["speed"] == 1.0
    assert output.exists()
    data, sample_rate = soundfile.read(str(output), dtype="int16")
    assert sample_rate == SAMPLE_RATE == 24000
    assert data.ndim == 1  # mono
    assert len(data) == sum(len(chunk) for chunk in chunks)  # nada truncado
    assert soundfile.info(str(output)).subtype == "PCM_16"
    assert result["audioPath"] == str(output)
    assert result["sampleRate"] == 24000
    assert result["durationMs"] == round(7200 / 24000 * 1000)
    assert result["loadMs"] >= 0 and result["processingMs"] >= 0


def test_synthesize_rejects_bad_requests(tmp_path: Path):
    registry = EngineRegistry(write_manifest(tmp_path))

    with pytest.raises(WorkerError) as empty:
        registry.synthesize({"text": "   ", "outputPath": str(tmp_path / "a.wav")}, lambda stage: None)
    assert empty.value.code == "WORKER_PROTOCOL"

    with pytest.raises(WorkerError) as voice:
        registry.synthesize(
            {"text": QUESTION, "outputPath": str(tmp_path / "a.wav"), "voice": "am_michael"},
            lambda stage: None,
        )
    assert voice.value.code == "WORKER_PROTOCOL"
    assert "af_heart" in voice.value.message

    with pytest.raises(WorkerError) as speed:
        registry.synthesize(
            {"text": QUESTION, "outputPath": str(tmp_path / "a.wav"), "speed": 0},
            lambda stage: None,
        )
    assert speed.value.code == "WORKER_PROTOCOL"


def test_health_does_not_import_heavy_engines(tmp_path: Path):
    environment = dict(os.environ)
    environment["PYTHONPATH"] = str(PACKAGE_ROOT / "src")
    script = (
        "import json, sys\n"
        "from audio_worker.engines import EngineRegistry\n"
        "health = EngineRegistry(None).health()\n"
        "heavy = sorted(\n"
        "    name for name in ('torch', 'numpy', 'faster_whisper', 'kokoro', 'soundfile', 'spacy')\n"
        "    if name in sys.modules\n"
        ")\n"
        "sys.stderr.write(json.dumps({'heavy': heavy}) + '\\n')\n"
        "print(json.dumps(health))\n"
    )

    completed = subprocess.run(
        [sys.executable, "-c", script],
        capture_output=True,
        text=True,
        encoding="utf-8",
        cwd=str(tmp_path),
        env=environment,
        timeout=180,
    )

    assert completed.returncode == 0, completed.stderr
    assert json.loads(completed.stderr.strip().splitlines()[-1])["heavy"] == []
    health = json.loads(completed.stdout.strip().splitlines()[-1])
    assert health["transcription"]["status"] == "missing"


def test_offline_environment_is_applied():
    previous = {name: os.environ.get(name) for name in ("HF_HUB_OFFLINE", "TRANSFORMERS_OFFLINE")}
    try:
        engines.configure_offline_environment()
        assert os.environ["HF_HUB_OFFLINE"] == "1"
        assert os.environ["TRANSFORMERS_OFFLINE"] == "1"
    finally:
        for name, value in previous.items():
            if value is None:
                os.environ.pop(name, None)
            else:
                os.environ[name] = value

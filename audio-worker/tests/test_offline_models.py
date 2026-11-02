"""Validacao real com pesos preparados (marcada com o marcador ``models``).

Estes testes so correm quando os recursos foram preparados explicitamente com
``audio-worker-prepare``. A falta de pesos produz um skip explicito: nunca um
sucesso falso da validacao real. Executar com:

    .\\.venv\\Scripts\\python.exe -m pytest audio-worker/tests -q -m models
"""

from __future__ import annotations

import json
import os
from pathlib import Path

import pytest

pytestmark = pytest.mark.models

REPO_ROOT = Path(__file__).resolve().parents[2]
QUESTION = "Tell me about a time you solved a difficult problem at work."
LOOPBACK = "This is a check of the local speech engine prepared for phase one."


def manifest_path() -> Path:
    from_environment = os.environ.get("LANGUAGE_TUTOR_MODELS_MANIFEST")
    if from_environment:
        return Path(from_environment)
    return REPO_ROOT / "models" / "models.json"


def models_ready() -> bool:
    path = manifest_path()
    if not path.is_file():
        return False
    try:
        manifest = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return False
    required = ("whisperDir", "kokoroConfig", "kokoroWeights", "kokoroVoice")
    return all(Path(manifest.get(key, "")).exists() for key in required)


def require_models() -> Path:
    """Marcador de skip explicito: sem pesos preparados nao ha validacao real."""
    path = manifest_path()
    if not models_ready():
        pytest.skip(f"Recursos locais ausentes ({path}); executar 'audio-worker-prepare'.")
    return path


def test_synthesize_question_then_transcribe_it(tmp_path: Path):
    manifest = require_models()
    import soundfile

    from audio_worker.engines import EngineRegistry, synthesis_cache_key

    registry = EngineRegistry(manifest)
    output = tmp_path / "question-cache" / f"{synthesis_cache_key(QUESTION)}.wav"

    stages: list[str] = []
    synthesized = registry.synthesize(
        {"text": QUESTION, "outputPath": str(output), "voice": "af_heart", "speed": 1.0},
        stages.append,
    )

    assert stages == ["loading", "processing"]
    assert synthesized["audioPath"] == str(output)
    assert synthesized["sampleRate"] == 24000
    assert synthesized["durationMs"] > 1000
    assert output.is_file()

    info = soundfile.info(str(output))
    assert info.samplerate == 24000
    assert info.channels == 1
    assert info.subtype == "PCM_16"

    transcribed = registry.transcribe(
        {
            "audioPath": str(output),
            "device": "cpu",
            "computeType": "int8",
            "language": "en",
        },
        lambda stage: None,
    )

    assert transcribed["status"] == "completed"
    assert transcribed["language"] == "en"
    assert transcribed["model"] == "small"
    assert transcribed["segments"]
    assert "difficult" in transcribed["text"].lower()


def test_transcribe_returns_no_speech_for_silence(tmp_path: Path):
    manifest = require_models()
    import numpy
    import soundfile

    from audio_worker.engines import EngineRegistry

    silence = tmp_path / "silencio.wav"
    soundfile.write(str(silence), numpy.zeros(24000, dtype=numpy.float32), 16000, subtype="PCM_16")

    result = EngineRegistry(manifest).transcribe(
        {"audioPath": str(silence), "device": "cpu", "computeType": "int8", "language": "en"},
        lambda stage: None,
    )

    assert result["status"] == "no_speech"
    assert result["text"] == ""

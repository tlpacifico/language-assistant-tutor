"""Medição da transcrição e da voz: estatística honesta, só a pasta indicada e
sem conteúdo nos resultados."""

from __future__ import annotations

import json
import wave
from pathlib import Path

import pytest

from audio_worker import benchmark
from audio_worker import engines
from audio_worker.engines import EngineRegistry
from audio_worker.protocol import WorkerError
from test_engines import create_kokoro_assets, create_whisper_assets, write_manifest


def _wav(path: Path, seconds: float = 2.0) -> None:
    with wave.open(str(path), "wb") as handle:
        handle.setnchannels(1)
        handle.setsampwidth(2)
        handle.setframerate(16000)
        handle.writeframes(b"\x00\x00" * int(16000 * seconds))


def test_summary_lists_each_value_and_never_claims_a_percentile():
    summary = benchmark.summarize([410, 380, 395, 900, 400])
    assert summary["values"] == [410, 380, 395, 900, 400]
    assert summary["median"] == 400
    assert summary["worst"] == 900
    assert summary["n"] == 5
    # Cinco amostras não sustentam um p95.
    assert not any("p9" in key for key in summary)
    assert benchmark.summarize([]) == {"values": [], "median": None, "worst": None, "n": 0}


class _FakeRegistry:
    """Motor controlado: 1.ª chamada «fria» (carrega), as seguintes «quentes»."""

    instances = 0

    def __init__(self, manifest=None):
        type(self).instances += 1
        self.calls: list[dict] = []
        self.warm = False

    def transcribe(self, params, progress):
        self.calls.append(dict(params))
        load = 0 if self.warm else 1200
        self.warm = True
        beam = params.get("beamSize", 5)
        return {
            "status": "completed",
            "text": "PRIVATE TRANSCRIPT",
            "segments": [],
            "model": "small",
            "language": "en",
            "loadMs": load,
            "processingMs": 500 if beam == 5 else 300,
        }

    def synthesize_stream(self, params, progress, emit, should_cancel=lambda: False):
        emit(0, str(Path(params["outputDir"]) / "chunk-0000.wav"), 24000, 900)
        emit(1, str(Path(params["outputDir"]) / "chunk-0001.wav"), 24000, 900)
        return {"chunks": 2, "sampleRate": 24000, "cancelled": False}


def test_runs_one_cold_and_five_loaded_repetitions_per_configuration(tmp_path: Path):
    _FakeRegistry.instances = 0
    recordings = tmp_path / "rec"
    recordings.mkdir()
    _wav(recordings / "a.wav", 2.0)
    _wav(recordings / "b.wav", 4.0)
    (recordings / "notes.txt").write_text("ignorado", encoding="utf-8")
    outside = tmp_path / "other"
    outside.mkdir()
    _wav(outside / "secret.wav")

    result = benchmark.run_benchmark(
        recordings,
        tmp_path / "out",
        manifest=None,
        registry_factory=_FakeRegistry,
    )

    files = result["recordings"]
    assert sorted(files) == ["a.wav", "b.wav"], "só os WAV da pasta indicada"
    for name, expected_seconds in (("a.wav", 2.0), ("b.wav", 4.0)):
        entry = files[name]
        assert entry["durationMs"] == int(expected_seconds * 1000)
        baseline = entry["configurations"]["baseline"]
        candidate = entry["configurations"]["candidate"]
        # Uma passagem fria (carrega o modelo) e cinco carregadas.
        assert baseline["cold"]["loadMs"] == 1200
        assert baseline["loaded"]["n"] == 5
        assert baseline["loaded"]["median"] == 500
        assert candidate["loaded"]["median"] == 300
        assert baseline["loaded"]["worst"] == 500
    # Cada configuração por gravação usa um motor novo para a passagem fria.
    assert _FakeRegistry.instances == 2 * 2


def test_results_are_written_only_to_the_output_folder_without_any_transcript(tmp_path: Path):
    recordings = tmp_path / "rec"
    recordings.mkdir()
    _wav(recordings / "a.wav")
    out = tmp_path / "out"
    benchmark.run_benchmark(recordings, out, manifest=None, registry_factory=_FakeRegistry)
    written = list(out.iterdir())
    assert len(written) == 1 and written[0].suffix == ".json"
    text = written[0].read_text(encoding="utf-8")
    assert "PRIVATE TRANSCRIPT" not in text
    assert json.loads(text)["recordings"]["a.wav"]
    # A pasta das gravações não é alterada.
    assert sorted(p.name for p in recordings.iterdir()) == ["a.wav"]


def test_voice_timing_reports_first_chunk_and_full_synthesis(tmp_path: Path):
    ticks = iter(range(0, 10_000, 100))
    recordings = tmp_path / "rec"
    recordings.mkdir()
    _wav(recordings / "a.wav")
    result = benchmark.run_benchmark(
        recordings,
        tmp_path / "out",
        manifest=None,
        registry_factory=_FakeRegistry,
        voice_text="Why does that matter?",
        clock=lambda: next(ticks) / 1000,
    )
    voice = result["voice"]
    assert voice["chunks"] == 2
    assert voice["firstChunkMs"] is not None and voice["fullMs"] >= voice["firstChunkMs"]


def test_transcribe_beam_size_is_optional_and_validated(tmp_path: Path, monkeypatch):
    registry = EngineRegistry(write_manifest(tmp_path))
    paths = registry.paths()
    create_whisper_assets(paths)
    create_kokoro_assets(paths)
    audio = tmp_path / "a.wav"
    _wav(audio)
    monkeypatch.setattr(engines, "missing_transcription_assets", lambda paths: [])
    seen: list[dict] = []

    class Model:
        def transcribe(self, path, **kwargs):
            seen.append(kwargs)
            return iter(()), None

    monkeypatch.setattr(engines, "load_whisper_model", lambda path, **kwargs: Model())
    base = {"audioPath": str(audio), "modelPath": str(paths.whisper_dir)}

    registry.transcribe(base, lambda stage: None)
    assert "beam_size" not in seen[-1], "o default não muda"

    registry.transcribe({**base, "beamSize": 1}, lambda stage: None)
    assert seen[-1]["beam_size"] == 1
    registry.transcribe({**base, "beamSize": 5}, lambda stage: None)
    assert seen[-1]["beam_size"] == 5
    for invalid in (0, 3, 2.5, "1", True):
        with pytest.raises(WorkerError) as error:
            registry.transcribe({**base, "beamSize": invalid}, lambda stage: None)
        assert error.value.code == "WORKER_PROTOCOL"

"""Medição local do tempo de transcrição e de voz.

Recebe uma pasta de gravações indicada pelo utilizador (só os ``*.wav`` do
nível superior; nunca varre outras pastas) e uma pasta de resultados local.
Compara a configuração atual (``beam_size=5``) com o candidato (``beam_size=1``)
mantendo inglês, VAD, modelo ``small`` e CPU/int8. Por configuração e gravação
faz uma passagem fria (modelo novo) e ``REPETITIONS`` carregadas, e apresenta
cada valor, a mediana e o pior tempo. Com cinco amostras não se calcula p95.

Os resultados só contêm durações: nunca o texto transcrito, nem caminhos de
áudio. Não são enviados nem versionados. A fidelidade do texto compara-se à
mão, com referência humana, fora deste ficheiro.
"""

from __future__ import annotations

import argparse
import datetime
import json
import shutil
import statistics
import time
import wave
from pathlib import Path
from typing import Any, Callable

REPETITIONS = 5

CONFIGURATIONS = (
    {"name": "baseline", "beamSize": 5},
    {"name": "candidate", "beamSize": 1},
)


def summarize(values: list[int | float]) -> dict:
    """Cada valor, mediana e pior tempo; sem percentis que a amostra não sustenta."""
    if not values:
        return {"values": [], "median": None, "worst": None, "n": 0}
    return {
        "values": list(values),
        "median": statistics.median(values),
        "worst": max(values),
        "n": len(values),
    }


def _duration_ms(path: Path) -> int:
    with wave.open(str(path), "rb") as handle:
        return int(round(handle.getnframes() / handle.getframerate() * 1000))


def _noop(stage: str) -> None:
    return None


def run_benchmark(
    recordings: str | Path,
    out: str | Path,
    *,
    manifest: str | Path | None,
    registry_factory: Callable[[Any], Any] | None = None,
    repetitions: int = REPETITIONS,
    configurations: tuple[dict, ...] = CONFIGURATIONS,
    voice_text: str | None = None,
    clock: Callable[[], float] = time.perf_counter,
) -> dict:
    """Corre a medição e escreve um JSON (só durações) na pasta de resultados."""
    if registry_factory is None:
        from .engines import EngineRegistry

        registry_factory = EngineRegistry
    source = Path(recordings)
    destination = Path(out)
    files = sorted(p for p in source.iterdir() if p.is_file() and p.suffix.lower() == ".wav")

    result: dict[str, Any] = {
        "schemaVersion": 1,
        "repetitions": repetitions,
        "recordings": {},
    }
    for file in files:
        entry: dict[str, Any] = {"durationMs": _duration_ms(file), "configurations": {}}
        for configuration in configurations:
            params = {"audioPath": str(file), "beamSize": configuration["beamSize"]}
            registry = registry_factory(manifest)  # motor novo: passagem fria
            cold = registry.transcribe(dict(params), _noop)
            loaded = [
                registry.transcribe(dict(params), _noop)["processingMs"]
                for _ in range(repetitions)
            ]
            entry["configurations"][configuration["name"]] = {
                "beamSize": configuration["beamSize"],
                "cold": {"loadMs": cold["loadMs"], "processingMs": cold["processingMs"]},
                "loaded": summarize(loaded),
            }
        result["recordings"][file.name] = entry

    if voice_text:
        registry = registry_factory(manifest)
        directory = destination / "voice-tmp"
        started = clock()
        first: list[float] = []

        def emit(index: int, path: str, sample_rate: int, duration_ms: int) -> None:
            if not first:
                first.append(clock() - started)

        stream = registry.synthesize_stream(
            {
                "text": voice_text,
                "outputDir": str(directory.resolve()),
                "voice": "af_heart",
                "speed": 1.0,
            },
            _noop,
            emit,
        )
        full = clock() - started
        shutil.rmtree(directory, ignore_errors=True)
        result["voice"] = {
            "chunks": stream["chunks"],
            "firstChunkMs": int(round(first[0] * 1000)) if first else None,
            "fullMs": int(round(full * 1000)),
        }

    destination.mkdir(parents=True, exist_ok=True)
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    (destination / f"benchmark-{stamp}.json").write_text(
        json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    return result


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="audio_worker.benchmark", description=__doc__)
    parser.add_argument("--recordings", required=True, help="Pasta com os WAV a medir.")
    parser.add_argument("--out", required=True, help="Pasta local de resultados.")
    parser.add_argument("--manifest", default=None, help="Manifesto de modelos preparado.")
    parser.add_argument("--voice-text", default=None, help="Texto para medir a voz (opcional).")
    parser.add_argument("--repetitions", type=int, default=REPETITIONS)
    args = parser.parse_args(argv)
    result = run_benchmark(
        args.recordings,
        args.out,
        manifest=args.manifest,
        repetitions=args.repetitions,
        voice_text=args.voice_text,
    )
    print(json.dumps(result, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

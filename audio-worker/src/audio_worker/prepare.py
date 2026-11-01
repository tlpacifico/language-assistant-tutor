"""Preparacao offline explicita dos recursos do auxiliar.

Este comando e o unico ponto autorizado a aceder a rede. Descarrega o modelo
Whisper ``small`` (faster-whisper, CTranslate2), a configuracao/pesos/voz do
Kokoro-82M e garante o recurso ingles do spaCy exigido pelo Misaki, alem de
materializar o fallback linguistico do eSpeak-NG incluido no pacote
``espeakng-loader``. No fim escreve ``models.json`` e faz uma sintese fixa para
revelar dependencias tardias.

Nunca usa gravacoes pessoais e nunca e chamado durante a pratica.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

WHISPER_REPO = "Systran/faster-whisper-small"
WHISPER_MODEL_NAME = "small"
KOKORO_REPO = "hexgrad/Kokoro-82M"
KOKORO_WEIGHTS_FILE = "kokoro-v1_0.pth"
KOKORO_VOICE = "af_heart"
SPACY_MODEL = "en_core_web_sm"

#: Frase fixa da preparacao; nao usa gravacoes pessoais.
SMOKE_TEXT = "The local speech engine is ready for the interview practice."

DEFAULT_REPO_ROOT = Path(__file__).resolve().parents[3]


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="audio-worker-prepare",
        description=(
            "Prepara offline os modelos e recursos locais do auxiliar. "
            "Este é o único comando que acede à rede."
        ),
    )
    parser.add_argument(
        "--models-dir",
        default=None,
        metavar="PASTA",
        help="Pasta de destino dos modelos (por omissão <repositório>/models).",
    )
    parser.add_argument(
        "--manifest",
        default=None,
        metavar="FICHEIRO",
        help="Manifesto a escrever (por omissão <models-dir>/models.json).",
    )
    parser.add_argument(
        "--skip-smoke-test",
        action="store_true",
        help="Não executar a síntese de verificação no fim (não recomendado).",
    )
    return parser


def _log(message: str) -> None:
    print(f"[audio-worker-prepare] {message}", file=sys.stderr, flush=True)


def _resolve_paths(args: argparse.Namespace) -> tuple[Path, Path]:
    models_dir = Path(args.models_dir) if args.models_dir else DEFAULT_REPO_ROOT / "models"
    manifest = Path(args.manifest) if args.manifest else models_dir / "models.json"
    return models_dir.resolve(), manifest.resolve()


def _model_revision(repo_id: str) -> str | None:
    """Revisao (commit) do modelo, registada no manifesto."""
    try:
        from huggingface_hub import HfApi

        return HfApi().model_info(repo_id).sha
    except Exception as error:  # rede ou API indisponivel
        _log(f"aviso: não foi possível obter a revisão de {repo_id}: {error}")
        return None


def prepare_whisper(models_dir: Path) -> tuple[Path, str | None]:
    from huggingface_hub import snapshot_download

    destination = models_dir / "faster-whisper-small"
    _log(f"a descarregar {WHISPER_REPO} para {destination}")
    snapshot_download(repo_id=WHISPER_REPO, local_dir=str(destination))
    revision = _model_revision(WHISPER_REPO)
    return destination, revision


def prepare_kokoro(models_dir: Path) -> tuple[Path, Path, Path, str | None]:
    from huggingface_hub import hf_hub_download

    destination = models_dir / "kokoro"
    destination.mkdir(parents=True, exist_ok=True)
    _log(f"a descarregar {KOKORO_REPO} para {destination}")
    config = Path(hf_hub_download(repo_id=KOKORO_REPO, filename="config.json", local_dir=str(destination)))
    weights = Path(
        hf_hub_download(repo_id=KOKORO_REPO, filename=KOKORO_WEIGHTS_FILE, local_dir=str(destination))
    )
    voice = Path(
        hf_hub_download(
            repo_id=KOKORO_REPO,
            filename=f"voices/{KOKORO_VOICE}.pt",
            local_dir=str(destination),
        )
    )
    revision = _model_revision(KOKORO_REPO)
    return config, weights, voice, revision


def prepare_spacy_model() -> tuple[str, str | None]:
    """Garante o recurso ingles do spaCy que o Misaki tenta obter sozinho."""
    try:
        import spacy.util
    except ImportError as error:
        raise SystemExit(f"Falta o spaCy no ambiente: {error}")

    if not spacy.util.is_package(SPACY_MODEL):
        _log(f"a instalar o recurso inglês do spaCy ({SPACY_MODEL})")
        completed = subprocess.run(
            [sys.executable, "-m", "spacy", "download", SPACY_MODEL],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
        )
        if completed.returncode != 0:
            raise SystemExit(
                f"Não foi possível instalar {SPACY_MODEL}: {completed.stderr.strip()}"
            )
    import spacy

    version = spacy.util.get_package_version(SPACY_MODEL)
    return SPACY_MODEL, version


def prepare_espeak_fallback() -> dict:
    """Materializa/valida o fallback linguistico (eSpeak-NG incluido no pacote)."""
    import espeakng_loader

    library = Path(espeakng_loader.get_library_path())
    data = Path(espeakng_loader.get_data_path())
    if not library.is_file():
        raise SystemExit(f"Falta a biblioteca eSpeak-NG em {library}")
    _log(f"eSpeak-NG: {library}")
    return {
        "espeakLibrary": str(library),
        "espeakData": str(data),
        "espeakFallback": "misaki.espeak.EspeakFallback(en-us)",
    }


def write_manifest(
    manifest_path: Path,
    *,
    whisper_dir: Path,
    whisper_revision: str | None,
    kokoro_config: Path,
    kokoro_weights: Path,
    kokoro_voice: Path,
    kokoro_revision: str | None,
    spacy_version: str | None,
    resources: dict,
) -> dict:
    manifest: dict[str, Any] = {
        "schemaVersion": 1,
        "preparedAt": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "whisperDir": str(whisper_dir),
        "whisperModel": WHISPER_MODEL_NAME,
        "kokoroConfig": str(kokoro_config),
        "kokoroWeights": str(kokoro_weights),
        "kokoroVoice": str(kokoro_voice),
        "revisions": {
            "fasterWhisperSmall": whisper_revision,
            "kokoro82m": kokoro_revision,
        },
        "resources": {
            "spacyModel": SPACY_MODEL,
            "spacyModelVersion": spacy_version,
            **resources,
        },
    }
    manifest_path.parent.mkdir(parents=True, exist_ok=True)
    temporary = manifest_path.with_name(manifest_path.name + ".part")
    temporary.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    os.replace(temporary, manifest_path)
    return manifest


def run_smoke_test(manifest_path: Path, smoke_output: Path) -> None:
    """Sintese fixa que revela dependencias tardias antes de usar a app."""
    from .engines import EngineRegistry, configure_offline_environment

    # Tudo ja esta descarregado: a sintese corre em modo offline, exatamente como
    # durante a pratica. Se faltar um recurso, falha aqui e nao na entrevista.
    configure_offline_environment()
    registry = EngineRegistry(manifest_path)
    _log("a executar a síntese de verificação")
    result = registry.synthesize(
        {
            "text": SMOKE_TEXT,
            "outputPath": str(smoke_output),
            "voice": KOKORO_VOICE,
            "speed": 1.0,
        },
        lambda stage: _log(f"síntese: {stage}"),
    )
    if result["durationMs"] <= 0:
        raise SystemExit("A síntese de verificação não produziu áudio.")
    _log(f"áudio de verificação: {smoke_output} ({result['durationMs']} ms)")


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    models_dir, manifest_path = _resolve_paths(args)
    models_dir.mkdir(parents=True, exist_ok=True)

    from .engines import enable_online_environment

    enable_online_environment()

    started = time.perf_counter()
    whisper_dir, whisper_revision = prepare_whisper(models_dir)
    kokoro_config, kokoro_weights, kokoro_voice, kokoro_revision = prepare_kokoro(models_dir)
    spacy_name, spacy_version = prepare_spacy_model()
    resources = prepare_espeak_fallback()

    write_manifest(
        manifest_path,
        whisper_dir=whisper_dir,
        whisper_revision=whisper_revision,
        kokoro_config=kokoro_config,
        kokoro_weights=kokoro_weights,
        kokoro_voice=kokoro_voice,
        kokoro_revision=kokoro_revision,
        spacy_version=spacy_version,
        resources=resources,
    )
    _log(f"manifesto escrito em {manifest_path} (spaCy: {spacy_name} {spacy_version})")

    if not args.skip_smoke_test:
        smoke_output = models_dir / "smoke-test" / f"{KOKORO_VOICE}-smoke.wav"
        smoke_output.parent.mkdir(parents=True, exist_ok=True)
        run_smoke_test(manifest_path, smoke_output)

    elapsed = time.perf_counter() - started
    print(f"Preparação concluída em {elapsed:.1f} s. Manifesto: {manifest_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

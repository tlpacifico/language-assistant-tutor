"""Motores locais: faster-whisper (transcricao) e Kokoro (sintese).

Regras desta camada:

* tudo corre em CPU, com o modelo ``small`` em int8 e a voz ``af_heart`` a
  24 kHz;
* os pesos sao lidos de caminhos locais absolutos indicados no manifesto; nada
  e descarregado durante a pratica (``HF_HUB_OFFLINE=1``);
* ``health`` responde apenas com verificacoes de ficheiros e de dependencias,
  sem carregar modelos;
* os imports dos motores sao tardios, dentro de ``load_*``, para que o health e
  os testes de protocolo nao importem torch;
* o texto e os segmentos devolvidos pelo Whisper sao preservados tal como o
  motor os produziu.
"""

from __future__ import annotations

import hashlib
import importlib.util
import json
import os
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Mapping

from .protocol import (
    ARTIFACT_MISSING,
    INVALID_AUDIO,
    MODEL_MISSING,
    STORAGE_WRITE_FAILED,
    WORKER_PROTOCOL,
    WORKER_UNAVAILABLE,
    ManifestError,
    WorkerError,
    component_state,
)

#: Taxa de amostragem do Kokoro; a saida e sempre WAV PCM16 mono a 24 kHz.
SAMPLE_RATE = 24000
DEFAULT_VOICE = "af_heart"
DEFAULT_SPEED = 1.0
MAX_SPEED = 3.0

WHISPER_DEVICE = "cpu"
WHISPER_COMPUTE_TYPE = "int8"
WHISPER_LANGUAGE = "en"
WHISPER_MODEL_NAME = "small"
WHISPER_REQUIRED_FILES = ("model.bin", "config.json", "tokenizer.json", "vocabulary.txt")

KOKORO_LANG_CODE = "a"  # ingles americano
KOKORO_REPO_ID = "hexgrad/Kokoro-82M"

SPACY_MODEL = "en_core_web_sm"

TRANSCRIPTION_DEPENDENCIES = ("faster_whisper", "ctranslate2")
SYNTHESIS_DEPENDENCIES = ("kokoro", "misaki", "torch", "soundfile", "spacy")

ProgressCallback = Callable[[str], None]

#: Nomes apresentados nas mensagens quando falta uma dependencia pip.
DEPENDENCY_LABELS = {
    "faster_whisper": "faster-whisper",
    "ctranslate2": "ctranslate2",
    "kokoro": "kokoro",
    "misaki": "misaki",
    "torch": "torch",
    "soundfile": "soundfile",
    "spacy": "spacy",
}

CACHE_KEY_PREFIX = "kokoro-v1"
MAX_REPORTED_PROBLEMS = 3


def configure_offline_environment() -> None:
    """Bloqueia downloads e telemetria do Hugging Face neste processo.

    Tem de ser chamado antes de importar ``faster_whisper``/``kokoro``/
    ``transformers``. A preparacao offline (``prepare.py``) e o unico ponto que
    reverte estas variaveis, e falso antes de importar os motores.
    """
    os.environ["HF_HUB_OFFLINE"] = "1"
    os.environ["TRANSFORMERS_OFFLINE"] = "1"
    os.environ["HF_HUB_DISABLE_TELEMETRY"] = "1"


def enable_online_environment() -> None:
    """Permite downloads explicitos; so a preparacao offline deve usar isto."""
    os.environ["HF_HUB_OFFLINE"] = "0"
    os.environ["TRANSFORMERS_OFFLINE"] = "0"
    os.environ.pop("HF_HUB_DISABLE_TELEMETRY", None)


def synthesis_cache_key(text: str, voice: str = DEFAULT_VOICE, speed: float = DEFAULT_SPEED) -> str:
    """Chave estavel (sha256) da cache ``question-cache/<sha256>.wav``.

    A chave inclui texto normalizado nas pontas, voz e velocidade arredondada a
    tres casas: mudar a voz ou a velocidade produz um ficheiro diferente, para
    nunca servir audio de outra voz. O texto e usado tal como esta (apenas sem
    espacos nas pontas) para nao fundir pedidos diferentes.
    """
    payload = "|".join((CACHE_KEY_PREFIX, voice, f"{float(speed):.3f}", text.strip()))
    return hashlib.sha256(payload.encode("utf-8")).hexdigest()


@dataclass(frozen=True)
class ModelPaths:
    """Caminhos locais dos recursos preparados offline."""

    whisper_dir: Path
    whisper_model_name: str = WHISPER_MODEL_NAME
    kokoro_config: Path | None = None
    kokoro_weights: Path | None = None
    kokoro_voice: Path | None = None
    spacy_model: str = SPACY_MODEL

    @classmethod
    def from_manifest(cls, manifest: Mapping[str, Any]) -> "ModelPaths":
        if not isinstance(manifest, Mapping):
            raise ManifestError("O manifesto de modelos não é um objeto JSON.", status="error")
        version = manifest.get("schemaVersion")
        if version is not None and version != 1:
            raise ManifestError(
                f"Versão de manifesto de modelos não suportada: {version!r}.", status="error"
            )

        whisper_dir = manifest.get("whisperDir")
        if not isinstance(whisper_dir, str) or not whisper_dir.strip():
            raise ManifestError("O manifesto de modelos não indica «whisperDir».", status="error")

        for key in ("whisperDir", "kokoroConfig", "kokoroWeights", "kokoroVoice"):
            value = manifest.get(key)
            if isinstance(value, str) and value.strip() and not Path(value).is_absolute():
                raise ManifestError(
                    f"O caminho «{key}» do manifesto tem de ser absoluto: «{value}».",
                    status="error",
                )

        resources = manifest.get("resources") or {}
        if not isinstance(resources, Mapping):
            raise ManifestError("O campo «resources» do manifesto tem de ser um objeto.", status="error")
        spacy_model = resources.get("spacyModel") or SPACY_MODEL
        if not isinstance(spacy_model, str):
            raise ManifestError("O campo «resources.spacyModel» tem de ser texto.", status="error")

        model_name = manifest.get("whisperModel") or WHISPER_MODEL_NAME
        if not isinstance(model_name, str):
            model_name = WHISPER_MODEL_NAME

        return cls(
            whisper_dir=Path(whisper_dir),
            whisper_model_name=model_name,
            kokoro_config=_optional_path(manifest.get("kokoroConfig")),
            kokoro_weights=_optional_path(manifest.get("kokoroWeights")),
            kokoro_voice=_optional_path(manifest.get("kokoroVoice")),
            spacy_model=spacy_model,
        )


def _optional_path(value: Any) -> Path | None:
    if isinstance(value, str) and value.strip():
        return Path(value)
    return None


def load_manifest(manifest_path: str | Path | None) -> dict:
    """Le o manifesto de modelos preparado; mensagens em PT-PT."""
    if manifest_path is None or str(manifest_path).strip() == "":
        raise ManifestError(
            "Não existe um manifesto de modelos configurado. Execute a preparação offline dos modelos."
        )
    path = Path(manifest_path)
    if not path.is_file():
        raise ManifestError(
            f"O manifesto de modelos não existe em «{path}». Execute a preparação offline dos modelos."
        )
    try:
        manifest = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        raise ManifestError(f"O manifesto de modelos em «{path}» não é JSON válido.", status="error") from None
    if not isinstance(manifest, dict):
        raise ManifestError(f"O manifesto de modelos em «{path}» não é um objeto JSON.", status="error")
    return manifest


def _module_available(name: str) -> bool:
    try:
        return importlib.util.find_spec(name) is not None
    except (ImportError, ValueError):
        return False


def _package_directory(name: str) -> Path | None:
    try:
        spec = importlib.util.find_spec(name)
    except (ImportError, ValueError):
        return None
    if spec is None or not spec.submodule_search_locations:
        return None
    return Path(next(iter(spec.submodule_search_locations)))


def _missing_modules(names: tuple[str, ...]) -> list[str]:
    problems = []
    for name in names:
        if _module_available(name):
            continue
        label = DEPENDENCY_LABELS.get(name, name)
        problems.append(f"Falta a dependência Python «{label}».")
    return problems


def _missing_spacy_model(name: str) -> list[str]:
    if _module_available(name):
        return []
    return [f"Falta o recurso inglês do spaCy «{name}»; execute a preparação offline."]


def _missing_espeak() -> list[str]:
    directory = _package_directory("espeakng_loader")
    if directory is None:
        return ["Falta a biblioteca eSpeak-NG de reserva (pacote «espeakng_loader»)."]
    # ``os.name`` em vez de ``platform.system()``: no Windows o segundo corre o
    # comando externo ``ver``, e o health tem de continuar sem subprocessos.
    if os.name == "nt":
        library = directory / "espeak-ng.dll"
    elif sys.platform == "darwin":
        library = directory / "libespeak-ng.dylib"
    else:
        library = directory / "libespeak-ng.so"
    missing: list[str] = []
    if not library.is_file():
        missing.append(f"Falta a biblioteca eSpeak-NG em «{library}».")
    if not (directory / "espeak-ng-data").is_dir():
        missing.append(f"Faltam os dados do eSpeak-NG em «{directory / 'espeak-ng-data'}».")
    return missing


def missing_transcription_assets(paths: ModelPaths) -> list[str]:
    """Problemas que impedem a transcricao, sem carregar nada."""
    problems: list[str] = []
    directory = Path(paths.whisper_dir)
    if not directory.is_dir():
        problems.append(
            f"Falta a pasta do modelo de transcrição em «{directory}». Execute a preparação offline."
        )
    else:
        absent = [name for name in WHISPER_REQUIRED_FILES if not (directory / name).is_file()]
        if absent:
            problems.append(
                f"Faltam ficheiros do modelo de transcrição em «{directory}»: {', '.join(absent)}."
            )
    problems.extend(_missing_modules(TRANSCRIPTION_DEPENDENCIES))
    return problems


def kokoro_file_problems(paths: ModelPaths) -> list[str]:
    """Ficheiros de configuracao, pesos e voz do Kokoro."""
    problems: list[str] = []
    templates = (
        (
            "Falta a configuração do Kokoro (config.json)",
            "O manifesto de modelos não indica a configuração do Kokoro (config.json).",
            paths.kokoro_config,
        ),
        (
            "Faltam os pesos do Kokoro (kokoro-v1_0.pth)",
            "O manifesto de modelos não indica os pesos do Kokoro (kokoro-v1_0.pth).",
            paths.kokoro_weights,
        ),
        (
            "Falta a voz af_heart",
            "O manifesto de modelos não indica a voz af_heart.",
            paths.kokoro_voice,
        ),
    )
    for label, absent, value in templates:
        if value is None:
            problems.append(absent)
        elif not Path(value).is_file():
            problems.append(f"{label} em «{value}». Execute a preparação offline.")
    return problems


def misaki_resource_problems(paths: ModelPaths) -> list[str]:
    """Recursos linguisticos que o Misaki tenta obter sozinho se faltarem."""
    return _missing_spacy_model(paths.spacy_model) + _missing_espeak()


def missing_synthesis_assets(paths: ModelPaths) -> list[str]:
    """Problemas que impedem a sintese, sem carregar nada nem descarregar nada.

    Inclui os recursos que o Misaki tenta obter sozinho (modelo ingles do spaCy
    e eSpeak-NG), verificados aqui para impedir downloads implicitos.
    """
    return (
        kokoro_file_problems(paths)
        + _missing_modules(SYNTHESIS_DEPENDENCIES)
        + misaki_resource_problems(paths)
    )


def _describe(problems: list[str]) -> str:
    if len(problems) <= MAX_REPORTED_PROBLEMS:
        return "; ".join(problems)
    remaining = len(problems) - MAX_REPORTED_PROBLEMS
    suffix = "e mais 1 problema." if remaining == 1 else f"e mais {remaining} problemas."
    return "; ".join(problems[:MAX_REPORTED_PROBLEMS]) + f"; {suffix}"


def _state_from(problems: list[str]) -> dict:
    if not problems:
        return component_state("ready")
    return component_state("missing", _describe(problems))


def load_whisper_model(model_path: str, *, device: str = WHISPER_DEVICE, compute_type: str = WHISPER_COMPUTE_TYPE):
    """Constroi o ``WhisperModel`` a partir de uma pasta local, sem rede."""
    configure_offline_environment()
    from faster_whisper import WhisperModel

    return WhisperModel(
        model_path,
        device=device,
        compute_type=compute_type,
        local_files_only=True,
    )


def load_kokoro_pipeline(paths: ModelPaths, *, device: str = "cpu"):
    """Constroi um ``KPipeline`` ingles com configuracao, pesos e recursos locais.

    O Misaki pode tentar descarregar ``en_core_web_sm`` durante a construcao do
    ``G2P``; os recursos sao verificados antes para que tal nunca aconteca.
    """
    configure_offline_environment()
    # Nunca construir o pipeline (e o G2P do Misaki) com recursos em falta: o
    # Misaki tentaria instala-los e isso e um download durante o servico.
    problems = kokoro_file_problems(paths) + misaki_resource_problems(paths)
    if problems:
        raise WorkerError(_describe(problems), code=MODEL_MISSING)

    from kokoro import KPipeline
    from kokoro.model import KModel

    model = KModel(
        repo_id=KOKORO_REPO_ID,
        config=str(paths.kokoro_config),
        model=str(paths.kokoro_weights),
    ).to(device).eval()
    return KPipeline(
        lang_code=KOKORO_LANG_CODE,
        repo_id=KOKORO_REPO_ID,
        model=model,
        trf=False,
        device=device,
    )


def _write_wav(destination: Path, samples) -> None:
    """Escreve WAV PCM16 mono a 24 kHz, substituindo o destino atomicamente."""
    import soundfile

    temporary = destination.with_name(destination.name + ".part")
    try:
        destination.parent.mkdir(parents=True, exist_ok=True)
        soundfile.write(str(temporary), samples, SAMPLE_RATE, subtype="PCM_16", format="WAV")
        os.replace(temporary, destination)
    except Exception as error:
        raise WorkerError(
            f"Não foi possível guardar o áudio sintetizado em «{destination}».",
            code=STORAGE_WRITE_FAILED,
        ) from error
    finally:
        if temporary.exists():
            temporary.unlink(missing_ok=True)


class EngineRegistry:
    """Motores locais servidos sequencialmente, um trabalho de cada vez.

    A instancia mantem os modelos ja carregados em memoria; o servidor usa uma
    unica instancia e nunca corre duas operacoes ao mesmo tempo.
    """

    def __init__(self, manifest_path: str | Path | None = None):
        configure_offline_environment()
        self._manifest_path = Path(manifest_path) if manifest_path is not None else None
        self._paths: ModelPaths | None = None
        self._whisper_key: str | None = None
        self._whisper_model: Any = None
        self._kokoro_key: tuple[str, str] | None = None
        self._kokoro_pipeline: Any = None

    # -- disponibilidade ---------------------------------------------------

    def paths(self) -> ModelPaths:
        """Caminhos do manifesto, lidos uma vez por processo."""
        if self._paths is None:
            self._paths = ModelPaths.from_manifest(load_manifest(self._manifest_path))
        return self._paths

    def health(self) -> dict:
        """``ComponentState`` de transcricao e sintese, sem carregar modelos."""
        try:
            paths = self.paths()
        except ManifestError as error:
            state = component_state(error.status, error.message)
            return {"transcription": dict(state), "synthesis": dict(state)}
        return {
            "transcription": _state_from(missing_transcription_assets(paths)),
            "synthesis": _state_from(missing_synthesis_assets(paths)),
        }

    def model_name(self) -> str:
        try:
            return self.paths().whisper_model_name
        except WorkerError:
            return WHISPER_MODEL_NAME

    # -- transcricao -------------------------------------------------------

    def transcribe(self, params: Mapping[str, Any], progress: ProgressCallback) -> dict:
        audio_path = _required_text(params, "audioPath")
        device = params.get("device", WHISPER_DEVICE)
        if device != WHISPER_DEVICE:
            raise WorkerError(
                f"Só o dispositivo «{WHISPER_DEVICE}» é suportado (recebido «{device}»).",
                code=WORKER_PROTOCOL,
            )
        compute_type = params.get("computeType", WHISPER_COMPUTE_TYPE)
        if compute_type != WHISPER_COMPUTE_TYPE:
            raise WorkerError(
                f"Só o tipo de computação «{WHISPER_COMPUTE_TYPE}» é suportado (recebido «{compute_type}»).",
                code=WORKER_PROTOCOL,
            )
        language = params.get("language", WHISPER_LANGUAGE)
        if language != WHISPER_LANGUAGE:
            raise WorkerError(
                f"A transcrição suporta apenas inglês («{WHISPER_LANGUAGE}»), sem tradução.",
                code=WORKER_PROTOCOL,
            )

        model_path = params.get("modelPath")
        if model_path is None:
            model_path = str(self.paths().whisper_dir)
        elif not isinstance(model_path, str) or not model_path.strip():
            raise WorkerError(
                "O parâmetro «modelPath» tem de ser um caminho de texto.", code=WORKER_PROTOCOL
            )
        model_directory = Path(model_path)

        audio = Path(audio_path)
        if not audio.is_file():
            raise WorkerError(
                f"O ficheiro de áudio não existe em «{audio}».", code=ARTIFACT_MISSING
            )

        problems = missing_transcription_assets(
            ModelPaths(
                whisper_dir=model_directory,
                whisper_model_name=self.model_name(),
            )
        )
        if problems:
            raise WorkerError(_describe(problems), code=MODEL_MISSING)

        progress("loading")
        started = time.perf_counter()
        try:
            model = self._whisper(str(model_directory))
        except WorkerError:
            raise
        except Exception as error:
            raise WorkerError(
                f"Não foi possível carregar o modelo de transcrição em «{model_directory}». "
                "Execute de novo a preparação offline.",
                code=MODEL_MISSING,
            ) from error
        load_ms = _elapsed_ms(started)

        progress("processing")
        started = time.perf_counter()
        try:
            segments_iterator, _info = model.transcribe(
                str(audio),
                language=WHISPER_LANGUAGE,
                task="transcribe",
                vad_filter=True,
            )
            segments = [
                {
                    "startMs": max(0, round(segment.start * 1000)),
                    "endMs": max(0, round(segment.end * 1000)),
                    "text": segment.text,
                }
                for segment in segments_iterator
            ]
        except WorkerError:
            raise
        except Exception as error:
            if _is_decode_error(error):
                raise WorkerError(
                    f"O ficheiro de áudio em «{audio}» não pôde ser lido como WAV válido.",
                    code=INVALID_AUDIO,
                ) from error
            raise WorkerError(
                "Não foi possível transcrever o áudio. Tente novamente.", retryable=True
            ) from error
        processing_ms = _elapsed_ms(started)

        text = "".join(segment["text"] for segment in segments).strip()
        return {
            "status": "completed" if text else "no_speech",
            "text": text,
            "segments": segments,
            "model": self.model_name(),
            "language": WHISPER_LANGUAGE,
            "loadMs": load_ms,
            "processingMs": processing_ms,
        }

    # -- sintese -----------------------------------------------------------

    def synthesize(self, params: Mapping[str, Any], progress: ProgressCallback) -> dict:
        text = params.get("text")
        if not isinstance(text, str) or not text.strip():
            raise WorkerError("O texto para síntese está vazio.", code=WORKER_PROTOCOL)

        output_path = _required_text(params, "outputPath")
        destination = Path(output_path)
        if not destination.is_absolute():
            raise WorkerError(
                "O caminho de saída da síntese tem de ser absoluto.", code=WORKER_PROTOCOL
            )

        voice = params.get("voice", DEFAULT_VOICE)
        if voice != DEFAULT_VOICE:
            raise WorkerError(
                f"A voz suportada nesta fase é «{DEFAULT_VOICE}» (recebida «{voice}»).",
                code=WORKER_PROTOCOL,
            )

        speed = params.get("speed", DEFAULT_SPEED)
        if isinstance(speed, bool) or not isinstance(speed, (int, float)):
            raise WorkerError("A velocidade da síntese tem de ser um número.", code=WORKER_PROTOCOL)
        speed = float(speed)
        if not 0 < speed <= MAX_SPEED:
            raise WorkerError(
                f"A velocidade da síntese tem de estar entre 0 e {MAX_SPEED:g}.", code=WORKER_PROTOCOL
            )

        paths = self.paths()
        problems = missing_synthesis_assets(paths)
        if problems:
            raise WorkerError(_describe(problems), code=MODEL_MISSING)

        progress("loading")
        started = time.perf_counter()
        pipeline = self._kokoro(paths)
        load_ms = _elapsed_ms(started)

        progress("processing")
        started = time.perf_counter()
        import numpy

        chunks = []
        try:
            for chunk in pipeline(text, voice=str(paths.kokoro_voice), speed=speed):
                audio = getattr(chunk, "audio", None)
                if audio is None:
                    continue
                chunks.append(numpy.asarray(audio, dtype=numpy.float32).reshape(-1))
        except WorkerError:
            raise
        except Exception as error:
            raise WorkerError(
                "Não foi possível sintetizar o texto. Tente novamente.", retryable=True
            ) from error
        if not chunks:
            raise WorkerError(
                "A síntese não produziu áudio para este texto.", retryable=True
            )

        samples = numpy.concatenate(chunks)
        _write_wav(destination, samples)
        processing_ms = _elapsed_ms(started)

        return {
            "audioPath": str(destination),
            "sampleRate": SAMPLE_RATE,
            "durationMs": int(round(samples.size / SAMPLE_RATE * 1000)),
            "loadMs": load_ms,
            "processingMs": processing_ms,
        }

    # -- cache de modelos --------------------------------------------------

    def _whisper(self, model_path: str):
        if self._whisper_model is None or self._whisper_key != model_path:
            self._whisper_model = load_whisper_model(
                model_path, device=WHISPER_DEVICE, compute_type=WHISPER_COMPUTE_TYPE
            )
            self._whisper_key = model_path
        return self._whisper_model

    def _kokoro(self, paths: ModelPaths):
        key = (str(paths.kokoro_config), str(paths.kokoro_weights))
        if self._kokoro_pipeline is None or self._kokoro_key != key:
            self._kokoro_pipeline = load_kokoro_pipeline(paths)
            self._kokoro_key = key
        return self._kokoro_pipeline


def _required_text(params: Mapping[str, Any], name: str) -> str:
    value = params.get(name)
    if not isinstance(value, str) or not value.strip():
        raise WorkerError(f"O parâmetro «{name}» é obrigatório.", code=WORKER_PROTOCOL)
    return value


def _elapsed_ms(started: float) -> int:
    return max(0, int(round((time.perf_counter() - started) * 1000)))


def _is_decode_error(error: BaseException) -> bool:
    try:
        import av
    except Exception:
        return False
    decode_errors = tuple(
        getattr(av.error, name)
        for name in ("FFmpegError", "InvalidDataError")
        if hasattr(av.error, name)
    )
    return bool(decode_errors) and isinstance(error, decode_errors)

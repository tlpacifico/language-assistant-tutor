"""Servidor do auxiliar: uma linha JSON por mensagem em stdin/stdout.

O stdout transporta exclusivamente mensagens do protocolo v1. Qualquer escrita
das bibliotecas cai no stderr, que a app drena continuamente. Os motores sao
importados tardiamente e o ambiente fica offline antes de os importar, para que
a pratica nunca descarregue modelos.
"""

from __future__ import annotations

import argparse
import contextlib
import json
import os
import sys
import traceback
from typing import Any, Callable, TextIO

from .protocol import (
    HEALTH,
    SHUTDOWN,
    SYNTHESIZE,
    TRANSCRIBE,
    WorkerError,
    encode_message,
    error_message,
    parse_request,
    progress_message,
    result_message,
)

MANIFEST_ENV = "LANGUAGE_TUTOR_MODELS_MANIFEST"

#: Mensagem generica para falhas inesperadas, sempre em PT-PT.
UNEXPECTED_FAILURE = (
    "O auxiliar falhou ao processar o pedido. Tente novamente ou reinicie o auxiliar."
)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="audio_worker",
        description=(
            "Auxiliar local da fase 1: transcreve com faster-whisper e sintetiza com "
            "Kokoro, servindo o protocolo JSON v1 em stdin/stdout."
        ),
    )
    parser.add_argument(
        "--models-manifest",
        default=None,
        metavar="CAMINHO",
        help=(
            "Manifesto de modelos preparado offline (caminho absoluto). "
            f"Por omissão usa a variável {MANIFEST_ENV}."
        ),
    )
    return parser


def _reconfigure(stream: Any, *, encoding: str, errors: str) -> None:
    reconfigure = getattr(stream, "reconfigure", None)
    if not callable(reconfigure):
        return
    try:
        reconfigure(encoding=encoding, errors=errors)
    except (ValueError, OSError):  # stream sem buffer ou ja fechado
        pass


def _write_message(stream: TextIO, message: dict) -> None:
    line = encode_message(message)
    try:
        stream.write(line + "\n")
    except UnicodeEncodeError:
        # stdout com uma codificacao legada (ex.: cp1252): escapar acentos.
        stream.write(json.dumps(message, ensure_ascii=True, separators=(",", ":")) + "\n")
    stream.flush()


@contextlib.contextmanager
def library_output_on_stderr(stderr: TextIO):
    """Desvia para o stderr tudo o que for escrito em ``sys.stdout``.

    As bibliotecas (Kokoro, Hugging Face, loguru) escrevem por vezes em stdout;
    durante o servico essa saida nao pode contaminar o protocolo.
    """
    previous = sys.stdout
    sys.stdout = stderr
    try:
        yield
    finally:
        sys.stdout = previous


def _default_registry(manifest_path: str | None):
    # Import tardio: ``engines`` nao importa torch/faster-whisper no topo, mas o
    # ambiente offline fica definido antes de qualquer import de motor real.
    from .engines import EngineRegistry, configure_offline_environment

    configure_offline_environment()
    return EngineRegistry(manifest_path)


def _dispatch(registry: Any, op: str, params: dict, report_progress: Callable[[str], None]) -> dict:
    if op == HEALTH:
        return registry.health()
    if op == TRANSCRIBE:
        return registry.transcribe(params, report_progress)
    if op == SYNTHESIZE:
        return registry.synthesize(params, report_progress)
    # ``parse_request`` so aceita operacoes conhecidas e ``shutdown`` sai antes.
    raise WorkerError(f"Operação não suportada: {op}.", code="WORKER_PROTOCOL")


def serve(
    stdin: TextIO,
    stdout: TextIO,
    stderr: TextIO,
    registry: Any = None,
    manifest_path: str | None = None,
) -> int:
    """Serve pedidos sequencialmente ate EOF ou ``shutdown``.

    ``registry`` permite substituir os motores nos testes; em producao e
    construido a partir do manifesto de modelos.
    """
    if registry is None:
        registry = _default_registry(manifest_path)

    with library_output_on_stderr(stderr):
        while True:
            line = stdin.readline()
            if line == "":
                return 0
            if not line.strip():
                continue

            try:
                request = parse_request(line)
            except WorkerError as error:
                _write_message(stdout, error_message("", error))
                continue

            if request.op == SHUTDOWN:
                _write_message(stdout, result_message(request.id, {"stopped": True}))
                return 0

            def report_progress(stage: str, request_id: str = request.id) -> None:
                _write_message(stdout, progress_message(request_id, stage))

            try:
                result = _dispatch(registry, request.op, dict(request.params), report_progress)
            except WorkerError as error:
                _write_message(stdout, error_message(request.id, error))
            except Exception as error:  # falha inesperada: nunca fica em silencio
                # Canal estruturado sem texto do erro, caminhos, nomes de
                # utilizadores ou linhas de código. O supervisor só persiste isto.
                frames = traceback.extract_tb(error.__traceback__)
                own_frames = [frame for frame in frames if os.path.basename(frame.filename) in {"__main__.py", "engines.py", "protocol.py"}]
                for frame in own_frames[-12:]:
                    stderr.write("TUTOR_DIAGNOSTIC " + json.dumps({
                        "component": "worker", "operation": request.op, "code": "UNEXPECTED",
                        "durationMs": None, "file": os.path.basename(frame.filename), "line": frame.lineno,
                    }) + "\n")
                stderr.write(
                    f"falha ao processar {request.op} ({request.id}): "
                    f"{type(error).__name__}: {error}\n"
                )
                stderr.write(traceback.format_exc())
                stderr.flush()
                _write_message(stdout, error_message(request.id, WorkerError(UNEXPECTED_FAILURE, retryable=True)))
            else:
                _write_message(stdout, result_message(request.id, result))


def main(argv: list[str] | None = None) -> int:
    """Ponto de entrada ``python -u -m audio_worker``."""
    args = build_parser().parse_args(argv)
    manifest_path = args.models_manifest or os.environ.get(MANIFEST_ENV) or None

    protocol_out = sys.stdout
    _reconfigure(protocol_out, encoding="utf-8", errors="strict")
    _reconfigure(sys.stderr, encoding="utf-8", errors="backslashreplace")
    return serve(sys.stdin, protocol_out, sys.stderr, manifest_path=manifest_path)


if __name__ == "__main__":
    raise SystemExit(main())

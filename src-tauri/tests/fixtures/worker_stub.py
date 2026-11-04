#!/usr/bin/env python3
"""Auxiliar falso para os testes do supervisor Rust (tarefa 5).

O supervisor inicia ``<python> -u -m audio_worker --models-manifest <caminho>``.
Nos testes o ficheiro indicado nao e um manifesto de modelos: e um cenario. A
chave ``stub`` desse JSON descreve atrasos e falhas, e os testes copiam este
ficheiro para o diretorio de trabalho como ``audio_worker.py`` para que
``-m audio_worker`` o importe. Nao importa ``audio_worker``, ``faster_whisper``,
``kokoro`` nem qualquer modelo: usa apenas a biblioteca padrao, por isso a
suite nao depende de modelos, voz ou microfone.

Cenario (manifesto, chave ``stub``; todas as chaves sao opcionais):

  log               caminho do registo (uma linha "<op> <id>" por mensagem)
  logPath           alias de ``log``
  pidFile           caminho onde escreve o PID do processo
  loadDelay         segundos de "carregamento" antes de processar um trabalho
  loadMarker        ficheiro criado quando o trabalho "carrega" os modelos
  resultDelay       segundos entre ``processing`` e a resposta final
  stderrLines       linhas escritas em stderr no arranque (teste de drenagem)
  crashOnce         o primeiro trabalho termina o processo sem resposta (código 3)
  nonJsonOnce       o primeiro trabalho escreve texto invalido em stdout
  extraResultOnce   o primeiro trabalho repete a resposta final
  stateFile         ficheiro que conserva, entre processos, o que ja foi consumido
  ignoreShutdown    fica vivo depois de responder a ``shutdown``
  failHealth        responde ao handshake com um erro

As falhas ``*Once`` são consumidas uma vez por *conjunto de processos*: o
estado fica em ``stateFile``, para que um reinício do auxiliar (um processo
novo) já responda normalmente e os testes verifiquem a recuperação.

O caminho do cenario tambem pode vir da variavel
``LANGUAGE_TUTOR_STUB_SCENARIO``. Os pedidos seguem o protocolo v1 real
(``audio-worker/src/audio_worker/protocol.py``) e as respostas seguem
``contracts/worker-v1.examples.json``.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
from typing import Any

PROTOCOL_VERSION = 1

HEALTH = "health"
TRANSCRIBE = "transcribe"
SYNTHESIZE = "synthesize"
SHUTDOWN = "shutdown"

SCENARIO_ENV = "LANGUAGE_TUTOR_STUB_SCENARIO"
MANIFEST_ENV = "LANGUAGE_TUTOR_MODELS_MANIFEST"

MISSING_VOICE = "Falta a voz af_heart; execute a preparação offline dos modelos."


def scenario_path(argv: list[str]) -> str | None:
    """Caminho do cenario: ambiente primeiro, depois ``--models-manifest``."""
    from_env = os.environ.get(SCENARIO_ENV)
    if from_env:
        return from_env
    parser = argparse.ArgumentParser(prog="audio_worker", add_help=False)
    parser.add_argument("--models-manifest", default=None)
    known, _unknown = parser.parse_known_args(argv)
    return known.models_manifest or os.environ.get(MANIFEST_ENV) or None


def load_scenario(path: str | None) -> dict[str, Any]:
    if not path:
        return {}
    try:
        with open(path, encoding="utf-8") as handle:
            data = json.load(handle)
    except (OSError, ValueError):
        return {}
    stub = data.get("stub") if isinstance(data, dict) else None
    return stub if isinstance(stub, dict) else {}


def _as_float(value: Any, default: float = 0.0) -> float:
    try:
        return float(value)
    except (TypeError, ValueError):
        return default


def _as_int(value: Any, default: int = 0) -> int:
    try:
        return int(value)
    except (TypeError, ValueError):
        return default


def _as_bool(value: Any) -> bool:
    return bool(value) if value is not None else False


def write_message(message: dict[str, Any]) -> None:
    """Uma unica linha JSON em stdout, sem texto adicional."""
    sys.stdout.write(json.dumps(message, ensure_ascii=False, separators=(",", ":")) + "\n")
    sys.stdout.flush()


def component(status: str, message: str | None = None) -> dict[str, Any]:
    return {"status": status, "message": message}


def result_message(request_id: str, result: dict[str, Any]) -> dict[str, Any]:
    return {
        "protocolVersion": PROTOCOL_VERSION,
        "id": request_id,
        "event": "result",
        "ok": True,
        "result": result,
    }


def error_message(request_id: str, code: str, message: str, retryable: bool) -> dict[str, Any]:
    return {
        "protocolVersion": PROTOCOL_VERSION,
        "id": request_id,
        "event": "result",
        "ok": False,
        "error": {"code": code, "message": message, "retryable": retryable},
    }


def progress_message(request_id: str, stage: str) -> dict[str, Any]:
    return {
        "protocolVersion": PROTOCOL_VERSION,
        "id": request_id,
        "event": "progress",
        "stage": stage,
    }


class Stub:
    """Estado do auxiliar falso entre pedidos."""

    def __init__(self, scenario: dict[str, Any]) -> None:
        self.log_path = scenario.get("log") or scenario.get("logPath")
        self.state_file = scenario.get("stateFile")
        self.load_delay = _as_float(scenario.get("loadDelay"))
        self.load_marker = scenario.get("loadMarker")
        self.result_delay = _as_float(scenario.get("resultDelay"))
        self.stderr_lines = _as_int(scenario.get("stderrLines"))
        self.ignore_shutdown = _as_bool(scenario.get("ignoreShutdown"))
        self.fail_health = _as_bool(scenario.get("failHealth"))
        self.reject_transcription = _as_bool(scenario.get("rejectTranscription"))
        self.crash_once = _as_bool(scenario.get("crashOnce"))
        self.non_json_once = _as_bool(scenario.get("nonJsonOnce"))
        self.extra_result_once = _as_bool(scenario.get("extraResultOnce"))
        self.state = self.load_state()

    def load_state(self) -> dict[str, Any]:
        if not self.state_file:
            return {}
        try:
            with open(self.state_file, encoding="utf-8") as handle:
                state = json.load(handle)
        except (OSError, ValueError):
            return {}
        return state if isinstance(state, dict) else {}

    def save_state(self) -> None:
        if not self.state_file:
            return
        try:
            with open(self.state_file, "w", encoding="utf-8") as handle:
                json.dump(self.state, handle)
                handle.flush()
        except OSError:
            pass

    def consume_once(self, name: str) -> bool:
        """Verdadeiro apenas na primeira vez, mesmo entre processos."""
        if self.state.get(name):
            return False
        self.state[name] = True
        self.save_state()
        return True

    def log(self, op: str, detail: str = "") -> None:
        if not self.log_path:
            return
        line = f"{op} {detail}".rstrip()
        try:
            with open(self.log_path, "a", encoding="utf-8") as handle:
                handle.write(line + "\n")
                handle.flush()
        except OSError:
            pass

    def flood_stderr(self) -> None:
        if self.stderr_lines <= 0:
            return
        chunk = "stub: ruido de stderr " * 4 + "\n"
        for _ in range(self.stderr_lines):
            sys.stderr.write(chunk)
        sys.stderr.flush()

    def create_artifact(self, params: dict[str, Any]) -> None:
        """Escreve um ficheiro provisório antes de anunciar ``processing``."""
        output = params.get("outputPath")
        if not isinstance(output, str) or not output:
            return
        parent = os.path.dirname(output)
        if parent:
            try:
                os.makedirs(parent, exist_ok=True)
            except OSError:
                return
        try:
            with open(output, "wb") as handle:
                handle.write(b"RIFF-stub-incompleto")
        except OSError:
            pass


def health_result() -> dict[str, Any]:
    return {
        "transcription": component("ready"),
        "synthesis": component("missing", MISSING_VOICE),
    }


def transcription_result() -> dict[str, Any]:
    return {
        "status": "completed",
        "text": "Last year I reorganised our release checklist because deploys kept failing on Fridays.",
        "segments": [
            {"startMs": 0, "endMs": 2400, "text": " Last year I reorganised our release checklist"},
            {"startMs": 2400, "endMs": 5100, "text": " because deploys kept failing on Fridays."},
        ],
        "model": "small",
        "language": "en",
        "loadMs": 4200,
        "processingMs": 3120,
    }


def synthesis_result(params: dict[str, Any]) -> dict[str, Any]:
    return {
        "audioPath": params.get("outputPath", ""),
        "sampleRate": 24000,
        "durationMs": 3120,
        "loadMs": 2400,
        "processingMs": 1450,
    }


def handle_job(stub: Stub, request: dict[str, Any]) -> None:
    request_id = str(request.get("id", ""))
    op = str(request.get("op", ""))
    params = request.get("params") or {}

    write_message(progress_message(request_id, "loading"))
    if stub.load_delay > 0:
        time.sleep(stub.load_delay)
    if stub.load_marker:
        try:
            with open(stub.load_marker, "a", encoding="utf-8") as handle:
                handle.write("loaded\n")
                handle.flush()
        except OSError:
            pass

    if op == SYNTHESIZE:
        stub.create_artifact(params)

    write_message(progress_message(request_id, "processing"))

    if stub.crash_once and stub.consume_once("crash"):
        sys.stderr.write("stub: falha simulada sem resposta\n")
        sys.stderr.flush()
        os._exit(3)

    if stub.non_json_once and stub.consume_once("nonJson"):
        sys.stdout.write("isto não é uma mensagem do protocolo v1\n")
        sys.stdout.flush()
        while True:  # o supervisor tem de encerrar este processo
            time.sleep(0.2)

    if stub.result_delay > 0:
        time.sleep(stub.result_delay)

    if op == TRANSCRIBE and stub.reject_transcription:
        write_message(error_message(request_id, "INVALID_AUDIO", "PRIVATE_WORKER_RESPONSE", True))
        return
    payload = transcription_result() if op == TRANSCRIBE else synthesis_result(params)
    message = result_message(request_id, payload)
    write_message(message)
    if stub.extra_result_once and stub.consume_once("extra"):
        write_message(message)


def main(argv: list[str] | None = None) -> int:
    # Garantir UTF-8 em stdin/stdout, como o auxiliar real.
    for stream in (sys.stdin, sys.stdout):
        reconfigure = getattr(stream, "reconfigure", None)
        if callable(reconfigure):
            reconfigure(encoding="utf-8", errors="strict")
    reconfigure = getattr(sys.stderr, "reconfigure", None)
    if callable(reconfigure):
        reconfigure(encoding="utf-8", errors="backslashreplace")

    scenario = load_scenario(scenario_path(list(argv or sys.argv[1:])))
    stub = Stub(scenario)
    stub.flood_stderr()

    pid_file = scenario.get("pidFile")
    if isinstance(pid_file, str) and pid_file:
        try:
            with open(pid_file, "w", encoding="utf-8") as handle:
                handle.write(str(os.getpid()))
        except OSError:
            pass

    stub.log("start", str(os.getpid()))

    for raw in sys.stdin:
        line = raw.strip()
        if not line:
            continue
        try:
            request = json.loads(line)
        except ValueError:
            stub.log("invalid-line")
            continue
        if not isinstance(request, dict):
            stub.log("invalid-request")
            continue

        op = str(request.get("op", ""))
        request_id = str(request.get("id", ""))
        stub.log(op, request_id)

        if op == SHUTDOWN:
            write_message(result_message(request_id, {"stopped": True}))
            stub.log("exit")
            if stub.ignore_shutdown:
                while True:  # teste do encerramento forçado
                    time.sleep(0.2)
            return 0

        if op == HEALTH:
            if stub.fail_health:
                write_message(
                    error_message(
                        request_id,
                        "MODEL_MISSING",
                        "O manifesto de modelos ainda não foi preparado.",
                        False,
                    )
                )
            else:
                write_message(result_message(request_id, health_result()))
            continue

        if op in (TRANSCRIBE, SYNTHESIZE):
            handle_job(stub, request)
            continue

        write_message(
            error_message(
                request_id,
                "WORKER_PROTOCOL",
                f"Operação desconhecida: {op}.",
                False,
            )
        )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())

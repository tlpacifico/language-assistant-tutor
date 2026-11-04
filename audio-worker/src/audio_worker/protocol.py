"""Protocolo linha-a-linha v1 entre a app Tauri e o auxiliar Python.

Cada linha de stdin e um pedido JSON com ``protocolVersion``, ``id``, ``op`` e
``params``. Cada pedido valido produz exatamente uma mensagem final ``result``;
mensagens ``progress`` podem antecedela. Os nomes JSON sao camelCase e os
valores enumerados snake_case, como no resto dos contratos da fase 1.
"""

from __future__ import annotations

import json
from dataclasses import dataclass, field
from typing import Any, Mapping

PROTOCOL_VERSION = 1

HEALTH = "health"
TRANSCRIBE = "transcribe"
SYNTHESIZE = "synthesize"
PRELOAD = "preload"
SHUTDOWN = "shutdown"

OPERATIONS = (HEALTH, TRANSCRIBE, SYNTHESIZE, PRELOAD, SHUTDOWN)

#: Etapas aceites nas mensagens de progresso.
PROGRESS_STAGES = ("loading", "processing")

#: Estados de um componente em ``health`` (``ComponentState`` do dominio).
COMPONENT_STATUSES = ("ready", "missing", "loading", "error")

#: Codigos estaveis de ``AppError`` partilhados com o dominio Rust.
BUSY = "BUSY"
ARTIFACT_MISSING = "ARTIFACT_MISSING"
INVALID_AUDIO = "INVALID_AUDIO"
MODEL_MISSING = "MODEL_MISSING"
WORKER_UNAVAILABLE = "WORKER_UNAVAILABLE"
WORKER_PROTOCOL = "WORKER_PROTOCOL"
STORAGE_WRITE_FAILED = "STORAGE_WRITE_FAILED"

ERROR_CODES = (
    BUSY,
    "DEVICE_UNAVAILABLE",
    "DEVICE_FORMAT_UNSUPPORTED",
    "CAPTURE_INTERRUPTED",
    STORAGE_WRITE_FAILED,
    ARTIFACT_MISSING,
    INVALID_AUDIO,
    MODEL_MISSING,
    WORKER_UNAVAILABLE,
    WORKER_PROTOCOL,
    "WORKER_TIMEOUT",
    "STALE_SOURCE",
)

MAX_ID_LENGTH = 128


class WorkerError(Exception):
    """Erro com a forma ``AppError`` do dominio, com mensagem em PT-PT."""

    code = WORKER_UNAVAILABLE
    retryable = False

    def __init__(self, message: str, *, code: str | None = None, retryable: bool | None = None):
        super().__init__(message)
        self.message = message
        if code is not None:
            self.code = code
        if retryable is not None:
            self.retryable = retryable

    def to_error(self) -> dict:
        return {"code": self.code, "message": self.message, "retryable": bool(self.retryable)}


class ProtocolError(WorkerError):
    """Pedido que nao respeita o protocolo v1."""

    code = WORKER_PROTOCOL
    retryable = False


class ManifestError(WorkerError):
    """Manifesto de modelos ausente ou ilegivel.

    ``status`` distingue "ainda nao preparado" (``missing``) de "ilegivel"
    (``error``) para que ``health`` possa reportar o ``ComponentState`` certo.
    """

    code = MODEL_MISSING

    def __init__(self, message: str, *, status: str = "missing"):
        super().__init__(message)
        self.status = status if status in COMPONENT_STATUSES else "error"


@dataclass(frozen=True)
class WorkerRequest:
    """Pedido valido do protocolo v1."""

    protocol_version: int
    id: str
    op: str
    params: Mapping[str, Any] = field(default_factory=dict)

    def param(self, name: str, default: Any = None) -> Any:
        return self.params.get(name, default)


def parse_request(line: str) -> WorkerRequest:
    """Converte uma linha de stdin num ``WorkerRequest``.

    Levanta ``ProtocolError`` (codigo ``WORKER_PROTOCOL``) para qualquer linha
    que nao seja um pedido valido do protocolo v1.
    """
    if not isinstance(line, str):
        raise ProtocolError("A mensagem recebida não é texto.")
    payload = line.strip()
    if not payload:
        raise ProtocolError("A mensagem recebida está vazia.")

    try:
        message = json.loads(payload)
    except ValueError:
        raise ProtocolError("A linha recebida não é um pedido válido do protocolo v1.") from None

    if not isinstance(message, dict):
        raise ProtocolError("A mensagem recebida tem de ser um objeto JSON.")

    version = message.get("protocolVersion")
    if isinstance(version, bool) or not isinstance(version, int):
        raise ProtocolError("A mensagem não indica uma versão de protocolo válida.")
    if version != PROTOCOL_VERSION:
        raise ProtocolError(
            f"Versão de protocolo não suportada: {version}. Este auxiliar fala o protocolo {PROTOCOL_VERSION}."
        )

    request_id = message.get("id")
    if not isinstance(request_id, str) or not request_id:
        raise ProtocolError("A mensagem não tem um identificador de pedido válido.")
    if len(request_id) > MAX_ID_LENGTH:
        raise ProtocolError("O identificador do pedido é demasiado longo.")

    op = message.get("op")
    if not isinstance(op, str) or op not in OPERATIONS:
        raise ProtocolError(f"Operação desconhecida: {op!r}. Operações aceites: {', '.join(OPERATIONS)}.")

    params = message.get("params", {})
    if params is None:
        params = {}
    if not isinstance(params, dict):
        raise ProtocolError("Os parâmetros do pedido têm de ser um objeto JSON.")

    return WorkerRequest(protocol_version=version, id=request_id, op=op, params=params)


def encode_message(message: Mapping[str, Any]) -> str:
    """Codifica uma mensagem numa unica linha JSON, sem quebra de linha final."""
    return json.dumps(message, ensure_ascii=False, separators=(",", ":"))


def component_state(status: str, message: str | None = None) -> dict:
    """``ComponentState`` do dominio: estado e mensagem opcional em PT-PT."""
    if status not in COMPONENT_STATUSES:
        raise ValueError(f"Estado de componente desconhecido: {status!r}")
    return {"status": status, "message": message}


def result_message(request_id: str, result: Mapping[str, Any]) -> dict:
    return {
        "protocolVersion": PROTOCOL_VERSION,
        "id": request_id,
        "event": "result",
        "ok": True,
        "result": dict(result),
    }


def error_message(request_id: str, error: WorkerError) -> dict:
    return {
        "protocolVersion": PROTOCOL_VERSION,
        "id": request_id,
        "event": "result",
        "ok": False,
        "error": error.to_error(),
    }


def progress_message(request_id: str, stage: str) -> dict:
    if stage not in PROGRESS_STAGES:
        raise ValueError(f"Etapa de progresso desconhecida: {stage!r}")
    return {
        "protocolVersion": PROTOCOL_VERSION,
        "id": request_id,
        "event": "progress",
        "stage": stage,
    }

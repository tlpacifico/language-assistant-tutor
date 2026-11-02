"""Auxiliar local de transcricao e sintese da fase 1.

O processo serve uma linha JSON por mensagem em stdin/stdout (protocolo v1) e
delega a transcricao ao faster-whisper e a sintese ao Kokoro, sempre em CPU e
sem download durante a pratica.
"""

__version__ = "0.1.0"

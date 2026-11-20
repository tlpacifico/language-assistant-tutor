-- Migração 006: preferências do leitor de artigos (reprodução automática e
-- velocidade), numa única linha. Velocidades em décimos (8 = 0.8×, 10 = 1.0×,
-- 12 = 1.2×) para evitar problemas de igualdade com vírgula flutuante.

CREATE TABLE article_preferences (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    autoplay INTEGER NOT NULL CHECK (autoplay IN (0, 1)),
    speed_x10 INTEGER NOT NULL CHECK (speed_x10 IN (8, 10, 12)),
    updated_at TEXT NOT NULL
);
INSERT INTO article_preferences (id, autoplay, speed_x10, updated_at)
VALUES (1, 1, 10, '2026-11-20T00:00:00Z');

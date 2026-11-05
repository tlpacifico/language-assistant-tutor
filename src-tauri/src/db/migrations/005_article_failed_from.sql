-- Migração 005: estado de onde veio a falha recuperável da conversa sobre
-- artigos, para o `Retry` repetir o passo certo (resposta, relatório ou gravação).

ALTER TABLE article_sessions ADD COLUMN failed_from TEXT;

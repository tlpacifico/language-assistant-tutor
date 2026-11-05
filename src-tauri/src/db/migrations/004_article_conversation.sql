-- Migração 004: conversa em inglês sobre um artigo.
--
-- Domínio próprio, sem tocar nas tabelas STAR/práticas (v1-v3). Fontes,
-- turnos, versões, pedidos e relatórios são imutáveis enquanto a sessão existe;
-- só a eliminação lógica (`deleted_at`) permite remover linhas.

CREATE TABLE article_sessions (
    id TEXT PRIMARY KEY NOT NULL,
    title TEXT,
    article_text TEXT NOT NULL CHECK (length(trim(article_text)) > 0),
    state TEXT NOT NULL CHECK (state IN (
        'draft', 'awaiting_tutor', 'ready', 'recording', 'transcribing',
        'needs_review', 'recoverable_error', 'finishing', 'completed', 'paused')),
    paused_from TEXT,
    version INTEGER NOT NULL CHECK (version >= 1),
    -- Incrementa ao revogar, terminar ou corrigir: invalida pedidos em curso.
    generation INTEGER NOT NULL DEFAULT 1 CHECK (generation >= 1),
    active_branch_id TEXT NOT NULL,
    auth_destination TEXT,
    auth_notice_version TEXT,
    error_json TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT,
    CHECK ((auth_destination IS NULL) = (auth_notice_version IS NULL))
);

-- Uma única conversa de artigo ativa de cada vez.
CREATE UNIQUE INDEX article_sessions_one_active
    ON article_sessions ((1))
    WHERE state <> 'completed' AND deleted_at IS NULL;

CREATE TABLE article_branches (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL REFERENCES article_sessions(id),
    parent_branch_id TEXT REFERENCES article_branches(id),
    fork_turn_id TEXT,
    created_at TEXT NOT NULL,
    CHECK ((parent_branch_id IS NULL) = (fork_turn_id IS NULL))
);

CREATE TABLE article_turns (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL REFERENCES article_sessions(id),
    branch_id TEXT NOT NULL REFERENCES article_branches(id),
    -- Posição na conversa; um ramo continua depois do turno de bifurcação.
    ordinal INTEGER NOT NULL CHECK (ordinal >= 1),
    speaker TEXT NOT NULL CHECK (speaker IN ('tutor', 'user')),
    created_at TEXT NOT NULL,
    UNIQUE (branch_id, ordinal)
);

-- Cada versão aplica-se a um ramo; a efetiva num ramo é a mais recente do
-- próprio ramo ou, em falta, a do ancestral mais próximo.
CREATE TABLE article_versions (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL REFERENCES article_sessions(id),
    turn_id TEXT NOT NULL REFERENCES article_turns(id),
    branch_id TEXT NOT NULL REFERENCES article_branches(id),
    seq INTEGER NOT NULL CHECK (seq >= 1),
    -- Nulo para falas do tutor.
    source TEXT CHECK (source IN ('automatic_sent', 'manual_review')),
    text TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE (turn_id, branch_id, seq)
);

CREATE TABLE article_requests (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL REFERENCES article_sessions(id),
    operation TEXT NOT NULL CHECK (operation IN ('opening', 'reply', 'feedback')),
    generation INTEGER NOT NULL,
    branch_id TEXT NOT NULL REFERENCES article_branches(id),
    turn_id TEXT REFERENCES article_turns(id),
    attempt INTEGER NOT NULL CHECK (attempt >= 1),
    source_version_ids_json TEXT NOT NULL,
    config_json TEXT NOT NULL,
    context_json TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('pending', 'completed', 'failed', 'stale', 'interrupted')),
    usage_json TEXT,
    error_json TEXT,
    created_at TEXT NOT NULL,
    finished_at TEXT
);

CREATE TABLE article_reports (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL REFERENCES article_sessions(id),
    branch_id TEXT NOT NULL REFERENCES article_branches(id),
    request_id TEXT NOT NULL REFERENCES article_requests(id),
    version_ids_json TEXT NOT NULL,
    report_json TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE article_artifacts (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL REFERENCES article_sessions(id),
    turn_id TEXT REFERENCES article_turns(id),
    kind TEXT NOT NULL CHECK (kind IN ('answer_audio', 'tutor_audio')),
    -- Relativo à pasta de dados; sempre sob `articles/<sessionId>/`.
    path TEXT NOT NULL UNIQUE,
    state TEXT NOT NULL CHECK (state IN ('present', 'pending_delete')),
    created_at TEXT NOT NULL,
    CHECK (path LIKE 'articles/' || session_id || '/%' AND path NOT LIKE '%..%')
);

CREATE TABLE article_receipts (
    request_id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL,
    operation TEXT NOT NULL,
    version_after INTEGER NOT NULL,
    created_at TEXT NOT NULL
);

-- Imutabilidade das fontes: só a eliminação lógica permite apagar linhas.
CREATE TRIGGER article_sessions_text_fixed
BEFORE UPDATE OF article_text, title, created_at ON article_sessions
BEGIN
    SELECT RAISE(ABORT, 'article source is immutable');
END;

CREATE TRIGGER article_turns_no_update
BEFORE UPDATE ON article_turns
BEGIN
    SELECT RAISE(ABORT, 'article turn is immutable');
END;

CREATE TRIGGER article_versions_no_update
BEFORE UPDATE ON article_versions
BEGIN
    SELECT RAISE(ABORT, 'article version is immutable');
END;

CREATE TRIGGER article_reports_no_update
BEFORE UPDATE ON article_reports
BEGIN
    SELECT RAISE(ABORT, 'article report is immutable');
END;

CREATE TRIGGER article_turns_no_delete
BEFORE DELETE ON article_turns
WHEN (SELECT deleted_at FROM article_sessions WHERE id = OLD.session_id) IS NULL
BEGIN
    SELECT RAISE(ABORT, 'article turn is immutable');
END;

CREATE TRIGGER article_versions_no_delete
BEFORE DELETE ON article_versions
WHEN (SELECT deleted_at FROM article_sessions WHERE id = OLD.session_id) IS NULL
BEGIN
    SELECT RAISE(ABORT, 'article version is immutable');
END;

CREATE TRIGGER article_reports_no_delete
BEFORE DELETE ON article_reports
WHEN (SELECT deleted_at FROM article_sessions WHERE id = OLD.session_id) IS NULL
BEGIN
    SELECT RAISE(ABORT, 'article report is immutable');
END;

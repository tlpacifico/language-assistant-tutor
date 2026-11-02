-- Migração 001: esquema inicial das entrevistas guiadas (fase 2).
--
-- Apenas dados das sessões novas. Não existem tabelas de fases futuras
-- (histórias, feedback, custos) nem caminhos de eliminação: os textos
-- automáticos e confirmados são imutáveis e a aplicação não apaga sessões
-- nesta fase.
--
-- Arrays pequenos (competências, critérios, segmentos, configuração) ficam em
-- TEXT como JSON validado por serde na aplicação. Relações e invariantes usam
-- colunas, CHECKs, chaves estrangeiras compostas e triggers. O áudio nunca é
-- guardado como BLOB: só referências relativas a ficheiros locais.

CREATE TABLE content_seeds (
    version TEXT PRIMARY KEY NOT NULL,
    applied_at TEXT NOT NULL
);

CREATE TABLE questions (
    id TEXT PRIMARY KEY NOT NULL,
    text TEXT NOT NULL CHECK (length(trim(text)) > 0),
    category TEXT NOT NULL CHECK (
        category IN (
            'conflict_collaboration',
            'leadership_initiative',
            'failure_learning',
            'problem_solving',
            'priorities_deadlines',
            'change_adaptation'
        )
    ),
    competencies_json TEXT NOT NULL CHECK (json_valid(competencies_json)),
    criteria_json TEXT NOT NULL CHECK (json_valid(criteria_json)),
    version INTEGER NOT NULL CHECK (version >= 1),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE sessions (
    id TEXT PRIMARY KEY NOT NULL,
    category TEXT NULL CHECK (
        category IS NULL OR category IN (
            'conflict_collaboration',
            'leadership_initiative',
            'failure_learning',
            'problem_solving',
            'priorities_deadlines',
            'change_adaptation'
        )
    ),
    question_count INTEGER NOT NULL CHECK (question_count > 0),
    position INTEGER NOT NULL CHECK (position >= 1),
    status TEXT NOT NULL CHECK (status IN ('in_progress', 'completed')),
    mode TEXT NOT NULL CHECK (mode = 'local_v2'),
    version INTEGER NOT NULL CHECK (version >= 1),
    created_at TEXT NOT NULL,
    completed_at TEXT NULL,
    CHECK (position <= question_count),
    CHECK ((status = 'completed') = (completed_at IS NOT NULL))
);

-- No máximo uma sessão em curso. O índice parcial cobre apenas as linhas em
-- curso e indexa uma expressão constante, pelo que a unicidade impede a
-- segunda sessão em curso sem afetar as sessões concluídas.
CREATE UNIQUE INDEX one_active_session
    ON sessions((1))
    WHERE status = 'in_progress';

CREATE TABLE session_questions (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    source_question_id TEXT NOT NULL REFERENCES questions(id),
    source_version INTEGER NOT NULL CHECK (source_version >= 1),
    position INTEGER NOT NULL CHECK (position >= 1),
    text TEXT NOT NULL CHECK (length(trim(text)) > 0),
    category TEXT NOT NULL CHECK (
        category IN (
            'conflict_collaboration',
            'leadership_initiative',
            'failure_learning',
            'problem_solving',
            'priorities_deadlines',
            'change_adaptation'
        )
    ),
    competencies_json TEXT NOT NULL CHECK (json_valid(competencies_json)),
    criteria_json TEXT NOT NULL CHECK (json_valid(criteria_json)),
    selected_recording_id TEXT NULL,
    selection_version INTEGER NOT NULL DEFAULT 0 CHECK (selection_version >= 0),
    UNIQUE (session_id, position),
    UNIQUE (session_id, source_question_id),
    -- A gravação selecionada tem de pertencer a esta pergunta da sessão. É
    -- diferida para permitir inserir a gravação e selecioná-la no mesmo commit.
    FOREIGN KEY (id, selected_recording_id)
        REFERENCES recordings(session_question_id, id)
        DEFERRABLE INITIALLY DEFERRED
);

CREATE TABLE recordings (
    id TEXT PRIMARY KEY NOT NULL,
    session_question_id TEXT NOT NULL REFERENCES session_questions(id),
    ordinal INTEGER NOT NULL CHECK (ordinal >= 1),
    state TEXT NOT NULL CHECK (
        state IN ('reserved', 'capturing', 'publish_pending', 'ready', 'interrupted')
    ),
    original_file TEXT NULL,
    normalized_file TEXT NULL,
    capture_json TEXT NULL CHECK (capture_json IS NULL OR json_valid(capture_json)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (session_question_id, ordinal),
    -- Uma gravação só fica pronta depois de o original estar publicado.
    CHECK (state <> 'ready' OR original_file IS NOT NULL)
);

-- Índices auxiliares das chaves estrangeiras compostas.
CREATE UNIQUE INDEX recordings_identity ON recordings(session_question_id, id);

CREATE TABLE transcripts (
    id TEXT PRIMARY KEY NOT NULL,
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    status TEXT NOT NULL CHECK (status IN ('completed', 'no_speech')),
    text TEXT NOT NULL,
    segments_json TEXT NOT NULL CHECK (json_valid(segments_json)),
    model TEXT NOT NULL,
    language TEXT NOT NULL,
    config_json TEXT NOT NULL CHECK (json_valid(config_json)),
    load_ms INTEGER NOT NULL CHECK (load_ms >= 0),
    processing_ms INTEGER NOT NULL CHECK (processing_ms >= 0),
    created_at TEXT NOT NULL
);

CREATE UNIQUE INDEX transcripts_identity ON transcripts(recording_id, id);

CREATE TABLE reviews (
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    version INTEGER NOT NULL CHECK (version >= 1),
    source_transcript_id TEXT NULL,
    origin TEXT NOT NULL CHECK (origin IN ('automatic', 'manual_explicit')),
    text TEXT NOT NULL,
    saved_at TEXT NOT NULL,
    PRIMARY KEY (recording_id, version),
    -- A transcrição de origem, quando existe, pertence à mesma gravação.
    FOREIGN KEY (recording_id, source_transcript_id)
        REFERENCES transcripts(recording_id, id)
);

CREATE TABLE confirmations (
    id TEXT PRIMARY KEY NOT NULL,
    session_question_id TEXT NOT NULL UNIQUE,
    recording_id TEXT NOT NULL,
    review_version INTEGER NOT NULL CHECK (review_version >= 1),
    source_transcript_id TEXT NULL,
    origin TEXT NOT NULL CHECK (origin IN ('automatic', 'manual_explicit')),
    text TEXT NOT NULL CHECK (length(trim(text)) > 0),
    confirmed_at TEXT NOT NULL,
    -- A gravação confirmada pertence à pergunta da sessão...
    FOREIGN KEY (session_question_id, recording_id)
        REFERENCES recordings(session_question_id, id),
    -- ...e a versão da revisão e a transcrição de origem são as dessa gravação.
    FOREIGN KEY (recording_id, review_version)
        REFERENCES reviews(recording_id, version),
    FOREIGN KEY (recording_id, source_transcript_id)
        REFERENCES transcripts(recording_id, id)
);

CREATE TABLE operations (
    id TEXT PRIMARY KEY NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('transcribe', 'synthesize')),
    state TEXT NOT NULL CHECK (
        state IN ('running', 'completed', 'failed', 'interrupted', 'no_speech')
    ),
    session_id TEXT NULL REFERENCES sessions(id),
    session_question_id TEXT NULL REFERENCES session_questions(id),
    recording_id TEXT NULL REFERENCES recordings(id),
    worker_generation INTEGER NULL CHECK (worker_generation IS NULL OR worker_generation > 0),
    source_config_json TEXT NULL CHECK (
        source_config_json IS NULL OR json_valid(source_config_json)
    ),
    error_json TEXT NULL CHECK (error_json IS NULL OR json_valid(error_json)),
    created_at TEXT NOT NULL,
    finished_at TEXT NULL,
    -- Um resultado só encerra o seu pedido: só a operação em curso não tem fim.
    CHECK ((state = 'running') = (finished_at IS NULL))
);

CREATE TABLE command_receipts (
    request_id TEXT PRIMARY KEY NOT NULL,
    kind TEXT NOT NULL CHECK (
        kind IN ('start_session', 'save_review', 'confirm_answer', 'advance_session')
    ),
    payload_hash TEXT NOT NULL CHECK (length(payload_hash) = 64),
    session_id TEXT NULL REFERENCES sessions(id),
    entity_id TEXT NULL,
    result_version INTEGER NULL CHECK (result_version IS NULL OR result_version >= 0)
);

-- Transcrições automáticas: cada resultado é imutável e identificado.
CREATE TRIGGER transcripts_immutable_update
BEFORE UPDATE ON transcripts
BEGIN
    SELECT RAISE(ABORT, 'A transcrição automática é imutável e não pode ser alterada.');
END;

CREATE TRIGGER transcripts_immutable_delete
BEFORE DELETE ON transcripts
BEGIN
    SELECT RAISE(ABORT, 'A transcrição automática é imutável e não pode ser eliminada.');
END;

-- Revisões: acrescentam-se versões, nunca se reescrevem nem se eliminam.
CREATE TRIGGER reviews_append_only_update
BEFORE UPDATE ON reviews
BEGIN
    SELECT RAISE(ABORT, 'Uma versão de revisão é imutável e não pode ser alterada.');
END;

CREATE TRIGGER reviews_append_only_delete
BEFORE DELETE ON reviews
BEGIN
    SELECT RAISE(ABORT, 'Uma versão de revisão é imutável e não pode ser eliminada.');
END;

-- Confirmações: o texto confirmado e as referências ficam fixos.
CREATE TRIGGER confirmations_immutable_update
BEFORE UPDATE ON confirmations
BEGIN
    SELECT RAISE(ABORT, 'A resposta confirmada é imutável e não pode ser alterada.');
END;

CREATE TRIGGER confirmations_immutable_delete
BEFORE DELETE ON confirmations
BEGIN
    SELECT RAISE(ABORT, 'A resposta confirmada é imutável e não pode ser eliminada.');
END;

-- Perguntas da sessão: a cópia do conteúdo à data de início nunca muda; só a
-- seleção da gravação e a sua versão podem evoluir antes da confirmação.
CREATE TRIGGER session_questions_content_immutable
BEFORE UPDATE OF
    id,
    session_id,
    source_question_id,
    source_version,
    position,
    text,
    category,
    competencies_json,
    criteria_json
ON session_questions
BEGIN
    SELECT RAISE(ABORT, 'O conteúdo copiado da pergunta é imutável e não pode ser alterado.');
END;

CREATE TRIGGER session_questions_immutable_delete
BEFORE DELETE ON session_questions
BEGIN
    SELECT RAISE(ABORT, 'A pergunta da sessão é imutável e não pode ser eliminada.');
END;

CREATE TRIGGER recordings_immutable_delete
BEFORE DELETE ON recordings
BEGIN
    SELECT RAISE(ABORT, 'Uma gravação guardada é imutável e não pode ser eliminada.');
END;

CREATE TRIGGER sessions_immutable_delete
BEFORE DELETE ON sessions
BEGIN
    SELECT RAISE(ABORT, 'Uma sessão é imutável e não pode ser eliminada nesta fase.');
END;

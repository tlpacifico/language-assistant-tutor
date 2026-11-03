-- Migração 003: práticas, histórias, reformulação e propriedade das respostas (fase 4).
--
-- A base evolui de v2 para v3 preservando todos os dados, recibos, referências,
-- índices e triggers. As únicas tabelas reconstruídas são `answer_slots` (papel
-- alargado a `attempt` e nova coluna `practice_id`) e `ai_requests` (origem,
-- variante e geração). As restantes tabelas v2 ficam intactas. As novas tabelas
-- desta fase (práticas, histórias, reformulações, avaliações de prática e
-- preferências) são criadas vazias; as tarefas seguintes escrevem nelas.
--
-- A ligação abre com `foreign_keys=OFF` antes da transação e volta a ativá-lo
-- em todos os caminhos; `foreign_key_check` é verificada antes do commit. Os
-- backups usam `CREATE TABLE ... AS SELECT` (sem constraints) para não propagar
-- as chaves estrangeiras das outras tabelas, que continuam a apontar, por nome,
-- para as tabelas recriadas.

-- ============================================================
-- Práticas: identidade, estado e alvo próprios.
-- ============================================================

CREATE TABLE practices (
    id TEXT PRIMARY KEY NOT NULL,
    session_question_id TEXT NOT NULL REFERENCES session_questions(id),
    source_session_id TEXT NOT NULL REFERENCES sessions(id),
    ordinal INTEGER NOT NULL CHECK (ordinal >= 1),
    previous_practice_id TEXT REFERENCES practices(id),
    answer_id TEXT UNIQUE NOT NULL REFERENCES answer_slots(id) DEFERRABLE INITIALLY DEFERRED,
    status TEXT NOT NULL CHECK (status IN ('preparing', 'answering', 'confirmed', 'abandoned')),
    version INTEGER NOT NULL CHECK (version >= 1),
    created_at TEXT NOT NULL,
    confirmed_at TEXT,
    abandoned_at TEXT,
    UNIQUE (session_question_id, ordinal),
    CHECK ((status = 'confirmed') = (confirmed_at IS NOT NULL)),
    CHECK ((status = 'abandoned') = (abandoned_at IS NOT NULL))
);

-- No máximo uma prática pendente de cada vez.
CREATE UNIQUE INDEX one_pending_practice
    ON practices((1))
    WHERE status IN ('preparing', 'answering');

-- A prática só nasce de uma pergunta confirmada de uma sessão concluída.
CREATE TRIGGER practice_source_required
BEFORE INSERT ON practices
WHEN NOT EXISTS (
        SELECT 1 FROM sessions s
         WHERE s.id = NEW.source_session_id AND s.status = 'completed'
    )
  OR NOT EXISTS (
        SELECT 1 FROM session_questions q
         WHERE q.id = NEW.session_question_id
           AND q.session_id = NEW.source_session_id
    )
  OR NOT EXISTS (
        SELECT 1 FROM confirmations c
         WHERE c.session_question_id = NEW.session_question_id
    )
BEGIN
    SELECT RAISE(ABORT, 'Prática exige pergunta confirmada de sessão concluída');
END;

-- O predecessor, quando existe, é uma prática confirmada da mesma pergunta.
CREATE TRIGGER practice_predecessor
BEFORE INSERT ON practices
WHEN NEW.previous_practice_id IS NOT NULL
  AND NOT EXISTS (
        SELECT 1 FROM practices p
         WHERE p.id = NEW.previous_practice_id
           AND p.session_question_id = NEW.session_question_id
           AND p.status = 'confirmed'
    )
BEGIN
    SELECT RAISE(ABORT, 'A prática anterior tem de estar confirmada e ser da mesma pergunta');
END;

-- Identidade, proveniência, ordinal, predecessor e alvo são imutáveis.
CREATE TRIGGER practices_identity_immutable
BEFORE UPDATE OF id, session_question_id, source_session_id, ordinal,
                   previous_practice_id, answer_id
ON practices
BEGIN
    SELECT RAISE(ABORT, 'Identidade da prática é imutável');
END;

-- Só se abandona uma prática ainda pendente.
CREATE TRIGGER practices_abandon_pending
BEFORE UPDATE OF status
ON practices
WHEN NEW.status = 'abandoned' AND OLD.status NOT IN ('preparing', 'answering')
BEGIN
    SELECT RAISE(ABORT, 'Só se abandona uma prática pendente');
END;

-- Estados terminais não mudam.
CREATE TRIGGER practices_terminal_immutable
BEFORE UPDATE OF status
ON practices
WHEN OLD.status IN ('confirmed', 'abandoned')
BEGIN
    SELECT RAISE(ABORT, 'Uma prática confirmada ou abandonada não muda de estado');
END;

-- Exclusão entre prática pendente e entrevista em curso.
CREATE TRIGGER practice_blocks_active_session
BEFORE INSERT ON practices
WHEN NEW.status IN ('preparing', 'answering')
  AND EXISTS (SELECT 1 FROM sessions WHERE status = 'in_progress')
BEGIN
    SELECT RAISE(ABORT, 'Termine a entrevista em curso antes de praticar');
END;

CREATE TRIGGER active_session_blocks_practice
BEFORE INSERT ON sessions
WHEN NEW.status = 'in_progress'
  AND EXISTS (SELECT 1 FROM practices WHERE status IN ('preparing', 'answering'))
BEGIN
    SELECT RAISE(ABORT, 'Confirme ou abandone a prática pendente antes de iniciar uma entrevista');
END;

CREATE TRIGGER active_session_blocks_practice_update
BEFORE UPDATE OF status
ON sessions
WHEN NEW.status = 'in_progress'
  AND EXISTS (SELECT 1 FROM practices WHERE status IN ('preparing', 'answering'))
BEGIN
    SELECT RAISE(ABORT, 'Confirme ou abandone a prática pendente antes de iniciar uma entrevista');
END;

-- ============================================================
-- answer_slots: alargar o papel e acrescentar a prática.
-- ============================================================

CREATE TABLE answer_slots_backup AS SELECT * FROM answer_slots;
DROP TABLE answer_slots;

CREATE TABLE answer_slots (
    id TEXT PRIMARY KEY NOT NULL,
    session_question_id TEXT NOT NULL REFERENCES session_questions(id),
    session_id TEXT NOT NULL REFERENCES sessions(id),
    role TEXT NOT NULL CHECK (role IN ('initial', 'follow_up', 'attempt')),
    text TEXT NOT NULL CHECK (length(trim(text)) > 0),
    selected_recording_id TEXT,
    selection_version INTEGER NOT NULL DEFAULT 0 CHECK (selection_version >= 0),
    practice_id TEXT REFERENCES practices(id) DEFERRABLE INITIALLY DEFERRED,
    CHECK ((role = 'attempt') = (practice_id IS NOT NULL)),
    FOREIGN KEY (id, selected_recording_id)
        REFERENCES recordings(session_question_id, id)
        DEFERRABLE INITIALLY DEFERRED
);

INSERT INTO answer_slots
    (id, session_question_id, session_id, role, text, selected_recording_id,
     selection_version, practice_id)
SELECT id, session_question_id, session_id, role, text, selected_recording_id,
       selection_version, NULL
  FROM answer_slots_backup;

DROP TABLE answer_slots_backup;

-- Papéis de entrevista: no máximo um inicial e um seguimento por pergunta.
CREATE UNIQUE INDEX answer_slots_unique_initial_follow_up
    ON answer_slots(session_question_id, role)
    WHERE role IN ('initial', 'follow_up');

-- Uma prática tem exatamente um slot de tentativa.
CREATE UNIQUE INDEX answer_slots_unique_attempt
    ON answer_slots(practice_id)
    WHERE role = 'attempt';

CREATE TRIGGER answer_slots_immutable
BEFORE UPDATE OF id, session_question_id, session_id, role, text, practice_id
ON answer_slots
BEGIN
    SELECT RAISE(ABORT, 'Resposta imutável');
END;

CREATE TRIGGER answer_slots_no_delete
BEFORE DELETE ON answer_slots
BEGIN
    SELECT RAISE(ABORT, 'Resposta imutável');
END;

CREATE TRIGGER answer_slots_selection_locked
BEFORE UPDATE OF selected_recording_id, selection_version
ON answer_slots
WHEN EXISTS (SELECT 1 FROM confirmations c WHERE c.session_question_id = OLD.id)
BEGIN
    SELECT RAISE(ABORT, 'Resposta confirmada');
END;

CREATE TRIGGER sync_initial_selection
AFTER UPDATE OF selected_recording_id, selection_version
ON answer_slots
WHEN NEW.role = 'initial'
BEGIN
    UPDATE session_questions
       SET selected_recording_id = NEW.selected_recording_id,
           selection_version = NEW.selection_version
     WHERE id = NEW.session_question_id;
END;

CREATE TRIGGER slot_context
BEFORE INSERT ON answer_slots
WHEN NOT EXISTS (
        SELECT 1 FROM session_questions q
         WHERE q.id = NEW.session_question_id
           AND q.session_id = NEW.session_id
    )
  OR (NEW.role = 'initial' AND NEW.id <> NEW.session_question_id)
  OR (NEW.role = 'follow_up' AND NEW.id = NEW.session_question_id)
  OR (NEW.role = 'attempt' AND NEW.practice_id IS NULL)
  OR (NEW.role = 'attempt' AND NEW.id = NEW.session_question_id)
  OR (NEW.role = 'attempt' AND NOT EXISTS (
        SELECT 1 FROM practices p
         WHERE p.id = NEW.practice_id
           AND p.session_question_id = NEW.session_question_id
           AND p.source_session_id = NEW.session_id
    ))
BEGIN
    SELECT RAISE(ABORT, 'Contexto da resposta inválido');
END;

-- ============================================================
-- ai_requests: origem, variante e geração.
-- ============================================================

CREATE TABLE ai_requests_backup AS SELECT * FROM ai_requests;
DROP TABLE ai_requests;

CREATE TABLE ai_requests (
    id TEXT PRIMARY KEY NOT NULL,
    purpose TEXT NOT NULL CHECK (
        purpose IN ('follow_up', 'feedback', 'practice_feedback', 'rewrite')
    ),
    question_id TEXT NOT NULL REFERENCES session_questions(id),
    session_id TEXT NOT NULL REFERENCES sessions(id),
    practice_id TEXT REFERENCES practices(id),
    origin_key TEXT NOT NULL,
    variant_key TEXT NOT NULL,
    sources_json TEXT NOT NULL CHECK (json_valid(sources_json)),
    generation INTEGER NOT NULL DEFAULT 1 CHECK (generation >= 1),
    prompt_version TEXT NOT NULL DEFAULT 'v1',
    rubric_version TEXT NOT NULL DEFAULT 'star-text-v1',
    schema_version TEXT NOT NULL DEFAULT 'ai-v3-1',
    CHECK (origin_key IN ('original:' || question_id, 'practice:' || practice_id)),
    UNIQUE (purpose, origin_key, variant_key)
);

-- Pedidos antigos são todos da origem original com a variante por omissão.
INSERT INTO ai_requests
    (id, purpose, question_id, session_id, practice_id, origin_key, variant_key,
     sources_json, generation, prompt_version, rubric_version, schema_version)
SELECT id, purpose, question_id, session_id, NULL,
       'original:' || question_id, 'default',
       sources_json, 1, prompt_version, rubric_version, schema_version
  FROM ai_requests_backup;

DROP TABLE ai_requests_backup;

CREATE TRIGGER ai_requests_origin_coherence
BEFORE INSERT ON ai_requests
WHEN (NEW.practice_id IS NULL AND NEW.origin_key <> 'original:' || NEW.question_id)
  OR (NEW.practice_id IS NOT NULL AND NEW.origin_key <> 'practice:' || NEW.practice_id)
  OR (NEW.practice_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM practices p
         WHERE p.id = NEW.practice_id
           AND p.session_question_id = NEW.question_id
           AND p.source_session_id = NEW.session_id
    ))
BEGIN
    SELECT RAISE(ABORT, 'Origem do pedido de IA inválida');
END;

CREATE TRIGGER ai_requests_immutable_after_reserve
BEFORE UPDATE OF purpose, question_id, session_id, practice_id, origin_key,
                   variant_key, sources_json, prompt_version, rubric_version,
                   schema_version
ON ai_requests
WHEN EXISTS (SELECT 1 FROM ai_attempts WHERE request_id = OLD.id)
BEGIN
    SELECT RAISE(ABORT, 'Pedido de IA reservado é imutável');
END;

-- ============================================================
-- Histórias locais.
-- ============================================================

CREATE TABLE stories (
    id TEXT PRIMARY KEY NOT NULL,
    version INTEGER NOT NULL CHECK (version >= 1),
    title TEXT NOT NULL CHECK (length(trim(title)) > 0),
    situation TEXT NOT NULL DEFAULT '',
    task TEXT NOT NULL DEFAULT '',
    actions TEXT NOT NULL DEFAULT '',
    outcome TEXT NOT NULL DEFAULT '',
    competencies_json TEXT NOT NULL CHECK (json_valid(competencies_json)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE story_versions (
    story_id TEXT NOT NULL REFERENCES stories(id),
    version INTEGER NOT NULL CHECK (version >= 1),
    content_json TEXT NOT NULL CHECK (json_valid(content_json)),
    created_at TEXT NOT NULL,
    PRIMARY KEY (story_id, version)
);

CREATE TRIGGER story_versions_append_only
BEFORE UPDATE ON story_versions
BEGIN
    SELECT RAISE(ABORT, 'Uma versão de história é imutável');
END;

CREATE TRIGGER story_versions_no_delete
BEFORE DELETE ON story_versions
BEGIN
    SELECT RAISE(ABORT, 'Uma versão de história não pode ser eliminada');
END;

-- ============================================================
-- Preparação e apoio à captura.
-- ============================================================

CREATE TABLE preparations (
    answer_id TEXT PRIMARY KEY NOT NULL REFERENCES answer_slots(id),
    version INTEGER NOT NULL DEFAULT 0 CHECK (version >= 0),
    selected_story_id TEXT REFERENCES stories(id),
    selected_story_version INTEGER CHECK (
        selected_story_version IS NULL OR selected_story_version >= 1
    ),
    locked_capture_id TEXT REFERENCES recordings(id),
    lock_state TEXT NOT NULL CHECK (
        lock_state IN ('editable', 'pending_start', 'locked', 'start_uncertain')
    )
);

CREATE TABLE preparation_captures (
    recording_id TEXT PRIMARY KEY NOT NULL REFERENCES recordings(id),
    answer_id TEXT NOT NULL REFERENCES answer_slots(id),
    preparation_version INTEGER NOT NULL CHECK (preparation_version >= 0),
    snapshot_json TEXT CHECK (snapshot_json IS NULL OR json_valid(snapshot_json)),
    state TEXT NOT NULL CHECK (state IN ('candidate', 'started', 'failed', 'uncertain')),
    created_at TEXT NOT NULL
);

-- ============================================================
-- Avaliação de prática e família de rubrica.
-- ============================================================

CREATE TABLE practice_reports (
    practice_id TEXT PRIMARY KEY NOT NULL REFERENCES practices(id),
    network_attempt_id TEXT UNIQUE NOT NULL REFERENCES ai_attempts(id),
    rubric_version TEXT NOT NULL,
    prompt_version TEXT NOT NULL,
    schema_version TEXT NOT NULL,
    json TEXT NOT NULL CHECK (json_valid(json))
);

CREATE TRIGGER practice_reports_no_update
BEFORE UPDATE ON practice_reports
BEGIN
    SELECT RAISE(ABORT, 'Relatório de prática é imutável');
END;

CREATE TRIGGER practice_reports_no_delete
BEFORE DELETE ON practice_reports
BEGIN
    SELECT RAISE(ABORT, 'Relatório de prática não pode ser eliminado');
END;

CREATE TABLE evaluation_families (
    session_question_id TEXT PRIMARY KEY NOT NULL REFERENCES session_questions(id),
    rubric_version TEXT NOT NULL,
    schema_version TEXT NOT NULL,
    created_at TEXT NOT NULL
);

-- ============================================================
-- Reformulação e voz derivada.
-- ============================================================

CREATE TABLE rewrites (
    id TEXT PRIMARY KEY NOT NULL,
    request_id TEXT UNIQUE NOT NULL REFERENCES ai_requests(id),
    network_attempt_id TEXT UNIQUE NOT NULL REFERENCES ai_attempts(id),
    origin_key TEXT NOT NULL,
    level TEXT NOT NULL CHECK (level IN ('B1', 'B2', 'C1')),
    json TEXT NOT NULL CHECK (json_valid(json)),
    spoken_text TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TRIGGER rewrites_no_update
BEFORE UPDATE ON rewrites
BEGIN
    SELECT RAISE(ABORT, 'Reformulação é imutável');
END;

CREATE TRIGGER rewrites_no_delete
BEFORE DELETE ON rewrites
BEGIN
    SELECT RAISE(ABORT, 'Reformulação não pode ser eliminada');
END;

CREATE TABLE rewrite_audio (
    rewrite_id TEXT NOT NULL REFERENCES rewrites(id),
    config_hash TEXT NOT NULL,
    relative_file TEXT NOT NULL,
    text_hash TEXT NOT NULL,
    voice TEXT NOT NULL,
    speed REAL NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('pending', 'ready', 'failed')),
    PRIMARY KEY (rewrite_id, config_hash)
);

-- ============================================================
-- Preferência local de nível de reformulação.
-- ============================================================

CREATE TABLE practice_preferences (
    id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
    rewrite_level TEXT NOT NULL DEFAULT 'B1' CHECK (rewrite_level IN ('B1', 'B2', 'C1')),
    version INTEGER NOT NULL DEFAULT 1 CHECK (version >= 1),
    updated_at TEXT NOT NULL
);

# Fase 2 — Entrevista guiada Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans para execução direta, tarefa a tarefa. Usar superpowers:subagent-driven-development apenas se o utilizador escolher explicitamente delegação. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Permitir completar e recuperar entrevistas locais configuráveis, preservando perguntas, áudios, textos e confirmações num histórico persistente.

**Architecture:** React/TypeScript apresenta preparação, banco, entrevista e histórico; Rust mantém a autoridade sobre transições e persistência. SQLite guarda os dados das sessões novas, com transações curtas; os originais e derivados continuam em ficheiros. Adaptar o serviço existente e reutilizar a porta de áudio e o protocolo Python, sem criar outro coordenador de dispositivos.

**Tech Stack:** Tauri 2, React, TypeScript, Vite, Rust, rusqlite com SQLite incorporado, CPAL, Rodio, Hound, Rubato, atomicwrites e auxiliar Python faster-whisper/Kokoro já existentes.

**Spec:** [Fase 2 — Entrevista guiada](../specs/2026-11-02-phase-2-guided-interview-design.md). Ler também [AGENTS.md](../../../AGENTS.md), [stack](../../tech-stack.md), [fases](2026-11-01-star-interview-tutor-phases.md) e [validação da fase 1](../../phase-1/validation.md).

**Estado:** plano preparado a pedido do utilizador em 2026-11-02; nenhuma tarefa executada. O pedido de plano permite detalhar a spec anterior, sem significar implementação, validação manual da fase 1 ou conclusão da fase 2. Os comandos e os resultados esperados abaixo são instruções futuras, não resultados observados.

## Global Constraints

- «Uma sessão em curso de cada vez, com progresso e retoma após reinício.»
- «Confirmação explícita de uma versão imutável do texto.»
- «SQLite para os dados das novas sessões; áudio em ficheiros locais.»
- «Escolher inicialmente “Todas as categorias”»; quantidade inicial três, opção de uma e limite no total elegível.
- «Sortear perguntas elegíveis sem repetição.» Guardar seleção, ordem e cópias antes de apresentar a sessão.
- «Separar “Guardar revisão”, “Confirmar resposta” e “Próxima pergunta”.» Na última, usar «Terminar sessão».
- «Não reduzir o total nem repetir perguntas silenciosamente.»
- «Depois de confirmar, a resposta fica apenas de leitura.»
- «Não manter JSON e SQLite como duas autoridades das mesmas sessões novas.»
- «Não mudar de sessão ou de página durante captura/finalização.» Consulta durante inferência não muda a identidade do trabalho.
- «Só navegar ou fechar se a versão mais recente estiver guardada ou tiver sido explicitamente descartada.»
- «Não truncar áudio ou texto silenciosamente.»
- Interface, mensagens e documentação em português de Portugal; perguntas e treino em inglês.
- Preservar fase 1, configuração, modelos, originais, resultados automáticos e revisões. Não confirmar dados antigos retroativamente.
- Sem API, seguimentos, feedback, histórias, comparação de tentativas, eliminação de sessões ou instalador autónomo.
- Não alterar motores, voz, formatos ou prazos de áudio como efeito lateral desta fase.

## Review Focus

1. Disco cheio ou base bloqueada depois de finalizar WAV: preservar o ficheiro e repetir a publicação após reinício, sem nova captura nem duplicação — tarefa 4.
2. Edição durante «Guardar e sair» e confirmação concorrente com nova transcrição: não fechar com texto pendente nem confirmar outra versão — tarefas 5 e 8.
3. Duplo clique, comando repetido após reinício e pedido antigo depois de outra sessão: não criar sessões, confirmações ou avanços extra — tarefas 3 e 5.
4. Interrupção antiga com `capture: None`, timeout e nova captura: conservar a identidade e o botão de terminar da nova captura — tarefa 6.
5. Migração abortada, versão futura ou treino antigo corrompido: não recriar dados, escrever formato desconhecido ou esconder os restantes itens válidos — tarefas 1 e 9.

---

## 1. Base observada e escolhas técnicas

### O que já existe

- `src-tauri/src/service.rs`: `AppService`, coordenador `transitions`, `spawn_blocking`, supervisão de áudio/auxiliar, cache e publicação recuperável em memória.
- `src-tauri/src/domain.rs`: `AppState`, `AppSnapshot`, `Attempt`, `Transcript`, `Review`, `Action` e erros; a pergunta ainda é fixa.
- `src-tauri/src/storage.rs`: `AttemptStore` com JSON atómico em `phase1/`; `load` e leitura de manifestos são privados.
- `src-tauri/src/audio/mod.rs`: uma thread proprietária dos dispositivos; `CaptureInterrupted` já transporta `recording_id`. Preservar essa correção.
- `src-tauri/src/worker/`: protocolo v1 para saúde, síntese, transcrição e encerramento; não precisa de receber conceitos de sessão.
- `src/features/practice/`: página e hook centrados numa resposta. `saveReview()` já distingue `saved`, `pending` e `failed`; preservar a proteção contra fecho com edições recentes.
- `src/App.tsx`: monta apenas `Practice`; `src/lib/tauri.ts` concentra os comandos.
- Testes Rust em `src-tauri/tests/`, Vitest e Testing Library no frontend. `scripts/verify.ps1` executa oito checks e inclui todas as suítes Rust/React descobertas.

### Dependência e acesso aos dados

Adicionar apenas `rusqlite = { version = "0.41.1", features = ["bundled"] }`
ao Cargo e guardar o lockfile durante a tarefa 1. É uma referência consultada
para este plano, ainda sem compilação neste projeto. Se a resolução falhar,
registar o motivo e usar uma versão compatível da mesma biblioteca; não mudar
a stack. Não instalar dependências ao preparar o plano.

Usar uma ligação `rusqlite::Connection` protegida por mutex dentro de `Database`;
todas as chamadas síncronas correm em `spawn_blocking`. A ligação nunca atravessa
um `await` com um guard adquirido. Não acrescentar ORM, pool, plugin SQL no
frontend ou servidor. Seleção por `ORDER BY random() LIMIT ?` dentro da
transação de criação evita outra dependência para sortear 20 perguntas.

Na abertura: verificar `user_version` antes de qualquer alteração persistente;
recusar versão superior a 1. Ativar `foreign_keys=ON`, `busy_timeout=3000` ms e
`synchronous=FULL`; manter journal `DELETE` nesta fase. Migrar versão 0→1 e
inserir o banco inicial numa transação; atualizar `user_version` apenas no fim.
Uma exceção implica rollback. Não recuperar erros por apagar/recriar a base.

### Ficheiros locais

```text
app_data_dir()/
  tutor.sqlite3
  sessions/<session-id>/<session-question-id>/<recording-id>/
    original.partial.wav # captura ainda não publicada; nome já usado por Capture
    original.wav          # WAV finalizado; nunca sobrescrito
    normalized.wav        # derivado, quando necessário
  phase1/                # conservar; apenas leitura na aplicação nova
    attempts/...
    question-cache/...
```

Conservar a configuração em `app_config_dir()`/`LANGUAGE_TUTOR_CONFIG` e os
modelos na localização existente. A cache de síntese pode continuar em
`phase1/question-cache/`, porque é partilhada e a chave já inclui texto/voz/
velocidade. Não converter nem mover treinos anteriores.

## 2. Ficheiros e responsabilidades

| Ação | Caminho | Responsabilidade |
| --- | --- | --- |
| Criar | `src-tauri/src/interview/mod.rs`, `types.rs` | Tipos da fase 2, validação e regras puras de transição. |
| Criar | `src-tauri/src/db/mod.rs`, `migrations/001_guided_interview.sql` | Ligação, migrações e execução síncrona protegida. |
| Criar | `src-tauri/src/db/questions.rs`, `sessions.rs`, `answers.rs` | Banco, criação/consulta de sessões, gravações/revisões/confirmações. |
| Criar | `src-tauri/src/db/seed-v1.json` | As 20 perguntas e os critérios da spec, com UUIDs estáveis. |
| Criar | `src-tauri/src/artifacts.rs` | Caminhos controlados, reserva, validação WAV e reconciliação. |
| Criar | `src-tauri/src/service/interview.rs` | Métodos de sessão do mesmo `AppService`, com o coordenador existente. |
| Criar | `src-tauri/src/legacy.rs` | Fachada de consulta, sem escrita, sobre `AttemptStore`. |
| Modificar | `src-tauri/src/domain.rs`, `service.rs`, `commands.rs`, `lib.rs` | Reutilizar tipos de áudio; integrar o novo estado, SQLite, comandos e arranque recuperável. |
| Modificar | `src-tauri/src/storage.rs` | Expor leitura por ID e enumeração, sem mudar o formato antigo. |
| Modificar | `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock` | Única dependência nova e resolução real. |
| Criar | `contracts/interview-v2.example.json` | Exemplo comum Rust/TypeScript, com confirmação e operação pendente. |
| Criar | `src/lib/interview-contracts.ts`, `interview-client.ts` | DTOs e cliente IPC injetável da fase 2. |
| Criar | `src/features/sessions/SessionSetup.tsx`, `SessionSetup.test.tsx` | Preparar, iniciar e retomar. |
| Criar | `src/features/questions/QuestionBank.tsx`, `QuestionBank.test.tsx` | Listar, adicionar e editar perguntas. |
| Criar | `src/features/interview/Interview.tsx`, `useInterview.ts`, `Interview.test.tsx` | Captura, revisão, confirmação e avanço. |
| Criar | `src/features/history/History.tsx`, `History.test.tsx` | Sessões, detalhes e treinos antigos. |
| Criar | `src/features/navigation/NavigationGuard.tsx`, `NavigationGuard.test.tsx` | Decisão partilhada guardar/descartar/cancelar, incluindo fecho. |
| Modificar | `src/App.tsx`, `src/features/practice/practice.css` | Navegação sem router novo e reutilização dos estilos existentes. |
| Criar | `src/App.test.tsx` | Percurso integrado com cliente controlado. |
| Criar | `src-tauri/tests/interview_contracts.rs`, `database.rs`, `questions.rs`, `sessions.rs`, `answers.rs`, `artifacts.rs`, `legacy.rs` | Contratos, transações e falhas com diretórios temporários. |
| Modificar | `src-tauri/tests/service.rs`, `src-tauri/tests/domain.rs` | Adaptar o harness ao novo armazenamento sem retirar regressões. |
| Modificar | `README.md`, `AGENTS.md`, `docs/tech-stack.md`, plano das fases | Estado real, execução e decisões de persistência. |
| Criar na execução | `docs/phase-2/setup.md`, `docs/phase-2/validation.md` | Comandos e evidências reais, matriz F2-01 a F2-16. |

Não copiar o serviço inteiro. `service/interview.rs` é um módulo filho de
`service.rs` com `impl AppService`, partilhando `transitions`; métodos públicos
adquirem o coordenador uma vez e chamam auxiliares privados sem o readquirir.
Não manter dois serviços com acesso independente ao mesmo microfone.

## 3. Contratos e modelo físico

### Convenções e tipos

Manter JSON camelCase, enums snake_case, UUIDs em texto, datas UTC RFC3339 e
durações em milissegundos. `type Id = String`, `type Version = u64` e
`type Result<T> = std::result::Result<T, AppError>` nas assinaturas deste plano.
Os DTOs têm equivalentes TypeScript com os mesmos nomes.

| Tipo | Campos e valores |
| --- | --- |
| `Category` | `conflict_collaboration`, `leadership_initiative`, `failure_learning`, `problem_solving`, `priorities_deadlines`, `change_adaptation`; etiquetas portuguesas da spec. |
| `QuestionInput` | `text`, `category`, `competencies: Vec<String>`, `criteria: Vec<String>`; critérios específicos editáveis. |
| `Question` | `id`, campos de `QuestionInput`, `version`, `createdAt`, `updatedAt`. |
| `StartSession` | `requestId`, `category: Option<Category>` (`null` = todas), `count: u32`. |
| `SessionSummary` | `id`, `category`, `count`, `confirmedCount`, `position` (1-based), `status: in_progress/completed`, `mode: local_v2`, `version`, `createdAt`, `completedAt?`. |
| `SessionQuestion` | `id`, `sessionId`, `sourceQuestionId`, `sourceVersion`, `position`, `text`, `category`, `competencies`, `criteria` efetivos, `selectedRecordingId?`, `selectionVersion`. |
| `AnswerTarget` | `sessionId`, `sessionQuestionId`, `recordingId`, `selectionVersion`; não aceitar caminhos do frontend. |
| `ReviewOrigin` | `automatic` ou `manual_explicit`. |
| `ReviewDraft` | `recordingId`, `sourceTranscriptId?`, `origin`, `text`, `version`, `savedAt`; versão zero representa revisão ainda não guardada. |
| `SaveReview` | `requestId`, `target: AnswerTarget`, `expectedReviewVersion`, `sourceTranscriptId?`, `origin`, `text`. |
| `ConfirmAnswer` | campos de `SaveReview`, mais `expectedSessionVersion`; confirma e guarda o texto num único commit. |
| `Confirmation` | `id`, `sessionQuestionId`, `recordingId`, `reviewVersion`, `sourceTranscriptId?`, `origin`, `text`, `confirmedAt`. |
| `AdvanceSession` | `requestId`, `sessionId`, `sessionQuestionId`, `expectedSessionVersion`. |
| `RecordingView` | `id`, `sessionQuestionId`, `ordinal`, `state`, `capture: CaptureMeta?`, `originalFile?`, `normalizedFile?`, `transcripts: Vec<Transcript>`, `review: Option<ReviewDraft>`, `warnings: Vec<AppError>`. |
| `QuestionDetail` | `question: SessionQuestion`, `recordings: Vec<RecordingView>`, `confirmation: Option<Confirmation>`. |
| `SessionDetail` | `summary: SessionSummary`, `questions: Vec<QuestionDetail>`. |
| `OperationTarget` | `sessionId`, `sessionQuestionId`, `recordingId?`, `selectionVersion`, `operationId`, `workerGeneration`; captura mantém também `recording_id` do evento nativo. |
| `InterviewSnapshot` | `revision` monotónica em memória, `database: ready/error/opening`, `activeSession: Option<SessionSummary>`, `current: Option<QuestionDetail>`, `availability`, `recordingId?`, `finalizing: bool`, `modelJob?` com alvo, `playback?`, `warnings`, `lastError?`. |
| `RecoveryReport` | `published: Vec<Id>`, `warnings: Vec<AppError>`, `interruptedOperations: Vec<Id>`. |

`Transcript`, `CaptureMeta`, `Availability` e `AppError` vêm de `domain.rs`.
Adicionar aos resultados guardados a configuração efetiva do reconhecimento,
sem alterar o protocolo Python: Rust já conhece os parâmetros enviados.
Manter os DTOs antigos para ler `phase1/`; a interface de sessão usa os novos.

Novos erros estáveis: `INVALID_INPUT`, `INSUFFICIENT_QUESTIONS`,
`ACTIVE_SESSION_EXISTS`, `ANSWER_CONFIRMED`, `ANSWER_REQUIRED`,
`DATABASE_UNAVAILABLE`, `DATABASE_VERSION_UNSUPPORTED`. Reutilizar `BUSY`,
`STALE_SOURCE`, `STORAGE_WRITE_FAILED` e os erros de áudio existentes.

### Esquema inicial

Arrays pequenos de competências, critérios e segmentos usam JSON TEXT validado
por serde na aplicação; relações e invariantes usam colunas e FKs. Não guardar
os áudios como BLOBs.

| Tabela | Colunas principais e regras |
| --- | --- |
| `content_seeds` | `version PRIMARY KEY`, `applied_at`; marca `questions-v1` na mesma transação do seed. |
| `questions` | `id PK`, `text`, `category`, `competencies_json`, `criteria_json`, `version`, datas; campos obrigatórios. |
| `sessions` | `id PK`, `category NULL`, `question_count CHECK > 0`, `position`, `status`, `mode CHECK = 'local_v2'`, `version`, datas; `position BETWEEN 1 AND question_count`. |
| `session_questions` | `id PK`, `session_id FK`, `source_question_id FK`, `source_version`, `position`, cópia de texto/categoria/arrays, `selected_recording_id NULL`, `selection_version`; `UNIQUE(session_id, position)`, `UNIQUE(session_id, source_question_id)`. |
| `recordings` | `id PK`, `session_question_id FK`, `ordinal`, `state: reserved/capturing/publish_pending/ready/interrupted`, caminhos relativos, metadados JSON, datas; `UNIQUE(session_question_id, ordinal)`. |
| `transcripts` | `id PK`, `recording_id FK`, texto/status/segmentos/modelo/língua/configuração/tempos/data, imutáveis. |
| `reviews` | `(recording_id, version) PK`, `source_transcript_id NULL`, `origin`, `text`, `saved_at`; acrescentar versões em vez de reescrever anteriores. |
| `confirmations` | `id PK`, `session_question_id UNIQUE FK`, `recording_id`, `review_version`, `source_transcript_id NULL`, `origin`, `text`, data; FK para a versão da revisão e coerência com a gravação da pergunta. |
| `operations` | `id PK`, tipo/estado, sessão/pergunta/gravação/generação, configuração de origem, erro e tempos; um resultado só encerra o seu pedido. |
| `command_receipts` | `request_id PK`, `kind`, `payload_hash`, `session_id`, `entity_id`, `result_version`; escrita na transação da mutação, sem duplicar textos completos. |

Criar `CREATE UNIQUE INDEX one_active_session ON sessions((1)) WHERE
status = 'in_progress'`. Adicionar FKs compostas para garantir que seleção,
confirmação e transcrição de origem pertencem à mesma pergunta/gravação; usar
índices únicos auxiliares `(session_question_id,id)` e `(recording_id,id)` onde
necessário. As FKs de seleção podem ser `DEFERRABLE INITIALLY DEFERRED` para
permitir inserir a gravação e alterar a seleção na mesma transação.

Triggers `BEFORE UPDATE/DELETE` impedem alterações de transcrições,
confirmações, versões de revisão e conteúdos copiados das perguntas. O campo
de seleção de `session_questions` pode mudar antes de confirmar. Não incluir
tabelas das fases futuras nem comandos de eliminação.

### Concorrência e repetição

Mutações recebem UUID `requestId` gerado no cliente e conservado numa repetição
do mesmo pedido. Verificar recibo antes de validar versões: mesmo ID e payload
devolvem o resultado existente sem executar outra mutação; payload diferente
devolve `STALE_SOURCE`. Guardar hash SHA-256 de serialização canónica de campos
do pedido, sem incluir `requestId` e sem depender da ordem de mapas.

`sessions.version` controla início/avanço/fecho e confirmação;
`selectionVersion` protege a seleção da gravação; a versão da revisão controla
edições. Atualizações usam condições de versão e verificam linhas afetadas.
Resultados de modelos não dependem da versão global da sessão, pois guardar
uma revisão durante inferência é permitido: validar o alvo, geração, operação,
seleção e ausência de confirmação. Um resultado tardio nunca troca seleção.

O replay de um início antigo devolve a sessão original, mesmo se concluída;
não cria uma entrevista nova. O replay de avanço não aplica outro avanço.
Não manter transações ou mutexes de estado durante I/O de áudio ou inferência.

## 4. Sequência de execução

Antes de implementar: consultar o estado Git e preservar as alterações já
existentes; usar o isolamento habitual da execução. Reconciliar os critérios
manuais pendentes da fase 1 com a evidência do utilizador. Este plano não os
marca como cumpridos nem exige repetir evidência já registada.

Os comandos seguintes partem da raiz. Em PowerShell, quando necessário:
`$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"`. Cada tarefa segue o ciclo
teste que falha → confirmação da falha pertinente → implementação mínima →
teste verde. Falhas por ambiente em falta não contam como teste de regressão.
Os commits indicados são pontos sugeridos para a execução; incluir apenas os
ficheiros da tarefa, sem adicionar alterações preexistentes indiscriminadamente.

### Tarefa 1 — Contratos, esquema e abertura recuperável de SQLite

**Ficheiros:** criar `interview/{mod.rs,types.rs}`, `db/mod.rs`, a migração,
`contracts/interview-v2.example.json`, `src/lib/interview-contracts.ts` e testes
`interview_contracts.rs`/`database.rs`; modificar `lib.rs`, Cargo e lock.
Os caminhos Rust desta secção são relativos a `src-tauri/src/`.

**Interfaces:**

- `Database::open(path: PathBuf) -> Result<Database>`; `Database: Clone + Send + Sync` por `Arc<Mutex<Connection>>` interno.
- `Database::with_connection<T>(&self, f: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T>`; síncrono, nunca chamado diretamente num comando async.
- Tipos da secção 3 e `validate_question(input: QuestionInput) -> Result<QuestionInput>`; `validate_count(count: u32, eligible: usize) -> Result<()>`.

- [ ] Escrever `fresh_database_has_version_one`, `failed_migration_rolls_back`, `future_version_is_not_modified`, `foreign_keys_reject_cross_recording_sources` e `v2_contract_matches_fixture`. Asserções: `user_version == 1`; erro injetado conserva esquema/dados/versão; versão 99 não cria tabelas nem seed; fonte de outra gravação é recusada; JSON Rust coincide com o exemplo.
- [ ] Executar `cargo test --manifest-path src-tauri/Cargo.toml --test database --test interview_contracts`; confirmar falha por contrato/esquema ausente.
- [ ] Adicionar rusqlite e implementar os tipos, esquema, triggers, restrições e transação de migração. Acrescentar teste SQL que tenta alterar texto confirmado e transcrição e recebe erro.
- [ ] Reexecutar o comando e `npm.cmd run typecheck`; exigir todos verdes e saída 0. Guardar o lock realmente resolvido.
- [ ] Commit sugerido: `feat: add guided interview contracts and sqlite schema`.

### Tarefa 2 — Banco inicial e edição persistente

**Ficheiros:** criar `db/questions.rs`, `db/seed-v1.json`, `src-tauri/tests/questions.rs`; completar validação e integração do seed em `db/mod.rs`.

**Interfaces:** métodos síncronos em `Database`:

- `list_questions(category: Option<Category>) -> Result<Vec<Question>>`.
- `create_question(input: QuestionInput) -> Result<Question>`.
- `update_question(id: &str, expected_version: Version, input: QuestionInput) -> Result<Question>`.
- `ensure_question_seed() -> Result<()>`; invocado dentro da preparação inicial, marca `questions-v1` atomicamente.

- [ ] Escrever `seed_is_once_and_preserves_edits`, `bank_covers_six_categories`, `question_trims_only_outer_whitespace`, `blank_fields_are_rejected`, `stale_edit_does_not_overwrite`. Asserções: 20 perguntas, distribuição 4/3/3/4/3/3, IDs repetíveis, edição preservada após reabertura, texto interior intacto e versão antiga recusada.
- [ ] Executar `cargo test --manifest-path src-tauri/Cargo.toml --test questions`; observar falha pertinente.
- [ ] Transcrever as 20 linhas da spec para o seed com UUIDs fixos. Guardar critérios específicos na pergunta; definir critérios STAR comuns versionados numa constante e copiá-los na criação de sessão. Validar categoria, arrays não vazios e elementos sem espaços exteriores. Não usar `INSERT OR REPLACE` no banco existente.
- [ ] Reexecutar a suíte; verificar também rollback do seed incompleto, sem marca de versão aplicada.
- [ ] Commit sugerido: `feat: persist the editable interview question bank`.

### Tarefa 3 — Criação atómica, seleção fixa e consulta de sessão

**Ficheiros:** criar `db/sessions.rs`, `src-tauri/tests/sessions.rs`; completar tipos de consulta.

**Interfaces:** métodos de `Database`:

- `start_session(request: StartSession) -> Result<SessionDetail>`.
- `active_session() -> Result<Option<SessionDetail>>`.
- `list_sessions() -> Result<Vec<SessionSummary>>`.
- `load_session(session_id: &str) -> Result<SessionDetail>`.

- [ ] Escrever `starts_one_three_and_available_count`, `insufficient_category_creates_nothing`, `snapshots_survive_bank_edits`, `repeated_start_returns_original_session`, `only_one_session_can_be_active`. Asserções: posições 1..N sem repetição; quantidade zero ou excessiva é recusada; texto/competências/critérios não mudam; mesmo recibo após reabrir não insere outra sessão; duas ligações concorrentes não ultrapassam o índice único.
- [ ] Executar `cargo test --manifest-path src-tauri/Cargo.toml --test sessions`; observar as falhas.
- [ ] Implementar `BEGIN IMMEDIATE`, validação, sorteio SQL, cópias completas com critérios STAR e inserção de recibo na mesma transação. Validar contagem real antes do commit. Não testar aleatoriedade com expectativas estatísticas frágeis; testar conjunto, unicidade e preservação da ordem guardada.
- [ ] Reexecutar a suíte e testar erro injetado após inserir a primeira pergunta: a sessão inteira desaparece por rollback.
- [ ] Commit sugerido: `feat: create recoverable sessions with fixed question snapshots`.

### Tarefa 4 — Originais, publicações interrompidas e transcrições

**Ficheiros:** criar `artifacts.rs`, `db/answers.rs`, `src-tauri/tests/artifacts.rs`; acrescentar casos em `answers.rs` de testes.

**Interfaces:**

- `ArtifactStore::new(root: PathBuf) -> ArtifactStore`.
- `ArtifactStore::reserve(session_id: &str, question_id: &str, recording_id: &str) -> Result<RecordingTarget>`; `RecordingTarget` é o tipo de áudio existente.
- `ArtifactStore::inspect(target: &RecordingTarget) -> Result<CapturedAudio>`; valida `original.wav` finalizado, não promove `original.partial.wav`.
- `Database::reserve_recording(question_id: &str, expected_selection: Version) -> Result<RecordingView>`; gera ID/ordinal antes de abrir o microfone.
- `Database::publish_recording(capture: CapturedAudio) -> Result<RecordingView>`; idempotente pelo ID reservado, liga o alvo já guardado em SQLite.
- `Database::append_transcript(target: OperationTarget, transcript: Transcript, configuration: serde_json::Value) -> Result<RecordingView>`.
- `Database::set_normalized(recording_id: &str, relative_path: &str) -> Result<()>`.
- `reconcile(database: &Database, artifacts: &ArtifactStore) -> Result<RecoveryReport>`.

- [ ] Escrever `finalized_audio_survives_database_failure`, `reopen_reconciles_once`, `part_file_is_not_a_saved_answer`, `older_publication_cannot_replace_newer_selection`, `missing_audio_keeps_text`, `new_capture_failure_keeps_previous_selection`, `transcript_append_does_not_replace_review`. Asserções: WAV existe após falha, publicação após reabrir cria um só registo pronto; `original.partial.wav` nunca é apresentado como resposta válida; original em falta mantém textos legíveis.
- [ ] Executar `cargo test --manifest-path src-tauri/Cargo.toml --test artifacts --test answers`; confirmar falha.
- [ ] Reservar uma linha antes da captura; o original finalizado é a evidência recuperável se a escrita seguinte falhar. Nunca depender apenas do atual `pending_publish` em memória. Reconciliação inspeciona reservas pendentes, recupera WAV fechado e marca `incomplete=true` se o fecho não tiver metadados confirmados. Ausência de original torna captura interrompida, sem inventar duração ou sucesso.
- [ ] Na publicação, mudar seleção apenas para ordinal superior e pergunta ainda não confirmada; incrementar `selectionVersion`. Manter gravações anteriores. Validar WAV com Hound e reutilizar normalização. Resolver caminhos só por IDs/raiz, recusando `..`, absolutos e ligações que escapem à pasta gerida. Acrescentar `paths_cannot_escape_artifact_root`.
- [ ] Guardar resultados automáticos e estado da operação atomicamente; nunca reescrever texto automático ou revisão. Reconciliação marca trabalhos antigos `interrupted`, preserva órfãos desconhecidos com aviso e não os elimina.
- [ ] Reexecutar as suítes com falha injetada no commit e base reaberta; exigir recuperação sem nova captura e sem transcrição duplicada.
- [ ] Commit sugerido: `feat: preserve recording artifacts across publication failures`.

### Tarefa 5 — Revisão versionada, confirmação e avanço idempotente

**Ficheiros:** completar `db/answers.rs`, `db/sessions.rs`, regras em `interview/mod.rs`; testes `answers.rs` e `sessions.rs`.

**Interfaces:** métodos de `Database`:

- `save_review(request: SaveReview) -> Result<ReviewDraft>`.
- `confirm_answer(request: ConfirmAnswer) -> Result<Confirmation>`.
- `advance_session(request: AdvanceSession) -> Result<SessionDetail>`; na última posição conclui a sessão.

- [ ] Escrever `save_does_not_confirm`, `confirmation_freezes_exact_seen_text`, `stale_review_cannot_confirm`, `manual_no_speech_keeps_origin`, `cannot_confirm_without_original`, `advance_requires_confirmation`, `replayed_advance_does_not_skip`, `last_advance_completes_only_full_session`. Asserções: revisão mantém texto exato, incluindo espaços; validar `trim()` apenas para recusar vazio; confirmação não altera posição; final tem N confirmações e `mode=local_v2`, sem feedback pendente.
- [ ] Executar `cargo test --manifest-path src-tauri/Cargo.toml --test answers --test sessions`; confirmar as falhas.
- [ ] Implementar revisão append-only e confirmação numa transação, com versão esperada, alvo selecionado, original finalizado presente e pertencente à pergunta. `manual_explicit` permite revisão sem resultado automático ou sobre `no_speech`; não fabrica transcrição. Confirmar insere a revisão exata se mudou e a confirmação, atualiza a versão da sessão e guarda recibo. Um retry igual consulta o recibo antes de recusar por já confirmado.
- [ ] Implementar avanço com posição/pergunta/versão esperadas e recibo. Na última pergunta verificar todas as confirmações dentro da transação. Nunca usar a conclusão de uma transcrição como ação de avanço.
- [ ] Acrescentar `confirmation_failure_rolls_back_review_and_receipt`, `same_request_with_other_text_is_rejected`, `late_transcript_cannot_change_confirmation`, `old_start_replay_after_completion_does_not_create_session`. Reabrir a base entre pedido e replay. Verificar também tentativa SQL de reescrever a confirmação.
- [ ] Reexecutar ambas as suítes; saída 0.
- [ ] Commit sugerido: `feat: confirm immutable answers and advance sessions safely`.

### Tarefa 6 — Integrar o serviço, comandos, eventos e arranque

**Ficheiros:** modificar `service.rs`, `commands.rs`, `lib.rs`, `domain.rs`, `src-tauri/tests/service.rs` e `domain.rs` de testes; criar `service/interview.rs` e `src/lib/interview-client.ts`.

**Interfaces:**

- Adaptar `AppService::new` a `Database` e `ArtifactStore` no lugar do `AttemptStore`; reter a porta de áudio, launcher, manifesto e geração existentes. Inicialização/recuperação em `spawn_blocking`, com snapshot `opening` e janela utilizável.
- `AppService::snapshot() -> InterviewSnapshot`; `subscribe` publica `interview://snapshot` com esse DTO.
- Métodos async de `AppService`: `start_session(StartSession)`, `save_review(SaveReview)`, `confirm_answer(ConfirmAnswer)`, `advance_session(AdvanceSession)`, `start_recording(session_question_id: Id, expected_selection: Version)`, `stop_recording()`, `retry_save_recording(recording_id: Id)`, `transcribe_response(target: AnswerTarget)`, todos `-> Result<InterviewSnapshot>`.
- Métodos async de leitura: `list_questions`, `create_question`, `update_question`, `list_sessions`, `load_session` com os tipos das tarefas anteriores. Leituras devolvem DTOs; mutações de perguntas devolvem `Question`.
- `play_audio(target: AudioSelection) -> Result<InterviewSnapshot>`, com `AudioSelection = Question { sessionId, sessionQuestionId } | Recording { recordingId } | Legacy { attemptId }`; resolver o caminho no núcleo, permitir leitura de histórico, recusar durante captura.
- Reutilizar `refresh_availability`, `stop_audio`, `restart_worker`, `prepare_close`; adicionar `retry_database() -> Result<InterviewSnapshot>` para repetir abertura/reconciliação sem reiniciar a janela.
- `InterviewClient`: métodos camelCase correspondentes e `subscribe(listener) -> Unsubscribe`; concentrar nomes de comandos e evento neste ficheiro. Comandos Tauri novos prefixados `interview_`, incluindo `interview_snapshot`, `interview_list_questions`, `interview_create_question`, `interview_update_question`, `interview_start_session`, `interview_list_sessions`, `interview_load_session`, `interview_start_recording`, `interview_stop_recording`, `interview_retry_save_recording`, `interview_transcribe_response`, `interview_save_review`, `interview_confirm_answer`, `interview_advance_session`, `interview_play_audio`, `interview_stop_audio`, `interview_refresh_availability`, `interview_restart_worker`, `interview_prepare_close`, `interview_retry_database`.

- [ ] Adaptar o harness existente para uma base temporária com seed e sessão iniciada explicitamente com a pergunta fixa dos testes. Manter as asserções de recuperação, timeout, cache, exclusão de reprodução e revisão; mudar apenas contratos e preparação necessários. Acrescentar `recording_requires_active_unconfirmed_question`, `confirmed_question_rejects_capture_and_transcription`, `database_error_keeps_window_recoverable`, `history_read_does_not_retarget_model_job`.
- [ ] Executar `cargo test --manifest-path src-tauri/Cargo.toml --test service --test domain`; confirmar falha dos novos contratos antes de implementar a integração.
- [ ] Ligar métodos da base em `spawn_blocking`, coordenados pelo `transitions` existente. Carregar a sessão ativa após commits; snapshots refletem apenas estado guardado. Separar revisão global do snapshot e versões persistentes. Não adquirir `Mutex<AppState>` enquanto se chama um auxiliar que o readquire para registar erro.
- [ ] Trocar pergunta constante pelo texto da cópia atual ao sintetizar. Ligar resultado do auxiliar a `OperationTarget` sem alterar o protocolo v1; o mapa de operações em Rust faz essa associação. Rever início automático de transcrição para usar apenas gravação publicada e pergunta não confirmada. Resultado cujo alvo já foi confirmado ou substituído termina a operação como interrompida/obsoleta e liberta o trabalho em memória, sem alterar textos ou seleção e sem deixar a interface ocupada.
- [ ] Preservar e executar `repeated_start_returns_busy_instead_of_blocking`, `late_interruption_from_an_older_capture_is_ignored`, `timed_out_stop_recovers_capture`. Acrescentar `late_worker_result_cannot_target_next_question` e `question_cache_distinguishes_session_texts`. Usar prazos curtos de teste para detetar deadlock sem deixar a suíte presa.
- [ ] Registar os comandos no `lib.rs`, gerir erro da base sem `expect` que encerre a aplicação e ligar o novo cliente. Testar serialização IPC contra o exemplo v2 e verificar erros em português. Encerrar a captura e o auxiliar no fecho normal; erro de publicação impede indicar que tudo ficou guardado.
- [ ] Executar as suítes anteriores, `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` e `npm.cmd run typecheck`; exigir saída 0.
- [ ] Commit sugerido: `feat: connect guided sessions to the existing audio service`.

### Tarefa 7 — Preparação, navegação e banco de perguntas na interface

**Ficheiros:** criar `SessionSetup.tsx`, `QuestionBank.tsx` e respetivos testes nos caminhos da secção 2; modificar `src/App.tsx` e CSS; criar `src/App.test.tsx` inicial.

**Interfaces:** `SessionSetup({ client, snapshot, onStarted })` e
`QuestionBank({ client })`; `client: InterviewClient`,
`snapshot: InterviewSnapshot`, `onStarted(snapshot: InterviewSnapshot): void`.
`App` recebe cliente injetável nos testes e mantém a subscrição durante toda a
navegação. Estado local `page: setup/bank/interview/history`, sem router ou
gestor global de estado adicional. A identidade ativa vem de Rust.

- [ ] Escrever `defaults_to_three_and_all_categories`, `insufficient_questions_require_explicit_adjustment`, `active_session_offers_resume`, `bank_edit_survives_reload`, `bank_rejects_empty_fields`. Asserções: uma categoria com duas perguntas continua com quantidade três e aviso; só inicia depois da alteração explícita; o banco envia competências/critérios e versão esperada.
- [ ] Executar `npm.cmd test -- src/features/sessions/SessionSetup.test.tsx src/features/questions/QuestionBank.test.tsx src/App.test.tsx`; confirmar falhas.
- [ ] Implementar preparação e formulário de banco com campos rotulados. Consultar total elegível pelo banco, revalidado no núcleo. Bloquear submissões repetidas e conservar o UUID ao repetir uma submissão falhada; nova intenção recebe novo UUID. Mostrar «Retomar sessão» em vez de outro início quando houver sessão em curso.
- [ ] Usar navegação comum e estado de disponibilidade/erro da base; `retryDatabase` permite recuperar. Durante captura/finalização, bloquear mudança de página. Durante modelo ocupado, permitir leitura sem iniciar outra operação incompatível.
- [ ] Reexecutar testes e `npm.cmd run typecheck`; exigir saída 0 e mensagens em português de Portugal.
- [ ] Commit sugerido: `feat: add session setup and editable question bank screens`.

### Tarefa 8 — Entrevista, confirmação e proteção de alterações

**Ficheiros:** criar os ficheiros de `features/interview/` e `features/navigation/` da secção 2; integrar em `App.tsx`. Reutilizar lógica visual/estilos da prática existente, sem montar dois clientes de áudio.

**Interfaces:**

- `useInterview(client: InterviewClient, snapshot: InterviewSnapshot)` expõe `review`, `setReviewText(text)`, `saveReview(): Promise<'saved'|'pending'|'failed'>`, `discardReview()`, `confirm(): Promise<boolean>`; refs mantêm versão e texto recentes.
- `Interview({ client, snapshot, onSnapshot })`, com `onSnapshot(snapshot: InterviewSnapshot): void`.
- `NavigationGuard` recebe callbacks de guardar/descartar, estado dirty/ocupado e intenção `navigate | record | close`; uma única decisão controla captura nova, mudança de página e fecho. Manter rascunhos por `recordingId` ao consultar histórico durante inferência.

- [ ] Escrever `shows_question_two_of_three`, `save_does_not_enable_next_until_confirmation`, `confirmation_locks_exact_editor_text`, `last_question_uses_finish_label`, `manual_revision_of_no_speech_is_explicit`, `prior_recordings_remain_readable`. Asserções: confirmar e avançar são dois pedidos distintos; botão final chama avanço da última pergunta; interface não mostra feedback nem reabre confirmação.
- [ ] Escrever no guard `save_and_leave_keeps_newer_edit`, `failed_save_does_not_destroy_window`, `cancel_keeps_editor`, `discard_requires_explicit_choice`, `capture_blocks_navigation`. Simular promessa de gravação pendente, editar de novo, resolver e verificar que nem navegação nem `destroy()` são chamados.
- [ ] Executar `npm.cmd test -- src/features/interview/Interview.test.tsx src/features/navigation/NavigationGuard.test.tsx`; confirmar falhas.
- [ ] Implementar cópia editável distinta do original, versões e origem manual explícita. Durante confirmação impedir edição/troca de gravação; o pedido inclui exatamente o texto apresentado, versões esperadas e um UUID estável. Falha mantém texto; confirmação recebida é só de leitura. «Próxima pergunta» só aparece habilitada com confirmação persistida.
- [ ] Integrar fecho Tauri num único listener no nível da aplicação: `prepareClose` finaliza captura, a decisão resolve revisões e só depois se chama `destroy()`. Uma confirmação ou gravação pendente não pode ser ultrapassada pelo fecho; permitir repetir depois do erro. Não retirar a proteção existente de `SaveOutcome` ao adaptar o hook.
- [ ] Acrescentar `old_snapshot_does_not_erase_draft`, `confirm_failure_preserves_text`, `double_next_click_sends_one_request`, `synthesis_failure_still_allows_recording`. Rejeitar snapshots com revisão monotónica inferior. Adaptar os testes válidos de `Practice.test.tsx` para a nova página antes de retirar o componente antigo da aplicação; conservar módulos antigos apenas se ainda servirem testes/consulta, sem UI duplicada.
- [ ] Reexecutar testes e `npm.cmd run typecheck`; saída 0.
- [ ] Commit sugerido: `feat: guide recording review confirmation and session progress`.

### Tarefa 9 — Histórico, retoma e consulta dos treinos da fase 1

**Ficheiros:** criar `legacy.rs`, `src-tauri/tests/legacy.rs`, `History.tsx` e `History.test.tsx`; modificar `storage.rs`, comandos, cliente, `App.tsx` e `App.test.tsx`.

**Interfaces:**

- Expor em `AttemptStore` `list_saved() -> Result<Vec<RestoreResult>>` e `load_saved(attempt_id: &str) -> Result<RestoreResult>`, reutilizando a leitura existente. Nunca escrever ao consultar; uma pasta inválida produz `RestoreResult { attempt: None, warnings }` e não aborta a enumeração.
- `LegacyStore::new(root: PathBuf) -> LegacyStore`; `list() -> Result<Vec<RestoreResult>>`, `load(id: &str) -> Result<RestoreResult>`; expor só leitura e resolução de original para reprodução. Sem referência ao escritor no fluxo runtime.
- Comandos `interview_list_legacy`, `interview_load_legacy`, cliente `listLegacy()/loadLegacy(id)` com tipos antigos importados de `contracts.ts`. Não reutilizar `AppSnapshot` antigo como se fosse uma sessão.
- `History({ client, onResume })`, com `onResume(sessionId: string): void`. Ler sessões por `loadSession`, não alterar a seleção corrente ao abrir detalhes.

- [ ] Escrever `legacy_list_ignores_last_pointer`, `corrupt_legacy_item_does_not_hide_valid_items`, `legacy_read_changes_no_bytes`, `legacy_rejects_path_escape`; testar manifests, revisões e originais com hashes antes/depois da leitura. Confirmar ausência de tabelas/sessões criadas para esses itens.
- [ ] Executar `cargo test --manifest-path src-tauri/Cargo.toml --test legacy --test storage`; confirmar falha dos métodos novos.
- [ ] Implementar enumeração por pastas, avisos por artefacto e leitura protegida sem mudar o formato JSON. Manter os testes antigos de armazenamento, incluindo escrita atómica, como regressão desse formato.
- [ ] Escrever `history_separates_raw_review_and_confirmed`, `resume_keeps_confirmed_question_awaiting_next`, `missing_audio_keeps_confirmed_text`, `legacy_has_no_confirmation_action`; executar `npm.cmd test -- src/features/history/History.test.tsx src/App.test.tsx` e observar falhas.
- [ ] Implementar listagem por data, estado e progresso; detalhes de todas as perguntas e gravações. A sessão concluída mostra resumo, não feedback pendente. Treinos anteriores têm área própria e apenas leitura/reprodução. Retoma obtém snapshot corrente sem avançar automaticamente.
- [ ] Completar teste integrado `three_question_session_can_resume_and_finish`: criar cliente controlado, responder/confirmar primeira, simular reabertura, avançar explicitamente, terminar restantes e consultar três confirmações. Nenhuma chamada de IA ou dispensa de seguimento existe no cliente.
- [ ] Reexecutar as suítes Rust/React indicadas e `npm.cmd run typecheck`; exigir saída 0.
- [ ] Commit sugerido: `feat: browse session history and preserve legacy practice recordings`.

### Tarefa 10 — Verificação do conjunto e evidência Windows

**Ficheiros:** `scripts/verify.ps1` apenas se os novos testes não forem já
descobertos; `README.md`, `AGENTS.md`, `docs/tech-stack.md`, plano das fases,
`docs/phase-2/setup.md` e `docs/phase-2/validation.md`.

**Interfaces:** manter os comandos de execução existentes. Não criar um novo
instalador ou promessa de execução sem Python; o alvo continua Windows em
desenvolvimento e release `--no-bundle`.

- [ ] Criar matriz F2-01 a F2-16 em `docs/phase-2/validation.md`, inicialmente pendente, com ligação às tarefas e testes da secção 5. Documentar configuração, diretórios, SQLite e retoma em `setup.md` sem copiar dados pessoais para Git.
- [ ] Executar `.\scripts\verify.ps1` e registar saída real: todos os checks têm de terminar com 0. Não copiar contagens da fase 1 como se fossem as desta execução. Resolver falhas pertinentes antes de repetir.
- [ ] Executar `.\.venv\Scripts\python.exe -m pytest audio-worker/tests/test_offline_models.py -m models -q`; registar resultado real, distinguindo `passed` de `skipped` e preservando configuração/modelos existentes.
- [ ] Executar `npm.cmd run tauri -- build --no-bundle` e testar o arranque/fecho do binário Windows. Registar que não é um instalador autónomo nem valida a distribuição da fase 5.
- [ ] Executar com o utilizador: sessão de três perguntas reais, uma com termos técnicos; audição de perguntas/originais; revisão e confirmação; sair e retomar após confirmação antes de avançar; terminar e abrir histórico. Registar duração dos áudios e tempos de carregamento/síntese/transcrição, incluindo perceção da espera.
- [ ] Validar uma sessão de uma pergunta e outra quantidade disponível, insuficiência numa categoria, edição no banco sem alterar cópia de sessão, percurso offline, transcrição indisponível com revisão explícita, erro de escrita e recuperação. Quando se usarem falhas simuladas, identificar a simulação e não a apresentar como hardware real.
- [ ] Atualizar documentos com o que passou e o que falta. Marcar a fase 2 concluída apenas com critérios demonstrados e validação manual; manter a fase 1 com o estado que a sua própria evidência sustenta. Não iniciar fase 3.
- [ ] Commit sugerido: `docs: record guided interview setup and validation evidence`.

## 5. Cobertura dos critérios e revisão do plano

| Critério | Tarefas | Evidência principal prevista |
| --- | --- | --- |
| F2-01 | 1, 2 | Seed transacional, 20 perguntas, seis categorias, edições preservadas. |
| F2-02 | 2, 7 | Validação, versões e formulário, reabertura da base. |
| F2-03 | 3, 7, 10 | Quantidades 1/3/outra, insuficiência e ajuste explícito. |
| F2-04 | 3 | Seleção sem repetição, ordem e cópias persistidas. |
| F2-05 | 6, 8, 10 | Percurso de três perguntas com áudio real e alternativa sem síntese. |
| F2-06 | 1, 4, 5, 9 | Relações, imutabilidade e apresentação separada das versões. |
| F2-07 | 5, 8 | Confirmação exata, controlo de versões, rollback e bloqueio do editor. |
| F2-08 | 5, 8 | Pré-condições, recibos, versão da sessão e botão final. |
| F2-09 | 4, 6, 8 | Capturas independentes, seleção só depois de publicação, confirmação bloqueia reabertura. |
| F2-10 | 8 | Guard comum e testes de edição durante escrita/fecho. |
| F2-11 | 4, 5, 6, 9 | Reabertura real da base em testes e retoma na janela. |
| F2-12 | 4 | Falhas entre WAV e SQLite, reconciliação e ficheiros em falta. |
| F2-13 | 5, 6 | Repetição de comandos, timeouts, interrupção antiga e alvos de operações. |
| F2-14 | 9, 10 | Histórico, reprodução e hashes inalterados dos treinos antigos. |
| F2-15 | 1, 3 | Rollback de migração, versão futura e índice de sessão única. |
| F2-16 | 6, 9, 10 | Sem integração externa no código/cliente e percurso completo offline na janela. |

Revisão documental deste plano: todos os critérios têm tarefas e verificações;
as cinco condições de risco têm testes atribuídos; SQLite e ficheiros têm
recuperação separada; o protocolo Python permanece v1; nenhum passo depende de
IA ou entidades futuras. As assinaturas e o exemplo v2 devem permanecer
coerentes em Rust e TypeScript durante a execução. Estes são compromissos do
plano, não afirmações de testes já executados.

## 6. Referências técnicas consultadas para o plano

- [rusqlite — repositório oficial](https://github.com/rusqlite/rusqlite): acesso direto a SQLite e funcionalidade `bundled`.
- [rusqlite — documentação de Connection](https://docs.rs/rusqlite/latest/rusqlite/struct.Connection.html): referência consultada identificava 0.41.1; a compilação será verificada na tarefa 1.
- [SQLite — PRAGMA](https://www.sqlite.org/pragma.html): versão da aplicação, chaves estrangeiras, journal e timeout.
- [SQLite — índices parciais](https://www.sqlite.org/partialindex.html): restrição de uma única sessão em curso.

Não foram instalados pacotes, modificados ficheiros de produto, executados
builds ou ensaiados comandos de teste para elaborar este plano.

# Fase 3 — Plano de implementação do tutor com IA

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Objetivo:** acrescentar seguimentos contextuais e feedback fundamentado às entrevistas, com envio controlado, consumo observável e recuperação.

**Arquitetura:** Rust é autoridade das sessões, alvos, pedidos e evidências. Uma resposta inicial e o seu seguimento usam o mesmo percurso de áudio através de alvos explícitos; SQLite preserva todos os textos e tentativas. O HTTP funciona fora dos bloqueios do serviço e o frontend apenas apresenta estados e intenções.

**Stack:** React/TypeScript, Tauri 2/Rust, SQLite/rusqlite, auxiliar Python existente; acrescentar `reqwest = 0.13.5` (JSON e TLS Rustls) e `keyring = 3.6.3` com `windows-native`, mantendo Cargo.lock.

**Spec:** [Fase 3 — Tutor com IA](../specs/2026-11-02-phase-3-ai-tutor-design.md).

Data: 2026-11-02. Estado: revisto e aprovado tecnicamente pelo coordenador, por delegação do utilizador; implementação verificada automaticamente; revisão técnica do coordenador concluída; aceitação manual pendente. O utilizador autorizou antecipar a fase 3 e delegou no coordenador a revisão de spec, plano e implementação. As aceitações manuais das fases 1 e 2 continuam pendentes. Um agente implementa sequencialmente; o coordenador revê o resultado e pode pedir correções, sem agentes adicionais de revisão.

## Restrições globais

- Manter React/TypeScript, Rust/Tauri, SQLite `rusqlite` e auxiliar Python.
- Introduzir `ai_v3` para novas sessões; `local_v2` continua disponível, sem seguimentos, custos fictícios ou feedback retroativo.
- A aplicação continua a controlar a entrevista; a LLM recomenda conteúdo e não executa transições, comandos ou ferramentas.
- Quantidade e progresso contam apenas perguntas principais. Para N principais, há no máximo N seguimentos.
- Não enviar áudio, caminhos locais, rascunhos, transcrições automáticas distintas da versão confirmada, banco de perguntas completo ou notas pessoais.
- Não fazer retries automáticos nem reparações pagas automáticas nesta fase.
- Ausente é `null`/desconhecido, nunca zero inferido.
- Não manter o `transitions` de `AppService` durante o `await` HTTP.
- Relatórios válidos são imutáveis e mantêm as fontes/prompt/rubrica.
- Português de Portugal na interface, documentação e explicações; perguntas, seguimentos, citações e correções inglesas em inglês.
- Sem fase 4, instalador, novos motores, nota global ou avaliação acústica/emocional.

## Foco da revisão

1. Migração de sessão v1 a meio de uma resposta: identidades, caminhos e recibos mantêm-se e o percurso local continua; teste na tarefa 1.
2. Dispensa enquanto HTTP está suspenso: avançar não espera pelo HTTP e resultado tardio só acrescenta consumo; teste na tarefa 5.
3. Falha SQLite depois de resposta remota: repetir publicação conserva o resultado e não cobra novo pedido; teste na tarefa 5.
4. Citação Unicode e ausência sem texto: rejeitar correspondências fabricadas e aceitar ausência com fontes válidas; teste na tarefa 3.
5. Cache maior que entrada, uso parcial ou mudança de modelo/tarifário: não produzir custos negativos ou totais completos fictícios; teste na tarefa 4.

## 1. Estrutura e contratos de integração

Criar `src-tauri/src/ai/{mod,types,validation,prompts,usage,transport,credentials}.rs`, `src-tauri/src/db/ai.rs` e `src-tauri/src/service/ai.rs`. São, respetivamente, contratos puros/validação, adaptação HTTP/segredos, transações e coordenação. Não mover módulos alheios à alteração.

Criar `src/lib/ai-contracts.ts`, `src/features/ai/{AiSettings,FeedbackPanel}.tsx` e testes correspondentes. Estender `InterviewClient` existente, o fake partilhado, preparação, entrevista, histórico e App. O frontend continua a ter um único adaptador Tauri.

### Alvos de resposta e compatibilidade

Adicionar `AnswerSlot { id, sessionQuestionId, role: initial|follow_up, text, selectedRecordingId, selectionVersion }`. A principal continua em `SessionQuestion`, responsável apenas pelo banco/copias/progresso; não inserir seguimentos nessa tabela. `QuestionDetail` conserva `question`, `recordings`, `confirmation` para a resposta inicial e ganha `initialAnswerId`, `followUp: FollowUpDetail|null`, `followUpStage`. `FollowUpDetail { answer: AnswerSlot, recordings, confirmation }`. `InterviewSnapshot` ganha `currentAnswer: AnswerSlot|null`, `aiJob: AiJobView|null` e `pendingAiPublication: Id|null`; `current` continua a principal, sem alterar o significado de `Pergunta X de N`.

`AnswerTarget` ganha `answerId`; `sessionQuestionId` continua a identificar a principal. `RecordingView` e `Confirmation` ganham `answerId`; `Confirmation` conserva `sessionQuestionId` como principal. `OperationTarget` ganha `answerId: Id|null` para síntese/transcrição. `StartSession` ganha `mode: SessionMode` com default de desserialização `local_v2`, preservando clientes/testes que omitem o campo. O JSON/TS novos enviam o campo explicitamente. `startRecording(answerId, expectedSelection)` passa um ID de alvo, nunca um caminho. Para inicial, `answerId == sessionQuestionId`.

`AudioSelection` ganha `FollowUp { sessionId, sessionQuestionId, answerId }`; as variantes existentes mantêm-se. O cache Kokoro continua indexado pelo texto/voz/velocidade. Artefactos de novos seguimentos ficam em `sessions/<session>/<answerId>/<recording>/`; alvos iniciais conservam os caminhos existentes. A reconciliação resolve sempre o proprietário persistido; não depende de a pergunta estar corrente.

### Migração SQLite v2

Manter o ficheiro `001_guided_interview.sql` intacto como origem v1. A migração nova reconstrói as tabelas necessárias com cópia explícita, no padrão seguro do SQLite: `PRAGMA foreign_keys=OFF` apenas na ligação exclusiva de arranque, antes da transação; criar/copiar/remover/renomear; recriar índices e triggers; `PRAGMA foreign_key_check` deve devolver zero linhas antes de commit; restaurar FK ON mesmo após erro. Não usar `writable_schema` ou substituições de SQL em `sqlite_master`.

- `sessions`: alargar CHECK de modo a `local_v2|ai_v3`; estado continua `in_progress|completed`. Feedback pendente não ocupa índice de sessão ativa.
- `answer_slots`: `id` PK, `session_question_id` FK, `role` CHECK, `text`, seleção/versionamento; UNIQUE `(session_question_id,role)`. Preencher initial com ID da principal e copiar seleção/texto. Imutabilidade de identidade/papel/texto; seleção só antes da confirmação. A aplicação cria o slot inicial na mesma transação de criação da principal.
- `recordings`: trocar proprietário `session_question_id` por `answer_id` FK para slot; IDs, ordinais, estados e caminhos são copiados sem alteração. O índice composto passa a `(answer_id,id)`.
- `confirmations`: `answer_id` UNIQUE e FK composta `(answer_id,recording_id)`; copiar identidades e revisão. Continuam append-only. Reviews/transcripts não mudam conteúdo.
- `session_questions`: seleção passa a viver no slot; DTO ainda pode preencher campos antigos a partir do slot initial durante a transição. Remover a FK cíclica antiga ao reconstruir. Os triggers das cópias de conteúdo mantêm-se.
- `operations` local ganha `answer_id` nullable, preenchido nos trabalhos antigos com principal; mantém a FK da principal para contexto. As variantes locais de kind permanecem apenas `transcribe|synthesize`; pedidos externos têm tabela própria.
- `ai_settings`: singleton com versão/configuração pública JSON e consentimento de destino; sem segredo.
- `ai_stages`: uma linha por principal de sessão IA, etapa `not_requested|running|failed|not_needed|follow_up|answered|waived`, generation positiva, motivo/erro, ID da confirmação inicial; FK de contexto. Criar com confirmação inicial.
- `ai_requests`: operação lógica única `(purpose,session_question_id)` para `follow_up|feedback`, fontes imutáveis e versões de prompt/rubrica/esquema. `ai_attempts`: ID PK, request ID, generation, configuração/tarifa copiadas, `reserved|running|completed|failed|interrupted|discarded`, datas, envio `not_sent|possibly_sent|received`, consumo e erro seguros. Não guardar corpos HTTP de erro ou chave.
- `ai_reports`: uma avaliação válida imutável por principal, tentativa de origem, confirmação inicial e eventual seguimento, JSON validado. Triggers impedem UPDATE/DELETE.
- `command_receipts`: conservar recibos antigos e alargar kind para comandos IA. O payload hash segue o padrão SHA-256 existente. Repetição com mesmo conteúdo não cria nova tentativa.

Atualizar `Database::open` para aplicar v1 e depois v2, uma transação de migração por abertura que altera versão apenas depois das verificações. `open_with_schema` de testes deve continuar a testar rollback com esquema inválido sem inadvertidamente aplicar v2 sobre um esquema arbitrário. Bases futuras são recusadas antes de alterações persistentes.

### Pedidos e DTOs públicos

Usar `Result<T>`/`AppError` do projeto, IDs UUID e datas RFC3339; JSON camelCase e enums snake_case. Tipos Rust vivem em `ai/types.rs`, espelhados por `ai-contracts.ts` e fixtures `contracts/ai-v3.example.json`.

- `AiSettingsInput { baseUrl, model, tariff: Tariff|null }`; `Tariff { model, currency:"USD", inputPerMillion, outputPerMillion, cachedInputPerMillion:null|number, observedAt, sourceUrl }`.
- `AiSettingsView { version, baseUrl, model, tariff, credentialConfigured, consent: Consent|null }`; `Consent { destination, noticeVersion:"text-external-v1", acceptedAt }`.
- `SaveAiSettings { requestId, expectedVersion, input }`; `AcceptAiConsent { requestId, expectedSettingsVersion, destination, noticeVersion }`.
- `AiQuestionCommand { requestId, sessionId, sessionQuestionId, expectedSessionVersion }` para pedir/dispensar; dispensa acrescenta `reason: unavailable|request_failed|user_cancelled_wait`.
- `AiSessionCommand { requestId, sessionId }` para gerar feedback. Apenas entrevista concluída, modo IA, com fontes fixas.
- `AiJobView { attemptId, purpose, sessionId, sessionQuestionId, stage: requesting|publishing }`.
- `FeedbackView { sessionId, state: pending|running|partial|failed|ready, reports, priorities, usage, attempts }`; local devolve ausência de avaliação IA, não um estado ready fictício.

`AppService` expõe `load_ai_settings()->Result<AiSettingsView>`, `save_ai_settings(SaveAiSettings)->Result<AiSettingsView>`, `save_ai_key(String)->Result<AiSettingsView>`, `remove_ai_key()->Result<AiSettingsView>`, `accept_ai_consent(AcceptAiConsent)->Result<AiSettingsView>`, `request_follow_up(AiQuestionCommand)->Result<InterviewSnapshot>`, `waive_follow_up(WaiveFollowUp)->Result<InterviewSnapshot>`, `generate_feedback(AiSessionCommand)->Result<FeedbackView>`, `load_feedback(Id)->Result<FeedbackView>`, `retry_ai_publication(Id)->Result<InterviewSnapshot>`.

Comandos Tauri `interview_ai_*` correspondem aos nomes acima. `InterviewClient` usa nomes camelCase sem prefixo; `saveAiKey` só recebe segredo, nenhuma resposta o contém. `requestFollowUp` e `generateFeedback` reservam/lançam tarefa e devolvem estado imediatamente; a conclusão chega pelo evento `interview://snapshot`. Histórico/FeedbackPanel relê `loadFeedback` quando o job da sua sessão termina. Eventos não substituem versões persistidas.

### Credenciais, HTTP e limites fixados

`CredentialStore: Send+Sync` define `get()->Result<Option<String>>`, `set(&str)->Result<()>`, `delete()->Result<()>`; acesso serializado e executado em spawn_blocking. Produção Windows: `keyring::Entry`, service `language-assistant-tutor`, user `opencode-api`; `NoEntry` é ausência, restantes erros são mensagens fixas sem debug do erro. `cfg(not(windows))` devolve indisponibilidade explícita, nunca mock de produção. Chave em memória só durante guardar/chamada, sem derive Debug nos tipos que a transportam.

URL normalizada remove barras finais, exige HTTPS, host e ausência de userinfo/query/fragment; rejeita base terminada em `/chat/completions`, orientando a introduzir a base. Consentimento vinculado à base canónica completa, não só ao host. Qualquer alteração da base invalida consentimento e remove a credencial anterior antes de permitir chave para novo destino; não encaminhar automaticamente a chave de OpenCode para outro host. A operação falha de forma segura se remover a chave falhar. Guardar modelo/tarifa não faz rede.

Transporte async `AiTransport::complete(TransportRequest)->Pin<Box<dyn Future<Output=Result<RemoteCompletion>>+Send>>`; request contém configuração efetiva, mensagens e segredo transitório; resposta contém modelo, finishReason, conteúdo e consumo. Testes injetam implementações com barreiras. Produção `reqwest` com redirects desativados, retries desativados explicitamente, timeout total 90 s e ligação 10 s; sem streaming, tools ou catálogo autenticado. Ler resposta em chunks até 1 MiB, recusando excesso mesmo sem Content-Length. Validar HTTP e envelope antes do conteúdo; `finish_reason=length` é inválido. Não expor URL com parâmetros nem corpo bruto em erro. Classificar 401, 402/403, 429, 5xx, timeout/rede, envelope inválido e limite.

Limites em bytes UTF-8: pergunta de seguimento 2.000; resposta confirmada 64.000 por fonte; pergunta+critérios 16.000; corpo de entrada serializado incluindo prompts 160.000; saída HTTP 1.048.576. `max_tokens`: 800 para decisão, 4.000 para relatório. Excesso gera erro local antes de envio, sem truncar texto. Limites são técnicos de bytes, não alegação de capacidade exata de tokens. Defaults modelo/base da spec; tarifa de referência datada editável, sempre identificada como referência de 2026-11-02 e desativada para outro modelo.

### JSON do modelo e consumo

Prompts `follow-up-v1`, `feedback-v1`, rubrica `star-text-v1`, esquema `ai-v3-1`. Pedir apenas um objeto JSON, sem fences; usar `serde(deny_unknown_fields)` nas estruturas. Material do utilizador em mensagem user serializada como dados; system fixa regras sem tools. Não depender de `response_format:json_schema`.

Decisão: `{schemaVersion, decision:"ask"|"continue", reason, missingDimension:null|dimension, question:null|string}`. Dimensões: `relevance|situation|individual_responsibility|actions|result|learning`; `ask` exige dimensão e pergunta não vazia; `continue` exige ambos null. `reason` não vazio. Só uma criação de slot por principal, em transação.

Relatório: `{schemaVersion, content:[ContentObservation], strengths:[QuotedObservation], gaps:[GapObservation], english:{necessaryCorrections:[Correction], optionalStyle:[Correction]}, priorities:[Priority], fewerPrioritiesReason:null|string}`. Seis `ContentObservation` únicas, uma por dimensão, com `{id,dimension,rating:"clear"|"partial"|"not_identified",explanation,evidence:[Evidence]}`. `Evidence` é união tagged `quote {confirmationId,role,quote}` ou `absence {dimension,assessedConfirmationIds}`. `clear` exige quote, `partial` exige quote, `not_identified` exige absence. `QuotedObservation {id, explanation,evidence:[quote]}`; `GapObservation` permite quote ou absence. `Correction {id,explanation,evidence:quote,replacement}`. `Priority {observationId,action}` referencia uma observação do próprio relatório. IDs únicos no relatório, arrays até 20 observações por secção e três prioridades; textos explicativos até 4.000 bytes, replacements até 8.000. Ausências só dimensões fornecidas e lista exatamente igual às fontes recebidas; quote não vazia deve ser substring literal da fonte/papel correto. `fewerPrioritiesReason` obrigatório se menos de três prioridades. Rejeitar relatório completo inválido.

Normalizar `usage.prompt_tokens` como entrada total incluindo cache; `completion_tokens` como saída; `prompt_tokens_details.cached_tokens` como subconjunto de entrada. Ausentes ficam null. Números negativos/não inteiros de tokens, cache superior à entrada ou custos não finitos/negativos ficam desconhecidos com aviso, não anulam conteúdo válido. `usage.cost`, quando fornecido em USD, é custo reportado; preservar metadados `opencode.billing_source` e `opencode.estimated` quando presentes, sem inferir faturação final. Campos desconhecidos do envelope não são enviados ao frontend nem guardados integralmente.

Estimativa local = `((input-cache)*inputRate + cache*cacheRate + output*outputRate)/1e6` quando campos/rates necessários existem; sem cache separado usar entrada normal e marcar conservadora. Se cache existe e tarifa cache falta, usar normal para toda a entrada e marcar conservadora. Sem entrada ou saída conhecidas, estimativa completa null. Não extrapolar output zero. Usar valores decimais finitos, apresentar pelo menos seis casas para pequenos custos; não calcular com texto arredondado da UI. Totais: `knownReportedSubtotal`, `knownEstimatedSubtotal`, `unknownAttemptCount`, tokens parciais e completude; nunca somar reportado e estimado do mesmo pedido como gasto. Agregação de prioridades usa a ordem definida na spec e conserva IDs/origens.

## 2. Tarefas de execução

Cada tarefa: escrever testes indicados, executá-los e observar falha antes de implementação; implementar, repetir até verde e registar comandos/resultados. Sem commits automáticos neste plano: o coordenador decide integração após revisão. Os comandos abaixo assumem Cargo no PATH; quando necessário usar `$env:USERPROFILE\.cargo\bin\cargo.exe`, como em verify.ps1.

### Tarefa 1 — Migração e alvos partilhados de áudio

**Ficheiros:** criar `src-tauri/src/db/migrations/002_ai_tutor.sql`; modificar `db/{mod,sessions,answers,operations}.rs`, `interview/types.rs`, `service{.rs,/interview.rs}`, `artifacts.rs`, contratos/fake e consumidores TS dos campos afetados; criar `src-tauri/tests/ai_migration.rs` e estender `tests/{answers,artifacts,service,sessions}.rs`.

**Interfaces:** produzir AnswerSlot/currentAnswer/answerId e os DTOs de §1; `Database::load_answer_slot(&str)->Result<AnswerSlot>`; adaptar reserva, seleção, confirmação, listagens e contagens para slot. Captura inicial mantém mesmos caminhos e guardas. Nesta tarefa ainda não há chamada remota.

- [x] Escrever `migrates_populated_v1_without_changing_sources_or_paths`, `migration_failure_rolls_back_schema_and_version`, `future_database_is_refused_before_write` e `fresh_database_reaches_v2`. Fixture v1 inclui sessão concluída e outra em curso, confirmação, revisão manual sem transcrição, operação pendente, recibo e ficheiros de gravação; comparar conteúdo/IDs após abrir v2 e `foreign_key_check` vazio.
- [x] Escrever `follow_up_slot_has_own_recordings_and_confirmation`, `initial_count_ignores_follow_up` e `follow_up_cannot_select_initial_recording`; tentar violar unicidade e FK diretamente no SQLite.
- [ ] Executar `cargo test --manifest-path src-tauri/Cargo.toml --test ai_migration` e testes novos de áudio; confirmar falha por contrato ausente, não por erro na fixture.
- [x] Implementar migração e alvos. Extrair um resolvedor de resposta corrente usado por `automatic_target`, validação de capture/review/transcribe e seleção do frontend; não duplicar os pipelines. `ArtifactStore::annotate` e reconciliação percorrem inicial e seguimento.
- [x] Executar testes Rust completos e `npm.cmd run typecheck`; passar regressões antes da tarefa seguinte. Atualizar fixtures existentes onde contrato mudou sem enfraquecer assertions.

### Tarefa 2 — Configuração e armazenamento protegido

**Ficheiros:** criar `ai/{mod,types,credentials}.rs`, configuração em `db/ai.rs`; modificar Cargo.toml/Cargo.lock, `lib.rs`, `service.rs`, `commands.rs`; criar `src-tauri/tests/ai_settings.rs`.

**Interfaces:** consumir Database e tipos §1; produzir CredentialStore/WindowsCredentialStore e métodos/comandos de configuração/consentimento. Novo construtor `AppService::new_with_ai(..., Arc<dyn AiTransport>, Arc<dyn CredentialStore>)` preserva `new` para consumidores existentes, delegando nos adaptadores de produção; testes podem usar o novo construtor.

- [x] Escrever testes para chave nunca reaparecer nos DTOs/Debug/config SQLite, substituir/remover, erro de store sem fallback, URL inválida, versão antiga, consentimento para destino diferente e mudança de base sem reaproveitar segredo.
- [ ] Executar `cargo test --manifest-path src-tauri/Cargo.toml --test ai_settings`; observar falha.
- [x] Implementar configuração, recibos, validação e storage com I/O fora da thread da UI. Serializar alterações de configuração e consulta do segredo para o snapshot do pedido; pedido em curso mantém destino e chave transitória antigos sem contaminar seguintes.
- [x] Repetir testes e `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`. Não aceder à credencial pessoal em testes; testes usam fake de instância, não store global.

### Tarefa 3 — Prompts e validação de decisões/evidências

**Ficheiros:** criar `ai/{validation,prompts}.rs`, fixtures `contracts/ai-v3.example.json`, `src-tauri/tests/ai_validation.rs`.

**Interfaces:** `build_messages(purpose: AiPurpose, sources: &AiSources)->Result<Vec<ChatMessage>>`; `validate_follow_up(raw:&str)->Result<FollowUpDecision>`; `validate_feedback(raw:&str,sources:&AiSources)->Result<QuestionReport>`; tipos conforme §1. `AiSources` contém só pergunta/critérios copiados e confirmações de origem identificadas.

- [x] Testar decisão ask/continue, dimensão inexistente, pergunta vazia/longa, JSON com fences/truncado/campos extras e tentativa de segunda pergunta.
- [x] Testar citação literal Unicode, pontuação alterada, confirmação fora do pedido, papel errado, parcial sem quote, absence com fontes incompletas, prioridade órfã/duplicada, falta de dimensão e nota global/campo acústico extra. Fixtures válidas incluem resultado negativo/qualitativo e aprendizagem sem métricas.
- [ ] Executar `cargo test --manifest-path src-tauri/Cargo.toml --test ai_validation` e confirmar falhas esperadas.
- [x] Implementar prompts e validador; testar prompt com resposta contendo instruções de ignorar rubrica: continua na mensagem de dados, sem se tornar system. Não alegar que esse teste prova imunidade semântica.
- [ ] Repetir testes; validar fixture Rust/TS com mesmos campos e referências.

### Tarefa 4 — HTTP limitado e contabilização

**Ficheiros:** criar `ai/{transport,usage}.rs`, `src-tauri/tests/{ai_transport,ai_usage}.rs`; Cargo.toml adiciona reqwest e features necessárias verificadas na versão instalada.

**Interfaces:** produzir AiTransport/RemoteCompletion e `estimate_usage(&NormalizedUsage,&Tariff)->CostEstimate`; `aggregate_usage(&[AttemptUsage])->SessionUsage`. Configuração/limites/normalização em §1 são autoridade.

- [x] Servidor controlado local nos testes: capturar body/auth; responder JSON válido, 401/402/403/429/500, redirect, timeout, Content-Length falso/ausente com corpo excessivo, JSON inválido e finish_reason length. Endpoint HTTP de loopback permitido apenas pelo construtor interno de teste; validação pública continua HTTPS.
- [x] Testar tokens ausentes/zero real, custo fornecedor zero explícito, cache, cache impossível, consumo parcial, preço ausente, mudança de modelo e totais com tentativa descartada/desconhecida. `input=1000,cache=200,output=100` e tarifa `0.30/0.006/1.20` → estimativa USD `0.0003612`.
- [ ] Executar `cargo test --manifest-path src-tauri/Cargo.toml --test ai_transport --test ai_usage`; observar falha.
- [x] Implementar cliente sem retries/redirects; body enviado só contém modelo/messages/max_tokens/stream=false. Remover corpos brutos de erros e não persistir segredo. Transporte não tem acesso ao SQLite, áudio ou ficheiros profissionais.
- [x] Repetir testes, incluindo uma única requisição recebida no servidor mesmo em resposta 5xx; confirmar redirecionamento não recebe Authorization no segundo servidor.

### Tarefa 5 — Seguimento persistente e coordenação assíncrona

**Ficheiros:** criar `service/ai.rs`, estender `db/ai.rs`, `service{.rs,/interview.rs}`, `commands.rs`, `lib.rs`; criar `src-tauri/tests/ai_service.rs`.

**Interfaces:** produzir request_follow_up/waive_follow_up/retry_ai_publication; transações `reserve_ai_attempt`, `finish_ai_attempt` e `waive_follow_up` validam fonte/geração/recibo. Tipos de tentativa e resultado privados ao módulo podem ser definidos conforme dados §1, sem alterar DTOs públicos. Novo `AiRuntime` no serviço contém um único slot de rede e um mapa de resultados por publicar; não reutilizar `model_job` do auxiliar Python.

- [x] Escrever testes com transporte suspenso: confirmação inexistente/consentimento ausente não chama rede; versão antiga recusada; cliques com mesmo request ID só reservam uma tentativa; segundo job recebe busy; resultado cria um único slot; resposta ao seguimento nunca gera outro; avanço local não contorna etapa IA.
- [x] Testar dispensa durante rede, avançar e gravar seguinte antes de libertar transporte; ao libertar, consumo é guardado e conteúdo descartado. Áudio/consulta/navegação permanecem operacionais durante HTTP.
- [x] Testar reinício com pedido running → interrupted/possibly_sent sem reenvio, pedido interrompido repetido só por ação explícita, fechar app durante HTTP sem depender de timeout.
- [x] Injetar falha de publicação depois de receber resposta: estado mostra `pendingAiPublication`; retry publica mesmo conteúdo/consumo e transporte continua com contador 1. Não aceitar nova chamada paga enquanto existe publicação pendente. Depois de dispensa, republicação só fecha tentativa/contabiliza, sem criar seguimento.
- [ ] Executar `cargo test --manifest-path src-tauri/Cargo.toml --test ai_service`; observar falha.
- [x] Implementar fluxo reserva sob transitions → libertar → HTTP em task → readquirir → validar/publicar → snapshot. Marcar possibly_sent antes de iniciar transporte; falhas anteriores de configuração/chave não são chamadas cobradas. Se guardar consumo/resultado falha, conservar pacote em memória; só libertar pendência após commit. A unicidade no SQLite é última defesa.
- [x] Repetir teste ai_service e regressão service/answers/artifacts. Atualizar captura/reprodução do seguimento para passar slot e não principal confirmada.

### Tarefa 6 — Feedback granular, histórico e recuperação

**Ficheiros:** estender `db/ai.rs`, `service/ai.rs`, `ai/usage.rs`; criar `src-tauri/tests/ai_feedback.rs`.

**Interfaces:** produzir generate_feedback/load_feedback/FeedbackView. Um batch reserva recibo, percorre principais sem relatório válido e pede sequencialmente; usa o mesmo slot global de rede de tarefa 5 durante o batch. Falha para o batch e conserva relatórios anteriores. Retry com novo request ID só percorre faltantes; replay do mesmo ID não reinicia batch. Após crash não retoma rede automaticamente.

- [x] Escrever testes sessão com três perguntas: primeiro relatório válido, segundo falha → partial e apenas duas chamadas; retry recebe segundo e terceiro → ready sem reavaliar primeira. Avaliação de sessão terminada não bloqueia nova captura local/IA.
- [x] Testar fontes inicial+seguimento, relatório inválido com consumo conhecido, imutabilidade do válido, consulta histórica durante outra sessão, geração recusada antes de terminar entrevista e erro SQLite seguido de publicação sem nova cobrança.
- [ ] Executar `cargo test --manifest-path src-tauri/Cargo.toml --test ai_feedback`; observar falha.
- [x] Implementar estado derivado de fontes/relatórios/tentativas; ready apenas com todos os relatórios válidos. Construir prioridades localmente por gaps de conteúdo e depois necessaryCorrections, preservando origem e máximo três. Não acrescentar narração factual por algoritmo.
- [x] Repetir testes ai_feedback/ai_service/ai_usage; confirmar totais incluem pedidos de seguimento e todas tentativas de feedback, inclusive descartados.

### Tarefa 7 — Interface integrada e comandos reais

**Ficheiros:** criar `src/lib/ai-contracts.ts`, `src/features/ai/{AiSettings,FeedbackPanel}.tsx` e testes; estender `interview-{contracts,client}.ts`, fake, `App.tsx`, `SessionSetup.tsx`, `Interview.tsx`, `useInterview.ts`, `History.tsx` e CSS existente. Registar todos comandos em `src-tauri/src/lib.rs`.

**Interfaces:** consumir comandos/DTOs de §1; nenhuma transição persistente ou cálculo de custo exclusivo no React. O resolvedor de resposta em useInterview seleciona currentAnswer e encontra a confirmação/gravações respetivas. QuestionDetail continua usado no histórico com inicial e seguimento separados.

- [x] Escrever testes: modo local predefinido e ai_v3 explícito; configuração mascara/limpa chave em sucesso/erro/desmontagem, alteração de destino pede novo consentimento; confirmar não chama requestFollowUp; clique explícito envia; faltando configuração oferece guardar/dispensar com motivo.
- [x] Testar fluxo inicial confirmada → pedir → seguimento → revisão/confirmar → avanço, sem contador extra; falha/retry/dispensa e resultado atrasado; troca de página e fecho durante HTTP não ficam presos. Não permitir dispensa de seguimento já criado.
- [x] Testar feedback pendente/parcial/pronto e fontes consultáveis no histórico; custos desconhecidos aparecem por extenso, subtotal não é chamado total; retry de publicação é distinto de repetir pedido pago. Originais iniciais e do seguimento reproduzem pelos seus IDs.
- [ ] Executar `npm.cmd test`; observar falhas dos cenários novos.
- [x] Implementar e integrar ecrãs existentes, configuração IA na navegação e FeedbackPanel no resumo/histórico. Reutilizar estilo/foco/labels existentes, sem redesenho genérico. Aviso de serviço externo apresenta destino completo antes de consentir e erro de retry alerta para possível novo custo.
- [x] Executar `npm.cmd run typecheck`, `npm.cmd test`, `npm.cmd run build`; repetir testes Rust de contratos/comandos se a integração alterou campos. Fake deve simular estados reais, não autorizar operações que Rust recusa.

### Tarefa 8 — Verificação final e documentação fiel

**Ficheiros:** criar `docs/phase-3/{setup,validation}.md`; atualizar README, AGENTS.md, stack, plano das fases, estado da spec e deste plano. Não apagar pendências das fases anteriores.

**Interfaces:** consumir implementação final, comandos reais e outputs; produzir instruções de configuração/execução e matriz F3-01…F3-16 com distinção automática/manual.

- [x] Executar `.\scripts\verify.ps1` completo e registar contagens reais/saída 0. Se falhar, corrigir causa e repetir checks afetados, depois o agregador antes de declarar automação verde.
- [x] Executar `npm.cmd run tauri -- build --no-bundle` e arranque/fecho do binário como regressão do desktop; registar comandos efetivos do setup local, sem afirmar pacote instalado validado.
- [x] Conferir links locais, `git diff --check` e diff contra spec: cada critério tem evidência ou pendência explícita. Verificar ausência de chave/respostas reais nos novos artefactos e que snapshots/erros não expõem segredo.
- [ ] Registar teste real de credenciais Windows e inferência paga apenas se houver credencial/saldo configurados pelo utilizador e ação autorizada com texto de teste. Sem isso, marcar pendente; não solicitar nem ler chave por chat e não fazer inferência paga automática para testes.
- [ ] Deixar manualmente pendentes F3-16, relevância semântica/utilidade, medições de espera e segurança efetiva do armazenamento protegido quando não exercitada. Documentar roteiro de três principais com falha/retry/dispensa/retoma, feedback parcial e consulta de fontes.
- [x] Entregar ao coordenador lista de ficheiros, desvios/decisões, comandos/resultados e limitações. Fase implementada/verificada automaticamente não equivale a aceite/concluída.

## 3. Cobertura e decisões verificadas

| Critérios | Tarefas/evidência prevista |
| --- | --- |
| F3-01 | 1, 7, 8: migração com dados, regressão local/legado e desktop |
| F3-02 | 2, 7; exercício Windows real pendente na 8 |
| F3-03 | 2, 3, 4, 5, 7: autorização/fonte/carga observada |
| F3-04, F3-05 | 1, 5, 7: slots/áudio/limites no núcleo e UI |
| F3-06, F3-07, F3-08 | 5, 6, 7: barreiras, recibos, restart e publicação |
| F3-09, F3-10 | 3, 6, 7: validação literal, ausência e apresentação |
| F3-11 | 3 e 8: fixtures de contrato e validação humana pendente |
| F3-12 | 6, 7: relatório parcial e retry granular |
| F3-13 | 4, 6, 7: normalização, cache, tentativas e rótulos |
| F3-14, F3-15 | 2, 4, 5, 6, 7: falhas controladas e UI sem bloqueio |
| F3-16 | 8: aceitação humana e inferência real pendentes até execução |

Fontes primárias consultadas em 2026-11-02: [reqwest ClientBuilder 0.13.5](https://docs.rs/reqwest/0.13.5/reqwest/struct.ClientBuilder.html), [keyring 3.6.3](https://docs.rs/keyring/3.6.3/keyring/), [OpenCode Inference API](https://opencode.ai/v2/docs/console/api/inference/). `keyring` 3.6.3 foi escolhido deliberadamente porque documenta `windows-native` e Entry; a linha 4 altera a composição de stores. Confirmar features exatas do reqwest ao resolver dependências, sem mudar os requisitos de TLS, limites e ausência de retry. Não houve inferência paga ao escrever o plano.

Auto-revisão: cobertura F3 mapeada; nomes públicos definidos em §1 e usados nas tarefas; resolvedor de alvos partilhado evita fork do áudio; falhas de publicação e faturação incerta são testadas; modo/local e validações humanas continuam explícitos. Detalhes internos que não alteram estes contratos podem ser ajustados pelo implementador e registados no relatório.

## Registo da execução

As oito tarefas têm implementação e evidência automática no [relatório](../../phase-3/implementation-report.md) e na [validação](../../phase-3/validation.md). Os nomes/agrupamentos de testes foram adaptados (HTTP interno, ai_service agrega coordenação e feedback). Os passos de observar falha antes de cada implementação não foram cumpridos integralmente: TDD foi parcial e essas caixas permanecem por marcar. A fixture nova ai-v3 documenta fontes/decisão; o contrato local v2 continua testado por igualdade exata Rust/JSON. Credenciais reais e inferência paga não foram exercitadas e a aceitação manual continua pendente.

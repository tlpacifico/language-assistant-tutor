# Fase 4 — Plano de implementação: melhorar e repetir

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans ou superpowers:subagent-driven-development apenas quando a implementação for autorizada. Steps use checkbox (`- [ ]`) syntax for tracking.

**Objetivo:** permitir reformular, ouvir, repetir, avaliar e comparar respostas, preservando factos e versões, com histórias locais de preparação.

**Arquitetura:** práticas individuais pertencem à pergunta fixa de uma sessão concluída, mas têm proprietário e alvo de resposta próprios. Rust/SQLite conservam fontes, variantes, operações e snapshots; React apresenta os dados e intenções. Reutilizar captura, auxiliar Python, HTTP, consentimento e consumo atuais através de resolvedores de alvos explícitos.

**Stack:** a existente — React/TypeScript, Rust/Tauri 2, rusqlite/SQLite, faster-whisper/Kokoro no auxiliar Python e DeepSeek Flash pela API paga OpenCode. Sem nova dependência planeada.

**Spec:** [Melhorar e repetir](../specs/2026-11-03-phase-4-improve-repeat-design.md), revista pelo coordenador.

Data: 2026-11-03. Estado: revisto e aprovado tecnicamente pelo coordenador por delegação do utilizador, para futura execução quando autorizada. **Só documentação autorizada.** Os comandos e alterações descritos abaixo são futuros; não foram executados para este plano. Não instalar, migrar dados, ler credenciais/dados pessoais ou fazer inferência paga. A fase 3 existente no checkout é preservada; aceitação manual das fases anteriores continua pendente. Não iniciar fase 5.

## Restrições globais

- Nunca muda `position`, `question_count`, etapa de seguimento, confirmação, gravação selecionada ou relatório original.
- Histórias exclusivamente locais nesta fase: nenhum campo nem identificador de história entra na API.
- Uma prática não confirmada de cada vez; começar práticas apenas quando não há entrevista em curso.
- Uma prática confirmada nunca cria seguimento, mesmo que o modelo o recomende.
- B1 inicialmente, com seleção B1/B2/C1 na reformulação; nível é objetivo de linguagem, não diagnóstico CEFR.
- Não gerar automaticamente feedback/reformulação ao confirmar.
- Uma chamada remota de cada vez, sem retries pagos automáticos.
- Rede não mantém Mutex SQLite ou bloqueio global de transições; consulta e navegação continuam possíveis.
- Sugestões válidas são imutáveis; reutilizar resultado válido para mesma origem/nível/versões.
- A gravação começa e termina por ação do utilizador, sem interrupções automáticas por duração.
- Não truncar texto, áudio, história ou JSON silenciosamente; distinguir custo desconhecido de zero.
- Português de Portugal na interface/documentação/explicações; perguntas, citações, respostas e reformulações em inglês.

## Foco da revisão

1. Microfone arranca, mas a escrita do snapshot falha: continuar captura manual e preservar áudio/apoio candidato; tarefa 3 testa metadados pendentes e recuperação.
2. Comando antigo aponta para a pergunta original enquanto uma prática está aberta: recusar associação cruzada, sem modificar originais; tarefas 1–2 testam proprietários e FK.
3. Resposta remota de B1 chega depois de selecionar B2 ou abrir outra prática: guardar só variante/alvo de origem sem trocar seleção na UI; tarefas 5 e 7.
4. Palavra exclusiva apenas na história: nunca surgir em body HTTP, transcrição automática, editor pré-preenchido ou confirmação; tarefas 3, 4 e 7.
5. Tentativas de rede repetidas aparecem na prática e na sessão de proveniência: totais distintos por ID, sem dupla contagem ou total conhecido fictício; tarefa 6.

## 1. Ponto de partida real e estrutura dos ficheiros

O código atual tem tipos IA em `src-tauri/src/ai/mod.rs`, JSON validado em `ai/validation.rs`, transporte em `ai/transport.rs`, transações em `db/ai.rs` e coordenação em `service/ai.rs`. Não assumir os nomes de ficheiros previstos num plano antigo se não existirem no checkout.

`answer_slots` v2 contém `session_question_id`, `session_id`, papel `initial|follow_up` e seleção. A coluna histórica `recordings.session_question_id` **já referencia o ID do slot**, assim como a proprietária de `confirmations` e `operations`; não a renomear globalmente como parte desta fase. `QuestionDetail::active()` apresenta inicial/seguimento; não criar um `QuestionDetail` fictício para práticas. `ai_requests` é único por finalidade/pergunta, `ai_reports` por pergunta. Estes são os pontos que exigem migração.

Criar módulos focados `src-tauri/src/practice.rs`, `db/{practice,stories,rewrites}.rs`, `service/practice.rs` e `ai/rewrite.rs`. Adaptar `ai/{mod,validation,transport}.rs`, `db/ai.rs`, `interview/types.rs`, `service{.rs,/interview.rs,/ai.rs}`, `artifacts.rs` e comandos/registo Tauri. Extrair apenas os auxiliares partilhados de resposta/captura atualmente dependentes da entrevista ativa; não construir um segundo serviço de áudio.

Frontend: criar `src/lib/practice-contracts.ts`, `src/features/practice/{Practice,RewritePanel,Comparison}.tsx` e `src/features/stories/{StoryBank,StorySupport}.tsx`, com testes próximos. Estender cliente/fake único, `App.tsx`, histórico, entrevista, hook de revisão e CSS existente. `practice.css` já existe: ampliar, não substituir o estilo da aplicação.

## 2. Contratos e decisões de construção

### 2.1 Identidades, estados e alvos

Novos tipos em `practice.rs`, com IDs UUID, versões u64, datas UTC RFC3339 e JSON camelCase/enums snake_case:

```text
ResponseOrigin = {type:"original", sessionId, sessionQuestionId}
               | {type:"practice", practiceId}
CaptureOwner = {type:"interview", sessionId, sessionQuestionId, answerId}
             | {type:"practice", practiceId, answerId}
PracticeStatus = preparing | answering | confirmed | abandoned
RewriteLevel = B1 | B2 | C1
CreatePractice = {requestId, sessionId, sessionQuestionId, previousPracticeId:null|Id}
PracticeCommand = {requestId, practiceId, expectedVersion}
PracticeDetail = {id, sourceSessionId, sessionQuestionId, ordinal,
  previousPracticeId, status, version, createdAt, confirmedAt, abandonedAt,
  question:{text,category,competencies,criteria}, answer:AnswerSlot,
  recordings:RecordingView[], confirmation:Confirmation|null, preparation:PreparationView}
PracticeAnswerTarget = {owner:CaptureOwner, recordingId, selectionVersion}
ExerciseSnapshot = {revision, activePractice:PracticeDetail|null,
  activeOwner:CaptureOwner|null, pendingPreparation:Id|null}
RewriteCommand = {requestId, origin:ResponseOrigin, level:RewriteLevel}
EvaluatePractice = {requestId, practiceId, expectedVersion}
PracticeReportView = {practiceId,state:pending|running|failed|ready,
  report:null|JSON,rubricVersion,promptVersion,usage,networkAttempts}
ComparisonRequest = {left:ResponseOrigin,right:ResponseOrigin}
ComparisonView = {left:ComparisonEntry,right:ComparisonEntry,rubricCompatibility:same|different|unknown}
StoryView = {id,version,title,situation,task,actions,outcome,competencies,
  missingFields:(situation|task|actions|outcome)[],createdAt,updatedAt}
PreparationView = {answerId,version,lockState:editable|pending_start|locked|start_uncertain,
  selectedStoryId:null|Id,selectedStoryVersion:null|Version,
  snapshot:null|{storyId,storyVersion,content:StoryInput},
  support:none|story|historical_none,lockedCaptureId:null|Id}
RewritePreference = {version,level:RewriteLevel,updatedAt}
ReportVersions = {rubricVersion,promptVersion,schemaVersion}
NetworkAttemptView = {networkAttemptId,requestId,purpose,origin:ResponseOrigin,
  state:running|completed|failed|interrupted|discarded,generation,
  usage:NormalizedUsage,createdAt,finishedAt:null|String}
ComparisonEntry = {origin:ResponseOrigin,createdAt,confirmedAt,
  question:{text,category,competencies,criteria},
  answers:{role:initial|follow_up|attempt,recordings:RecordingView[],confirmation:Confirmation}[],
  report:null|{content:JSON,versions:ReportVersions},
  reportState:pending|running|failed|ready|not_available,
  rewrites:RewriteView[],preparation:PreparationView}
UsageGroup = {totals:SessionUsage,networkAttemptCount:u64,
  byPurpose:{purpose,usage:SessionUsage,networkAttemptCount:u64}[]}
TrainingUsageView = {sessionId,originalInterview:UsageGroup,laterPractice:UsageGroup,combined:UsageGroup}
```

`ResponseOrigin::original` engloba confirmação inicial e seguimento confirmado. `practice` só engloba a sua confirmação com papel `attempt`. `CaptureOwner` distingue uma única resposta: inicial/seguimento identificados por slot no caso interview. `practiceId` é sempre tentativa de treino; `networkAttemptId` identifica chamada HTTP. Não expor campos ambíguos `attemptId` nos contratos novos.

`ComparisonEntry` contém origem, datas, pergunta, respostas separadas por papel, gravações/automáticos/confirmações, relatório com versões ou pendente e sugestões em zona própria. O núcleo exige mesma pergunta de sessão e origens distintas confirmadas; origens `original` são de sessão concluída. Ordenar pela criação, independentemente da ordem pedida, e não criar texto analítico sobre melhoria.

Adicionar `practice: ExerciseSnapshot` à fotografia `InterviewSnapshot`, mantendo campos da entrevista. Nunca colocar prática em `active_session` ou alterar `current` para simular uma pergunta de entrevista. O único estado de captura/auxiliar/reprodução continua partilhado. `AnswerSlot` ganha `practiceId:null|Id` e papel `attempt`; `sessionQuestionId` continua a principal de proveniência. O campo histórico `session_id` do slot é a sessão **de proveniência**, não uma sessão ativa nova.

Resolvedor interno `Database::resolve_capture_owner(&CaptureOwner)->Result<ResolvedAnswer>` produz `ResolvedAnswer {owner:CaptureOwner,slot:AnswerSlot,sourceSessionId:Id,sessionQuestionId:Id,question:{text,category,competencies,criteria},confirmation:Option<Confirmation>,ownerVersion:Version,writable:bool,artifactParent:{sourceSessionId,answerId}}`. A seleção está no slot; `ownerVersion` é versão da sessão ou prática conforme owner. Não contém histórias nem credenciais. `AppService` usa-o em captura, transcrição, gravação de revisão, confirmação e eventos atrasados. Para pedidos antigos de entrevista, adaptar `AnswerTarget` ao owner interview e recusar slots `attempt`; para práticas usar owner explícito. Não aceitar prática só porque o ID do slot pertence à mesma sessão histórica.

### 2.2 Esquema SQLite v3

Criar `003_improve_repeat.sql`; conservar migrações v1/v2. Na abertura suportar vazio→v1→v2→v3, v1→v2→v3 e v2→v3 numa transação por abertura; versão futura recusada antes de escrita. Desativar FK apenas na ligação exclusiva antes da transação se reconstrução exigir, reativar em todos os caminhos, verificar `foreign_key_check` antes do commit. Recriar triggers/índices sem contornar imutabilidade do produto; rollback conserva user_version e dados. Não usar `writable_schema`.

**Práticas:** `practices(id PK, session_question_id FK, source_session_id FK, ordinal>=1, previous_practice_id FK nullable, answer_id UNIQUE FK diferida, status CHECK, version>=1, created_at, confirmed_at, abandoned_at)`. UNIQUE `(session_question_id,ordinal)`, índice parcial constante único para estados `preparing|answering`. CHECK coerência de datas/estado. Trigger exige pergunta da sessão de proveniência concluída e confirmação inicial existente; predecessor, se presente, confirmado e da mesma pergunta. Identidade/proveniência/ordinal/predecessor imutáveis. Abandono só de estado pendente, sem gravação ativa/finalização/publicação pendente; conserva todos dados e impede novas alterações/avaliação.

**Exclusão entre treinos:** triggers no INSERT de practices e INSERT/UPDATE de sessions impedem ter prática pendente e entrevista `in_progress` simultaneamente. Verificar também no serviço sob transitions. Prática confirmada/abandonada não bloqueia outra; pedidos IA pendentes não são prática ativa. Contador/seleções de sessão original não mudam ao criar/finalizar prática.

**Slots:** reconstruir `answer_slots` alargando papel a `initial|follow_up|attempt` e acrescentando `practice_id` FK diferida. Remover UNIQUE global `(session_question_id,role)` e criar índice parcial único para `initial|follow_up`, mais UNIQUE parcial por `practice_id` para `attempt`. CHECK `(role='attempt')=(practice_id IS NOT NULL)`. Trigger de contexto exige sessão/pergunta coerentes e prática ligada à mesma pergunta; FK composta para impedir troca de answer_id. Slots antigos copiam IDs/textos/seleções, `practice_id=null`; trigger sync_initial_selection só atua em `initial`. Triggers de confirmação, revisões e seleção bloqueada continuam a cobrir todos os slots.

**Histórias:** `stories(id PK,version>=1,title,situation,task,actions,outcome,competencies_json,created_at,updated_at)` com validação em Rust e CHECK de título. `story_versions(story_id,version,content_json,created_at)` PK composta, append-only. Alterar banco faz UPDATE com versão esperada mais INSERT de versão na mesma transação; sem DELETE. `preparations(answer_id PK FK,version,selected_story_id nullable,selected_story_version nullable,locked_capture_id nullable FK recordings,lock_state:editable|pending_start|locked|start_uncertain)`. Suporte de seguimento resolve a preparação do slot initial, nunca cria associação própria. `preparation_captures(recording_id PK FK,answer_id,preparation_version,snapshot_json nullable,state:candidate|started|failed|uncertain,created_at)` conserva cópia fixada da versão; um marcador de ausência de história distingue «sem apoio» de falta de persistência. Snapshot nunca contém dados obtidos da história mais tarde. Para alvos v2 com áudio já publicado ou confirmação, inicializar preparação como ausência histórica bloqueada, sem inventar snapshot de história; apenas alvos ainda sem evidência de captura podem escolher apoio novo. Reservas v2 interrompidas sem original são indicadas como históricas sem evidência suficiente, conservadas e tratadas pela reconciliação antes de permitir nova seleção.

**IA:** reconstruir `ai_requests` preservando IDs/fontes/versões antigos; acrescentar `practice_id nullable`, `origin_key`, `variant_key` e `generation>=1`; alargar purpose a `follow_up|feedback|practice_feedback|rewrite`. `question_id/session_id` continuam proveniência. `origin_key` CHECK corresponde a `original:<question_id>` ou `practice:<practice_id>`; prática exige FK composta com pergunta/sessão. `variant_key` vale `default` para avaliações/seguimentos; para rewrite é nível+prompt+rubrica+esquema. UNIQUE `(purpose,origin_key,variant_key)`. Pedidos antigos ganham origin original e default, sem reescrever sources_json ou versões. Trigger impede alterar fontes/origem/variante/versões depois da reserva; só geração de retry pode aumentar. `ai_attempts` conserva formato/IDs/consumo; acrescentar snapshot de metadados de origem se necessário para consulta, nunca dados de histórias.

Manter `ai_reports` original e triggers intactos. Criar `practice_reports(practice_id PK FK, network_attempt_id UNIQUE FK ai_attempts, rubric_version, prompt_version, schema_version, json)` append-only. `evaluation_families(session_question_id PK FK,rubric_version,schema_version,created_at)` fixa rubrica da família; inicializar a partir dos ai_reports existentes por join ao pedido, sem alterar relatório. Quando não há relatório original, primeira avaliação de prática fixa rubrica atual `star-text-v1`; uma avaliação original futura da família usa versão compatível suportada. Se uma versão antiga não for executável, recusar nova avaliação com explicação e permitir consulta; nunca substituir relatórios antigos. Comparação de dados importados/versões distintas mostra limitação.

`rewrites(id PK, request_id UNIQUE FK ai_requests, network_attempt_id UNIQUE FK ai_attempts, origin_key, level CHECK B1|B2|C1, json, spoken_text, created_at)` append-only; FK/origin/level têm de corresponder ao request. `rewrite_audio(rewrite_id FK,config_hash,relative_file,text_hash,voice,speed,state:pending|ready|failed, PRIMARY KEY(rewrite_id,config_hash))`; metadados deriváveis podem ser atualizados, texto válido não. `practice_preferences` singleton com `rewrite_level` default B1, versão e data; separado de configuração/consentimento IA.

Recibos novos em `ai_receipts` já existente (kind/hash/entity) para comandos sem rede e mutações; não alargar CHECK de command_receipts sem necessidade. Decisão concreta: resolvedor `lookup_command_identity(requestId)` consulta `ai_attempts.command_id`, `ai_receipts.id` e `command_receipts.request_id` dentro da transação antes de toda reserva/mutação nova; mais de uma ocorrência histórica é recusada como conflito, não escolhida arbitrariamente. Mesma tabela/kind/hash devolve efeito original; ocorrência noutra tabela ou conteúdo diferente recusa reutilização. Os escritores antigos passam pelo mesmo preflight sob transação SQLite, evitando corridas entre famílias sem mudar IDs antigos. Mudança de nível/preferência não envia nada.

### 2.3 Histórias e primeira captura

`StoryInput {title,situation,task,actions,outcome,competencies}`: trim exterior em todos os campos/competências, título 1…200 bytes, cada nota até 16.000 bytes, soma notas até 48.000, 1…20 competências de até 120 bytes. Exigir pelo menos uma nota não vazia e uma competência; deduplicar competências exatas após trim, preservar interior. Não exigir resultado nem STAR completo. Completeness é rótulo derivado dos campos em falta, não avaliação de qualidade. Filtro local simples por competência normalizada com trim e comparação sem distinção de maiúsculas.

`SelectStory {requestId,owner:CaptureOwner,expectedPreparationVersion,storyId:null|Id,expectedStoryVersion:null|Version}` só permite initial corrente de entrevista ou prática pendente antes de primeira captura bem-sucedida. Confirmar versão da história no commit; cliente antigo não seleciona silenciosamente conteúdo diferente. Snapshot contém cópia de StoryInput e IDs/versão exclusivamente locais.

Antes de abrir microfone, transação reserva recording e candidato imutável de snapshot/ausência (`pending_start`). Se essa transação falha, não abrir microfone. Ao `AudioPort::start_capture` ter sucesso, manter imediatamente owner/recording/candidato no estado em memória e bloquear edição do apoio; marcar started/locked em SQLite. Se esta escrita falha, **não parar captura**: snapshot expõe `pendingPreparation`, retry de metadados é local, terminar manualmente continua acessível e publicação guarda áudio antes de depender do snapshot. Abandono/nova gravação/fecho bem-sucedido aguardam resolver pendência; fecho solicitado pelo utilizador pode finalizar áudio, mas não indicar sucesso enquanto houver escrita pendente.

Para recuperação da janela entre áudio e SQLite, criar na pasta já reservada `preparation-start.json` com recordingId, snapshot/hash e resultado de arranque (`pending|started|failed`), por escrita atómica. O candidato durável existe antes do hardware. Sucesso fixa marcador started e DB e move prática de preparing para answering; falha conhecida fixa failed e liberta preparação, mesmo que um retry de escrita seja necessário. Falha de marcador após sucesso deixa candidato/estado em memória e pendência visível; nunca apagar pasta com original finalizado.

No arranque, started ou original.wav finalizado permite consolidar snapshot candidato; failed não fixa apoio e mantém gravação interrompida para consulta. Pending sem evidência suficiente significa `start_uncertain`: conservar candidato, mostrar interrupção/incerteza e exigir resolução local explícita antes de editar apoio/regravar. «Manter este apoio e continuar» fixa candidato; «O microfone não chegou a iniciar» regista decisão do utilizador e liberta seleção, conservando auditoria. Não fingir atomicidade entre dispositivo, ficheiro e SQLite. Nunca inferir notas novas da história editada durante recuperação. Mesmo sem história, persistir ausência fixada para impedir seleção retroativa.

### 2.4 Reformulação, avaliação e limites

`Database::load_exercise_sources(&ResponseOrigin)->Result<ExerciseSources>` resolve pergunta/critérios fixos e confirmações; original exige sessão concluída e confirmação inicial; practice exige status confirmed. `ExerciseSources` contém `origin`, `AiSources` e versões/rubrica, **sem Preparation/Story**. DTOs de transporte serializam apenas `{question,criteria,answers}`; identidade de origem/IDs da história não entram na mensagem. Não chamar `ai_sources(...,"feedback")` para original local: criar resolvedor de fontes de exercício separado, preservando restrição local no feedback da entrevista.

Ampliar `TransportRequest` com `task: AiTask` tipado (`FollowUp`, `OriginalFeedback`, `PracticeFeedback{rubricVersion}`, `Rewrite{level,rubricVersion}`) em vez de tratar todo purpose não-follow_up como feedback. Uma única função build_body escolhe prompts/máximo de saída. Adaptar chamadas originais sem mudar os bodies efetivos da fase 3; manter testes de regressão. Só papel `attempt` numa avaliação/reformulação de prática, sem inicial antiga ou seguimento antigo. Validador atual compara papel com fontes; explicitar conjunto de papéis autorizado e prompt de prática, não substituir attempt por initial.

Prompt rewrite `rewrite-v1`, esquema `rewrite-v1`, rubrica factual `factual-rewrite-v1`; saída:

```text
{schemaVersion:"rewrite-v1", level:"B1"|"B2"|"C1",
 segments:[{id,text,evidence:[{confirmationId,role,quote}]}],
 necessaryChanges:[{source:{confirmationId,role,quote},segmentIds:[Id],explanation}],
 optionalStyle:[{source:{confirmationId,role,quote},segmentIds:[Id],explanation}],
 missingInformation:[{dimension,assessedConfirmationIds,preparationQuestion,explanation}]}
```

Exigir 1…40 segmentos, IDs únicos, texto não vazio até 2.000 bytes por segmento, máximo 12.000 bytes de texto total unido por `\n\n`. O texto de voz é construído **só** dessa união; o modelo não fornece segundo campo de voz divergente. Cada segmento tem 1…10 citações literais não vazias de fontes autorizadas (até 64.000 bytes por citação). Alterações necessárias/estilo até 20 por lista, fonte literal e 1…40 segmentIds existentes únicos, explicação PT até 4.000 bytes. Lacunas até seis dimensões STAR únicas, IDs de todas as fontes avaliadas, pergunta de preparação inglesa até 500 bytes e explicação PT até 4.000. JSON sem campos extras/fences, nível igual ao pedido; rejeitar referências inválidas/segmentos sem evidência ou resultado truncado completo. Nenhum placeholder/explicação é introduzido pelo programa no texto de voz; prompt proíbe marcadores e factos novos. Validação literal não garante fidelidade da paráfrase nem ausência semântica de placeholders; apresentar limitação e testar manualmente.

Prompt de avaliação prática `practice-feedback-v1`, esquema existente `ai-v3-1`, rubrica `star-text-v1` quando compatível com família. Mesmo JSON/limites/citações/ausências/classificações/correção versus estilo/prioridades da fase 3, com papel `attempt` e só uma confirmação; não incluir campo de seguimento ou nota. Registar rubrica e fonte em cada relatório próprio.

Limites existentes preservados: 64.000 bytes UTF-8 por confirmação, 16.000 para pergunta+critérios, 160.000 body serializado, resposta HTTP 1 MiB, timeout 90 s/ligação 10 s, sem redirects/retries. Rewrite `max_tokens=4.000`; avaliação prática 4.000. Resultado que não cabe é falha recuperável, não JSON parcial. Preferências não alteram limites de textos guardados previamente; excesso só impede operação que não pode processá-lo, com mensagem sem perda.

Áudio não tem temporizador de paragem: manter capacidade de captura e deadlines de transcrição proporcionais à duração já existentes no worker. Não introduzir timer de aviso ou limite novo de duração nesta fase; «Terminar gravação» permanece por ação do utilizador. Não recortar áudio nem prometer recuperar gravação ainda não finalizada. Uma falha de transcrição preserva original e permite retry/revisão manual como atualmente. Texto de síntese da reformulação é limitado aos 12.000 bytes já validados; síntese local não custa API.

### 2.5 Áudio, concorrência, recuperação e custos

Reutilizar raiz de artefactos `sessions/<sourceSessionId>/<answerSlotId>/<recordingId>/`: prática usa slot exclusivo e sessão apenas como proveniência, sem linha fictícia de sessão ativa. Conferir proprietário no núcleo em todos os caminhos. `PendingRecording`/reconciliação ganham owner discriminado para não perder práticas ao procurar sessão ativa. Operações locais existentes podem usar session_id de proveniência e slot como atualmente; resultado aplica-se ao seu owner e geração, não ao treino apresentado.

Adicionar `AudioSelection::Rewrite{rewriteId}` e `PracticeQuestion{practiceId}`. Cache reformulação em `rewrite-cache/<rewriteId>/<sha256(texto+voz+velocidade+versão)>.wav`, resolvido no núcleo, com metadata em rewrite_audio. Texto guardado antes de sintetizar; ausência de derivado reabre só trabalho local. Captura interrompe reprodução; uma única operação local de transcrição/síntese; publicação tardia não inicia reprodução se começou captura entretanto. Cache original de perguntas não é movida. Reconciliação valida caminhos, falta/ficheiro incompleto e guarda aviso sem apagar dados; síntese falhada conserva texto.

`ReservedAttempt` atual evolui para guardar origem/finalidade/variant/generation; nomes de DTO novos dizem networkAttemptId. Reserva valida resultado válido existente antes de ocupar slot remoto: mesma variante devolve existente sem cobrança. Novo requestId após falha cria nova chamada ligada ao pedido lógico; replay do mesmo id é no-op. `AiRuntime` único de busy/pending continua a coordenar todas finalidades; publicação pendente bloqueia nova rede até ser resolvida, mas não áudio/consulta. finish_ai faz dispatch por enum, compara origem/fontes/generation/variante, grava consumo e só depois aplica resultado no alvo correto. Novo treino aberto não invalida fontes imutáveis antigas nem faz resultado tardio alterar a seleção de nível da UI.

Preparar fecho: finalizar captura por pedido explícito, guardar revisão conforme guard existente e tentar publicação local pendente. Se pendência de escrita não foi resolvida, não declarar fecho seguro; não aguardar rede 90 s, marcar operação interrompida/incerta e invalidar geração em memória. No reinício não reenviar rede ou recomeçar captura automaticamente; resultado remoto já recebido mas perdido após crash permanece incerto. Nunca limpar pacote pending antes de commit local.

`load_training_usage(sessionId)->TrainingUsageView` devolve `originalInterview`, `laterPractice`, `combined` (cada um SessionUsage e contagens por finalidade); seleção por DISTINCT ai_attempts.id. Original = follow_up/feedback existentes; posterior = rewrite (inclusive original) e practice_feedback. `load_practice`/relatório mostram subset da prática, sem acrescentá-lo novamente ao combined. Derivar consumo sem UPDATE no relatório original. Custos reportados/estimados continuam separados, tarifas copiadas por chamada, resultados inválidos/descartados incluídos, unknowns/partial visíveis. Não renovar preços como parte desta fase.

### 2.6 Métodos públicos e integração UI

Métodos novos de `AppService` async retornam `Result<T,AppError>`; respetivos comandos `interview_<nome_snake_case>` e InterviewClient camelCase:

```text
list_stories(competency:Option<String>)->Vec<StoryView>
create_story(requestId:Id,input:StoryInput)->StoryView
update_story(requestId:Id,id:Id,expectedVersion:Version,input:StoryInput)->StoryView
select_story(SelectStory)->InterviewSnapshot
resolve_preparation(recordingId:Id,requestId:Id,decision:keep|did_not_start)->InterviewSnapshot
retry_preparation_publication(recordingId:Id)->InterviewSnapshot
create_practice(CreatePractice)->PracticeDetail
load_practice(practiceId:Id)->PracticeDetail
list_practices(sessionQuestionId:Id)->Vec<PracticeDetail>
abandon_practice(PracticeCommand)->PracticeDetail
start_practice_recording(owner:CaptureOwner,expectedSelection:Version)->InterviewSnapshot
save_practice_review(requestId:Id,target:PracticeAnswerTarget,expectedReviewVersion:Version,
  sourceTranscriptId:Option<Id>,origin:ReviewOrigin,text:String)->InterviewSnapshot
confirm_practice_answer(requestId:Id,target:PracticeAnswerTarget,expectedReviewVersion:Version,
  expectedPracticeVersion:Version,sourceTranscriptId:Option<Id>,origin:ReviewOrigin,text:String)->PracticeDetail
transcribe_practice_response(target:PracticeAnswerTarget)->InterviewSnapshot
evaluate_practice(EvaluatePractice)->PracticeReportView
load_practice_report(practiceId:Id)->PracticeReportView
request_rewrite(RewriteCommand)->RewriteView
list_rewrites(origin:ResponseOrigin)->Vec<RewriteView>
compare_responses(ComparisonRequest)->ComparisonView
set_rewrite_level(requestId:Id,expectedVersion:Version,level:RewriteLevel)->RewritePreference
load_training_usage(sessionId:Id)->TrainingUsageView
```

`RewriteView {id:null|Id,origin:ResponseOrigin,level:RewriteLevel,state:pending|running|failed|ready,segments,spokenText:null|String,necessaryChanges,optionalStyle,missingInformation,versions:ReportVersions,usage:SessionUsage,networkAttempts:NetworkAttemptView[]}`; listas de conteúdo usam contrato de §2.4 e ficam vazias até ready, que sempre tem id e conteúdo validado. `PracticeReportView` acrescenta `schemaVersion`, usa `networkAttempts:NetworkAttemptView[]` e `usage:SessionUsage`; `report` tem JSON validado da fase 3 com fontes attempt. Reutilizar stopRecording/retrySaveRecording/playAudio/stopAudio/retryAiPublication/prepareClose, que passam a resolver owner internamente. Nova consulta prática não retargeta captura/trabalho. Criar/confirmar/abandonar são idempotentes e usam versões. Nenhuma página recebe segredo; novos pedidos passam mesmas guardas de configuração/consentimento existentes.

UI: banco de histórias na navegação; apoio opcional antes de gravar e leitura após lock; prática no histórico sem entrevista artificial; formulário/revisão partilhados com guard de guardar/descartar/cancelar. Abandono tem texto claro de conservação e leitura futura. RewritePanel mostra B1/B2/C1, variantes válidas, alterações necessárias/estilo e lacunas fora da voz; «Sugestão da IA — confirme os factos». Comparison distingue initial/follow_up/attempt, áudio e automático/confirmado, sugestão em zona separada; report pendente/diferente rubrica não parece melhoria comprovada. Custos mostram treino posterior separado. Durante rede pode navegar, gravar quando elegível e retomar sem repetir chamada.

## 3. Tarefas executáveis futuras

Cada tarefa segue vermelho→verde: criar testes nomeados com os valores/contratos de §2, executar e confirmar falha pelo comportamento ausente, implementar só o necessário, repetir checks. Comandos abaixo são **instruções futuras**, não evidência de execução. Cargo poderá exigir `$env:USERPROFILE\.cargo\bin\cargo.exe`, conforme setup existente. Não criar commits, publicar ou integrar sem instrução aplicável à futura execução.

### Tarefa 1 — Migração v3 e propriedade das respostas

**Ficheiros:** criar `db/migrations/003_improve_repeat.sql`, `practice.rs`, `db/practice.rs` e `src-tauri/tests/phase4_migration.rs`; alterar `db/mod.rs`, `interview/types.rs`, `db/{answers,sessions,operations,ai}.rs`.

**Interfaces:** produzir tipos §2.1, tabelas §2.2 e `resolve_capture_owner`. Inicialmente expor consultas de práticas sem rede; original/follow_up mantêm contratos anteriores. Preservar CHECKs, triggers, caminhos e conteúdo dos dados existentes.

- [ ] Escrever `migrates_v0_v1_v2_preserving_all_source_ids`, `migration_rollback_restores_v2`, `future_version_is_refused_without_write`: fixture temporária v2 com sessão local/IA, seguimento, revisões/confirmados, relatório, tentativas falhadas e recibos; comparar valores e foreign_key_check vazio.
- [ ] Escrever `slot_owner_cannot_cross_practice_or_source_question`, `multiple_attempt_slots_keep_unique_initial_and_follow_up` e `pending_practice_and_active_interview_are_mutually_exclusive`, incluindo SQL direto para tentar violar constraints.
- [ ] Executar `cargo test --manifest-path src-tauri/Cargo.toml --test phase4_migration`; observar falhas esperadas; implementar migração/resolvedor e repetir até zero falhas.
- [ ] Executar testes Rust existentes de migração/sessões/answers/IA para comprovar regressão; atualizar fixtures só quando novos campos exigirem, sem enfraquecer assertions antigas.

### Tarefa 2 — Ciclo local completo de prática

**Ficheiros:** criar `service/practice.rs`, `src-tauri/tests/practice_service.rs`; adaptar `service{.rs,/interview.rs}`, `db/answers.rs`, `artifacts.rs`, comandos/lib e `practice.rs`.

**Interfaces:** consumir owner/resolvedor da tarefa 1; produzir create/load/list/abandon, revisão/confirmação e adaptação partilhada de captura/transcrição. Estado de prática independente do trabalho IA. Captura/worker/reprodução permanecem únicos.

- [ ] Escrever testes criação idempotente a partir de local_v2/ai_v3 concluídas; recusar sessão corrente, treino JSON, origem sem confirmação, predecessor de outra pergunta e segunda prática pendente.
- [ ] Testar regravação conserva tomadas e ordinal; confirmação imutável exata; transcrição falha e retry usa mesmo ficheiro; falha de publicação mantém gravação anterior/áudio final; abandono conserva revisão/gravações e proíbe reabertura/avaliação.
- [ ] Testar comando antigo para original não captura no slot practice; evento tardio aplica só ao owner; prática confirmada liberta treino novo; reiniciar retoma revisão e estado sem abrir microfone. Captura prolongada num relógio controlado não chama stop_capture; paragem continua exclusivamente manual e deadlines proporcionais do worker mantêm comportamento existente.
- [ ] Executar `cargo test --manifest-path src-tauri/Cargo.toml --test practice_service`; confirmar vermelho, implementar auxiliares partilhados e repetir verde. Repetir service/artifacts existentes e conferir totais/seleções/relatórios originais inalterados.

### Tarefa 3 — Histórias locais e snapshot no arranque da captura

**Ficheiros:** criar `db/stories.rs`, `src-tauri/tests/stories.rs`; estender `service/practice.rs`, `service/interview.rs`, `artifacts.rs`, comandos e tipos.

**Interfaces:** produzir list/create/update/select/resolve/retryPreparation de §2.3/§2.6. O resolvedor de fontes IA nunca recebe StoryInput/PreparationView.

- [ ] Testar trim exterior, história parcial válida, limite exato/excesso, versão concorrente, filtro e append-only; sem comandos de eliminação.
- [ ] Testar snapshot da versão escolhida na primeira captura com sucesso; editar história depois não muda snapshot; remover apoio antes da captura fixa ausência; seguimento partilha principal; falha conhecida de microfone permite escolher outro apoio.
- [ ] Injetar falha SQLite antes de start: nenhum microfone aberto. Falha após start: captura permanece ativa, Terminar funciona e áudio é preservado; retry local consolida mesmo snapshot sem consultar versão atual. Reinício com marcador started/original finalizado consolida; pending ambíguo mostra start_uncertain e resolve explicitamente sem descartar ficheiros.
- [ ] Executar `cargo test --manifest-path src-tauri/Cargo.toml --test stories`; confirmar falha, implementar e repetir verde. Testar marcador exclusivo `ONLY_IN_STORY_742` nunca pré-preenche revisão/confirmado/transcrição e não entra no resolvedor de fontes.

### Tarefa 4 — Contratos de IA para reformulação e avaliação isolada

**Ficheiros:** criar `ai/rewrite.rs`, `src-tauri/tests/rewrite_validation.rs` e fixture `contracts/practice-v4.example.json`; adaptar `ai/{mod,validation,transport}.rs` e fontes em `db/rewrites.rs`.

**Interfaces:** `load_exercise_sources`, `AiTask`, `validate_rewrite(raw:&str,sources:&AiSources,level:RewriteLevel)->Result<ValidatedRewrite>`, `build_rewrite_messages(&AiSources,RewriteLevel)->Result<Vec<Value>>`. Reutilizar validate_feedback para papel attempt com contrato de prompt explícito, limites originais intactos.

- [ ] Escrever testes de segmentos/citações Unicode corretos, papel/source errado, referências órfãs, nível diferente, JSON extra/truncado, segmento sem evidência, excesso bytes e ausência com fontes incompletas. Testar spokenText igual exatamente à união dos segmentos, sem explicações/lacunas anexadas.
- [ ] Capturar body de original local/IA com seguimento versus prática: prática contém apenas confirmação attempt, nunca resposta antiga/relatório/rewrite/história. Marcador `ONLY_IN_STORY_742` ausente em todas finalidades, inclusive follow_up e feedback antigos.
- [ ] Fixtures com equipa versus indivíduo, negação, incerteza, número e resultado qualitativo/negativo exercitam contrato e instruções do prompt; não afirmar que teste literal garante fidelidade semântica. Resposta com instruções maliciosas continua serializada como dados.
- [ ] Executar `cargo test --manifest-path src-tauri/Cargo.toml --test rewrite_validation`; confirmar vermelho, implementar e repetir verde. Regressões de build_body/feedback/HTTP demonstram bodies/limites antigos preservados e ausência de retry pago.

### Tarefa 5 — Pedidos persistentes e resultados imutáveis

**Ficheiros:** criar `db/rewrites.rs`, `src-tauri/tests/practice_ai.rs`; adaptar `db/ai.rs`, `service/ai.rs`, `service/practice.rs`, comandos e tipos.

**Interfaces:** produzir request/listRewrite e evaluate/loadPracticeReport, preferências B1 e integração de finalidades/variantes em ReservedAttempt. Reutilizar AiRuntime, consentimento e pending de publicação; não criar segundo gate remoto.

- [ ] Testar mesma origem/nível/versões devolve variante pronta sem rede; mudar B1→B2 cria variante nova; mudar preferência não altera anteriores. Replay idempotente, requestId diferente após erro cria só nova networkAttempt; colisão de IDs/kinds é recusada.
- [ ] Testar avaliação exige prática confirmada e só role attempt, não faz seguimento, mantém original; relatório válido não é regenerado. Rubrica familiar nasce da original existente ou star-text-v1 na primeira avaliação e fica fixa; versão incompatível produz erro explícito sem substituir histórico.
- [ ] Usar transporte suspenso para resultado B1 após UI selecionar B2/abrir outra prática: só guarda B1 de origem. Nova operação simultânea recebe busy; navegação/áudio elegível continuam. Falha de publicação seguida de retry mantém contador rede 1 e conserva consumo; fechar/reiniciar não reenvia automaticamente.
- [ ] Executar `cargo test --manifest-path src-tauri/Cargo.toml --test practice_ai`; confirmar vermelho, implementar transações e dispatch de finalidades, repetir verde e `--test ai_service` existente, que inclui cenários de feedback.

### Tarefa 6 — Voz da sugestão, comparação e consumo

**Ficheiros:** estender `artifacts.rs`, `service/practice.rs`, `service/interview.rs`, `db/rewrites.rs`, `ai/usage.rs`; criar `src-tauri/tests/practice_outputs.rs`.

**Interfaces:** produzir AudioSelection Rewrite/PracticeQuestion, ComparisonView e TrainingUsageView. Texto validado é única fonte de voz; cache/index resolve ID sem caminhos externos.

- [ ] Testar Kokoro recebe só spokenText; nível/texto/voz mudam cache; ficheiro em falta ou falha síntese repete só auxiliar e não HTTP; captura interrompe playback e resultado de síntese tardio não toca sobre microfone.
- [ ] Testar comparação original+prática ou duas práticas da mesma principal; recusar rascunho/abandonada/outra pergunta. Preservar initial/seguimento separados, áudio automático/confirmado e relatório/versões; indicar versões diferentes e relatório ausente sem criar avaliação.
- [ ] Testar custos com retry falhado, resultado descartado, rewrite original e prática: originalInterview/laterPractice/combined por conjuntos de IDs disjuntos e combined conta cada rede uma vez. Consumo null permanece incompleto; áudio local não acrescenta custo API.
- [ ] Executar `cargo test --manifest-path src-tauri/Cargo.toml --test practice_outputs`; confirmar vermelho, implementar e repetir verde, incluindo reconciliação de derivado ausente e caminho inválido.

### Tarefa 7 — Interface completa de treino posterior

**Ficheiros:** criar contratos/componentes frontend de §1 e testes; adaptar cliente/fake, App, History, Interview, useInterview e CSS; registar comandos Tauri efetivos.

**Interfaces:** consumir apenas DTOs/comandos §2.6, sem regras persistentes ou composição de prompts no frontend. Resolvedor de rascunho recebe owner e mantém uma única revisão editável por gravação.

- [ ] Escrever testes Tentar novamente sem feedback/rewrite; bloqueio por entrevista/prática pendente; retoma/abandono preserva dados; guardar/descartar/cancelar antes de navegação/fecho; captura/prática sem sessão fictícia.
- [ ] Testar história parcial, seleção/substituição/remoção antes de captura, lock/snapshot após sucesso e erro de metadados sem esconder Terminar; edição de banco não muda apoio guardado nem preenche resposta.
- [ ] Testar nível B1 default e variantes, pedir explicitamente, ouvir apenas sugestão, avaliação attempt, retry de publicação separado de rede, comparação com fontes/seguimento/sugestão e rubrica limitada; custos posteriores separados e desconhecidos rotulados.
- [ ] Testar resposta remota fora da página e para outro nível não substitui seleção; carregar histórico/comparação/confirmar/editar história não envia API. Sem credencial mantém funções locais.
- [ ] Executar `npm.cmd test`; observar falhas relevantes, implementar interface integrada e repetir `npm.cmd run typecheck`, `npm.cmd test`, `npm.cmd run build`. Tests fakes reproduzem recusa de domínio; não usar fake para contornar requisitos Rust.

### Tarefa 8 — Verificação futura e documentação de execução

**Ficheiros:** criar `docs/phase-4/{setup,validation}.md`; atualizar README, AGENTS.md, stack apenas se decisão mudou, plano das fases e estado de spec/plano após execução autorizada. Documentação só deve afirmar resultados realmente obtidos.

- [ ] Executar futuro `.\scripts\verify.ps1` completo; registar contagens/saídas reais, corrigir falhas e repetir verificações afetadas. Build release `npm.cmd run tauri -- build --no-bundle` e smoke com dados temporários, sem tocar dados pessoais.
- [ ] Conferir diff contra F4-01…16, links locais, `git diff --check`, contratos Tauri/fake e nenhum identificador/notas de história em cargas. Não usar chave real/inferência paga em testes automáticos.
- [ ] Criar roteiro manual Windows F4-16: original com seguimento, origem local, resultado sem números/negativo/aprendizagem, duas práticas, história com factos não ditos, variantes B1/B2/C1, voz Kokoro, regravação, retoma e rede indisponível. Medir espera/fidelidade/utilidade/custo apenas quando executado futuramente com autorização.
- [ ] Marcar diferenças entre automação, inferência real e aceitação humana; manter todas pendências anteriores. Fase 4 não fica aceite só porque os checks passam. Entregar relatório de ficheiros, decisões/desvios, comandos reais e limitações ao coordenador.

## 4. Cobertura e auto-revisão documental

| Critérios | Tarefas e resultado verificável futuro |
| --- | --- |
| F4-01 | 1: migração vazio/v1/v2, rollback, futuras e invariantes SQL |
| F4-02, F4-03 | 4–5: cargas por origem, segmentos/citações, versões; fidelidade humana na 8 |
| F4-04 | 5, 7: níveis, reutilização, variantes imutáveis e preferência local |
| F4-05 | 6–7: síntese sem explicações e recuperação local |
| F4-06, F4-07, F4-08 | 1–2, 7: owner, versões, prática exclusiva, abandono e retoma |
| F4-09 | 4–5: papel attempt, fonte única, relatório próprio e retry |
| F4-10 | 6–7: comparação local e versões/pendências explícitas |
| F4-11, F4-12 | 3–4, 7: histórias, snapshot, marcador exclusivo e payloads isolados |
| F4-13, F4-14 | 2–5, 7: concorrência, generations, falhas, publicação, fecho e restart |
| F4-15 | 6–7: consumo disjunto por IDs e subtotais parciais |
| F4-16 | 8: validação humana futura, não produzida por esta tarefa documental |

Plano elaborado a partir do código do checkout e da spec referida, sem depender de preços novos ou funcionalidades externas não verificadas. Interfaces públicas novas definidas em §2; operações internas podem ser divididas em auxiliares privados sem alterar invariantes. A snapshot de preparação tem estado de incerteza explícito porque dispositivo/ficheiro/SQLite não formam uma transação comum. Os oito resultados são testáveis separadamente, mas a ordem respeita dependências: proprietários → prática/histórias → contratos IA → persistência IA → saídas → UI → evidência. A aprovação técnica deste documento não autoriza executar as tarefas; o pedido atual termina com spec e plano revistos.

# Conversa sobre artigos — plano de implementação

> **Para execução por agentes:** usar a skill `executing-plans` para execução direta, tarefa a tarefa; `subagent-driven-development` apenas se essa modalidade for escolhida pelo utilizador. Os passos usam caixas de verificação para acompanhamento.

**Objetivo:** conversar em inglês sobre um artigo, com turnos ligados entre si, envio automático autorizado e feedback final sobre coesão, coerência, desenvolvimento e interação.

**Arquitetura:** domínio próprio de artigos no mesmo núcleo Rust e SQLite, reutilizando captura, modelos locais, credenciais, transporte HTTP e normalização de consumo. React apresenta fotografias do estado persistido. A voz incremental entrega blocos apenas depois de o texto completo do tutor ter sido validado e guardado.

**Stack:** React/TypeScript, Tauri 2/Rust, rusqlite, CPAL/Rodio, auxiliar Python com faster-whisper/Kokoro e OpenCode Go. Sem novas dependências previstas.

**Spec:** [desenho aprovado](../specs/2026-11-04-article-conversation-design.md).

**Estado:** execução concluída em 2026-11-05 no ramo `feat/article-conversation`: tarefas 1 a 9 implementadas, com `verify.ps1` 8/8 (354 Rust, 147 Vitest, 56 Python), build `--no-bundle` e ensaio com dados temporários com saída 0. Desvios registados em [validação](../../article-conversation/validation.md) (sem capability no `health`, limite do artigo em 30 000 bytes, ramos antigos só em base de dados, sem regravação como ramo). **Pendente e não feito**: medições reais de desempenho e fidelidade (tarefa 8), roteiro manual com áudio, voz e inferência reais (tarefa 9) e validação visual. Nada disto é aceitação manual.

## Restrições globais

- Interface e explicações em português de Portugal; perguntas, comentários e respostas de treino em inglês.
- O utilizador começa e termina a resposta manualmente. Sem classificação de pronúncia, ritmo ou confiança emocional.
- Correções apenas no final; preservar áudio, transcrição, versões enviadas e ramos anteriores.
- Modo separado de STAR: não alterar a confirmação obrigatória nem o limite de seguimentos existentes.
- Guardar antes de enviar; apenas texto necessário segue para o fornecedor, nunca áudio ou chave nos logs.
- Consentimento explícito por sessão para artigo e envio automático; destino alterado ou autorização revogada bloqueiam novos envios.
- Identidade Tauri e diretório de diagnóstico preservados; operações novas nas listas fechadas.
- Sem inferência paga nos testes automáticos. Sem importação URL/PDF, GPU ou transcrição durante a gravação nesta entrega.
- Não iniciar fase 5 nem marcar fases anteriores como aceites.

## Foco de revisão

1. Clique duplo ou retoma durante um envio: um turno e uma aplicação do resultado (tarefas 2 e 4).
2. Artigo com instruções hostis, Unicode e dimensão excessiva: tratar como dados e recusar excesso sem cortar (tarefas 1 e 3).
3. Correção de um turno antigo com respostas posteriores: ramo novo, sem reinterpretar descendentes antigos (tarefa 5).
4. Interrupção de voz entre dois blocos ou perda do dispositivo: não reiniciar áudio atrasado nem perder texto (tarefa 7).
5. Revogação ou mudança de destino enquanto há rede em curso: não aplicar resposta obsoleta nem ocultar consumo conhecido (tarefas 3 e 4).

## Decisões técnicas e interfaces comuns

Estas decisões concretizam pontos deixados ao plano; não são medições de desempenho:

- Artigo entre 1 e 12 000 caracteres Unicode após remover espaços exteriores; título opcional até 200. Uma resposta transcrita pode ter até 8 000 caracteres para envio. Excesso conserva o original e pede revisão explícita; nunca corta automaticamente.
- Até 20 respostas por ramo; sugerir conclusão após cinco. Contexto de artigo e falas até 48 000 bytes UTF-8. Reservar 16 000 bytes dentro desse teto para a resposta seguinte; se já não houver essa margem, oferecer conclusão antes de outra gravação. Respeitar também o limite HTTP existente de 160 000 bytes, incluindo instruções e serialização. Artigo excessivo em bytes é recusado antes de criar a sessão.
- Comentário do tutor até 500 caracteres e pergunta até 300; abertura só contém pergunta. Feedback contém as quatro dimensões com estado `clear|partial|insufficient_evidence`, evidências e até três melhorias. A qualidade semântica continua a exigir validação manual.
- Uma conversa de artigo ativa de cada vez; pode coexistir guardada com uma entrevista STAR, mas só um proprietário pode usar captura/modelos/reprodução. A navegação não inicia nem retoma operações externas.
- Esquema SQLite v4 com tabelas próprias `article_sessions`, `article_branches`, `article_turns`, `article_versions`, `article_requests`, `article_reports`, `article_artifacts`, `article_receipts`. Não reutilizar confirmações STAR para envios automáticos.
- `article_sessions`: texto fixo, título, versão otimista, ramo ativo, estado e autorização com destino/versão do aviso. Revogação incrementa versão e invalida geração em curso.
- Ramos guardam pai e turno de bifurcação. Turnos e versões são imutáveis; o percurso ativo é prefixo ancestral até à bifurcação mais turnos do novo ramo. Uma correção cria uma versão do turno de bifurcação e substitui apenas essa versão na projeção do novo ramo.
- Pedidos guardam operação, geração, ramo, fontes exatas, configuração pública, contexto textual exato, estado e consumo. Não guardar a chave. Relatórios fixam IDs de versões e ramo. Recibos tornam comandos repetidos idempotentes; repetir após falha cria uma tentativa identificada, sem retry HTTP oculto.
- Artefactos exclusivos em `articles/<sessionId>/...`, resolvidos pelo núcleo. Eliminação lógica impede novas operações antes da limpeza dos ficheiros; ficheiros não removidos entram em reconciliação, sem apagar pesos ou caches partilhadas.
- Estados: `draft`, `awaiting_tutor`, `ready`, `recording`, `transcribing`, `needs_review`, `recoverable_error`, `finishing`, `completed`, `paused`. Estado de reprodução é independente; transições validadas no núcleo.

Criar `src-tauri/src/article/types.rs` e equivalentes camelCase em `src/lib/article-contracts.ts`:

```rust
struct ArticleCommand { request_id: String, session_id: String, expected_version: u64 }
struct ArticleCreate { request_id: String, title: Option<String>, text: String }
struct ArticleConsent { command: ArticleCommand, destination: String, notice_version: String }
struct ArticleCorrection { command: ArticleCommand, turn_id: String, text: String }
struct ArticleSnapshot { session: ArticleSessionView, turns: Vec<ArticleTurnView>, report: Option<ArticleReport>, audio: ArticleAudioView }
```

As views contêm apenas dados públicos: IDs/versões/estado, texto e origem de cada versão (`automatic_sent|manual_review`), reprodução disponível, erro recuperável e consumo opcional. Não expor caminhos arbitrários nem credenciais. Os detalhes de campos de apresentação podem ser definidos na tarefa 1 sem mudar estas identidades.

## Mapa de ficheiros

- Novo `src-tauri/src/article/{mod,types,state}.rs`: contratos e transições puras.
- Novo `src-tauri/src/db/articles.rs` e migração `004_article_conversation.sql`: transações, projeção de ramos e recuperação.
- Novo `src-tauri/src/ai/article.rs`: contexto, prompts e validação; adaptar `ai/transport.rs` para partilhar HTTP sem converter artigos em fontes STAR.
- Novo `src-tauri/src/service/article.rs` e `article_commands.rs`: orquestração e comandos; registar em `service.rs` e `lib.rs`.
- Novos `src/lib/article-{contracts,client}.ts` e `src/features/articles/{ArticleSetup,ArticleConversation,ArticleReport,useArticle}.tsx`: percurso e subscrição. O hook pode usar extensão `.ts` se não contiver JSX.
- Alterar `App.tsx`, `features/layout/TutorNavigation.tsx`, estilos e guarda de navegação para integrar a página e a propriedade do áudio.
- Alterar auxiliar, protocolo Rust, supervisor e reprodução para blocos de voz, mantendo os pedidos antigos.
- Testes novos por tarefa em `src-tauri/tests/article_*.rs`, `src/features/articles/*.test.tsx` e testes Python existentes. Acrescentar exemplos em `contracts/article-v1.examples.json` e no contrato do auxiliar.

## Tarefa 1 — domínio, limites e contratos

**Ficheiros:** novos módulos `article` e contratos/exemplos acima; `src-tauri/src/lib.rs`; teste `src-tauri/tests/article_domain.rs`.

**Interfaces:** `validate_article(input: &ArticleCreate) -> Result<()>`; `transition(state: ArticleState, event: ArticleEvent) -> Result<ArticleState>`; tipos comuns publicados em `article::types`.

- [ ] Escrever testes `article_limits_are_unicode_aware` (12 000 caracteres aceites, 12 001 recusados; vazio recusado), `article_context_reserves_next_answer`, `automatic_send_requires_consent` e `recording_only_stops_on_user_action`.
- [ ] Executar `cargo test --manifest-path src-tauri/Cargo.toml --test article_domain`; confirmar falha por interfaces ausentes.
- [ ] Implementar tipos, estados e validadores; definir exemplos de abertura, turno, correção, relatório e erro. Fixar aviso `article-auto-v1` com artigo, envio por turno, destino e áudio local explícitos.
- [ ] Repetir o comando, exigir saída 0; executar `npm.cmd run typecheck` após contratos TS.
- [ ] Rever diff e criar commit `feat: define article conversation contracts`.

## Tarefa 2 — persistência e migração v4

**Ficheiros:** `db/articles.rs`, `db/migrations/004_article_conversation.sql`, `db/mod.rs`; `tests/article_migration.rs` e `tests/article_repository.rs`.

**Interfaces:** métodos `Database::create_article(input: ArticleCreate) -> Result<ArticleSnapshot>`, `article_snapshot(session_id: &str) -> Result<ArticleSnapshot>`, `apply_article(command: &ArticleCommand, change: ArticleMutation) -> Result<ArticleSnapshot>`; enum de mutações tipado no domínio, sem SQL ou transições fornecidas pelo frontend.

- [ ] Testar v0/v1/v2/v3→v4, versão futura sem escrita e rollback de migração inválida; comparar conteúdo STAR/práticas antes e depois. Testar dois IDs de comando iguais, versão obsoleta, atualização proibida de fontes e eliminação restrita aos ficheiros exclusivos.
- [ ] Executar `cargo test --manifest-path src-tauri/Cargo.toml --test article_migration --test article_repository`; observar falhas iniciais.
- [ ] Implementar migração transacional e tabelas com FKs/triggers; estender o helper de migrações e todos os seus chamadores. Operações síncronas em `spawn_blocking`, sem mutex através de `await`.
- [ ] Repetir os testes e os testes de migração existentes; confirmar uma única sessão de artigo ativa e recibos idempotentes.
- [ ] Commit `feat: persist article conversations and immutable sources`.

## Tarefa 3 — diálogo e feedback na integração IA existente

**Ficheiros:** `ai/article.rs`, `ai/mod.rs`, `ai/transport.rs`; `tests/article_ai.rs`; exemplos do contrato.

**Interfaces:** `ArticleContext { article, turns, branch_id, source_version_ids }`; `ArticleAiTask::{Opening, Reply, Feedback}`; `build_article_body(settings: &AiSettingsView, task: ArticleAiTask, context: &ArticleContext) -> Result<serde_json::Value>`; `validate_article_reply(content: &str) -> Result<ArticleReply>` e `validate_article_report(content: &str, context: &ArticleContext) -> Result<ArticleReport>`.

- [ ] Testar artigos com instruções hostis delimitados como dados, papéis corretos no histórico, limites de bytes, abertura sem comentário, resposta com exatamente um campo de pergunta e relatórios que recusam citações inexistentes/do artigo atribuídas ao utilizador. Não prometer que testes estruturais provam resistência semântica a instruções hostis.
- [ ] Executar `cargo test --manifest-path src-tauri/Cargo.toml --test article_ai`; confirmar falha inicial.
- [ ] Extrair apenas o envio HTTP comum para aceitar corpo já construído e identidade de conversa; manter o trait `AiTransport` existente compatível. Adicionar `ArticleTransport::complete(settings, conversation_id, task, context)` com o mesmo resultado normalizado e credencial obtida no núcleo. O adaptador concreto usa o helper comum, sem duplicar autenticação, limites, timeout ou política de redirects/retries.
- [ ] Construir prompts separados de STAR, esquema `article-v1`, limite de saída 800 tokens para diálogo e 4 000 para relatório; usar `stream:false` existente. Identidade `x-opencode-session` estável por sessão de artigo, incluindo correções/retries. Fontes sempre persistidas antes do envio.
- [ ] Simular transporte e testar HTTP no servidor local usado pelos testes existentes: 401, 429, 5xx, timeout, corpo inválido e consumo ausente; nenhuma chave real. Repetir testes novos e de transporte/consumo existentes.
- [ ] Commit `feat: add grounded article dialogue and final feedback`.

## Tarefa 4 — orquestração, gravação e consentimento

**Ficheiros:** `service/article.rs`, `article_commands.rs`, `service.rs`, `lib.rs`, diagnóstico e `tests/article_service.rs`.

**Interfaces:** comandos `article_create(ArticleCreate)`, `article_authorize(ArticleConsent)`, `article_revoke(ArticleCommand)`, `article_start_recording(ArticleCommand)`, `article_stop_recording(ArticleCommand)`, `article_retry(ArticleCommand)`, `article_finish(ArticleCommand)`, `article_snapshot(session_id)` devolvem `ArticleSnapshot`; `article_list()` devolve resumos; `article_delete(ArticleCommand)` devolve `Result<()>`. Publicar evento `article_snapshot` após commit. Comandos assíncronos devolvem estado pendente; conclusão chega por evento.

- [ ] Testar guardar antes de enviar, envio automático apenas depois de parar e transcrever, ausência de envio com transcrição vazia/falhada, clique duplo, sessão STAR ativa sem captura simultânea, falha de escrita sem rede e snapshot consultado sem efeitos externos.
- [ ] Executar `cargo test --manifest-path src-tauri/Cargo.toml --test article_service`; verificar falhas iniciais.
- [ ] Integrar captura/normalização e worker com proprietário explícito `Article { session_id, turn_id }`; não atribuir eventos de artigos a answer slots STAR. Conservar campos e percursos existentes. A correlação inclui pedido, sessão, ramo, versão e geração.
- [ ] Implementar transições/retry; revogação ou mudança de destino invalida aplicação pendente, conserva consumo conhecido e não finge retirar texto já transmitido. Ao terminar, invalidar pergunta em voo e gerar relatório uma vez sobre a projeção fixada; zero respostas conclui sem avaliação fabricada.
- [ ] Testar revogação durante HTTP, mudança de destino antes do envio, terminar durante HTTP, ausência de retry no arranque e resultado tardio após eliminação. Repetir suite da tarefa e regressões de serviço STAR/práticas.
- [ ] Commit `feat: orchestrate uninterrupted article turns`.

## Tarefa 5 — correções, ramos e recuperação

**Ficheiros:** `db/articles.rs`, `service/article.rs`, `article_commands.rs`, `artifacts.rs`; `tests/article_recovery.rs`.

**Interfaces:** `article_correct(ArticleCorrection) -> Result<ArticleSnapshot>` cria ramo e deixa regeneração pendente de ação; `article_retry` da tarefa 4 regenera nesse ramo. `article_resume(ArticleCommand) -> Result<ArticleSnapshot>` recupera estado local sem HTTP. `project_article_branch(session_id: &str, branch_id: &str) -> Result<ArticleContext>` reconstrói fontes.

- [ ] Testar correção do primeiro turno depois de três respostas: ramo antigo intacto, contexto novo sem descendentes e relatório antigo conservado; testar correções sucessivas em ramos, texto manual após transcrição falhada e retry repetido.
- [ ] Executar `cargo test --manifest-path src-tauri/Cargo.toml --test article_recovery`; verificar falhas iniciais.
- [ ] Implementar projeção ancestral, correção append-only, invalidação de geração e recibos; distinguir correção de reconhecimento de nova tentativa. Repetir uma resposta grava novo turno num ramo, nunca substitui o ficheiro original.
- [ ] Reconciliar ficheiros e operações após reinício; captura interrompida é assinalada, não apresentada como gravação completa. Pedidos pendentes passam a recuperação explícita, sem alegar custo zero. Eliminação pode ser repetida e retoma a limpeza pendente.
- [ ] Repetir testes e verificar crash simulado entre ficheiro/commit/pedido e durante eliminação.
- [ ] Commit `feat: recover and branch article conversations safely`.

## Tarefa 6 — interface da conversa e histórico

**Ficheiros:** cliente/contratos e componentes novos do mapa, `App.tsx`, navegação, estilos e testes `ArticleSetup.test.tsx`, `ArticleConversation.test.tsx`, `ArticleReport.test.tsx`.

**Interfaces:** `ArticleClient` espelha comandos das tarefas 4–5 e `subscribe(handler: (snapshot: ArticleSnapshot) => void): Promise<() => void>`. Cliente falso injeta estados e resultados fora de ordem. Não acrescentar lógica autoritativa de transição no React.

- [ ] Testar artigo/limites, autorização não pré-selecionada, resposta sem editor obrigatório, zero correções entre turnos, comentário/pergunta em sequência, erro recuperável, botão de parar voz, sugestão de terminar na quinta resposta e bloqueio por limite antes de gravar.
- [ ] Executar `npm.cmd test -- src/features/articles`; confirmar falhas iniciais.
- [ ] Implementar página Artigos com preparação, conversa, transcrição consultável, ação de correção, retoma e lista de sessões. Mostrar ramos antigos em leitura; explicar que regenerar pode consumir API. Só autoplay de uma intervenção nova na sessão visível; histórico, retoma e remount não iniciam som.
- [ ] Integrar guarda de navegação/fecho e proprietário de áudio: não permitir mudar de página durante captura/finalização; sair cancela autoplay pendente sem perder persistência. Relatório mostra quatro dimensões, citações reproduzíveis e até três melhorias, com insuficiência de evidência explícita.
- [ ] Testar eventos duplicados/atrasados, indisponibilidade do microfone, revogação, ausência de relatório e teclado/foco nos erros; executar testes da tarefa, `npm.cmd run typecheck` e regressões App/navegação.
- [ ] Commit `feat: add article conversation interface`.

## Tarefa 7 — voz incremental com cancelamento

**Ficheiros:** `audio-worker/src/audio_worker/{protocol,engines,__main__}.py`, testes `test_protocol.py`/`test_engines.py`, `src-tauri/src/worker/{protocol,mod}.rs`, `audio/{mod,playback}.rs`, `service/article.rs`, `contracts/worker-v1.examples.json`; `tests/article_voice.rs`.

**Interfaces:** acrescentar operação opt-in `synthesize_stream`; eventos `audio_chunk { requestId, index, audioPath, sampleRate, durationMs }` com índices desde zero e um resultado terminal com número de blocos. WAVs completos por bloco, nunca ficheiro em escrita. Capability `synthesisChunks` no health; ausência usa síntese completa existente.

- [ ] Testar primeiro bloco entregue antes do resultado final, índices ordenados, terminal único, erros depois do primeiro bloco e pedidos `synthesize` antigos inalterados. Testar rejeição de paths fora da operação e blocos duplicados/fora de ordem.
- [ ] Executar `.\.venv\Scripts\python.exe -m pytest audio-worker/tests/test_protocol.py audio-worker/tests/test_engines.py -q` e `cargo test --manifest-path src-tauri/Cargo.toml --test article_voice`; verificar falhas iniciais.
- [ ] Gerar blocos Kokoro em ficheiros temporários e publicar após rename. No Rodio, acrescentar fontes ao mesmo player em vez de usar `play()` que substitui a anterior. Uma geração identifica a fila; fim momentâneo da fila não significa fim do pedido.
- [ ] Parar voz limpa fila e invalida geração; o supervisor descarta blocos tardios. Libertar worker após interrupção no limite de bloco, com mecanismo cooperativo de cancelamento e leitura de comandos independente da inferência; não matar o auxiliar nem descarregar modelos como via normal. Pedidos antigos continuam sequenciais.
- [ ] Testar cancelamento enquanto fila está vazia, novo início de captura, perda/restauro de saída e fallback de capability. Falha parcial mostra texto e oferece repetir voz, sem nova chamada IA. Conservar manifesto final para audição posterior; nunca reutilizar sequência parcial como cache completa.
- [ ] Repetir testes Python/Rust e testes de reprodução antigos; commit `perf: stream validated tutor speech in ordered chunks`.

## Tarefa 8 — medir e ajustar a transcrição

**Ficheiros:** novo `scripts/benchmark-audio.ps1`, `audio-worker/src/audio_worker/engines.py`, protocolos apenas se necessário; `docs/article-conversation/performance.md`; testes de motores e `tests/article_metrics.rs`; listas em `diagnostics.rs`.

**Interfaces:** benchmark recebe diretório de gravações indicado pelo utilizador e destino de resultados local; não varrer gravações pessoais. Mede modelos frios/carregados, duração da gravação, transcrição, primeiro bloco e síntese completa. Diagnósticos de execução usam operações fechadas `article_transcribe`, `article_reply`, `article_first_audio`, `article_feedback` com `duration_ms`, sem conteúdo/paths livres.

- [ ] Testar métricas com relógio controlado, ausência de conteúdo nos logs e perfil de transcrição reproduzível. Executar `cargo test --manifest-path src-tauri/Cargo.toml --test article_metrics` e testes Python; confirmar falha inicial.
- [ ] Implementar medições e comparar baseline `beam_size=5` com candidato `beam_size=1`, mantendo inglês, VAD, modelo small e CPU/int8. Não alterar o default antes da comparação de fidelidade. Dados de benchmark não são enviados nem versionados.
- [ ] Nas mesmas gravações, executar cinco repetições carregadas por configuração e uma fria, apresentando valores individuais, mediana e pior tempo; não chamar p95 a uma amostra de cinco. Conferir texto com referência humana e termos técnicos.
- [ ] Registar resultados reais e decisão: manter baseline se faltarem gravações/aceitação de fidelidade. Definir valores-alvo depois de medir; melhoria na voz deve ser avaliada no tempo até primeiro som e nas quebras entre blocos.
- [ ] Repetir testes após qualquer ajuste; commit `perf: measure article turn latency and transcription tradeoffs`.

## Tarefa 9 — integração, documentação e aceitação

**Ficheiros:** novos `docs/article-conversation/setup.md` e `validation.md`; atualizar README, AGENTS.md, `docs/ideia-e-objetivos.md`, `docs/tech-stack.md`, `docs/layout/validation.md` e `docs/diagnostics/validation.md`. Não alterar as pendências de aceitação das fases existentes.

- [ ] Acrescentar cenário integrado com captura/worker/HTTP controlados: autorizar→cinco turnos→corrigir turno antigo→regenerar→concluir→reabrir; verificar artefactos, fontes, consumo e inexistência de chamadas ao reabrir.
- [ ] Executar `.\scripts\verify.ps1` e exigir todas as verificações com saída 0; registar números observados, sem copiar contagens anteriores. Preparar Cargo no PATH como no script antes dos comandos individuais deste plano.
- [ ] Executar `.\.venv\Scripts\python.exe -m pytest audio-worker/tests -q -m models` se os modelos locais estiverem disponíveis; ausência permanece pendência explícita. Executar `npm.cmd run tauri -- build --no-bundle` e smoke com dados temporários segundo os procedimentos existentes, sem tocar nas sessões pessoais.
- [ ] Conferir documentação/links e `git diff --check`. Atualizar matriz visual para a nova página, sem afirmar validação visual antes de a fazer.
- [ ] Realizar roteiro manual: artigo real, cinco respostas, discordância, aprofundamento, recuperação de erro de reconhecimento, interrupção de voz, falha de rede e retoma. Inferência real exige autorização para os pedidos pagos; documentar latência percebida, continuidade e utilidade semântica separadas dos resultados automáticos.
- [ ] Commit `docs: record article conversation verification and remaining acceptance` apenas com resultados efetivamente observados. Entrega automática pode estar verificada enquanto a aceitação manual continua pendente.

## Ordem e passagem à execução

Executar 1→2→3→4→5→6→7→8→9. A tarefa 7 altera um contrato partilhado e exige regressão dos modos antigos; evitar implementação paralela destes ficheiros. Recomenda-se execução direta para manter o contexto de estado, protocolo e versões, com commits por tarefa.

O plano foi confrontado com o desenho: consentimento, fontes, limites, recuperação, diálogo, feedback, voz e medições têm tarefas responsáveis. Nenhum dos comandos de testes/build acima foi executado nesta etapa documental; os resultados esperados são critérios futuros, não resultados obtidos.

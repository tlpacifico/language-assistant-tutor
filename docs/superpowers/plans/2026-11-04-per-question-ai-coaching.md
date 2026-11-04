# Plano de implementação — avaliação por pergunta

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans para execução direta, tarefa a tarefa, com testes antes da implementação.

**Objetivo:** apresentar avaliação, correções e reformulação durante a entrevista, após encerrar cada pergunta.

**Arquitetura:** reutilizar relatórios e reformulações persistentes. Acrescentar um comando de avaliação de uma pergunta e um painel que coordena as duas operações por ação explícita. Manter o comando de feedback final e a restrição de sessão concluída para criar práticas.

**Stack:** React/TypeScript, Rust/Tauri 2 e SQLite existentes; nenhuma dependência nova.

**Spec:** [desenho aprovado](../specs/2026-11-04-per-question-ai-coaching-design.md), autorizado pelo utilizador em 2026-11-04 («pode avancar»).

## Restrições globais

- Texto confirmado e seguimento encerrado antes da avaliação/reformulação.
- Ação explícita e consentimento existentes antes de chamadas pagas; nenhum pedido ao montar ou retomar uma página.
- Preservar fontes, evidências, custos, modo local e recuperação; sem repetição automática.
- Executar diretamente numa branch própria no checkout atual, conforme autorização para avançar autonomamente; sem publicação nem chamadas pagas reais.

## Foco da revisão

- Seguimento pendente não pode originar avaliação nem reformulação definitiva.
- Avançar de pergunta durante a rede não associa o resultado à nova pergunta.
- Avaliação válida seguida de reformulação falhada não volta a cobrar avaliação.
- Retomar uma página apenas consulta resultados persistidos.
- Transcrição recebida após criar rascunho vazio preenche o editor sem perder edições locais.

### Tarefa 1: avaliação e reformulação de pergunta encerrada

**Ficheiros:** `src-tauri/src/db/ai.rs`, `src-tauri/src/db/rewrites.rs`, `src-tauri/src/service/ai.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`, `src/lib/interview-client.ts`, `src/test/fake-interview-client.ts`, `src-tauri/tests/ai_service.rs`, `src-tauri/tests/practice_ai.rs`.

**Interfaces:** `evaluateQuestion(AiQuestionCommand): Promise<FeedbackView>` / `AppService::evaluate_question`; reutilizar `reserve_ai(..., "feedback", ...)` e `request_rewrite`.

- [x] Acrescentar testes de avaliação antes do fim, bloqueio antes de resolver seguimento, reutilização no relatório final e reformulação de pergunta encerrada; executar e observar falhas.
- [x] Implementar elegibilidade no núcleo, comando individual e registo Tauri/cliente; permitir reformulação de originais IA encerrados sem antecipar criação de práticas.
- [x] Executar testes Rust relevantes e conferir fontes e imutabilidade.

### Tarefa 2: painel de avaliação e versão melhorada

**Ficheiros:** criar `src/features/ai/QuestionCoaching.tsx` e testes; adaptar `FeedbackPanel.tsx`, `RewritePanel.tsx`, `src/features/interview/Interview.tsx`, contratos e cliente falso quando necessário.

**Interfaces:** painel recebe `client`, `snapshot` e `onSnapshot`; usa comando individual, resultados existentes e `RewritePanel` para variantes e voz.

- [x] Testar clique explícito, sequência avaliação/reformulação, falha parcial, retoma sem chamadas e troca de pergunta durante pedido; observar falhas.
- [x] Implementar painel com botão «Avaliar e sugerir melhorias», aviso de custo, resultados por pergunta e recuperação independente.
- [x] Integrar apenas quando o seguimento estiver encerrado no modo IA; esclarecer função opcional da revisão de transcrição.
- [x] Executar Vitest e build TypeScript/Vite.

### Tarefa 3: editor e verificação final

**Ficheiros:** `src/features/interview/useInterview.ts`, testes de entrevista, documentação de validação e documentos das fases afetadas.

- [x] Reproduzir rascunho vazio seguido de transcrição/revisão automática; preservar edição em curso e revisão guardada.
- [x] Corrigir reconciliação e mensagem de origem apenas onde a reprodução demonstrar defeito.
- [x] Executar `.\scripts\verify.ps1`, rever diff e registo dos comandos; documentar resultados reais e pendências manuais.

## Registo de execução

- Preparação: checkout inicialmente limpo exceto o desenho desta tarefa; execução direta autorizada. Branch `feat/per-question-ai-coaching`.
- Decisão: o desenho aprovado serve de autoridade; o plano detalha a execução já autorizada, sem nova ronda de aprovação.
- Interfaces partilhadas: tarefa 2 consome o comando individual da tarefa 1; tarefa 3 verifica a revisão usada na tarefa 2. Sem contratos incompatíveis identificados na preparação.

- Tarefas 1–3: executadas e verificadas automaticamente. Execução final de verify.ps1: 8/8, 252 Rust, 112 React e 36 Python; saída 0 em 41,8 s. Ver docs/ai-coaching/validation.md.
- Decisão após regressão: exigir seguimento encerrado também nas fontes de feedback de sessões IA concluídas; corrigir o cenário artificial de stories, que omitia essa etapa, em vez de enfraquecer a regra.
- Integração: alterações disponíveis na branch local, sem publicação nem alteração do executável release em utilização.

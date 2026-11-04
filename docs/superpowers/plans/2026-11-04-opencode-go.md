# Plano — OpenCode Go

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans para executar diretamente.

**Goal:** usar a modalidade Go e os headers da integração do Marlo, com identidade própria.

**Architecture:** default Go e adaptação única em SQLite; contexto persistido no serviço passa ao transporte HTTP; configuração/consentimento existentes.

**Tech Stack:** Rust/Tauri, SQLite e React existentes, sem novas dependências.

**Spec:** [desenho](../specs/2026-11-04-opencode-go-design.md).

## Restrições globais e foco de revisão

Preservar chave protegida, respostas e histórico; invalidar consentimento quando se adapta o destino; nenhuma chamada automática. Não copiar credenciais nem a identidade do Marlo. Testar configurações personalizadas e escolha posterior explícita, estabilidade por conversa e consumo sem preços fixos.

### Tarefa 1 — contexto e transporte

Ficheiros: `src-tauri/src/ai/transport.rs`, `src-tauri/src/service/ai.rs`, `src-tauri/tests/ai_service.rs`.

Interfaces: acrescentar `conversation_id: String` a `TransportRequest`; `run_attempt` usa prática quando existe, caso contrário sessão; transporte envia User-Agent próprio e header de sessão estável, sem alterar corpo.

- [x] Testes RED: HTTP local recebe caminho/headers, IDs iguais em repetições e distintos entre conversas, custo zero Go preservado.
- [x] Implementar e verificar `cargo test --manifest-path src-tauri/Cargo.toml --lib` e testes de serviço.

### Tarefa 2 — configuração e adaptação única

Ficheiros: `src-tauri/src/ai/mod.rs`, `src-tauri/src/db/ai.rs`, testes novos `src-tauri/tests/opencode_go.rs`, `src-tauri/tests/ai_usage.rs`; `src/features/ai/AiSettings.tsx`, fixtures/testes React.

Interfaces: defaults Go sem tarifário fixo; `Database::ai_settings()` adapta uma vez a configuração antiga de referência, usando marcador em `ai_receipts`, incrementando versão e limpando consentimento/tarifário. Não toca na chave do sistema. Configurações posteriores são respeitadas.

- [x] Testes RED de default, adaptação idempotente, preservação de configuração personalizada e escolha posterior, credencial preservada e nova autorização obrigatória.
- [x] Implementar, ajustar testes de fórmula para usar tarifário de teste explícito e verificar testes afetados.

### Tarefa 3 — entrega

- [x] Atualizar stack, preparação e validação da fase 3 e diagnóstico; registar a substituição da decisão anterior por autorização atual.
- [x] Executar `scripts/verify.ps1`, conferir links/diff e gerar release `--no-bundle`. Preservar o executável anterior se estiver aberto, sem encerrar a sessão do utilizador.

## Execução

Execução direta na branch existente para preservar alterações anteriores. O utilizador escolheu explicitamente esta integração, pelo que se prossegue sem nova pausa de aprovação.

Resultado em 2026-11-04: as três tarefas concluídas. Os testes RED falharam primeiro pelos motivos previstos (campo de contexto inexistente, default por consumo, sem adaptação); depois da implementação passaram os testes focados de transporte, serviço, configuração e consumo. `scripts/verify.ps1` terminou com 8/8 checks e saída 0 (270 Rust, 119 React, 36 Python); o build release `--no-bundle` terminou com saída 0 e o smoke do binário saiu com código 0. Evidência completa em [validação Go](../../opencode-go/validation.md).

Ficou por verificar apenas o que depende do fornecedor e de utilização real: aceitação do tráfego do tutor na modalidade Go, custo real na conta e utilidade semântica. Nenhum pedido pago foi feito. Esta alteração não inicia a fase 5.

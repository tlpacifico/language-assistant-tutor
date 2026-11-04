# Plano — diagnóstico local

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans para executar tarefa a tarefa.

**Goal:** investigar falhas durante uso real, preservando diagnósticos entre builds.

**Architecture:** escritor Rust com fila limitada, eventos de campos fechados e exportação local; fronteiras React/IPC e supervisor do auxiliar alimentam o escritor. Build metadata produzida pelo build.rs.

**Tech Stack:** React/TypeScript, Rust/Tauri 2 e Python existentes; sem novas dependências.

**Spec:** [desenho](../specs/2026-11-04-local-diagnostics-design.md).

## Global Constraints

Português de Portugal. Dados em `app_data_dir()/diagnostics`; identidade Tauri estável; cinco ficheiros de 1 MiB, 30 dias. Sem conteúdo pessoal nem envio remoto. Preservar trabalho anterior na branch atual, que contém alterações por consolidar; execução direta pedida pelo utilizador.

## Review Focus

- Reiniciar com nova versão conserva eventos anteriores (teste Rust de reabertura).
- Disco indisponível não interrompe operações (teste Rust de falha de escrita).
- Eventos externos com segredos não entram nos ficheiros (testes Rust e React de campos fechados).
- Exportação inclui os eventos anteriores e não substitui ficheiros (teste Rust de barreira/exportação).
- Falha React permite exportar sem reiniciar automaticamente e perder rascunhos (teste do error boundary).

### Tarefa 1: armazenamento e builds

**Ficheiros:** criar `src-tauri/src/diagnostics.rs`, `src-tauri/tests/diagnostics.rs`; alterar `src-tauri/build.rs`, `src-tauri/src/lib.rs`.

**Interfaces:** `Diagnostics::open(root, build)`, `record(event)`, `status()`, `export(directory)`; `BuildInfo::current()`; escritor limitado, estado partilhado e barreira para exportação.

- [x] Escrever testes de preservação entre versões, privacidade, rotação, retenção, exportação e falha de disco; executar `cargo test --manifest-path src-tauri/Cargo.toml --test diagnostics`, observar falha antes da implementação.
- [x] Implementar escritor e metadata sem novas dependências; executar a mesma bateria até aprovar.

### Tarefa 2: captura e interface

**Ficheiros:** alterar `src-tauri/src/{lib,domain}.rs`, `src-tauri/src/worker/mod.rs`, `audio-worker/src/audio_worker/__main__.py`, `src/{main,App}.tsx`, `src/lib/interview-client.ts`; criar `src/lib/diagnostics.ts`, painel e error boundary em `src/features/diagnostics/`, respetivos testes.

**Interfaces:** comandos Tauri `diagnostics_status`, `diagnostics_record`, `diagnostics_export`; eventos apenas categorias/códigos permitidos. Exportação devolve caminho local; painel permite indicar diretório.

- [x] Escrever testes React da captura sem conteúdo pessoal, falhas de IPC e recuperação React; executar `npm.cmd test -- src/features/diagnostics src/lib/diagnostics.test.ts` e observar falha.
- [x] Implementar captura de códigos Rust, panic sem mensagem, erros Python com frames próprios, wrapper IPC e captura global; integrar painel e recuperação; aprovar testes.

### Tarefa 3: validação e documentação

**Ficheiros:** criar `docs/diagnostics/validation.md`; atualizar `AGENTS.md` e `docs/tech-stack.md`.

- [x] Executar `scripts/verify.ps1`, build release `--no-bundle`, smoke isolado, conferir JSON exportado e links locais; registar resultados reais e limites manuais.
- [x] Rever o diff exclusivamente desta alteração; preservar alterações anteriores e não fazer push sem pedido.

## Registo de execução

As caixas e a validação serão atualizadas com evidência. O pedido explícito inclui implementar, pelo que não se introduz uma nova pausa de aprovação. A branch atual contém trabalho necessário já autorizado; não se cria um checkout limpo que o exclua.

- Tarefas 1 e 2: testes focados falharam antes da implementação e aprovaram depois. Verificação agregada: 8/8, 262 Rust, 117 React, 36 Python. Interfaces IPC e escritor conferidas em conjunto.
- Revisão final independente: quatro problemas importantes corrigidos, com reprodução e testes de regressão (ver validação). Sem problemas menores adiados.
- Decisão de execução: manter as alterações na branch existente, sem commit ou push automático, para preservar o trabalho anterior por consolidar.
- Decisão de ensaio: usar `app.appDirectoriesOverride` numa configuração temporária de build e voltar a compilar a configuração normal no fim. Em Windows, os diretórios conhecidos do sistema não ficam necessariamente isolados por mudar apenas APPDATA/LOCALAPPDATA.

- Tarefa 3: build isolado, três ensaios WebView2 com saída 0 e persistência entre reinícios, build normal final com saída 0 (1m01s), ausência do diretório de ensaio no binário final e links locais conferidos. Evidência em `docs/diagnostics/validation.md`.

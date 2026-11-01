# language-assistant-tutor

Tutor desktop de entrevistas comportamentais em inglês, por voz, baseado na
estrutura STAR. Este repositório contém a base da fase 1: aplicação
Tauri 2 + React/TypeScript e os contratos de domínio partilhados.

O contexto do produto, a stack e o plano das fases estão em
[`docs/`](docs/) e em [`AGENTS.md`](AGENTS.md).

## Requisitos (Windows)

- Windows 10/11 com o runtime **Microsoft Edge WebView2**.
- **Node.js** `24.11.1` e **npm** `11.6.2` (usar `npm.cmd` em PowerShell).
- **Rust** `1.99.1` com a toolchain `x86_64-pc-windows-msvc` e os componentes
  `rustfmt` e `clippy` (fixados em `rust-toolchain.toml`).
- **Visual Studio 2022 Build Tools** com C++ e o **Windows SDK** (10.0.26100
  nesta máquina).

O Cargo não está no `PATH` por omissão nesta máquina. Numa sessão nova:

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
```

## Instalação

```powershell
npm.cmd install
```

## Comandos

| Objetivo | Comando |
| --- | --- |
| Janela em desenvolvimento (Vite + Tauri) | `npm.cmd run tauri -- dev` |
| Apenas o frontend no browser | `npm.cmd run dev` |
| Verificar tipos TypeScript | `npm.cmd run typecheck` |
| Build do frontend (typecheck + Vite) | `npm.cmd run build` |
| Testes da interface (Vitest) | `npm.cmd test` |
| Testes de domínio em Rust | `cargo test --manifest-path src-tauri/Cargo.toml --test domain` |
| Formatação Rust | `cargo fmt --manifest-path src-tauri/Cargo.toml` |
| Lint Rust | `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` |
| Compilar a aplicação sem instalador | `npm.cmd run tauri -- build --no-bundle` |
| Instalador completo | `npm.cmd run tauri -- build` |

`npm.cmd test` ainda não encontra ficheiros de teste: o Vitest está configurado
com `passWithNoTests` e a interface só ganha testes na tarefa 7.

## Estrutura atual

```text
index.html, package.json, tsconfig.json, vite.config.ts
rust-toolchain.toml
src/main.tsx                  Arranque React
src/App.tsx                   Janela com a pergunta fixa
src/lib/contracts.ts          Tipos do contrato v1 (camelCase, enums snake_case)
src-tauri/src/main.rs         Binário Windows
src-tauri/src/lib.rs          Arranque Tauri
src-tauri/src/domain.rs       Tipos do domínio, AppState e Action
src-tauri/tests/domain.rs     Regras de ocupação, geração e contrato
contracts/app-v1.example.json Exemplo partilhado e verificado no teste Rust
```

A pergunta da fase 1 é fixa:
`Tell me about a time you solved a difficult problem at work.`

Esta base não grava áudio, não chama o processo auxiliar e não guarda dados; a
experiência de prática, o armazenamento e o áudio entram nas tarefas seguintes.

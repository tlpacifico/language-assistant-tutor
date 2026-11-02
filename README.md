# language-assistant-tutor

Tutor desktop de entrevistas comportamentais em inglês, por voz, baseado na
estrutura STAR. Este repositório contém a implementação da **fase 1**: uma
aplicação Tauri 2 + React/TypeScript que apresenta uma pergunta fixa, grava a
resposta, reproduz o áudio, transcreve localmente com faster-whisper e permite
guardar uma revisão do texto. Tudo corre no computador; nenhuma gravação ou
transcrição é enviada para serviços externos.

O contexto do produto, a stack e o plano das fases estão em
[`docs/`](docs/) e em [`AGENTS.md`](AGENTS.md). A preparação real do ambiente
está em [`docs/phase-1/setup.md`](docs/phase-1/setup.md) e o estado dos critérios
de aceitação em [`docs/phase-1/validation.md`](docs/phase-1/validation.md).

## Requisitos (Windows)

Verificados nesta máquina (detalhe e versões em
[`docs/phase-1/setup.md`](docs/phase-1/setup.md)):

- Windows 10/11 com o runtime **Microsoft Edge WebView2**.
- **Node.js** `24.11.1` e **npm** `11.6.2` (usar `npm.cmd` em PowerShell).
- **Rust** `1.99.1` com a toolchain `x86_64-pc-windows-msvc` e os componentes
  `rustfmt` e `clippy` (fixados em `rust-toolchain.toml`).
- **Visual Studio** com ferramentas C++ e o **Windows SDK** (nesta máquina:
  Visual Studio Community 2026 `18.10.12224.181`, MSVC `14.44.35207` e Windows
  SDK `10.0.26100.0`).
- **Python 3.11** (não usar o 3.13 predefinido) para a `.venv` com o auxiliar.
- Cerca de **800 MB** livres para os modelos Whisper `small` e Kokoro-82M.

O Cargo não está no `PATH` por omissão nesta máquina. Numa sessão nova:

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
```

## Instalação e primeira execução

```powershell
npm.cmd install

# Ambiente Python (uma vez): ver docs/phase-1/setup.md
py -3.11 -m venv .venv
.\.venv\Scripts\python.exe -m pip install --upgrade pip
.\.venv\Scripts\python.exe -m pip install torch --index-url https://download.pytorch.org/whl/cpu
.\.venv\Scripts\python.exe -m pip install -r audio-worker/requirements.in
.\.venv\Scripts\python.exe -m pip install -e "audio-worker[test]"

# Modelos (único passo com rede; pasta fora do repositório)
.\.venv\Scripts\python.exe -m audio_worker.prepare --models-dir "$env:LOCALAPPDATA\language-assistant-tutor\Modelos de Voz"
```

Depois criar a configuração pessoal `audio.json` em
`%APPDATA%\dev.thacio.languageassistanttutor\audio.json` (ou no caminho de
`LANGUAGE_TUTOR_CONFIG`), com caminhos absolutos. O modelo está em
[`config/audio.example.json`](config/audio.example.json); **não versionar** o
ficheiro real:

```json
{
  "schemaVersion": 1,
  "pythonExecutable": "C:\\Projetos\\language-assistant-tutor\\.venv\\Scripts\\python.exe",
  "modelsManifest": "D:\\Modelos de Voz\\language-assistant-tutor\\models.json"
}
```

Abrir a janela:

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
npm.cmd run tauri -- dev
```

Sem Python, `audio.json` ou modelos, a janela abre e indica o componente em
falta; gravar, ouvir e guardar revisões continuam disponíveis.

## Comandos

| Objetivo | Comando |
| --- | --- |
| Todos os checks automáticos (pára no primeiro erro) | `.\scripts\verify.ps1` |
| Janela em desenvolvimento (Vite + Tauri) | `npm.cmd run tauri -- dev` |
| Apenas o frontend no browser | `npm.cmd run dev` |
| Verificar tipos TypeScript | `npm.cmd run typecheck` |
| Build do frontend (typecheck + Vite) | `npm.cmd run build` |
| Testes da interface (Vitest) | `npm.cmd test` |
| Testes Rust (todas as suítes) | `cargo test --manifest-path src-tauri/Cargo.toml` |
| Formatação Rust | `cargo fmt --manifest-path src-tauri/Cargo.toml` |
| Lint Rust | `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` |
| Compilar sem instalador (não é pacote autónomo) | `npm.cmd run tauri -- build --no-bundle` |
| Instalador completo (fase 5) | `npm.cmd run tauri -- build` |
| Criar o ambiente do auxiliar Python | `py -3.11 -m venv .venv` |
| Preparar modelos offline (rede) | `.\.venv\Scripts\python.exe -m audio_worker.prepare --models-dir <pasta absoluta>` |
| Testes do auxiliar Python | `.\.venv\Scripts\python.exe -m pytest audio-worker/tests -q` |
| Teste real com pesos preparados | `.\.venv\Scripts\python.exe -m pytest audio-worker/tests/test_offline_models.py -m models -q` |
| Verificar o ambiente Python | `.\.venv\Scripts\python.exe -m pip check` |

`.\scripts\verify.ps1` executa typecheck, testes React, build Vite,
`cargo fmt --check`, `clippy -D warnings`, testes Rust, pytest sem a marca
`models` e `pip check`. Resultado real desta máquina: 8/8 checks com saída 0
(entre 23,4 s e 116,4 s, conforme a cache de compilação e o Vitest a frio).
O teste com a marca `models` é executado à parte, porque exige pesos
preparados e um manifesto indicado por `LANGUAGE_TUTOR_MODELS_MANIFEST`.

## Estrutura atual

```text
index.html, package.json, tsconfig.json, vite.config.ts
rust-toolchain.toml
scripts/verify.ps1            Checks automáticos com controlo de exit codes
src/main.tsx                  Arranque React
src/App.tsx                   Janela com a pergunta fixa
src/lib/contracts.ts          Tipos do contrato v1 (camelCase, enums snake_case)
src/lib/tauri.ts              Adaptador PracticeClient over Tauri
src/features/practice/        Interface de prática e revisão
src-tauri/src/                Domínio, armazenamento, áudio, worker, serviço, comandos
src-tauri/tests/              Testes de domínio, storage, áudio, worker e serviço
contracts/                    Exemplos partilhados dos contratos v1
config/audio.example.json     Configuração pessoal documentada (não versionar a real)
audio-worker/                 Auxiliar Python (faster-whisper + Kokoro)
docs/phase-1/                 setup.md e validation.md
```

O áudio original e as revisões ficam na pasta de dados da aplicação
(`%APPDATA%\dev.thacio.languageassistanttutor\phase1`), nunca na árvore do
código. Modelos, `.venv`, áudio e `.superpowers/` estão no `.gitignore`.

## Estado

A implementação da fase 1 está completa e a verificação automática está verde
(74 testes Rust, 14 testes React, 36 testes Python sem marca `models`, 2 testes
Python reais com pesos preparados, `pip check` limpo, build release sem
instalador). A preparação real dos modelos foi executada e o risco de
compatibilidade entre `transformers 5.22.1` e os pesos do Kokoro 0.9.4 não se
materializou nesta máquina.

A fase **não** está aceite: faltam a validação do utilizador com três gravações
reais, a audição da voz, a remoção/restauro do microfone e a decisão sobre a
espera (CA-14). Ver a matriz honesta em
[`docs/phase-1/validation.md`](docs/phase-1/validation.md).

A pergunta da fase 1 é fixa:
`Tell me about a time you solved a difficult problem at work.`

# language-assistant-tutor

Tutor desktop de entrevistas comportamentais em inglês, por voz, baseado na
estrutura STAR. Este repositório contém a implementação das **fases 1 e 2**:
uma aplicação Tauri 2 + React/TypeScript que prepara uma entrevista guiada com
um banco de 20 perguntas, grava as respostas, reproduz o áudio, transcreve
localmente com faster-whisper, permite rever e confirmar cada texto e guarda o
histórico em SQLite. Tudo corre no computador; nenhuma gravação ou transcrição é
enviada para serviços externos.

O contexto do produto, a stack e o plano das fases estão em
[`docs/`](docs/) e em [`AGENTS.md`](AGENTS.md). A preparação real do ambiente
está em [`docs/phase-1/setup.md`](docs/phase-1/setup.md) e o estado dos critérios
de aceitação da fase 1 em [`docs/phase-1/validation.md`](docs/phase-1/validation.md).
A fase 2 está documentada em [`docs/phase-2/setup.md`](docs/phase-2/setup.md)
(execução e dados) e [`docs/phase-2/validation.md`](docs/phase-2/validation.md)
(matriz F2-01 a F2-16).

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
falta; preparar a sessão, editar o banco, rever texto e consultar o histórico
continuam disponíveis.

No primeiro arranque são criados `phase1\` e `tutor.sqlite3` na pasta de dados
da aplicação; a pasta `sessions\` aparece na primeira gravação. Ver
[`docs/phase-2/setup.md`](docs/phase-2/setup.md) para o formato e o significado
de cada caminho.

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
`models` e `pip check`. Resultado real da verificação da fase 2: 8/8 checks com
saída 0 em 18,5 s (147 testes Rust, 43 Vitest, 36 pytest sem `models`); a
duração depende da cache de compilação e do Vitest a frio. O teste com a marca
`models` é executado à parte, porque exige pesos preparados e um manifesto
indicado por `LANGUAGE_TUTOR_MODELS_MANIFEST`; nesta máquina deu `2 passed`,
não skip.

## Estrutura atual

```text
index.html, package.json, tsconfig.json, vite.config.ts
rust-toolchain.toml
scripts/verify.ps1            Checks automáticos com controlo de exit codes
src/main.tsx                  Arranque React
src/App.tsx                   Navegação: preparar, banco, entrevista, histórico
src/lib/contracts.ts          Tipos do contrato v1 (treinos da fase 1, leitura)
src/lib/interview-contracts.ts Contratos v2 da entrevista guiada
src/lib/interview-client.ts   Adaptador InterviewClient over Tauri
src/features/sessions/        Preparação e retoma da sessão
src/features/questions/       Banco de perguntas editável
src/features/interview/       Captura, revisão, confirmação e avanço
src/features/navigation/      Decisão guardar/descartar/cancelar (navegar e fechar)
src/features/history/         Histórico de sessões e treinos da fase 1
src/features/practice/        practice.css partilhado pela interface nova
src-tauri/src/                Domínio, SQLite, artefactos, áudio, worker, serviço, comandos
src-tauri/tests/              147 testes de domínio, base, artefactos, áudio, worker, sessões, serviço
contracts/                    Exemplos partilhados dos contratos v1 e v2
config/audio.example.json     Configuração pessoal documentada (não versionar a real)
audio-worker/                 Auxiliar Python (faster-whisper + Kokoro)
docs/phase-1/                 setup.md e validation.md
docs/phase-2/                 setup.md e validation.md
```

Os dados das sessões novas ficam em
`%APPDATA%\dev.thacio.languageassistanttutor\tutor.sqlite3` e os áudios em
`sessions\<session-id>\<session-question-id>\<recording-id>\`. A árvore
`phase1/` da pasta de dados é conservada e só é lida: os treinos anteriores
continuam consultáveis no histórico. Nada disto está na árvore do código.
Modelos, `.venv`, áudio, base de dados e `.superpowers/` estão no `.gitignore`.

## Estado

A implementação da **fase 1** está completa e a verificação automática está
verde (77 testes Rust, 15 testes React, 36 testes Python sem marca `models`, 2
testes Python reais com pesos preparados, `pip check` limpo, build release sem
instalador — contagens da altura; a suíte atual inclui os testes da fase 2). A
preparação real dos modelos foi executada e o risco de compatibilidade entre
`transformers 5.22.1` e os pesos do Kokoro 0.9.4 não se materializou nesta
máquina.

A fase 1 **não** está aceite: faltam a validação do utilizador com três
gravações reais, a audição da voz, a remoção/restauro do microfone e a decisão
sobre a espera (CA-14). Ver a matriz honesta em
[`docs/phase-1/validation.md`](docs/phase-1/validation.md).

A implementação da **fase 2** (entrevista guiada) está completa e a verificação
automática está verde: `.\scripts\verify.ps1` com 8/8 checks, 147 testes Rust,
43 testes Vitest e 36 testes Python sem a marca `models`, mais 2 testes reais
dos motores com pesos preparados; build release sem instalador, arranque e
fecho do binário com código 0. A fase 2 **não** está concluída nem aceite: falta
a validação manual do utilizador (sessão real de três perguntas, audição,
revisão/confirmação, sair e retomar, histórico, sessão de uma pergunta,
insuficiência, edição do banco, percurso offline e recuperação). Ver
[`docs/phase-2/validation.md`](docs/phase-2/validation.md).

A fase 3 (tutor com IA) **não** foi iniciada. O âmbito da fase 2 está em
[`docs/superpowers/specs/2026-11-02-phase-2-guided-interview-design.md`](docs/superpowers/specs/2026-11-02-phase-2-guided-interview-design.md).

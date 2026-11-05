# language-assistant-tutor

Tutor desktop de entrevistas comportamentais em inglês, por voz, baseado na
estrutura STAR. Este repositório contém a implementação das **fases 1 a 4**:
uma aplicação Tauri 2 + React/TypeScript que prepara uma entrevista guiada com
um banco de 20 perguntas, grava as respostas, reproduz o áudio, transcreve
localmente com faster-whisper, permite rever e confirmar cada texto e guarda o
histórico em SQLite. O modo local funciona no computador. O modo opcional com IA
envia apenas o texto confirmado necessário para seguimentos e feedback através
do OpenCode Go; as gravações permanecem locais. A integração Go foi autorizada em
2026-11-04 para usar a mesma modalidade do Marlo Expenses; ver
[preparação e validação](docs/opencode-go/validation.md).

O contexto do produto, a stack e o plano das fases estão em
[`docs/`](docs/) e em [`AGENTS.md`](AGENTS.md). A preparação real do ambiente
está em [`docs/phase-1/setup.md`](docs/phase-1/setup.md) e o estado dos critérios
de aceitação da fase 1 em [`docs/phase-1/validation.md`](docs/phase-1/validation.md).
A fase 2 está documentada em [`docs/phase-2/setup.md`](docs/phase-2/setup.md)
(execução e dados) e [`docs/phase-2/validation.md`](docs/phase-2/validation.md)
(matriz F2-01 a F2-16).

A fase 3 acrescenta configuração protegida, seguimentos únicos, feedback com
fontes verificadas e consumo/custos observáveis. Ver [preparação](docs/phase-3/setup.md)
e [validação](docs/phase-3/validation.md). Foi antecipada por autorização do
utilizador; as aceitações manuais das três fases continuam pendentes.

A fase 4 acrescenta reformulação B1/B2/C1 com voz Kokoro, novas tentativas
(práticas) com avaliação própria, comparação local e histórias locais de apoio.
Ver [preparação](docs/phase-4/setup.md) e [validação](docs/phase-4/validation.md).
Está implementada e verificada automaticamente, mas **não aceite**: falta o
roteiro manual F4-16 e a inferência paga real.

O layout foi atualizado a partir de `docs/Melhorar-layout-do-tutor`, com navegação
lateral, preparação da sessão e vistas adaptáveis. Ver [validação e capturas](docs/layout/validation.md).

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

Na primeira abertura de uma entrevista, a aplicação pede ao auxiliar que
carregue os modelos de voz em segundo plano (Whisper e Kokoro). Nesta máquina
são cerca de 10 s a quente e 14 s a frio, com um pico de ~1,7 GiB de memória;
enquanto isso os botões de áudio esperam, em vez de falharem com o auxiliar
ocupado. Depois do primeiro carregamento, os modelos ficam residentes no
processo do auxiliar até a aplicação fechar.

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
| Gerar instaladores NSIS/MSI (release; instalação por validar) | `npm.cmd run tauri -- build` |
| Criar o ambiente do auxiliar Python | `py -3.11 -m venv .venv` |
| Preparar modelos offline (rede) | `.\.venv\Scripts\python.exe -m audio_worker.prepare --models-dir <pasta absoluta>` |
| Testes do auxiliar Python | `.\.venv\Scripts\python.exe -m pytest audio-worker/tests -q` |
| Teste real com pesos preparados | `.\.venv\Scripts\python.exe -m pytest audio-worker/tests/test_offline_models.py -m models -q` |
| Verificar o ambiente Python | `.\.venv\Scripts\python.exe -m pip check` |

`.\scripts\verify.ps1` executa typecheck, testes React, build Vite,
`cargo fmt --check`, `clippy -D warnings`, testes Rust, pytest sem a marca
`models` e `pip check`. Resultado real da verificação mais recente: 8/8 checks com
saída 0 (275 testes Rust, 120 Vitest, 39 pytest sem `models`); a duração depende
da cache de compilação e do Vitest a frio. O teste com a marca
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
src/features/practice/        Treino posterior: prática, reformulação, comparação, consumo
src/features/stories/         Banco de histórias locais e apoio à preparação
src-tauri/src/                Domínio, SQLite, artefactos, áudio, worker, serviço, comandos
src-tauri/tests/               275 testes de domínio, base, artefactos, áudio, worker, sessões, serviço
contracts/                    Exemplos partilhados dos contratos v1 e v2
config/audio.example.json     Configuração pessoal documentada (não versionar a real)
audio-worker/                 Auxiliar Python (faster-whisper + Kokoro)
docs/phase-1/                 setup.md e validation.md
docs/phase-2/                 setup.md e validation.md
docs/releases/                Notas, artefactos e pendências de cada release
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
automática está verde: `.\scripts\verify.ps1` com 8/8 checks, 151 testes Rust,
53 testes Vitest e 36 testes Python sem a marca `models`, mais 2 testes reais
dos motores com pesos preparados; build release sem instalador, arranque e
fecho do binário com código 0. A fase 2 **não** está concluída nem aceite: falta
a validação manual do utilizador (sessão real de três perguntas, audição,
revisão/confirmação, sair e retomar, histórico, sessão de uma pergunta,
insuficiência, edição do banco, percurso offline e recuperação). Ver
[`docs/phase-2/validation.md`](docs/phase-2/validation.md).

A fase 3 (tutor com IA) está implementada e verificada automaticamente; a aceitação manual e a inferência paga real continuam pendentes. A integração passou a OpenCode Go em 2026-11-04. Ver [validação da fase 3](docs/phase-3/validation.md) e [validação Go](docs/opencode-go/validation.md).

A fase 4 (melhorar e repetir) está implementada e verificada automaticamente; não está aceite: falta o roteiro manual F4-16 e a inferência real (fidelidade, utilidade, espera, custo). Ver [validação da fase 4](docs/phase-4/validation.md).

Em 2026-11-04 foi publicada a primeira release, [v0.1.0](docs/releases/v0.1.0.md), com instalador NSIS, MSI e executável portátil. O build dos instaladores e o smoke do binário passaram; a instalação empacotada, a atualização por cima de outra versão e a desinstalação continuam por validar.

Ainda em 2026-11-04 foi publicada a [v0.2.0](docs/releases/v0.2.0.md): modelos de voz carregados ao abrir a entrevista (fim da espera da primeira reprodução/transcrição) e validação local da chave OpenCode `oc_sk_…` antes de qualquer pedido pago, com mensagens acionáveis para 401 e para o bloqueio do seguimento. Verificação 8/8 checks (275 Rust, 120 Vitest, 39 Python), build dos instaladores e smoke do portátil com saída 0. A instalação dos pacotes continua por validar e nenhum pedido pago foi feito.

Em 2026-11-05 ficou implementada a **conversa sobre um artigo** (modo novo, fora das cinco fases): página Artigos, envio automático autorizado por sessão, feedback final, correções em ramos, recuperação local e voz do tutor por blocos. Verificação automática: `.\scripts\verify.ps1` com 8/8 checks (354 Rust, 147 Vitest, 56 Python sem a marca `models`), build release `--no-bundle` e ensaio com dados temporários com saída 0. **Não está aceite**: faltam a conversa real com áudio, voz e inferência paga, as medições de desempenho e a validação visual. Ver [preparação](docs/article-conversation/setup.md), [validação](docs/article-conversation/validation.md) e [desempenho](docs/article-conversation/performance.md).

Em 2026-11-05 foi publicada a release [v0.3.0](docs/releases/v0.3.0.md), com a conversa sobre um artigo (instaladores NSIS/MSI e portátil com hashes no documento; smoke do portátil com código 0). A conversa sobre artigos não tem aceitação manual, a instalação dos pacotes continua por validar e a fase 5 não foi iniciada.

# Preparação e execução da fase 1 no Windows

Data da verificação: 2026-11-02 · Ramo: `feature/phase-1-local-audio`

Este documento regista o ambiente e os comandos que foram realmente executados
nesta máquina para a aplicação Tauri, o auxiliar Python e os modelos locais.
Os resultados de aceitação estão em [validation.md](validation.md). O contexto
do produto e as regras da fase estão na
[spec da fase 1](../superpowers/specs/2026-11-01-phase-1-local-audio-design.md).

## 1. Ambiente verificado

| Componente | Versão observada |
| --- | --- |
| Windows | Windows 11 Enterprise, `10.0.26300` (build 26300) |
| CPU / memória | Intel Core i7-12700H (14 núcleos, 20 threads), 34 010 849 280 bytes de RAM |
| GPU | NVIDIA GeForce RTX 3060 Laptop e Intel Iris Xe; **não usada** (motores em CPU) |
| WebView2 Runtime | `156.0.4314.50` / `156.0.4314.58` (runtime instalado em `C:\Program Files (x86)\Microsoft\EdgeWebView\Application`) |
| Visual Studio | Visual Studio Community 2026 `18.10.12224.181`, ferramentas C++ MSVC `14.44.35207` |
| Windows SDK | `10.0.26100.0` |
| Rust | `rustc 1.99.1` / `cargo 1.99.1`, toolchain `1.99.1-x86_64-pc-windows-msvc` (fixada em `rust-toolchain.toml`), componentes `rustfmt`, `clippy` e `rust-std` instalados |
| Node / npm | `24.11.1` / `11.6.2` (usar sempre `npm.cmd` em PowerShell) |
| Python | `3.11.9` na `.venv`; o Python predefinido da máquina é `3.13.14`, por isso usar `py -3.11` |
| PyTorch | `2.15.0+cpu` |
| Motores Python | `faster-whisper 1.2.1`, `ctranslate2 4.8.4`, `kokoro 0.9.4`, `misaki 0.9.4`, `transformers 5.22.1`, `spacy 3.8.18` |

Notas verificadas:

- O Cargo **não está no `PATH`** numa consola nova. Antes de qualquer comando
  Rust ou `tauri`:

  ```powershell
  $env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
  ```

  `scripts/verify.ps1` já acrescenta esse caminho quando o `cargo` não é
  encontrado. `npm.cmd run tauri -- ...` **também exige** o `cargo` no `PATH`:
  sem ele o CLI falha logo com `program not found` (observado nesta sessão).
- O runtime WebView2 já estava instalado; não foi preciso instalar nada para a
  janela abrir.
- Não foi necessário instalar o eSpeak-NG no sistema: o pacote
  `espeakng-loader 0.2.4` traz `espeak-ng.dll` e `espeak-ng-data` dentro da
  `.venv`.
- A `.venv` desta máquina foi criada e preenchida na tarefa 4 e foi reutilizada
  nesta verificação; `pip check` confirmou o conjunto instalado.

## 2. Criar a `.venv` (Python 3.11)

A partir da raiz do repositório. O PyTorch CPU é instalado **primeiro**, a partir
do índice oficial, para não trazer as rodas CUDA:

```powershell
py -3.11 -m venv .venv
.\.venv\Scripts\python.exe -m pip install --upgrade pip
.\.venv\Scripts\python.exe -m pip install torch --index-url https://download.pytorch.org/whl/cpu
.\.venv\Scripts\python.exe -m pip install -r audio-worker/requirements.in
.\.venv\Scripts\python.exe -m pip install -e "audio-worker[test]"
.\.venv\Scripts\python.exe -m pip check
```

Resultado observado na criação (`audio-worker/README.md` regista o detalhe):
`torch-2.15.0+cpu`, depois 100+ pacotes de `requirements.in` (nenhum erro de
build), `audio-worker-0.1.0` editável com `pytest 9.2.0`, e
`No broken requirements found.`

Para reconstruir a partir do lock versionado, depois do PyTorch CPU **fixado na
versão do lock** (`torch==2.15.0+cpu`):

```powershell
.\.venv\Scripts\python.exe -m pip install torch==2.15.0 --index-url https://download.pytorch.org/whl/cpu
.\.venv\Scripts\python.exe -m pip install -r audio-worker/requirements.lock.txt --extra-index-url https://download.pytorch.org/whl/cpu
.\.venv\Scripts\python.exe -m pip install -e "audio-worker[test]"
.\.venv\Scripts\python.exe -m pip check
```

O `--extra-index-url` mantém o PyPI como índice principal para as restantes
versões do lock e acrescenta o índice CPU apenas para resolver
`torch==2.15.0+cpu` (essa roda não existe no PyPI). Sem esse parâmetro, o pip
procuraria uma roda `+cpu` no PyPI e falharia. A sequência foi executada numa
`.venv` descartável em `%TEMP%` (356,8 s, com descargas; todos os passos com
saída 0, `No broken requirements found.` e `torch 2.15.0+cpu`) e o resultado
está registado em [validation.md](validation.md); a `.venv` descartável foi
apagada depois.
A criação acima (sem lock) resolve as versões atuais do índice CPU; é a
reconstrução a partir do lock que fixa o conjunto exato.

O conjunto resolvido (`audio-worker/requirements.lock.txt`) inclui os extras
ingleses de `misaki[en]` e as dependências transitivas reais: `soundfile`,
`numpy`, `spacy`, `transformers`, `espeakng-loader`, `av`, `ctranslate2` e
`pytest`.

### Recursos linguísticos

- **spaCy inglês**: `misaki.en.G2P` usa `en_core_web_sm`. É um recurso de dados,
  não uma dependência pip, e fica dentro da `.venv` (não em `models/`). O
  `audio-worker-prepare` instala-o explicitamente; recriar a `.venv` obriga a
  repetir a preparação.
- **eSpeak-NG**: incluído no wheel `espeakng-loader`; verificado nesta máquina
  em `C:\Repos\Thacio\language-assistant-tutor\.venv\Lib\site-packages\espeakng_loader\espeak-ng.dll`.

## 3. Preparar os modelos (único passo com rede)

```powershell
.\.venv\Scripts\python.exe -m audio_worker.prepare --models-dir "$env:LOCALAPPDATA\language-assistant-tutor\Modelos de Voz"
```

- A pasta indicada é **absoluta e fica fora do repositório**. Nesta máquina foi
  usada `C:\Users\exemplo\AppData\Local\language-assistant-tutor\Modelos de Voz`
  (contém espaços, como o exemplo de configuração). `models/` dentro do
  repositório também é ignorado pelo Git, mas não foi usado.
- O comando descarrega `Systran/faster-whisper-small` (revisão
  `536b0662742c02347bc0e980a01041f333bce120`), a configuração, os pesos
  `kokoro-v1_0.pth` e a voz `voices/af_heart.pt` de `hexgrad/Kokoro-82M`
  (revisão `f3ff3571791e39611d31c381e3a41a3af07b4987`), instala
  `en_core_web_sm 3.8.0` quando falta, valida o eSpeak-NG, escreve
  `<models-dir>\models.json` e termina com uma síntese fixa de verificação.
- Resultado real: **34,3 s** na primeira execução — medida com a pasta temporária
  sem espaços `%LOCALAPPDATA%\language-assistant-tutor\models`, antes de os
  ficheiros serem movidos para o caminho final com espaços (descarga completa,
  instalação do spaCy e síntese de verificação) — e **10,2 s** na repetição já
  sobre a pasta final. Tamanho final: 25 ficheiros,
  **814 144 526 bytes (776,4 MiB)**, dos quais
  `faster-whisper-small\model.bin` 461,1 MiB e
  `kokoro\kokoro-v1_0.pth` 312,1 MiB.
- A síntese de verificação produziu
  `<models-dir>\smoke-test\af_heart-smoke.wav` com 3 900 ms a 24 kHz.
- O `models.json` contém caminhos absolutos com espaços (`whisperDir`,
  `kokoroConfig`, `kokoroWeights`, `kokoroVoice`), as duas revisões e o inventário
  de recursos (`spacyModel`, `spacyModelVersion`, `espeakLibrary`, `espeakData`,
  `espeakFallback`).
- O comando é idempotente quanto aos ficheiros: repeti-lo valida as revisões no
  Hugging Face Hub e reutiliza a cache local (na segunda execução os 6 ficheiros
  do Whisper foram recontados a ~200 ficheiros/s e os pesos não foram
  transferidos de novo), mas **continua a contactar o Hub** e por isso exige
  rede — tal como a primeira execução. Foi o que aconteceu na repetição de
  10,2 s.

O teste real opcional corre explicitamente com o manifesto preparado:

```powershell
$env:LANGUAGE_TUTOR_MODELS_MANIFEST = "$env:LOCALAPPDATA\language-assistant-tutor\Modelos de Voz\models.json"
.\.venv\Scripts\python.exe -m pytest audio-worker/tests/test_offline_models.py -m models -q
```

Resultado: `2 passed` em 12,18 s (síntese da pergunta + transcrição; silêncio →
`no_speech`). Sem pesos, o mesmo comando faz *skip* explícito — um skip não
prova o percurso real.

## 4. Configuração pessoal `audio.json` (fora do Git)

A aplicação lê a configuração uma vez no arranque, de
`app_config_dir()/audio.json` ou do caminho indicado por
`LANGUAGE_TUTOR_CONFIG`. Nesta máquina o ficheiro ficou em:

```text
C:\Users\exemplo\AppData\Roaming\dev.thacio.languageassistanttutor\audio.json
```

Conteúdo usado (caminhos absolutos; a pasta de modelos contém espaços):

```json
{
  "schemaVersion": 1,
  "pythonExecutable": "C:\\Repos\\Thacio\\language-assistant-tutor\\.venv\\Scripts\\python.exe",
  "modelsManifest": "C:\\Users\\exemplo\\AppData\\Local\\language-assistant-tutor\\Modelos de Voz\\models.json"
}
```

Este ficheiro **não é versionado** e não deve ser copiado para o repositório.
O exemplo genérico está em
[`config/audio.example.json`](../../config/audio.example.json). O módulo Python é
fixo (`audio_worker`), instalado na `.venv`; o processo recebe
`--models-manifest <caminho>` e usa a pasta de dados da aplicação como diretório
de trabalho, por isso a aplicação pode ser aberta de qualquer diretório.

Depois de alterar `audio.json` é preciso reiniciar a aplicação. Sem Python ou
sem modelos, a janela abre e indica o componente em falta; gravar, ouvir e
guardar revisões continuam disponíveis.

## 5. Verificar o projeto

```powershell
.\scripts\verify.ps1
```

Executa, pela ordem, e pára no primeiro erro: `npm.cmd run typecheck`,
`npm.cmd test`, `npm.cmd run build`, `cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings`, `cargo test`, `pytest` sem a marca
`models` e `pip check`. Só imprime sucesso se todos terminarem com saída 0.
Resultado nesta máquina: 8/8 checks com saída 0 (23,4 s com tudo compilado e 116,4 s
quando o clippy e os testes precisaram de recompilar e o Vitest correu a frio; a
duração depende da cache).

## 6. Executar a aplicação

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
npm.cmd run tauri -- dev
```

O `tauri dev` arranca o Vite na porta 1420 e abre a janela «Tutor de
entrevistas» com a pergunta fixa
`Tell me about a time you solved a difficult problem at work.`

Compilação release sem instalador (antecipa problemas de compilação; **não** é
um pacote autónomo com Python incluído — o pacote final é da fase 5):

```powershell
npm.cmd run tauri -- build --no-bundle
```

O binário fica em `src-tauri\target\release\` e, para funcionar, continua a
depender da `.venv`, do `audio.json` e dos modelos preparados nesta máquina.

## 7. Problemas reais encontrados

| Sintoma | Causa | Resolução |
| --- | --- | --- |
| `failed to run 'cargo metadata' ... program not found` | `cargo` fora do `PATH` ao chamar o CLI do Tauri | acrescentar `%USERPROFILE%\.cargo\bin` ao `PATH` (secção 1) |
| `npm warn Unknown user config "python"` | configuração global do npm nesta máquina, não do projeto | aviso apenas; não afeta os comandos |
| A primeira síntese (e a síntese de verificação) demora ~7–8 s até produzir áudio | carregamento inicial do Kokoro em CPU | medido e registado em [validation.md](validation.md); nas chamadas seguintes o modelo é reutilizado (~1,2 s por síntese) |
| Avisos `UserWarning`/`FutureWarning` do PyTorch e do Misaki | bibliotecas de terceiros durante o carregamento | não impedem a síntese nem a transcrição; ficaram registados |

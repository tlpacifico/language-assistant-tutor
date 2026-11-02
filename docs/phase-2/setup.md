# Preparação e execução da fase 2 no Windows

Data da verificação: 2026-11-02 · Ramo: `feature/phase-2-guided-interview`

Este documento regista o que mudou para quem usa a aplicação na fase 2
(entrevista guiada), onde ficam os dados e os comandos que foram realmente
executados nesta máquina. A matriz de critérios F2-01 a F2-16 está em
[validation.md](validation.md). O ambiente base (Python, modelos, áudio e
`audio.json`) continua a ser o da fase 1 e está descrito em
[`docs/phase-1/setup.md`](../phase-1/setup.md); não é repetido aqui.

A fase 2 **não** está concluída: a implementação e a verificação automática
estão feitas, mas falta a validação manual do utilizador descrita na secção 6.

## 1. O que mudou para o utilizador

| Antes (fase 1) | Agora (fase 2) |
| --- | --- |
| Uma pergunta fixa em inglês. | Preparação da sessão: categoria e quantidade de perguntas principais (três por defeito, uma ou outra quantidade elegível). |
| Uma resposta por execução, guardada em JSON. | Entrevista com várias perguntas, progresso «Pergunta 2 de 3» e uma sessão em curso de cada vez. |
| — | Banco editável de 20 perguntas em seis categorias, com competências e critérios. |
| — | Revisão guardada, confirmação imutável do texto e avanço explícito, separados. |
| — | Histórico de sessões com perguntas, gravações, resultados automáticos, revisões e textos confirmados. |
| Treinos em `phase1/` (JSON). | Os treinos antigos continuam a ser consultados em «Histórico», em modo de leitura. |

As páginas da janela são «Preparar», «Banco de perguntas», «Entrevista» e
«Histórico». A interface, as explicações e os erros são em português de
Portugal; as perguntas e as respostas de treino são em inglês.

O que a fase 2 **não** faz: seguimentos, feedback, avaliação, classificações,
chamadas à API de IA, novas tentativas de respostas confirmadas, eliminação de
sessões e instalador autónomo. Uma sessão terminada não é uma entrevista
avaliada; o histórico apresenta um resumo sem nota nem feedback pendente.

## 2. Onde ficam os dados

Os caminhos são resolvidos pelo sistema operativo através de `app_data_dir()` e
`app_config_dir()` do Tauri. Nesta instalação de desenvolvimento ambos apontam
para a mesma pasta em `%APPDATA%`:

```text
%APPDATA%\dev.thacio.languageassistanttutor\
  audio.json                 # configuração pessoal da fase 1 (não versionada)
  tutor.sqlite3              # autoridade dos dados da fase 2
  tutor.sqlite3-journal      # journal DELETE enquanto uma transação está aberta
  sessions\<session-id>\<session-question-id>\<recording-id>\
      original.partial.wav   # captura ainda não finalizada
      original.wav           # WAV finalizado; nunca sobrescrito
      normalized.wav         # derivado de 16 kHz, quando necessário
  phase1\                    # preservado, apenas leitura na aplicação nova
      attempts\...           # treinos e revisões da fase 1
      question-cache\...     # cache de síntese partilhada
```

- **`tutor.sqlite3`** guarda o banco de perguntas, as cópias fixas de cada
  sessão, a posição e o estado da sessão, as gravações, as transcrições
  automáticas, as revisões append-only, as confirmações imutáveis, as operações
  e os recibos de comandos. É a única autoridade das sessões novas; não existe
  um segundo JSON com as mesmas sessões.
- **`sessions\`** guarda os áudios. O caminho é montado a partir de
  identificadores gerados no núcleo; a interface nunca envia caminhos. Só
  `original.wav` finalizado é publicado como resposta; `original.partial.wav`
  nunca é apresentado como uma resposta válida.
- **`phase1\`** mantém o formato JSON anterior. A aplicação nova só lê: lista
  treinos a partir das pastas de tentativas (não só de `last-attempt.json`),
  abre originais, transcrições e revisões, e não cria sessões, confirmações ou
  categorias para esses dados.
- **Modelos e `.venv`** continuam fora da pasta de dados e fora do repositório
  (ver [fase 1](../phase-1/setup.md), secções 2 e 3).
- **Nada disto é versionado.** `.gitignore` continua a excluir `.venv/`,
  `models/`, `dist/`, `node_modules/`, `audio.json` pessoal e `.superpowers/`.
  As gravações e a base de dados nunca devem ser copiadas para o repositório.

No primeiro arranque a aplicação cria `phase1\` e `tutor.sqlite3` (migração 001
mais o banco inicial). A pasta `sessions\` só aparece na primeira gravação, ao
reservar o diretório do original. A migração e o seed correm numa única
transação; uma falha não apaga nem recria a base existente. Nesta máquina, o
primeiro arranque da versão da fase 2 criou `tutor.sqlite3` com 135 168 bytes,
`user_version = 1` e 20 perguntas (distribuição 4/3/3/4/3/3), sem sessões.

## 3. Instalar a partir de um checkout limpo

Partindo da raiz do repositório e de um Windows com os pré-requisitos da fase 1
(WebView2, Node, Rust, Visual Studio C++, Python 3.11):

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
npm.cmd install

py -3.11 -m venv .venv
.\.venv\Scripts\python.exe -m pip install --upgrade pip
.\.venv\Scripts\python.exe -m pip install torch --index-url https://download.pytorch.org/whl/cpu
.\.venv\Scripts\python.exe -m pip install -r audio-worker/requirements.in
.\.venv\Scripts\python.exe -m pip install -e "audio-worker[test]"
.\.venv\Scripts\python.exe -m pip check

.\.venv\Scripts\python.exe -m audio_worker.prepare --models-dir "$env:LOCALAPPDATA\language-assistant-tutor\Modelos de Voz"
```

Depois criar `%APPDATA%\dev.thacio.languageassistanttutor\audio.json` com
caminhos absolutos para o Python da `.venv` e para o `models.json` preparado
(modelo em [`config/audio.example.json`](../../config/audio.example.json)).

A fase 2 não acrescenta passos de instalação: a única dependência nova é
`rusqlite 0.41.1` com a funcionalidade `bundled`, resolvida pelo `Cargo.lock`
(compila o SQLite incluído na biblioteca; não exige SQLite instalado no
sistema). Um `cargo build`/`npm.cmd run tauri -- dev` na primeira vez compila
essa dependência e demora mais do que nas execuções seguintes.

## 4. Executar

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"   # o cargo não está no PATH por omissão
npm.cmd run tauri -- dev
```

O `tauri dev` abre a janela «Tutor de entrevistas» em «Preparar». Sem Python,
sem `audio.json` ou sem modelos, a janela abre e indica o componente em falta;
a preparação, o banco, a revisão e o histórico continuam utilizáveis. Uma falha
de abertura da base mostra um erro recuperável com a ação de repetir a abertura,
sem fechar a janela.

Compilação release **sem instalador** (não é um pacote autónomo; depende da
`.venv`, do `audio.json` e dos modelos desta máquina — a distribuição é da fase
5):

```powershell
npm.cmd run tauri -- build --no-bundle
```

O binário fica em `src-tauri\target\release\language-assistant-tutor.exe`.

## 5. Verificar

```powershell
.\scripts\verify.ps1
```

Executa, pela ordem, e pára no primeiro erro: `npm.cmd run typecheck`,
`npm.cmd test`, `npm.cmd run build`, `cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings`, `cargo test`, `pytest` sem a marca
`models` e `pip check`. Resultado real desta verificação: **8/8 checks com saída
0 em 18,5 s** (147 testes Rust, 43 testes Vitest em 6 ficheiros, 36 testes
Python com 2 deselecionados, `pip check` limpo). O detalhe e a saída estão em
[validation.md](validation.md).

O teste real dos motores corre à parte, com o manifesto preparado:

```powershell
$env:LANGUAGE_TUTOR_MODELS_MANIFEST = "$env:LOCALAPPDATA\language-assistant-tutor\Modelos de Voz\models.json"
.\.venv\Scripts\python.exe -m pytest audio-worker/tests/test_offline_models.py -m models -q
```

Resultado real: `2 passed` em 48,14 s (não foi skip). Sem `models.json` válido o
mesmo comando faz skip explícito; um skip não prova o percurso real.

## 6. Validação manual que falta (ação do utilizador)

Com a aplicação aberta por `npm.cmd run tauri -- dev` e os modelos preparados:

1. **Sessão de três perguntas reais** em «Preparar»: categoria
   (por exemplo «Todas as categorias») e quantidade três. Responder às três
   perguntas com voz, incluindo uma resposta com termos técnicos. Registar a
   duração de cada áudio e os tempos de carregamento, síntese e transcrição,
   e a perceção da espera.
2. **Ouvir** a pergunta sintetizada (e repeti-la sem nova síntese) e ouvir os
   originais gravados de cada resposta.
3. **Rever e confirmar**: corrigir o texto automático, guardar a revisão,
   confirmar a resposta e só depois avançar. Confirmar que a resposta confirmada
   fica apenas de leitura.
4. **Sair e retomar**: depois de confirmar uma resposta e antes de avançar,
   fechar a aplicação e reabri-la; a sessão deve voltar à etapa persistida, com
   a confirmação pendente de avanço explícito.
5. **Terminar e consultar o histórico**: concluir a última pergunta, consultar
   o resumo da sessão e ouvir os originais a partir do histórico.
6. **Sessão de uma pergunta** e **outra quantidade elegível**; numa categoria
   com menos perguntas do que as pedidas, confirmar o aviso de insuficiência e
   que só inicia após ajuste explícito da quantidade.
7. **Editar o banco** (adicionar ou alterar uma pergunta) com uma sessão já
   iniciada e confirmar que a cópia dessa sessão não muda.
8. **Percurso offline**: desligar a rede e repetir uma sessão; a aplicação não
   deve tentar descarregar nada nem contactar serviços de IA.
9. **Transcrição indisponível**: parar o auxiliar (ou usar áudio sem fala
   reconhecida) e confirmar que a revisão manual explícita é possível, que o
   resultado `no_speech` é preservado e que nada é apresentado como problema de
   pronúncia.
10. **Erro de escrita e recuperação**: com uma revisão por guardar, fechar a
    janela e confirmar as opções guardar/descartar/cancelar; confirmar que uma
    falha mantém o texto e a janela. Se se usar uma falha simulada (por exemplo,
    bloquear o ficheiro), identificá-la como simulação — não é hardware real.

As gravações e a base ficam em `%APPDATA%\dev.thacio.languageassistanttutor\`,
nunca no repositório. Os critérios manuais da fase 1 que continuam pendentes
(CA-02, CA-03, CA-05, CA-06, CA-10, CA-13 parcial e CA-14) **não** são
fechados por esta validação da fase 2; ver
[`docs/phase-1/validation.md`](../phase-1/validation.md).

## 7. Problemas reais encontrados nesta fase

| Sintoma | Causa | Resolução |
| --- | --- | --- |
| `failed to run 'cargo metadata' ... program not found` | `cargo` fora do `PATH` ao chamar o CLI do Tauri | acrescentar `%USERPROFILE%\.cargo\bin` ao `PATH` |
| `npm warn Unknown user config "python"` | configuração global do npm nesta máquina, não do projeto | aviso apenas; não afeta os comandos |
| Primeira compilação mais lenta depois da fase 2 | compilação inicial de `rusqlite`/`libsqlite3-sys` com SQLite `bundled` | esperado; fica em cache nas execuções seguintes |
| Uma base criada por uma versão futura é recusada | `PRAGMA user_version` superior à suportada | comportamento pretendido: erro recuperável, sem escrita nem recriação da base |
| Testes Rust com o auxiliar simulado podem deixar processos Python | higiene dos testes, não comportamento da aplicação | terminar os processos órfãos; registado também na fase 1 |

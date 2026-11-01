# Fase 1 — Áudio local Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Entregar uma aplicação Windows que lê uma pergunta fixa, grava uma resposta, a transcreve localmente e guarda uma revisão sem perder o original.

**Architecture:** React/TypeScript apresenta o treino e comunica com comandos Tauri. Rust controla operações, captura/reprodução e ficheiros; um processo Python persistente executa faster-whisper e Kokoro por JSON em entrada/saída padrão. A fase usa JSON local, sem SQLite, serviços externos ou instalador final.

**Tech Stack:** Tauri 2, Rust, React, TypeScript, Vite, CPAL, Rodio, Hound, Rubato, atomicwrites, Python 3.11, faster-whisper e Kokoro-82M.

**Spec:** [Spec da fase 1](../specs/2026-11-01-phase-1-local-audio-design.md). Ler também o [AGENTS.md](../../../AGENTS.md) e respeitar o [plano das fases](2026-11-01-star-interview-tutor-phases.md).

**Estado:** plano técnico escrito; tarefas e verificações ainda não executadas. O pedido de avançar para este plano permite tratar a spec da fase 1 como referência aceite para o planeamento. Não regista aprovação da implementação nem inicia a fase 2.

## Global Constraints

- “Aplicação desktop executável em desenvolvimento no Windows.”
- “Interface e mensagens | Português de Portugal.”
- Pergunta exata: `Tell me about a time you solved a difficult problem at work.`
- “Inglês; transcrever, sem traduzir ou melhorar a resposta.”
- “faster-whisper, modelo `small` multilingue, CPU/int8, língua definida como inglês.”
- “Um trabalho de transcrição ou síntese de cada vez.”
- “Uma resposta ativa na interface; cada nova gravação tem identificador e ficheiros próprios.”
- “A captura não depende de o processo Python estar pronto.”
- “Normalizar um derivado para WAV PCM de 16 bits, mono, 16 kHz quando necessário para a transcrição.”
- “O áudio finalizado é guardado e pode ser reproduzido, mesmo quando a transcrição falha.”
- “Nesta fase, guardar uma revisão não equivale a confirmar uma resposta para avaliação.”
- Sem OpenCode, credenciais, SQLite, classificação de pronúncia, sessões de entrevista ou funcionalidades das fases seguintes.
- Resultados simulados são exclusivos dos testes. A aplicação normal usa os motores reais e informa indisponibilidade.

## Review Focus

1. Falha de escrita ou disco cheio: conservar a última resposta válida e a revisão anterior, sem mostrar sucesso falso. Testes da tarefa 2.
2. Dispositivo removido, formato inesperado ou consumidor lento: não bloquear o callback nem descartar áudio silenciosamente. Testes da tarefa 3.
3. Modelos, voz ou recursos linguísticos incompletos: indicar o componente em falta sem download durante a prática. Testes da tarefa 4.
4. Processo bloqueado, stdout inesperado ou resultado atrasado: libertar a operação, preservar artefactos e não aplicar resultados de outra geração. Testes das tarefas 5 e 6.
5. Revisão por guardar ao gravar outra resposta ou fechar: guardar, descartar ou cancelar explicitamente, preservando a edição se a escrita falhar. Testes da tarefa 7.

## 1. Ambiente e decisões técnicas

### Ambiente observado nesta sessão

- A pasta contém documentação e `.gitignore`, sem uma aplicação implementada. Na verificação final, o repositório Git já se encontrava inicializado, com ficheiros preparados no staging; preservar esse estado preexistente.
- Node `24.11.1`, npm `11.6.2`; Python 3.11, 3.10 e 3.13 enumerados pelo launcher `py`.
- Rust/Cargo não encontrados no PATH nem no caminho habitual verificado. Visual Studio 2022 Community com ferramentas C++ foi encontrado; verificar Windows SDK e WebView2 na execução.
- Não foi instalado software, transferido qualquer peso de modelo ou executado um build para elaborar este plano.

### Versões de referência

Valores consultados nos registos oficiais em 2026-11-01, a fixar nos manifestos e lockfiles ao executar:

| Componente | Referência |
| --- | --- |
| Rust | `1.99.1`, toolchain MSVC `x86_64-pc-windows-msvc`; componentes rustfmt e clippy. |
| Node | Manter o `24.11.1` disponível; usar `npm.cmd` nos comandos PowerShell. |
| React / React DOM | `19.3.1`. |
| Vite / TypeScript / Vitest | `8.3.5` / `7.0.2` / `5.0.5`. |
| Tauri Rust / API e CLI npm | `2.12.5` / `2.12.4`; manter a família 2. |
| CPAL / Rodio / Hound / Rubato | `0.18.2` / `0.23.0` / `3.5.1` / `5.0.1`. |
| atomicwrites | `0.4.4`, para substituição atómica de JSON também em Windows. |
| Python | Usar `py -3.11`, criando `.venv`; não usar o Python 3.13 predefinido. |
| faster-whisper / Kokoro / Misaki | `1.2.1` / `0.9.4` / `0.9.4`, com extras ingleses de Misaki. |

O Kokoro/Misaki publicado requer Python anterior a 3.13. Resolver as dependências transitivas compatíveis, incluindo PyTorch CPU, spaCy, Transformers, SoundFile e testes, na tarefa 4. Guardar `package-lock.json`, `Cargo.lock` e `audio-worker/requirements.lock.txt` após resolução e verificação; a consulta de versões não comprova compatibilidade do conjunto.

Usar `serde`/`serde_json`, `uuid`, `tokio`, `sha2` e `tempfile` como suporte Rust; Vitest, Testing Library e jsdom para testes da interface. Não acrescentar servidor HTTP, framework de estado global ou gerador de contratos nesta fase.

### Decisões fechadas para a fase

- Identificador Tauri: `dev.thacio.languageassistanttutor`.
- Captura e reprodução nativas em Rust: CPAL para entrada, Rodio para saída; uma thread de áudio possui os streams e recebe mensagens. Não guardar streams potencialmente não transferíveis num mutex partilhado entre comandos.
- Original WAV PCM16 à taxa e número de canais efetivos da entrada; aceitar I16, U16 e F32, convertendo amostras para PCM16 sem alterar a taxa. Outros formatos devolvem erro explícito, sem panic.
- Hound lê/escreve WAV; Rubato reamostra, com downmix antes da conversão para 16 kHz. Compensar o atraso do filtro e finalizar o último bloco sem cortar a resposta.
- Não definir limite de tempo de resposta nesta fase. Escrever o áudio em streaming com memória limitada; erros de capacidade, escrita ou buffer produzem interrupção explícita e tentativa de recuperar o ficheiro parcial.
- Kokoro em CPU, inglês americano `lang_code='a'`, voz `af_heart`, velocidade `1.0`; saída WAV PCM16 mono a 24 kHz.
- Executar Python por caminho absoluto configurado, `-u -m audio_worker`, com diretório de trabalho explícito. Em desenvolvimento usa a `.venv`; não depender do diretório de onde a app foi aberta.
- Prazos: handshake 10 s; carregamento de cada modelo 180 s; transcrição `max(120 s, 5 × duração do áudio)` após carregar; síntese 120 s após carregar; encerramento 3 s. Um progresso não renova indefinidamente um prazo. Um timeout encerra o auxiliar e invalida a sua geração.
- Persistência em `app_data_dir()/phase1`; modelos em diretório separado indicado pela configuração. Não escrever pesos ou gravações no código-fonte.

## 2. Estrutura dos ficheiros a criar

| Ficheiros | Responsabilidade |
| --- | --- |
| `package.json`, `package-lock.json`, `tsconfig.json`, `vite.config.ts`, `index.html` | Base React/Vite, testes e comandos npm. |
| `.gitignore` existente; `rust-toolchain.toml`, `README.md` novos | Preservar/rever exclusões; fixar toolchain e instruções reais. |
| `src/main.tsx`, `src/App.tsx` | Arranque e montagem da experiência. |
| `src/features/practice/{Practice.tsx,usePractice.ts,practice.css}` | Interface da fase 1 e estado da revisão local. |
| `src/lib/{contracts.ts,tauri.ts}` | Tipos do frontend e adaptador de comandos/eventos. |
| `src/test/setup.ts`, `src/features/practice/Practice.test.tsx` | Ambiente de testes e comportamento observável. |
| `src-tauri/{Cargo.toml,Cargo.lock,build.rs,tauri.conf.json,capabilities/default.json}` | Base Tauri, permissões e janela. |
| `src-tauri/src/{main.rs,lib.rs,domain.rs}` | Arranque e contratos/regras independentes do hardware. |
| `src-tauri/src/storage.rs` | Artefactos, JSON atómico e recuperação. |
| `src-tauri/src/audio/{mod.rs,capture.rs,playback.rs,normalize.rs}` | Porta de áudio, adaptadores nativos e conversão. |
| `src-tauri/src/worker/{mod.rs,protocol.rs}` | Supervisão do auxiliar e protocolo JSON. |
| `src-tauri/src/{service.rs,commands.rs}` | Coordenação do fluxo e comandos Tauri finos. |
| `src-tauri/tests/{domain.rs,storage.rs,audio.rs,worker.rs,service.rs}` | Testes de regras e integração com adaptadores controlados. |
| `src-tauri/tests/fixtures/worker_stub.py` | Auxiliar de teste com falha, atraso e respostas conhecidas. |
| `audio-worker/{pyproject.toml,requirements.in,requirements.lock.txt}` | Dependências Python e lock verificado. |
| `audio-worker/src/audio_worker/{__init__.py,__main__.py,protocol.py,engines.py,prepare.py}` | Protocolo, motores locais e preparação explícita. |
| `audio-worker/tests/{test_protocol.py,test_engines.py,test_offline_models.py}` | Contrato, carregamento e testes reais opcionais. |
| `contracts/worker-v1.examples.json`, `contracts/app-v1.example.json` | Exemplos partilhados e verificados nas três linguagens. |
| `config/audio.example.json`, `scripts/verify.ps1` | Configuração documentada e execução dos checks. |
| `docs/phase-1/{setup.md,validation.md}` | Preparação Windows e resultados reais de aceitação. |

Os testes Rust de cada módulo podem permanecer no ficheiro de teste indicado; a produção não precisa de um módulo por teste. Ícones/configurações padrão gerados pelo Tauri integram a base da tarefa 1. Não gerar o projeto por cima da documentação existente.

## 3. Contratos partilhados

### Tipos do domínio

Definir em `src-tauri/src/domain.rs`; duplicar apenas os tipos serializados em `src/lib/contracts.ts`, verificados contra os exemplos. JSON usa camelCase; enums usam snake_case. Identificadores são UUID em texto, datas UTC RFC3339 e durações inteiras em milissegundos.

| Tipo | Campos e valores obrigatórios |
| --- | --- |
| `AppError` | `code: String`, `message: String`, `retryable: bool`; códigos estáveis, mensagens em português. |
| `ComponentState` | `status: ready \| missing \| loading \| error`, `message: Option<String>`. |
| `Availability` | `microphone`, `playback`, `transcription`, `synthesis`: `ComponentState`. |
| `CaptureMeta` | `sampleRate: u32`, `channels: u16`, `durationMs: u64`, `bytes: u64`, `incomplete: bool`. |
| `Transcript` | `id`, `status: completed \| no_speech`, `text`, `segments: Vec<Segment>`, `model`, `language`, `createdAt`, `loadMs`, `processingMs`. |
| `Segment` | `startMs`, `endMs`, `text`; manter o conteúdo devolvido pelo motor. |
| `Review` | `sourceTranscriptId`, `text`, `savedAt`. |
| `Attempt` | `id`, `question`, `createdAt`, `capture: CaptureMeta`, `originalFile`, `normalizedFile: Option<String>`, `transcripts: Vec<Transcript>`, `review: Option<Review>`, `lastOperation: Option<OperationRecord>`. Ficheiros são referências relativas. |
| `OperationRecord` | `id`, `kind: transcribe \| synthesize`, `state: running \| completed \| failed \| interrupted \| no_speech`, `error: Option<AppError>`. |
| `OperationState` | Enum Rust `Running`, `Completed`, `Failed`, `Interrupted`, `NoSpeech`, serializado com os valores da linha anterior. |
| `ModelJob` | `id`, `attemptId: Option<String>`, `kind`, `stage: loading \| processing`. |
| `PlaybackTarget` | `question \| response`; só resolve ficheiros já pertencentes à app. |
| `AppSnapshot` | `revision: u64`, `question`, `availability: Availability`, `attempt: Option<Attempt>`, `recordingId: Option<String>`, `modelJob: Option<ModelJob>`, `playback: Option<PlaybackTarget>`, `lastError: Option<AppError>`. |
| `RestoreResult` | `attempt: Option<Attempt>`, `warnings: Vec<AppError>`; permite recuperar texto/revisão mesmo se faltar um áudio ou derivado. |

Erros mínimos: `BUSY`, `DEVICE_UNAVAILABLE`, `DEVICE_FORMAT_UNSUPPORTED`, `CAPTURE_INTERRUPTED`, `STORAGE_WRITE_FAILED`, `ARTIFACT_MISSING`, `INVALID_AUDIO`, `MODEL_MISSING`, `WORKER_UNAVAILABLE`, `WORKER_PROTOCOL`, `WORKER_TIMEOUT`, `STALE_SOURCE`. Ausência de fala é um resultado `no_speech`, não um erro de pronúncia.

`AppState` e `Action` ficam também em `domain.rs`. `AppState::can_start_recording(&self) -> Result<(), AppError>` verifica ocupação, mas não exige modelos prontos; `AppState::accepts_result(&self, request_id: &str, generation: u64) -> bool` protege resultados. A geração do auxiliar é interna e não vai para a interface.

### Persistência

```text
phase1/
  last-attempt.json                         # {schemaVersion: 1, attemptId}
  attempts/<attempt-id>/
    original.wav
    normalized.wav
    attempt.json                           # metadados/captura/operação; não duplica textos
    transcripts/<transcript-id>.json        # Transcript original imutável
    review.json                            # Review guardada, substituição atómica
  question-cache/<sha256>.wav
```

O tipo interno `AttemptManifest` contém os campos de `Attempt` exceto `transcripts` e `review`, acompanhado de `schemaVersion: 1`. A resposta completa é reconstruída a partir desse manifesto, dos JSON de transcrição e de `review.json`; não manter duas cópias autoritativas do mesmo texto. Resultados de transcrição usam o ID do pedido e são ordenados por data e ID. Um resultado persistido com o ID de uma operação marcada como `running` permite recuperar essa operação como `completed` ou `no_speech`, segundo o resultado; sem resultado, passa a `interrupted`.

Gravar metadados antes de atualizar `last-attempt.json`; só então publicar a nova resposta ativa. Se a atualização do apontador falhar, preservar a anterior e manter a nova gravação disponível para repetir a publicação. Nunca usar remove-then-rename para substituir JSON em Windows.

`AttemptStore` em `storage.rs`: `new(root: PathBuf) -> Self`, `allocate() -> Result<RecordingTarget, AppError>`, `publish(capture: CapturedAudio) -> Result<Attempt, AppError>`, `restore_last() -> Result<RestoreResult, AppError>`, `append_transcript(attempt_id: &str, transcript: Transcript) -> Result<Attempt, AppError>`, `save_review(attempt_id: &str, source_id: &str, text: &str) -> Result<Review, AppError>`, `set_operation(attempt_id: &str, operation: OperationRecord) -> Result<(), AppError>`. Publicação e escrita mantêm referências antigas em caso de falha; `restore_last` recupera artefactos válidos e devolve avisos se um componente faltar. Um manifesto/apontador ilegível dá erro recuperável, sem eliminar ficheiros nem impedir nova captura.

### Áudio

`RecordingTarget { id: String, directory: PathBuf }` e `CapturedAudio { id: String, original: PathBuf, meta: CaptureMeta }` definidos em `audio/mod.rs`.

Porta `AudioPort: Send + Sync`: `availability() -> AvailabilityAudio`, `start_capture(target: RecordingTarget) -> Result<(), AppError>`, `stop_capture() -> Result<CapturedAudio, AppError>`, `play(path: PathBuf) -> Result<(), AppError>`, `stop_playback() -> Result<(), AppError>`. `AvailabilityAudio` contém `microphone` e `playback: ComponentState`. Eventos `AudioEvent`: `CaptureInterrupted { capture: Option<CapturedAudio>, error }` e `PlaybackFinished`. Implementação `NativeAudio` usa a thread proprietária; testes usam `FakeAudio`.

`normalize_wav(source: &Path, destination: &Path) -> Result<CaptureMeta, AppError>` em `audio/normalize.rs`; nunca modifica `source`.

### Protocolo Python v1

Uma linha JSON por mensagem, com `protocolVersion: 1`, `id` e `op`. Operações: `health`, `transcribe`, `synthesize`, `shutdown`. `health` verifica disponibilidade sem carregar os modelos ou descarregar ficheiros.

| Operação | `params` | `result` de sucesso |
| --- | --- | --- |
| `health` | `{}` | `{transcription: ComponentState, synthesis: ComponentState}` |
| `transcribe` | `{audioPath, modelPath, device: "cpu", computeType: "int8", language: "en"}` | `{status: "completed" ou "no_speech", text, segments, model: "small", language: "en", loadMs, processingMs}` |
| `synthesize` | `{text, outputPath, voice: "af_heart", speed: 1.0}` | `{audioPath, sampleRate: 24000, durationMs, loadMs, processingMs}` |
| `shutdown` | `{}` | `{stopped: true}`, seguido de saída do processo. |

Resposta final: `{protocolVersion, id, event: "result", ok: true, result}` ou `{protocolVersion, id, event: "result", ok: false, error: AppError}`. Progresso: `{protocolVersion, id, event: "progress", stage: "loading" ou "processing"}`. Exatamente uma resposta final por pedido válido; preservar stdout exclusivamente para o protocolo e drenar stderr continuamente em Rust.

`WorkerClient` em `worker/mod.rs`: `start(config: WorkerConfig) -> Result<Self, AppError>`, `submit(request: WorkerRequest) -> Result<(), AppError>`, `restart() -> Result<(), AppError>`, `shutdown() -> Result<(), AppError>`. `WorkerConfig` contém caminhos absolutos do executável e manifesto de modelos, o nome fixo do módulo e os prazos. `WorkerEvent { generation: u64, message: WorkerMessage }` é entregue ao serviço por canal; `WorkerPort` oferece `submit` e `restart` para substituição nos testes.

O ficheiro pessoal `audio.json`, em `app_config_dir()` ou indicado por `LANGUAGE_TUTOR_CONFIG`, contém `schemaVersion: 1`, `pythonExecutable` e `modelsManifest`, ambos caminhos absolutos. O módulo é fixo `audio_worker`, instalado na `.venv`; o processo recebe `--models-manifest <caminho>` e usa a pasta de dados como diretório de trabalho. O manifesto preparado contém `whisperDir`, `kokoroConfig`, `kokoroWeights`, `kokoroVoice`, revisões dos modelos e inventário de recursos linguísticos. O exemplo usa caminhos Windows com espaços; não guardar a configuração pessoal no Git.

### Serviço e interface

`AppService` em `service.rs` recebe `AttemptStore`, `AudioPort` e `WorkerPort`; usa um coordenador serial para transições, delegando IO/inferência para trabalho assíncrono. Nunca mantém um lock de estado enquanto aguarda um dispositivo, processo ou ficheiro.

Comandos Tauri, definidos em `commands.rs`, delegam a métodos homónimos de `AppService` e devolvem `Result<AppSnapshot, AppError>`:

```text
get_snapshot()
refresh_availability()
start_recording()
stop_recording()
retry_save_recording()
transcribe_response(attempt_id: String)
play_audio(target: PlaybackTarget)
stop_audio()
save_review(attempt_id: String, source_id: String, text: String)
restart_worker()
prepare_close()
```

`AppService::on_audio_event(event: AudioEvent)` e `AppService::on_worker_event(event: WorkerEvent)` aplicam resultados e publicam `practice://snapshot` com revisão crescente. `prepare_close` finaliza captura válida, assinala jobs interrompidos e encerra o auxiliar; uma falha que possa perder dados impede fechar silenciosamente.

O adaptador TS expõe `PracticeClient { snapshot(), refresh(), startRecording(), stopRecording(), retrySaveRecording(), transcribe(attemptId), play(target), stop(), saveReview(attemptId, sourceId, text), restartWorker(), prepareClose(), subscribe(listener) }`, com Promises de `AppSnapshot`; `subscribe` devolve uma função de remoção do listener. A correspondência entre nomes TS e comandos está apenas em `src/lib/tauri.ts`.

## 4. Tarefas de implementação

Executar sequencialmente. Cada tarefa termina numa entrega verificável e num commit local quando for possível fazê-lo sem incluir alterações alheias. Git já existe: não o reinicializar nem alterar o staging preexistente. Antes de cada commit, conferir o conteúdo e limitar a alteração à tarefa. Não publicar nem criar PR. Os comandos seguintes são instruções futuras, não resultados já obtidos.

### Tarefa 1: Base desktop e contratos de operação

**Ficheiros:** criar os manifestos/configurações da base, `rust-toolchain.toml`, `src/{main.tsx,App.tsx}`, `src/lib/contracts.ts`, `src-tauri/src/{main.rs,lib.rs,domain.rs}`, exemplos em `contracts/` e `src-tauri/tests/domain.rs`; rever o `.gitignore` existente apenas se faltar uma exclusão necessária.

**Interfaces:** produz `AppState`, `Action`, os tipos da secção 3 e uma janela com a pergunta exata. `AppSnapshot` inicial tem `revision=0`, sem resposta/gravação/job; componentes não verificados ficam `loading` com mensagem “A verificar disponibilidade”, até à verificação real.

- [ ] Preparar Rust 1.99.1/MSVC, verificar SDK/WebView2 e criar a base sem sobrescrever docs. Scripts npm: `dev`=Vite, `build`=typecheck e Vite build, `typecheck`=tsc --noEmit, `test`=vitest run, `tauri`=CLI Tauri. Ignorar `.venv`, `target`, `node_modules`, modelos, áudio e configuração pessoal; guardar lockfiles.
- [ ] Escrever `recording_gate` e `stale_generation`, com estas asserções, contra o domínio ainda não implementado:

```rust
assert!(idle_without_worker.can_start_recording().is_ok());
assert_eq!(recording.can_start_recording().unwrap_err().code, "BUSY");
assert_eq!(transcribing.can_start_recording().unwrap_err().code, "BUSY");
assert!(!waiting.accepts_result("old-request", 1));
```

- [ ] Executar `cargo test --manifest-path src-tauri/Cargo.toml --test domain`; confirmar falha por comportamento/tipos ainda ausentes depois de resolver a compilação da base.
- [ ] Implementar os tipos e regras indicados. Ler `contracts/app-v1.example.json` no teste para fixar camelCase, enums e pergunta. A UI inicial apenas apresenta a pergunta; não simular áudio.
- [ ] Verificar o teste anterior, `npm.cmd run typecheck`, `npm.cmd run build` e abrir `npm.cmd run tauri -- dev`. Esperado: testes/build com saída 0 e janela com a pergunta.
- [ ] Commit: `chore: create phase one desktop foundation`.

### Tarefa 2: Artefactos locais e recuperação sem SQLite

**Ficheiros:** criar `src-tauri/src/storage.rs` e `src-tauri/tests/storage.rs`; atualizar `domain.rs` apenas para os contratos já definidos.

**Interfaces:** consome `RecordingTarget`, `CapturedAudio`, `Attempt`, `Transcript` e `Review`; produz `AttemptStore` e a estrutura de ficheiros da secção 3. Aceitar uma raiz de teste temporária e obter a raiz real pelo Tauri apenas na integração.

- [ ] Escrever testes `failed_publish_preserves_last`, `failed_review_write_preserves_saved_text`, `append_does_not_overwrite_raw`, `restore_reports_missing_original` e `restore_interrupted_job`. Fixar asserções:

```rust
assert_eq!(store.restore_last()?.attempt.unwrap().id, previous_id);
assert_eq!(read_saved_review()?.text, "original review");
assert_eq!(restored.transcripts.len(), 2);
assert_eq!(restored.last_operation.unwrap().state, OperationState::Interrupted);
```

- [ ] Executar `cargo test --manifest-path src-tauri/Cargo.toml --test storage`; confirmar falha antes de implementar `AttemptStore`.
- [ ] Implementar as assinaturas da secção 3, com `atomicwrites::AtomicFile`, ficheiros temporários no mesmo volume e validação de referências. Reconstruir transcrições e revisão a partir dos ficheiros autoritativos, sem duplicar os textos no manifesto. Não atualizar o apontador da última resposta enquanto a publicação não terminar.
- [ ] Exercitar substituição real de um JSON já existente em Windows e falhas injetadas antes da escrita/substituição. Metadados inválidos ou ficheiros em falta devolvem diagnóstico e conservam os restantes dados; um original recuperável permite retomar transcrição.
- [ ] Repetir o teste. Esperado: saída 0, dados antigos intactos nos cenários de falha e novos IDs sem colisão de pastas.
- [ ] Commit: `feat: persist local audio attempts and reviewed transcripts`.

### Tarefa 3: Captura, reprodução e normalização nativas

**Ficheiros:** criar `src-tauri/src/audio/{mod.rs,capture.rs,playback.rs,normalize.rs}` e `src-tauri/tests/audio.rs`; acrescentar as dependências de áudio ao Cargo.

**Interfaces:** consome `RecordingTarget`; produz `NativeAudio`, `CapturedAudio`, `AudioEvent` e `normalize_wav`, conforme a secção 3. A thread de áudio possui os streams; callbacks entregam amostras através de canal limitado e não escrevem em disco nem aguardam locks.

- [ ] Escrever `normalizes_stereo_48k_without_mutating_source`, `empty_audio_is_invalid`, `capture_failure_preserves_previous`, `overflow_is_reported` e `recording_stops_playback`. Para a conversão de 1 s de sinal estéreo, verificar:

```rust
assert_eq!((output.sample_rate, output.channels, output.bits_per_sample), (16000, 1, 16));
assert!(output.frames.abs_diff(16000) <= 16);
assert_eq!(sha256(source_before), sha256(source_after));
assert_eq!(overflow_error.code, "CAPTURE_INTERRUPTED");
```

- [ ] Executar `cargo test --manifest-path src-tauri/Cargo.toml --test audio`; confirmar falha sem os adaptadores/conversão.
- [ ] Implementar CPAL/Hound e Rodio. Converter I16/U16/F32, detetar dispositivo ausente e formato não suportado. Escrever em ficheiro provisório, finalizar o cabeçalho WAV antes de publicar; em erro preservar uma captura parcial válida com `incomplete=true` ou reportar que não existe áudio recuperável. Canal cheio não pode descartar amostras silenciosamente.
- [ ] Implementar `normalize_wav` com média dos canais, Rubato e PCM16, compensando atraso/final do filtro. Acrescentar teste de duração a 44,1 kHz e de atenuação de um sinal acima de 8 kHz, para evitar uma conversão por simples descarte de amostras.
- [ ] Repetir testes; executar uma verificação manual local de entrada/saída quando o adaptador estiver ligado na tarefa 6. Testes automáticos não ligam o microfone real. Esperado agora: testes com ficheiros sintéticos/adaptador controlado passam.
- [ ] Commit: `feat: add native audio capture playback and normalization`.

### Tarefa 4: Processo Python com motores reais e preparação offline

**Ficheiros:** criar o pacote `audio-worker`, `config/audio.example.json`, exemplos de protocolo e os três ficheiros de teste Python listados na estrutura.

**Interfaces:** `parse_request(line: str) -> WorkerRequest` e `encode_message(message: dict) -> str` em `protocol.py`; `EngineRegistry.health() -> dict`, `transcribe(params: dict, progress: Callable[[str], None]) -> dict` e `synthesize(params: dict, progress: Callable[[str], None]) -> dict` em `engines.py`. `prepare.main(argv: list[str] | None = None) -> int` prepara explicitamente os recursos; `__main__.main() -> int` serve JSON. `WorkerRequest` é dataclass com `protocol_version`, `id`, `op`, `params`.

- [ ] Criar `.venv` com `py -3.11 -m venv .venv`. Instalar PyTorch CPU a partir do índice oficial `https://download.pytorch.org/whl/cpu`, as dependências de `requirements.in` e o pacote editável com `.\.venv\Scripts\python.exe -m pip install -e "audio-worker[test]"`. Fixar faster-whisper, Kokoro e Misaki nas versões da secção 1; depois de verificar imports e compatibilidade, guardar o resultado de `pip freeze --exclude-editable` em `requirements.lock.txt`. Documentar a reinstalação com o índice CPU e os recursos spaCy necessários. Documentar o eventual requisito de eSpeak-NG/fallback no Windows após validar a combinação escolhida.
- [ ] Escrever `test_protocol_examples`, `test_missing_assets_does_not_connect`, `test_stdout_contains_only_json` e `test_voice_and_text_cache_key`. Fixar valores:

```python
assert request.op == "transcribe"
assert health["synthesis"]["status"] == "missing"
assert all(json.loads(line)["protocolVersion"] == 1 for line in stdout.splitlines())
assert first_key != changed_voice_key
```

- [ ] Executar `.\.venv\Scripts\python.exe -m pytest audio-worker/tests/test_protocol.py audio-worker/tests/test_engines.py -q`; confirmar falha por contratos/comportamentos ainda ausentes, não por dependências mal instaladas.
- [ ] Implementar parser e servidor sequencial. Imports dos motores são tardios; redirecionar stdout das bibliotecas para stderr. Aplicar `HF_HUB_OFFLINE=1` e `TRANSFORMERS_OFFLINE=1` antes dos imports no servidor; `health` verifica caminhos, voz, dependências e recursos ingleses sem os descarregar.
- [ ] Implementar preparação online explícita: descarregar `Systran/faster-whisper-small`, configuração/pesos do `hexgrad/Kokoro-82M` e `voices/af_heart.pt`; instalar/preparar o recurso spaCy inglês exigido por Misaki e materializar também o fallback linguístico escolhido. Registar revisões dos modelos, caminhos absolutos e recursos em `models.json`. Executar uma síntese fixa na preparação para revelar dependências tardias; não usar gravações pessoais para esse passo.
- [ ] Implementar loaders: `WhisperModel(local_path, device="cpu", compute_type="int8", local_files_only=True)` e transcrição `language="en", task="transcribe", vad_filter=True`, preservando texto/segmentos. Kokoro recebe caminhos locais de configuração/pesos e tensor de voz local, modelo CPU e pipeline inglês; verificar recursos de Misaki antes de construir o pipeline, pois a biblioteca pode tentar obtê-los automaticamente. Concatenar todos os segmentos de áudio gerados, sem truncar a pergunta.
- [ ] Acrescentar testes que impedem ligações de rede e subprocessos de download durante `health` e uso normal. Registar a marca pytest `models` em `pyproject.toml`; `test_offline_models.py` usa-a e só executa com recursos preparados: gerar a pergunta, ler WAV 24 kHz e transcrevê-lo com Whisper. Falta de modelos deve ser um skip explícito apenas neste teste opcional, nunca sucesso falso da validação real.
- [ ] Repetir testes unitários e `.\.venv\Scripts\python.exe -m pip check`. Esperado: saída 0; falhas de modelos identificam o recurso em falta sem contaminar stdout.
- [ ] Commit: `feat: add offline whisper and kokoro audio worker`.

### Tarefa 5: Supervisão e recuperação do auxiliar

**Ficheiros:** criar `src-tauri/src/worker/{mod.rs,protocol.rs}`, `src-tauri/tests/worker.rs` e `src-tauri/tests/fixtures/worker_stub.py`.

**Interfaces:** consome protocolo v1 e `AppError`; produz `WorkerClient`, `WorkerPort`, `WorkerConfig`, `WorkerRequest`, `WorkerMessage` e `WorkerEvent` definidos na secção 3. `WorkerConfig` obtém o executável/caminhos do ficheiro de configuração, validando existência e caminhos absolutos.

- [ ] Escrever `worker_crash_releases_job`, `timeout_rejects_old_generation`, `stderr_does_not_block`, `non_json_stdout_is_protocol_error` e `handshake_does_not_load_models`. Asserções essenciais:

```rust
assert_eq!(timeout.code, "WORKER_TIMEOUT");
assert!(restarted_generation > previous_generation);
assert!(!state.accepts_result(&old_id, previous_generation));
```

- [ ] Executar `cargo test --manifest-path src-tauri/Cargo.toml --test worker`; confirmar falha antes da supervisão.
- [ ] Implementar o processo com `tokio::process`, argumentos separados, stdin/stdout piped e stderr drenado. Usar processo sem janela visível em Windows. O handshake `health` não importa modelos pesados. Aplicar prazos por etapa e uma geração nova após reinício; EOF, JSON inválido e timeout falham o pedido pendente e permitem novo arranque.
- [ ] No timeout, terminar o processo, aguardar a saída e invalidar quaisquer resultados/ficheiros provisórios do pedido. Um novo pedido nunca reutiliza o identificador de um pedido terminado. No encerramento, enviar `shutdown`, esperar até 3 s e terminar forçadamente se necessário.
- [ ] Repetir testes com o Python 3.11 configurado e o stub de teste. Esperado: saída 0, sem processo filho pendurado e sem depender de modelos ou microfone.
- [ ] Commit: `feat: supervise local audio worker with recoverable jobs`.

### Tarefa 6: Coordenar o fluxo e expor comandos Tauri

**Ficheiros:** criar `src-tauri/src/{service.rs,commands.rs}` e `src-tauri/tests/service.rs`; integrar em `lib.rs` e `capabilities/default.json`; implementar `src/lib/tauri.ts`.

**Interfaces:** consome `AttemptStore`, `AudioPort`, `WorkerPort` e todos os tipos da secção 3; produz `AppService`, os comandos públicos e `practice://snapshot`. `PracticeClient` é o único acesso Tauri usado pela interface.

- [ ] Escrever `can_record_with_worker_missing`, `failed_stop_keeps_previous_attempt`, `retry_transcription_reuses_audio`, `late_result_does_not_change_active_review`, `save_publication_can_retry`, `retranscribe_preserves_existing_review` e `close_finalizes_capture`. Asserções essenciais:

```rust
assert_eq!(snapshot.attempt.unwrap().id, previous_id);
assert_eq!(retry.input_path, original_normalized_path);
assert_eq!(after_late_result.attempt.unwrap().review.unwrap().text, "my words");
assert_eq!(pending_jobs, 0);
```

- [ ] Executar `cargo test --manifest-path src-tauri/Cargo.toml --test service`; confirmar falha antes da coordenação.
- [ ] Implementar start/stop: terminar playback, reservar pasta/ID e iniciar captura. No stop, finalizar, publicar e só depois ativar a nova resposta. Se o transcritor estiver pronto, normalizar e iniciar automaticamente a transcrição; caso contrário, conservar áudio com ação de transcrever. `retry_save_recording` repete publicação de captura já finalizada, sem a gravar outra vez.
- [ ] Implementar voz da pergunta e cache SHA-256 de texto+voz+velocidade+revisão do modelo; `play_audio(question)` usa cache ou inicia síntese, validando o WAV antes de reproduzir. `play_audio(response)` resolve apenas o original da resposta ativa; o frontend nunca fornece caminhos arbitrários.
- [ ] Aplicar resultados só ao job/geração de origem. Guardar `Transcript` antes de publicá-lo; criar uma revisão inicial apenas se não existir. Preservar versões anteriores em nova transcrição. Um resultado sem fala guarda estado explícito, permite ouvir/gravar novamente e não cria qualquer nota.
- [ ] Implementar restauro, refresh, save de revisão e encerramento normal, com eventos de captura interrompida/playback concluído. O snapshot tem `revision` crescente; eventos/comandos fora de ordem não substituem um estado mais recente. Não ativar automaticamente um pedido após timeout.
- [ ] Repetir todos os testes Rust e verificar `npm.cmd run typecheck`. Esperado: saída 0. Abrir a app e confirmar que o microfone/ficheiros funcionam mesmo se Python não estiver configurado.
- [ ] Commit: `feat: connect local audio workflow to tauri commands`.

### Tarefa 7: Interface de prática e proteção das revisões

**Ficheiros:** criar `src/features/practice/{Practice.tsx,usePractice.ts,practice.css,Practice.test.tsx}`, `src/test/setup.ts`; ligar `App.tsx` e o adaptador Tauri.

**Interfaces:** consome `PracticeClient` e `AppSnapshot`; `Practice({client}: {client: PracticeClient})` permite testar com adaptador controlado. `usePractice(client)` mantém snapshot, texto de revisão e estado de escrita local; não controla o estado persistente da captura nem dos jobs.

- [ ] Escrever testes Testing Library: `manual_start_stop`, `shows_loading_without_freezing_editor`, `raw_transcript_is_readonly`, `save_failure_keeps_dirty_text`, `new_recording_with_dirty_review`, `close_with_dirty_review` e `older_snapshot_is_ignored`. Fixar os comportamentos observáveis:

```typescript
expect(screen.getByRole('button', {name: 'Terminar gravação'})).toBeEnabled();
expect(screen.getByLabelText('Transcrição automática')).toHaveAttribute('readonly');
expect(screen.getByLabelText('Revisão da transcrição')).toHaveValue('my edited words');
expect(client.startRecording).not.toHaveBeenCalled(); // enquanto a decisão sobre a revisão está pendente
```

- [ ] Executar `npm.cmd test -- src/features/practice/Practice.test.tsx`; confirmar falhas de comportamento antes de implementar a experiência.
- [ ] Implementar pergunta e ações “Ouvir pergunta”, “Iniciar gravação”, “Terminar gravação”, “Ouvir resposta”, “Parar áudio”, “Transcrever resposta”, “Guardar revisão” e “Reiniciar áudio local”, apresentadas conforme o estado. Mostrar duração de gravação, disponibilidade por componente, carregamento/processamento e erros com recuperação. O texto automático permanece apenas de leitura.
- [ ] Implementar guardar/descartar/cancelar ao iniciar outra gravação com edição por guardar. Só iniciar a captura depois de resolver a decisão. Não limpar `dirty` quando `saveReview` falhar. Eventos de transcrição nunca substituem uma edição local existente.
- [ ] Intercetar o fecho normal com `onCloseRequested`, impedir fecho imediato e resolver revisões por guardar. Após escolha válida, chamar `prepareClose`; fechar efetivamente só quando terminar com sucesso. Se o utilizador cancelar, manter a janela e os dados. Se houver erro de escrita, manter a revisão editável e explicar o erro.
- [ ] Mostrar revisão em falta/por guardar/guardada, sem chamar “confirmar” à gravação do texto nesta fase. Garantir navegação por teclado, rótulos dos controlos e estado legível além da cor; não introduzir pontuações, chat fictício ou feedback de IA.
- [ ] Repetir `npm.cmd test`, `npm.cmd run typecheck` e `npm.cmd run build`. Esperado: saída 0 e testes do fluxo sem chamadas reais ao microfone/modelos.
- [ ] Commit: `feat: add local voice practice and transcript review interface`.

### Tarefa 8: Validação real e instruções reproduzíveis

**Ficheiros:** criar `docs/phase-1/{setup.md,validation.md}`, `scripts/verify.ps1`; atualizar `README.md`, `AGENTS.md` e o estado da fase apenas com evidência. Ajustar `config/audio.example.json` e locks se a validação exigir compatibilidade diferente.

**Interfaces:** consome a aplicação integrada e o CLI `python -m audio_worker.prepare --models-dir <caminho absoluto>`. Produz instruções verificadas, medições e a matriz CA-01 a CA-15; não produz funcionalidades da fase 2.

- [ ] Implementar `scripts/verify.ps1` executando, com controlo de exit codes e interrupção na primeira falha: typecheck, testes React, build Vite, cargo fmt --check, cargo clippy --all-targets -- -D warnings, testes Rust, pytest sem a marca `models` e pip check. As mensagens de sucesso dependem de todos os comandos terem terminado com saída 0.
- [ ] Documentar preparação: `.venv` Python 3.11, dependências bloqueadas, recursos ingleses necessários, preparação explícita dos modelos, configuração pessoal fora do Git e execução `npm.cmd run tauri -- dev`. Usar caminhos com espaços no exemplo e não exigir execução a partir de um diretório implícito para resolver modelos.
- [ ] Executar `.\scripts\verify.ps1`. Esperado: todos os checks com saída 0. Executar separadamente `.\.venv\Scripts\python.exe -m pytest audio-worker/tests/test_offline_models.py -m models -q` com modelos preparados; um skip aqui não satisfaz o percurso real.
- [ ] Abrir a aplicação e validar com o utilizador pelo menos três gravações: curta, com termos técnicos e problemática. Testar ouvir Kokoro real, guardar/recuperar revisão, repetir transcrição sem nova captura, remover/restaurar o dispositivo e parar/reiniciar o auxiliar. Captura pessoal acontece por ação manual do utilizador, não por um teste automático.
- [ ] Repetir o percurso sem rede, depois da preparação, e com um recurso de voz em falta. Esperado: funcionamento offline quando preparado, erro explícito quando incompleto, sem tentativa de descarregar durante a prática. Verificar que iniciar outra gravação ou fechar não perde uma revisão.
- [ ] Registar em `validation.md` hardware, versões, IDs/revisões dos modelos, duração do áudio, carregamento inicial, síntese e transcrição a frio e em repetição. Não incluir áudios pessoais no Git. Marcar cada CA como verificado, falhado ou pendente, com evidência; a aceitabilidade da espera depende do utilizador.
- [ ] Opcionalmente verificar `npm.cmd run tauri -- build --no-bundle` para antecipar problemas de compilação release; não apresentar esse binário como instalador autónomo com Python incluído. O pacote final é da fase 5.
- [ ] Atualizar a documentação de estado apenas se todos os critérios estiverem verificados, ou registar os bloqueios concretos. Commit: `docs: record phase one setup and local audio validation`.

## 5. Cobertura da spec

| Critérios | Tarefas e prova principal |
| --- | --- |
| CA-01 | 1 e 7: janela e pergunta exata; execução real em 8. |
| CA-02 | 4 e 6: Kokoro real/cache; reprodução validada em 8. |
| CA-03, CA-04 | 1, 3, 6 e 7: exclusão de operações, captura manual e controlos. |
| CA-05 | 2, 3 e 6: ficheiro finalizado, publicação e playback independentes da transcrição. |
| CA-06 | 4 e 8: Whisper CPU/int8 em inglês com gravações reais. |
| CA-07, CA-08 | 2, 6 e 7: originais, versões, revisão e repetição sem sobrescrita. |
| CA-09 | 2 e 6: apontador atómico, restauro e nova captura com publicação segura. |
| CA-10 | 2, 3, 4 e 6: ficheiro/dispositivo/modelo em falta, áudio inválido e no_speech. |
| CA-11 | 5 e 6: morte, timeout, geração e rejeição de resultados atrasados. |
| CA-12 | 5, 6 e 7: trabalho fora da thread da interface e estados de progresso. |
| CA-13 | 4 e 8: modelos locais completos e validação sem rede. |
| CA-14, CA-15 | 8: documentação, medições, três gravações e validação pelo utilizador. |

## 6. Revisão do plano e execução

- [x] Ler a spec e mapear os 15 critérios para tarefas.
- [x] Definir ficheiros, contratos partilhados, versões de referência e limites por operação.
- [x] Associar os cinco riscos de revisão a testes das tarefas responsáveis.
- [x] Distinguir testes com adaptadores da integração com hardware/modelos reais.
- [x] Manter SQLite, API, entrevista completa e instalador final fora desta fase.
- [x] Fazer a revisão do plano diretamente, sem delegar uma segunda revisão.

Recomendação para a execução futura: **execução direta na mesma sessão**, com `executing-plans`, por as tarefas dependerem dos mesmos contratos e ser importante controlar custos. Não iniciar a implementação como consequência de criar este documento; o utilizador pediu o plano. Não iniciar automaticamente as fases seguintes quando este plano terminar.

## 7. Referências técnicas verificadas

- [Tauri: pré-requisitos Windows](https://v2.tauri.app/start/prerequisites/), [comandos Rust](https://v2.tauri.app/develop/calling-rust/) e [testes de APIs](https://v2.tauri.app/develop/tests/mocking/).
- [CPAL 0.18.2](https://docs.rs/cpal/0.18.2/cpal/), [Rodio 0.23.0](https://docs.rs/rodio/0.23.0/rodio/), [Hound 3.5.1](https://docs.rs/hound/3.5.1/hound/), [Rubato 5.0.1](https://docs.rs/rubato/5.0.1/rubato/) e [atomicwrites 0.4.4](https://docs.rs/atomicwrites/0.4.4/atomicwrites/).
- [faster-whisper: requisitos e utilização](https://github.com/SYSTRAN/faster-whisper).
- [Kokoro 0.9.4: requisitos Python](https://pypi.org/project/kokoro/0.9.4/), [modelo/voz](https://huggingface.co/hexgrad/Kokoro-82M) e [construtor com ficheiros locais](https://github.com/hexgrad/kokoro/blob/main/kokoro/model.py).
- [Misaki: preparação dos recursos ingleses](https://github.com/hexgrad/misaki/blob/main/misaki/en.py).
- Versões consultadas nos registos oficiais [npm](https://registry.npmjs.org/), [crates.io](https://crates.io/), [PyPI](https://pypi.org/) e no [canal Rust estável](https://static.rust-lang.org/dist/channel-rust-stable.toml).

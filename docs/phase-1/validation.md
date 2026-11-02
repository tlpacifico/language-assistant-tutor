# Validação da fase 1 — matriz CA-01 a CA-15

Data: 2026-11-02 · Ramo: `feature/phase-1-local-audio` · Ambiente descrito em
[setup.md](setup.md).

Este documento distingue **o que foi verificado** (com o comando e o resultado)
do que **só o utilizador pode validar** (microfone real, audição da voz, três
gravações e aceitabilidade da espera). A fase 1 **não está concluída**: os
critérios CA-02, CA-03, CA-05, CA-06, CA-10 e CA-14 dependem dessa validação
manual e ficam marcados como pendentes.

## 1. Ambiente e modelos usados

| Elemento | Valor |
| --- | --- |
| Sistema | Windows 11 Enterprise `10.0.26300`, build 26300 |
| CPU / RAM | Intel Core i7-12700H (14 núcleos / 20 threads), 34 010 849 280 bytes |
| GPU | não usada; motores forçados a CPU (`device="cpu"`, `compute_type="int8"`) |
| Rust / Node / Python | `1.99.1` / `24.11.1` (npm `11.6.2`) / `3.11.9` |
| PyTorch | `2.15.0+cpu` |
| faster-whisper | `1.2.1`, modelo `small` multilingue, revisão `536b0662742c02347bc0e980a01041f333bce120` |
| Kokoro / Misaki | `0.9.4` / `0.9.4`, revisão Kokoro-82M `f3ff3571791e39611d31c381e3a41a3af07b4987`, voz `af_heart`, `lang_code="a"`, velocidade `1.0` |
| spaCy | `3.8.18`, recurso `en_core_web_sm 3.8.0` |
| transformers | `5.22.1` (risco registado na tarefa 4: **os pesos do Kokoro carregaram e sintetizaram sem erro**) |
| Diretório dos modelos | `C:\Users\MarloGamer\AppData\Local\language-assistant-tutor\Modelos de Voz` (fora do repositório) |
| Manifesto | `...\Modelos de Voz\models.json`, `preparedAt` `2026-11-02T01:16:48Z` |

Preparação real (`python -m audio_worker.prepare --models-dir ...`): **34,3 s**
na primeira execução (descarga + spaCy + síntese de verificação) e **10,2 s** na
repetição, com 25 ficheiros e **814 144 526 bytes (776,4 MiB)** —
`faster-whisper-small\model.bin` 461,1 MiB e `kokoro\kokoro-v1_0.pth` 312,1 MiB.
A síntese de verificação produziu 3 900 ms de áudio a 24 kHz. Nenhuma gravação
pessoal foi criada ou guardada.

## 2. Verificação automática executada

| Comando | Resultado |
| --- | --- |
| `.\scripts\verify.ps1` | 8/8 checks com saída 0 (typecheck, 10 testes Vitest, build Vite, `cargo fmt --check`, `clippy -D warnings`, 63 testes Rust, 36 testes pytest sem `models`, `pip check`) |
| `.\.venv\Scripts\python.exe -m pytest audio-worker/tests/test_offline_models.py -m models -q` | `2 passed` em 12,18 s (síntese da pergunta + transcrição; silêncio → `no_speech`); **não foi skip** |
| `.\.venv\Scripts\python.exe -m audio_worker.prepare --models-dir ...` | saída 0; manifesto e síntese de verificação |
| `npm.cmd run tauri -- build --no-bundle` | saída 0 em 29,2 s; binário `src-tauri\target\release\language-assistant-tutor.exe` |
| Arranque do binário release | janela «Tutor de entrevistas» criada, processo a responder, auxiliar iniciado com a `.venv` e o `models.json` com espaços; fecho normal terminou o auxiliar |
| Teste offline com sockets bloqueados (`health` + síntese + transcrição) | saída 0: `health` `ready/ready`, síntese e transcrição corretas, sem qualquer ligação de rede |
| Manifesto com voz em falta | `health` → síntese `missing`; `synthesize` → erro `MODEL_MISSING` com o caminho da voz em falta e instrução de preparação |
| Procura de chamadas a APIs de IA (`opencode`, `openai`, `api.deepseek`, `https://api`) no código Rust/TS/Python | nenhuma ocorrência |

### Tempos medidos (automação, CPU, áudio sintetizado)

Medições feitas com `EngineRegistry` sobre os pesos preparados. São um indício
de desempenho, **não** substituem a validação do utilizador nem uma gravação
humana.

| Operação | Carregamento (`loadMs`) | Processamento (`processingMs`) |
| --- | --- | --- |
| `health` (sem carregar modelos) | — | 4 ms (parede) |
| Síntese da pergunta — 1.ª chamada (fria) | 7 731 ms | 1 150 ms |
| Síntese da pergunta — repetição | 0 ms (reutilizado) | 1 169 ms |
| Síntese de texto longo (26,1 s de áudio) | 0 ms | 6 318 ms |
| Transcrição da pergunta — 1.ª chamada (fria) | 4 181 ms | 1 838 ms |
| Transcrição da pergunta — repetição | 0 ms | 1 504 ms |
| Transcrição de 26,1 s de áudio em inglês | 0 ms | 3 057 ms |
| Repetição da transcrição do texto longo | 0 ms | 3 082 ms |

Numa execução anterior deste mesmo dia, o carregamento a frio foi 6 989 ms
(Kokoro) e 919 ms (Whisper); a variação é real e deve-se a cache de ficheiros e
antivírus. O limite de 180 s por modelo não foi aproximado.

A transcrição devolveu exatamente
`Tell me about a time you solved a difficult problem at work.` e o texto longo
foi transcrito sem tradução nem reescrita (apenas pontuação/vírgulas do motor).

## 3. Matriz de critérios

Legenda: **verificado** = existe evidência executada e reproduzível;
**pendente** = exige ação do utilizador (hardware, audição ou decisão);
**falhado** = houve evidência contrária.

| CA | Critério | Estado | Evidência / o que falta |
| --- | --- | --- | --- |
| CA-01 | A aplicação abre no Windows e apresenta a pergunta fixa correta. | **verificado** | Binário release arrancado: janela `Tutor de entrevistas`, `Responding=True`, auxiliar iniciado e fecho normal. Teste `shows_the_fixed_question` fixa o `<h1 lang="en">` com `Tell me about a time you solved a difficult problem at work.` Falta apenas confirmação visual humana, que não altera o critério. |
| CA-02 | Kokoro gera voz inglesa para a pergunta e o utilizador consegue ouvi-la novamente. | **pendente** | Geração real verificada (2 testes `models`, medições acima; PCM16 mono 24 kHz). Audição e repetição pela janela dependem do utilizador; a cache `question-cache/<sha256>.wav` está coberta por testes Rust. |
| CA-03 | Início e fim de gravação manuais, com estado e duração visíveis. | **pendente** | `manual_start_stop` (Vitest) e testes de captura Rust passam, mas usam áudio simulado. Falta uma gravação real com microfone. |
| CA-04 | Sem reprodução durante a captura nem operações duplicadas. | **verificado** | `playback_is_blocked_while_recording`, `recording_stops_playback`, testes de ocupação do serviço e botões desativados na interface. Regra aplicada pelo núcleo, não só pela UI. |
| CA-05 | Áudio finalizado guardado e reproduzível mesmo com falha de transcrição. | **pendente** | `failed_stop_keeps_previous_attempt`, `save_publication_can_retry`, escrita/publicação atómicas e reprodução a partir do original testadas; falta ouvir a resposta gravada no hardware real e com o transcritor indisponível. |
| CA-06 | faster-whisper transcreve uma resposta real em inglês, sem tradução nem avaliação. | **pendente** | Transcrição real de inglês sintetizado (pergunta e texto de 26,1 s) verificada, sem tradução/reescrita. Falta uma gravação humana (as três gravações da spec). |
| CA-07 | Texto automático preservado e revisão guardada em separado. | **verificado** | `raw_transcript_is_readonly`, `save_failure_keeps_dirty_text`, testes de armazenamento (`append_does_not_overwrite_raw`, `failed_review_write_preserves_saved_text`) e `retranscribe_preserves_existing_review`. |
| CA-08 | Repetir transcrição falhada não exige nova gravação; resultados posteriores não apagam revisões. | **verificado** | `retry_transcription_reuses_audio`, `late_result_does_not_change_active_review`, `retranscribe_preserves_existing_review` (Rust) e o fluxo de revisão na interface. |
| CA-09 | Nova gravação e reinício não apagam a última resposta nem a revisão guardada. | **verificado** | `failed_publish_preserves_last`, `replaces_existing_json_atomically`, `corrupt_pointer_is_recoverable`, `restore_interrupted_job`, `allocate_never_reuses_a_directory`. Nota: o reinício foi provado com armazenamento controlado, não com uma resposta real gravada. |
| CA-10 | Áudio vazio, ficheiros em falta, ausência de dispositivo e ausência de fala têm comportamento explícito e recuperável. | **pendente** | Verificados: `empty_audio_is_invalid`, `restore_reports_missing_original`, `no_speech` real com silêncio, erro `MODEL_MISSING` com voz em falta. Falta remover/restaurar o dispositivo real. |
| CA-11 | Falha ou bloqueio do auxiliar permite recuperar e repetir, sem aplicar resultados atrasados. | **verificado** | `worker_crash_releases_job`, `timeout_rejects_old_generation`, `shutdown_kills_unresponsive_worker`, `non_json_stdout_is_protocol_error`, `failed_restart_discards_the_pending_health_handle` e `late_result_does_not_change_active_review`. Executado com o stub controlado, não com o auxiliar real bloqueado pela interface. |
| CA-12 | Interface utilizável durante carregamento e processamento. | **verificado** | `shows_loading_without_freezing_editor` mantém o editor editável e os botões incompatíveis desativados; o worker é um processo separado (medições de 7,7 s de carregamento não bloqueiam a janela). Perceção real fica para o utilizador. |
| CA-13 | Com modelos preparados, o percurso funciona sem rede e não chama serviços de IA. | **verificado** | Execução com sockets bloqueados: `health` `ready`, síntese e transcrição corretas; `HF_HUB_OFFLINE=1`/`TRANSFORMERS_OFFLINE=1` aplicados antes dos imports; nenhuma referência a APIs de IA no código. |
| CA-14 | Gravações reais e tempos medidos registados; utilizador considera a espera aceitável. | **pendente** | Tempos acima medidos com áudio sintetizado. Faltam as três gravações reais (curta, com termos técnicos e problemática), a duração de cada uma e a decisão do utilizador sobre a espera. |
| CA-15 | Instruções verificadas para preparar e executar no Windows de desenvolvimento. | **verificado** | [setup.md](setup.md) regista os comandos realmente executados, versões observadas, caminho com espaços, `audio.json` fora do Git e `.\scripts\verify.ps1` (8/8 checks). |

## 4. Validação manual que falta (ação do utilizador)

Com a aplicação aberta por `npm.cmd run tauri -- dev` e o `audio.json` da
secção 4 de [setup.md](setup.md):

1. Ouvir a pergunta e confirmar que a voz inglesa é inteligível; ouvi-la outra
   vez sem nova síntese.
2. Gravar três respostas — curta, com termos técnicos e uma problemática
   (silêncio prolongado ou dispositivo removido a meio). Registar duração e o
   resultado mostrado (transcrição, `no_speech`, áudio incompleto recuperado).
3. Guardar uma revisão, fechar a aplicação, reabrir e confirmar que a resposta e
   a revisão continuam lá. Repetir a transcrição de uma resposta já transcrita e
   confirmar que a revisão guardada não é substituída.
4. Remover e restaurar o microfone; parar/reiniciar o auxiliar; confirmar que a
   interface recupera e permite repetir sem gravar de novo.
5. Repetir o percurso com a rede desligada e confirmar que nada tenta
   descarregar. Repetir com a voz em falta (renomear temporariamente
   `voices\af_heart.pt`) e confirmar o erro explícito.
6. Com uma revisão por guardar, iniciar outra gravação e fechar a janela;
   confirmar as opções guardar/descartar/cancelar e que uma falha de escrita
   mantém o texto.
7. Decidir se a espera medida permite praticar confortavelmente (CA-14). Sem
   essa decisão, a fase não é marcada como concluída.

As gravações reais ficam na pasta de dados da aplicação
(`%APPDATA%\dev.thacio.languageassistanttutor\phase1`), nunca no repositório.

## 5. Limites e riscos registados

- `transformers 5.22.1` com Kokoro 0.9.4: os pesos carregaram e sintetizaram com
  sucesso nesta máquina, incluindo smoke test e duas execuções de teste. O risco
  da tarefa 4 não se materializou, mas continua a ser uma combinação fora da
  base original do Kokoro.
- Avisos de terceiros durante o carregamento (`UserWarning` do LSTM do PyTorch,
  `FutureWarning` de `weight_norm`/`torch.jit.script`, `DeprecationWarning` do
  Misaki) não impedem o funcionamento e não são tratados nesta fase.
- O carregamento inicial a frio do Kokoro (~7 s) é a espera mais visível antes
  da primeira reprodução da pergunta; o Whisper variou entre ~0,9 s e ~4,2 s
  entre execuções.
- Testes Rust que usam o stub deixaram processos `python -m audio_worker`
  órfãos de execuções anteriores deste dia; foram terminados manualmente. É
  higiene de testes a rever, não um comportamento da aplicação.
- Nada nesta validação prova qualidade de pronúncia, fluência ou confiança —
  fora do âmbito da fase, como a spec exige.

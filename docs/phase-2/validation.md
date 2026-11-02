# Validação da fase 2 — matriz F2-01 a F2-16

Data: 2026-11-02 · Ramo: `feature/phase-2-guided-interview` · HEAD no início
desta verificação: `f2eeb20` · Ambiente descrito em [setup.md](setup.md).

Este documento distingue **o que foi verificado por automação** (comando,
teste e resultado reais) do que **só o utilizador pode validar** (sessões com
voz real, audição, espera, percurso offline pela janela e reinício real). A
fase 2 **não está concluída nem aceite**: a implementação e a verificação
automática estão feitas, mas a validação manual da secção 4 continua pendente.
A existência desta matriz não substitui essa validação.

Legenda dos estados:

| Estado | Significado |
| --- | --- |
| **verificado** | Existe evidência executada e reproduzível que cobre o critério por inteiro. |
| **parcial** | Uma parte do critério tem evidência executada; a restante exige ação do utilizador. |
| **pendente** | Só pode ser demonstrado pelo utilizador (voz real, audição, espera ou decisão). |
| **falhado** | Houve evidência contrária. Nenhum critério ficou neste estado. |

## 1. Ambiente e âmbito

O ambiente é o da fase 1, na mesma máquina (ver
[`docs/phase-1/setup.md`](../phase-1/setup.md)), com uma única dependência nova:

| Elemento | Valor observado |
| --- | --- |
| Windows / Rust / Node / Python | Windows 11 Enterprise `10.0.26300`; `rustc 1.99.1`; Node `24.11.1` (npm `11.6.2`); Python `3.11.9` na `.venv` |
| SQLite em Rust | `rusqlite 0.41.1` com `bundled`, `libsqlite3-sys 0.39.1` compilado no build release |
| Modelos | Whisper `small` e Kokoro-82M preparados fora do repositório (fase 1) |
| Configuração pessoal | `audio.json` em `%APPDATA%\dev.thacio.languageassistanttutor\` (não versionado) |
| Base de dados | `%APPDATA%\dev.thacio.languageassistanttutor\tutor.sqlite3` |

Nenhuma gravação pessoal foi criada nesta verificação. A base e os diretórios
foram criados pelo primeiro arranque da versão da fase 2 e inspecionados em
modo de leitura.

## 2. Verificação automática executada

| Comando | Saída | Resultado |
| --- | --- | --- |
| `.\scripts\verify.ps1` | 0 | 8/8 checks em 18,5 s (147 testes Rust, 43 Vitest, 36 pytest sem `models`, `pip check` limpo) |
| `.\.venv\Scripts\python.exe -m pytest audio-worker/tests/test_offline_models.py -m models -q` | 0 | `2 passed` em 48,14 s — **não foi skip** (com `LANGUAGE_TUTOR_MODELS_MANIFEST` para o manifesto preparado) |
| `npm.cmd run tauri -- build --no-bundle` | 0 | Build release em 1m 17s; binário `src-tauri\target\release\language-assistant-tutor.exe` |
| Arranque e fecho do binário release | 0 | Janela «Tutor de entrevistas», processo a responder, auxiliar iniciado e terminado, código de saída do processo 0 |
| Inspeção de `tutor.sqlite3` após os arranques | 0 | `user_version = 1`; 20 perguntas (4/3/3/4/3/3); uma marca `questions-v1`; 0 sessões |
| Procura de `opencode`, `openai`, `api.deepseek`, `https://api` em Rust/TS/Python | 0 ocorrências | Nenhum cliente ou chamada de IA no código desta fase |

### 2.1 `.\scripts\verify.ps1`

Saída real (excertos; a saída completa está no relatório da tarefa em
`.superpowers/sdd/2026-11-02-phase-2-guided-interview-implementation/task-10-report.md`):

```text
[1/8] TypeScript (tsc --noEmit)                OK (0,6 s)
[2/8] Testes React (Vitest)                     OK (8,6 s)
      Test Files  6 passed (6)
           Tests  43 passed (43)
[3/8] Build do frontend (typecheck + Vite)      OK (1,2 s)
[4/8] Formatação Rust (cargo fmt --check)       OK (0,4 s)
[5/8] Lint Rust (cargo clippy -D warnings)      OK (0,7 s)
[6/8] Testes Rust (cargo test)                  OK (5,0 s)
[7/8] Testes Python (pytest sem a marca models) OK (1,3 s)
      36 passed, 2 deselected in 0.67s
[8/8] Dependências Python (pip check)           OK (0,8 s)
      No broken requirements found.

Verificação concluída: 8/8 checks com saída 0 em 18,5 s.
```

Suítes Rust desta execução (147 testes, 0 falhas): `lib` 20, `answers` 16,
`artifacts` 10, `audio` 11, `database` 7, `domain` 8, `interview_contracts` 4,
`legacy` 4, `questions` 6, `service` 34, `sessions` 8, `storage` 12, `worker` 7.
Os dois testes Python deselecionados são os da marca `models`, executados à
parte na secção 2.2.

### 2.2 Teste real dos motores (`-m models`)

Saída real:

```text
..                                                                       [100%]
2 passed, 5 warnings in 48.14s
```

Os avisos são de terceiros e já conhecidos (`UserWarning` do LSTM do PyTorch,
`FutureWarning` de `weight_norm`/`torch.jit.script`, `DeprecationWarning` do
Misaki). Sem o manifesto preparado o mesmo comando faz skip explícito; este
resultado é `passed`, não `skipped`.

### 2.3 Build release sem instalador

Saída real (excertos):

```text
   Compiling libsqlite3-sys v0.39.1
   Compiling rusqlite v0.41.1
    Finished `release` profile [optimized] target(s) in 1m 17s
       Built application at: C:\Repos\Thacio\language-assistant-tutor\src-tauri\target\release\language-assistant-tutor.exe
EXIT=0
```

Este build **não** é um instalador autónomo e não valida a distribuição da fase
5: o binário continua a depender da `.venv`, do `audio.json` e dos modelos
preparados nesta máquina.

### 2.4 Arranque e fecho do binário

Saída real do segundo arranque (o primeiro criou a base):

```text
auxiliares antes: 0
base antes: True
pid: 43048
janela: [Tutor de entrevistas]
a responder: True
auxiliares durante a execucao: 2
fecho pedido: True
processo terminou: True
auxiliares depois do fecho: 0
codigo de saida do processo: 0
```

O fecho normal finaliza a captura pendente, encerra o auxiliar e termina com
código 0; nenhum processo `python -m audio_worker` ficou órfão. Os dois
processos durante a execução são o lançador da `.venv` e o Python que executa o
módulo.

Inspeção da base depois do primeiro arranque e de novo depois do segundo
(leitura apenas):

```text
user_version: 1
tabelas: command_receipts, confirmations, content_seeds, operations, questions,
         recordings, reviews, session_questions, sessions, transcripts
perguntas: 20
categorias: [('change_adaptation', 3), ('conflict_collaboration', 4),
             ('failure_learning', 3), ('leadership_initiative', 3),
             ('priorities_deadlines', 3), ('problem_solving', 4)]
sessoes: 0
seed: [('questions-v1',)]
```

Repetir o arranque manteve 20 perguntas e uma única marca de seed — o banco
inicial não foi duplicado nem reposto por cima de alterações.

## 3. Matriz de critérios

Ligação às tarefas do [plano técnico](../superpowers/plans/2026-11-02-phase-2-guided-interview-implementation.md)
e aos testes realmente executados. Os nomes de teste são os das suítes Rust
(`src-tauri/tests/*.rs`), Vitest (`*.test.tsx`) ou unitários internos.

| F2 | Critério | Tarefas | Estado | Evidência / o que falta |
| --- | --- | --- | --- | --- |
| F2-01 | Banco inicial contém 20 perguntas nas seis categorias; repetir o arranque não duplica nem repõe edições. | 1, 2 | **verificado** (automação) | `questions::seed_is_once_and_preserves_edits` (20 perguntas, IDs estáveis, reabrir não duplica, edição preservada, marca única), `questions::bank_covers_six_categories` (4/3/3/4/3/3), `questions::incomplete_seed_rolls_back_without_marker`, unitário `db::questions::tests::embedded_seed_is_valid_and_unpadded`. Arranque real duas vezes: 20 perguntas, uma marca `questions-v1`. |
| F2-02 | Adicionar e editar texto, categoria, competências e critérios persiste após reinício, com validação dos campos obrigatórios. | 2, 7 | **verificado** (automação) | `questions::blank_fields_are_rejected`, `question_trims_only_outer_whitespace` (persistência depois de reabrir), `stale_edit_does_not_overwrite`; `interview_contracts::question_validation_trims_outer_spaces_and_requires_content`; Vitest `QuestionBank.test.tsx`: `bank_edit_survives_reload`, `bank_rejects_empty_fields`, `creates_a_question_with_competencies_and_criteria`, `keeps_the_editor_open_when_the_save_fails`. |
| F2-03 | Quantidade inicial três; sessões com uma, três e outra quantidade elegível; insuficiência explícita, sem redução ou repetição silenciosa. | 3, 7, 10 | **parcial** | Regra e interface cobertas: `sessions::starts_one_three_and_available_count`, `insufficient_category_creates_nothing`, `interview_contracts::count_validation_rejects_zero_and_more_than_eligible` (`count == 0` devolve `INSUFFICIENT_QUESTIONS`), Vitest `defaults_to_three_and_all_categories`, `insufficient_questions_require_explicit_adjustment`. Falta completar com o utilizador uma sessão de uma pergunta e outra quantidade elegível (secção 4, itens 1 e 6). |
| F2-04 | Seleção e ordem sem repetição persistem; editar o banco não altera nenhuma cópia de sessão existente. | 3 | **verificado** (automação) | `sessions::starts_one_three_and_available_count` (posições 1..N, sem repetição, ordem guardada), `snapshots_survive_bank_edits` (texto, categoria, competências e critérios da sessão não mudam após editar o banco), `repeated_start_returns_original_session`, `failed_second_question_rolls_back_session`. |
| F2-05 | O utilizador completa três perguntas com voz, captura manual e progresso correto; falha de síntese permite responder pelo texto. | 6, 8, 10 | **parcial** | Automação: `service::question_audio_is_synthesized_and_validated`, `question_cache_distinguishes_session_texts`, `synthesis_failure_still_allows_recording`, `start_confirm_and_advance_persist_the_session`, `finishing_the_last_question_completes_the_session`; captura manual já com evidência da fase 1 (CA-03 pendente no hardware). Falta a sessão real de três perguntas com voz, incluindo uma com termos técnicos (secção 4, item 1). |
| F2-06 | Áudio original, derivado, resultados automáticos, revisão e confirmação permanecem distintos e rastreáveis. | 1, 4, 5, 9 | **verificado** (automação) | Esquema com tabelas e FKs separadas e triggers de imutabilidade: `database::triggers_protect_confirmed_text_and_transcripts`, `foreign_keys_reject_cross_recording_sources`; `answers::normalized_audio_is_linked_to_a_ready_recording`, `transcript_append_creates_and_closes_its_operation`, `transcript_append_does_not_replace_review`, `confirmation_freezes_exact_seen_text`; Vitest `history_separates_raw_review_and_confirmed`. |
| F2-07 | Guardar não confirma; confirmar fixa exatamente o texto visto; falha, versão antiga ou pedido repetido não cria confirmação indevida. | 5, 8 | **verificado** (automação) | `answers::save_does_not_confirm`, `confirmation_freezes_exact_seen_text`, `stale_review_cannot_confirm`, `same_request_with_other_text_is_rejected`, `confirmation_failure_rolls_back_review_and_receipt`; Vitest `confirmation_locks_exact_editor_text`, `confirm_failure_preserves_text`, `save_does_not_enable_next_until_confirmation`. |
| F2-08 | Não é possível avançar sem confirmar, saltar por duplo clique ou terminar antes de todas as confirmações; repetir avanço é seguro. | 5, 8 | **verificado** (automação) | `answers::advance_requires_confirmation`, `replayed_advance_does_not_skip`, `last_advance_completes_only_full_session`; `service::confirmation_and_advance_are_refused_during_capture`, `finishing_the_last_question_completes_the_session`; Vitest `double_next_click_sends_one_request`, `last_question_uses_finish_label`. |
| F2-09 | Nova captura antes da confirmação preserva anteriores; falha da nova captura não perde a seleção válida. Resposta confirmada não é reaberta. | 4, 6, 8 | **verificado** (automação) | `artifacts::new_capture_failure_keeps_previous_selection`, `older_publication_cannot_replace_newer_selection`; `answers::confirmed_question_keeps_selection_and_refuses_new_captures`; `service::failed_stop_keeps_previous_recording`, `confirmed_question_rejects_capture_and_transcription`; Vitest `prior_recordings_remain_readable`. |
| F2-10 | Fecho e navegação com alterações resolvem guardar/descartar/cancelar e não perdem edição feita durante uma escrita. | 8 | **verificado** (automação) | `NavigationGuard.test.tsx`: `save_and_leave_keeps_newer_edit`, `failed_save_does_not_destroy_window`, `cancel_keeps_editor`, `discard_requires_explicit_choice`, `capture_blocks_navigation`; `App.test.tsx`: resolução da revisão antes de fechar, finalização da captura antes de destruir, janela mantida quando o fecho falha, texto escrito durante a preparação do fecho, bloqueio de mudança de página durante captura/finalização. A experiência real de fecho com uma revisão por guardar continua na secção 4, item 10. |
| F2-11 | Reiniciar durante transcrição, após guardar revisão, após confirmar e após avançar recupera a etapa persistida, sem duplicações. | 4, 5, 6, 9 | **parcial** | Reabertura real da base em testes: `answers::advance_after_reopen_keeps_the_confirmation_pending_to_advance`, `sessions::old_start_replay_after_completion_does_not_create_session`, `artifacts::reopen_reconciles_once`, `finalized_audio_survives_database_failure`, `storage::restore_interrupted_job`; Vitest `resume_keeps_confirmed_question_awaiting_next` e o percurso integrado `three_question_session_can_resume_and_finish`. Falta reiniciar a aplicação real a meio de uma sessão (secção 4, item 4). |
| F2-12 | Falhas de escrita/publicação permitem recuperar sem nova gravação; referências em falta não apagam textos nem bloqueiam outros itens. | 4 | **verificado** (automação) | `artifacts::finalized_audio_survives_database_failure`, `part_file_is_not_a_saved_answer`, `missing_audio_keeps_text`, `locked_original_stays_publishable_for_the_next_start`, `corrupt_original_is_interrupted_not_retried`, `paths_cannot_escape_artifact_root`; `service::save_publication_can_retry`; `storage::failed_publish_preserves_last`; Vitest `missing_audio_keeps_confirmed_text`. |
| F2-13 | Pedidos repetidos não bloqueiam o serviço; eventos e resultados antigos não afetam gravações novas ou confirmações. | 5, 6 | **verificado** (automação) | `service::repeated_start_returns_busy_instead_of_blocking`, `late_interruption_from_an_older_capture_is_ignored`, `timed_out_stop_recovers_capture`, `late_worker_result_cannot_target_next_question`, `history_read_does_not_retarget_model_job`, `playback_is_allowed_while_transcribing`, `close_finalizes_capture_and_ignores_late_results`; `answers::late_transcript_cannot_change_confirmation`; Vitest `old_snapshot_does_not_erase_draft`. |
| F2-14 | Histórico permite consultar sessões e ouvir originais; treinos da fase 1 continuam consultáveis sem alteração dos seus ficheiros. | 9, 10 | **parcial** | Leitura e preservação: `legacy::legacy_list_ignores_last_pointer`, `corrupt_legacy_item_does_not_hide_valid_items`, `legacy_read_changes_no_bytes`, `legacy_rejects_path_escape`; `service::legacy_attempt_audio_is_read_only`; `sessions::list_sessions_reports_completed_sessions_and_order`; Vitest `History.test.tsx` (4 testes) e `history_separates_raw_review_and_confirmed`. Falta ouvir os originais pela janela com hardware real (secção 4, itens 2 e 5); a reprodução de áudio real continua também pendente na fase 1 (CA-02/CA-05). |
| F2-15 | Migração falhada preserva a base; versão futura não é escrita; apenas uma sessão permanece em curso. | 1, 3 | **verificado** (automação) | `database::failed_migration_rolls_back`, `future_version_is_not_modified`, `fresh_database_has_version_one`, `only_one_session_can_be_in_progress`; `sessions::only_one_session_can_be_active`, `insufficient_category_creates_nothing`; índice parcial `one_active_session` no esquema. |
| F2-16 | Com modelos preparados, a sessão completa funciona offline, sem chamadas à IA, dispensas fictícias ou feedback inventado. | 6, 9, 10 | **parcial** | Código sem chamadas de IA: 0 ocorrências de `opencode`, `openai`, `api.deepseek`, `https://api` em Rust/TS/Python; o cliente só usa comandos `interview_*` e o evento `interview://snapshot`; `last_advance_completes_only_full_session` fixa `mode = local_v2` sem feedback pendente; o histórico mostra um resumo sem avaliação. Os motores foram reexecutados com os pesos preparados (2 passed). Falta repetir uma sessão completa pela janela com a rede desligada (secção 4, item 8); essa repetição também continua pendente para a fase 1 (CA-13). |

Nenhum critério foi marcado **falhado**: não apareceu evidência contrária nesta
verificação.

## 4. Validação manual que falta (ação do utilizador)

Lista detalhada em [setup.md](setup.md), secção 6. Resumo do que a spec §10
exige e que **não** está demonstrado por esta automação:

- [ ] Sessão real de três perguntas, com respostas faladas, incluindo uma com
      termos técnicos; registar duração dos áudios e tempos de carregamento,
      síntese e transcrição, e a perceção da espera.
- [ ] Ouvir as perguntas sintetizadas e os originais gravados, e repetir uma
      pergunta sem nova síntese.
- [ ] Rever, guardar e confirmar cada texto; confirmar que guardar não confirma
      e que confirmar fixa o texto mostrado.
- [ ] Sair depois de confirmar e antes de avançar, reabrir e retomar a etapa
      persistida sem duplicar a confirmação nem avançar sozinho.
- [ ] Terminar a sessão e consultar o histórico, incluindo o resumo sem
      avaliação.
- [ ] Completar uma sessão de uma pergunta e outra quantidade elegível.
- [ ] Confirmar a insuficiência numa categoria e que só inicia após ajuste
      explícito da quantidade.
- [ ] Editar uma pergunta depois de iniciar a sessão e confirmar que a cópia da
      sessão não muda.
- [ ] Repetir o percurso com a rede desligada.
- [ ] Com a transcrição indisponível (auxiliar parado) ou `no_speech`, usar a
      revisão manual explícita e confirmar que nada é tratado como problema de
      pronúncia.
- [ ] Provocar uma falha de escrita e confirmar a recuperação; identificar
      qualquer falha simulada como simulação, não como hardware real.

Enquanto estes passos não tiverem evidência registada pelo utilizador, a fase 2
não é marcada como concluída.

## 5. Fase 1: critérios manuais que continuam pendentes

Esta verificação da fase 2 **não** fecha nem altera o registo da fase 1. Em
[`docs/phase-1/validation.md`](../phase-1/validation.md), continuam como antes:

| CA | Estado na fase 1 | Porque continua pendente |
| --- | --- | --- |
| CA-02 | pendente | Ouvir a voz Kokoro pela janela e repeti-la. |
| CA-03 | pendente | Gravação manual com microfone real e duração visível. |
| CA-05 | pendente | Ouvir a resposta guardada e recuperá-la com o transcritor indisponível. |
| CA-06 | pendente | Transcrever uma gravação humana em inglês. |
| CA-10 | pendente | Remover/restaurar o microfone real. |
| CA-13 | parcial | Repetir o percurso completo pela janela com a rede desligada. |
| CA-14 | pendente | Três gravações reais e decisão do utilizador sobre a espera. |

Os testes reais dos motores reexecutados nesta verificação (2 `passed`) são a
mesma evidência de motor já registada na fase 1; não substituem gravações
humanas nem audição, e não alteram nenhum destes estados.

## 6. Decisões tomadas durante a implementação (registo tipo ADR)

1. **rusqlite 0.41.1 com `bundled`, como previsto.** A resolução funcionou sem
   alterações; o `Cargo.lock` fixa `rusqlite 0.41.1` e `libsqlite3-sys 0.39.1`,
   ambos compilados no build release. Vantagem: sem dependência de um SQLite
   instalado no sistema. Custo: primeira compilação mais longa (~1m 17s no build
   release desta máquina). Uma única `Connection` dentro de `Arc<Mutex<…>>`,
   sempre em `spawn_blocking`; o guard nunca atravessa um `await`.
2. **A interface `practice` da fase 1 foi removida na tarefa 6**, quando o
   serviço passou a ser o da entrevista guiada. As proteções foram portadas, não
   perdidas: distinção `saved/pending/failed` ao guardar revisão, bloqueio de
   edição durante o pedido e decisão guardar/descartar/cancelar antes de navegar
   ou fechar vivem agora em `useInterview`/`NavigationGuard`. Só
   `practice.css` permanece, reutilizado pela interface nova. O formato JSON de
   `phase1/` mantém-se apenas para leitura dos treinos antigos.
3. **`ReviewDraft.savedAt` é nulo enquanto a revisão não está guardada**
   (`version = 0` representa o rascunho). Assim a interface distingue «nunca
   guardada» de «guardada numa data», sem inventar uma data para o rascunho e
   sem usar `null`/`0` como se fosse uma revisão persistida. A confirmação
   guarda no máximo uma versão exata por pergunta.
4. **`validate_count(0)` devolve `INSUFFICIENT_QUESTIONS`**, o mesmo código de
   uma quantidade superior ao total elegível. Zero não é um pedido válido de
   sessão nem um caso separado: a mensagem pede uma quantidade entre 1 e o total
   elegível, e nada é criado (a validação corre antes da transação).
5. **A reconciliação só considera repetíveis os erros de I/O transitórios do
   Windows: 32 (partilha), 5 (acesso negado) e 33 (bloqueio).** Todos os outros
   erros de I/O — incluindo o erro sintético do Hound para fim de ficheiro, que
   não tem código do sistema — são permanentes e marcam a gravação como
   interrompida, em vez de a repetirem indefinidamente. Um ficheiro bloqueado
   por antivírus continua publicável no arranque seguinte
   (`locked_original_stays_publishable_for_the_next_start`).

## 7. Limites e riscos registados

- A verificação automática usa áudio sintetizado e auxiliares controlados. Não
  prova qualidade de voz, inteligibilidade no hardware do utilizador, latência
  percebida nem o comportamento com microfone removido a meio.
- O build `--no-bundle` não é um instalador: não inclui Python, modelos nem a
  `.venv`. A fase 5 continua responsável pela distribuição.
- A base criada nesta máquina não contém sessões; os percursos de sessão foram
  exercitados por testes com diretórios temporários e por um cliente controlado,
  não por uma entrevista falada.
- A procura de chamadas de IA é uma verificação textual do código atual
  (0 ocorrências), não uma captura de tráfego de rede. A prova offline completa
  exige a repetição pela janela com a rede desligada.
- Avisos de terceiros durante o carregamento dos modelos (`PyTorch`,
  `weight_norm`, Misaki) não impedem o funcionamento e já estavam registados na
  fase 1.

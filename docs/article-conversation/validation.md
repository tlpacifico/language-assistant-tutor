# Conversa sobre um artigo — validação

Estado em 2026-11-05: **implementação e verificação automática concluídas; aceitação manual, áudio real, voz real e inferência paga por fazer.** Verificação automática não é aceitação manual: o modo **não** está aceite.

## Verificação automática observada

- `.\scripts\verify.ps1`: **8/8 checks, saída 0, 190,6 s** — TypeScript, **147 testes React** (20 ficheiros), build Vite, formatação e Clippy `-D warnings`, **354 testes Rust**, **56 testes Python** sem a marca `models`, `pip check` limpo.
- `npm run tauri -- build --no-bundle`: saída 0 (release, 1 min 27 s).
- Ensaio com dados temporários (`scripts/diagnostics-smoke.ps1` sobre um build isolado): janela viva, diagnóstico exportado, fecho com código 0. Não toca nas sessões pessoais.
- Os testes Python reais com modelos (`-m models`) **não foram corridos nesta verificação**.

Nada disto usou microfone real, voz Kokoro real, credencial protegida real nem pedidos pagos: áudio, auxiliar e fornecedor são adaptadores controlados.

## Matriz do desenho

| Requisito do desenho | Evidência automática | Aceitação manual |
|---|---|---|
| Artigo colado, cópia fixa, limites sem cortar | `article_domain`, `article_repository`, `ArticleSetup.test` | pendente |
| Autorização por sessão, nunca pré-selecionada; revogar/mudar destino bloqueia envios | `ArticleSetup.test`, `article_service` (revogação, destino alterado), `article_domain` | pendente |
| Gravação só termina por ação do utilizador; áudio guardado antes de enviar | `article_service` (`audio_is_saved_first…`), `article_domain` | pendente (microfone real) |
| Envio automático só com transcrição guardada; vazia/falhada/excessiva não se envia | `article_service`, `article_repository` | pendente |
| Fontes imutáveis; versão enviada vs. revista distintas | `article_repository`, `article_migration` | — |
| Artigo como dados não fiáveis; citações só do utilizador | `article_ai` | pendente (resistência semântica) |
| Comentário + pergunta, sem correções entre turnos; sugerir terminar à 5.ª; limite | `article_ai`, `ArticleConversation.test`, `article_repository` | pendente (utilidade) |
| Feedback: 4 dimensões, evidência insuficiente explícita, sem nota global | `article_ai`, `ArticleReport.test` | pendente (utilidade) |
| Correção em ramo novo, ramo/relatório antigos intactos, regeneração explícita | `article_recovery`, `article_service` | pendente |
| Resultados tardios, clique duplo, terminar/eliminar durante pedido | `article_service`, `article_repository`, `ArticleConversation.test` | — |
| Recuperação local sem envios no arranque; eliminação retomável | `article_recovery`, `article_service` | pendente (fecho abrupto real) |
| Isolamento do STAR (microfone/auxiliar) | `article_service` (`star_and_article…`) | pendente |
| Voz incremental: ordem, cancelamento, falha parcial | `article_voice`, `article_service` (voz), `test_stream` | **pendente (audição real; Kokoro por blocos)** |
| Diagnóstico sem conteúdo; operações fechadas | `article_service` (métricas), `diagnostics` | — |
| Cenário integrado (autorizar → 5 turnos → corrigir → regenerar → concluir → reabrir) | `article_service::full_conversation_…` | — |

## Desvios e limites conhecidos

- **Sem capability `synthesisChunks` no `health`**: acrescentar um campo a `HealthResult` partiria os literais existentes; o auxiliar é distribuído com a aplicação. Sem retorno automático à síntese completa se o auxiliar não conhecer `synthesize_stream`.
- **Cancelamento da voz**: pede ao auxiliar que pare entre blocos; um bloco já em geração acaba e é descartado. A transcrição seguinte espera até 2 s que o auxiliar fique livre.
- **Limite do artigo**: 30 000 bytes (a spec dizia 32 000) para caberem título, abertura, resposta e réplica em 48 000 bytes de contexto.
- **Ramos antigos** não aparecem na interface (só o ramo ativo); estão guardados e reconstruíveis (`project_article_branch`).
- **Repetir uma resposta como novo ramo** (regravar) não está implementado; só a correção de texto.
- A página Artigos **não teve validação visual**.
- Medições de desempenho e comparação `beam_size=5/1`: ver [desempenho](performance.md) — por fazer.

## Roteiro manual pendente

1. Conversa real sobre um artigo, com pelo menos cinco respostas, incluindo discordância e aprofundamento (continuidade, coesão do diálogo, espera percebida).
2. Recuperação de um erro de reconhecimento (texto manual) e de uma interrupção da voz a meio.
3. Falha de rede e repetição explícita; retoma depois de fechar a aplicação a meio de uma resposta.
4. Utilidade e fidelidade do feedback, e custo real (os pedidos pagos exigem a sua autorização).
5. Correção de uma resposta antiga e regeneração do relatório.
6. Validação visual da página Artigos (desktop e mobile).

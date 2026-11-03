# Validação da fase 4

Data: 2026-11-03. Estado: **implementada e verificada automaticamente; não aceite nem concluída.** A implementação foi autorizada pelo utilizador. Falta toda a validação humana (F4-16), a inferência paga real e a fidelidade semântica das reformulações. As pendências das fases [1](../phase-1/validation.md), [2](../phase-2/validation.md) e [3](../phase-3/validation.md) mantêm-se.

Três níveis de evidência, que não se substituem:

1. **Automação** (este documento): SQLite e ficheiros temporários reais, áudio simulado, credenciais falsas e HTTP de loopback controlado.
2. **Inferência real**: não executada. Nenhuma chave real, chamada ao fornecedor ou custo real.
3. **Aceitação humana**: não executada. O relato favorável do utilizador de 2026-11-03 (fase 3) não cobre a fase 4.

## Comandos reais (2026-11-03)

- `.\scripts\verify.ps1`: **8/8 checks, saída 0 em 41,7 s** (execução final). Contagens: **246 testes Rust** (soma das 25 suítes/binários de `cargo test`, todas `ok`), **97 Vitest em 11 ficheiros**, **36 pytest** (2 `models` desselecionados), `pip check` limpo, `tsc`, build Vite, `cargo fmt --check` e `clippy -D warnings` limpos.
- A primeira execução falhou no check 4 (`cargo fmt --check`: formatação de duas asserções em `src-tauri/tests/practice_service.rs`). Corrigido com `cargo fmt` (commit `fix:`) e o agregado repetido completo ficou verde.
- `npm.cmd run tauri -- build --no-bundle`: código 0, perfil release em 1m11s (1m15s no total) e `language-assistant-tutor.exe` gerado.
- Smoke do binário release com `APPDATA`/`LOCALAPPDATA` temporários vazios (sem configuração de áudio, sem dados pessoais): janela viva após 8 s, `CloseMainWindow=True`, saída dentro de 15 s com **código 0**; pastas temporárias removidas. Não é validação de instalador.
- `git diff --check` limpo. Os 36 comandos `interview_*` invocados em `src/lib/interview-client.ts` estão todos registados no Rust.
- O teste `-m models` (pesos reais) não foi repetido nesta fase; o auxiliar Python não mudou.

## Matriz F4-01 a F4-16

| Critério | Evidência automática (suítes) | Pendência humana / real |
| --- | --- | --- |
| F4-01 | `phase4_migration` (base vazia, v1, v2, rollback, versão futura, invariantes SQL, FK composta prática-slot), regressões `database`, `sessions`, `answers`, `ai_migration` | Atualizar uma base real pessoal (não feito) |
| F4-02 | `rewrite_validation`, `practice_ai`: fonte/nível fixos, cargas por origem, exclusão da resposta anterior ao reformular tentativa | Fidelidade com respostas reais |
| F4-03 | `rewrite_validation`: esquema, citações e segmentos inválidos recusados; instruções maliciosas na resposta ficam na mensagem de dados | Verificação humana de negação, incerteza, equipa vs. indivíduo, números e resultado negativo/qualitativo |
| F4-04 | `practice_ai`, `practice_outputs`, `Practice/RewritePanel.test.tsx`: B1 por omissão, B1/B2/C1, reutilização sem nova cobrança, variantes imutáveis, preferência local | Utilidade de cada nível |
| F4-05 | `practice_outputs`: texto de síntese sem lacunas/citações/explicações; síntese local sem nova inferência; UI | Audição Kokoro real |
| F4-06 | `practice_service`: só pergunta confirmada de sessão concluída, sem reabrir, sem alterar contador, sem seguimento | Percurso pela janela |
| F4-07 | `practice_service`, `stories`: captura/transcrição/revisão/confirmação, falha de escrita do snapshot mantém captura manual, preserva áudio | Falha de escrita real e reinício reais |
| F4-08 | `practice_service`, `Practice.test.tsx`: uma prática ativa, abandono sem eliminação, retoma, sem interferir na entrevista | Retoma real |
| F4-09 | `practice_ai`: papel attempt, fonte única, relatório próprio, retry sem repetir válidos nem nova gravação | Avaliação real da tentativa |
| F4-10 | `practice_outputs`, `Comparison.test.tsx`: comparação local, rubricas diferentes e pendências, sem afirmar melhoria por diferença textual | Utilidade da comparação |
| F4-11 | `stories`, `Stories.test.tsx`: validação, edição, snapshot histórico, uso sem STAR completo | Uso com histórias reais |
| F4-12 | `stories` (`story_marker_never_leaks_into_sources_or_answers`), `rewrite_validation` (corpo sem `ONLY_IN_STORY_742`, sem `storyId`/`snapshot`/`practiceId`/`sessionId`), `Stories.test.tsx`; edição posterior não altera snapshot | Factos só nas notas nunca tratados como ditos (julgamento humano) |
| F4-13 | `practice_ai`, `practice_service`, `RewritePanel.test.tsx`: gerações, resposta para outro nível/prática não troca seleção, comandos repetidos e cliques antigos sem duplicar efeitos | Cliques reais |
| F4-14 | `practice_ai`, `ai_service`: timeout, 401/403/429/5xx, JSON inválido, credencial indisponível, publicação falhada e retry sem rede, fecho; sem reenvio automático | Falhas reais de rede/armazenamento |
| F4-15 | `ai_usage`, `practice_outputs`, `TrainingUsagePanel`: consumo por IDs disjuntos, subtotais parciais, desconhecido não é zero, treino separado da entrevista | Comparar com faturação real |
| F4-16 | Não substituível por automação | Roteiro abaixo; **não executado** |

## Roteiro manual Windows (F4-16) — pendente

Pré-requisitos: aplicação em `tauri dev` ou binário release, modelos locais, e, para os passos com IA, chave guardada e consentimento (autorização explícita do utilizador antes de qualquer custo). Começar com texto de teste, sem dados pessoais em risco. Não enviar chaves por chat.

1. Sessão IA com uma pergunta principal e um seguimento; confirmar ambos e terminar a sessão.
2. No treino posterior, confirmar que a origem (inicial, seguimento) está identificada e que a sessão original não muda (contador, posição, relatório).
3. Usar uma resposta sem números, uma negativa e uma com aprendizagem: a reformulação não inventa números, responsabilidades ou resultados e assinala lacunas.
4. Pedir B1, B2 e C1; verificar que as variantes se mantêm, que repetir o mesmo pedido não cobra de novo e que mudar de nível durante um pedido não troca a seleção.
5. Ouvir a sugestão (Kokoro): só o texto sugerido, sem lacunas nem explicações.
6. Abrir uma prática, repetir com palavras próprias, rever e confirmar; abrir uma segunda prática noutra pergunta e confirmar que só existe uma pendente de cada vez.
7. Criar uma história com factos que não foram ditos (usar um termo exclusivo) e escolhê-la como apoio: o termo não aparece na transcrição, no editor, na confirmação nem na avaliação/reformulação.
8. Regravar, sair e retomar a prática; abandonar outra.
9. Desligar a rede: sem credencial/rede, consulta, comparação e histórico continuam; o pedido remoto falha de forma explícita, sem reenvio, e repete-se depois sem nova gravação.
10. Avaliar a tentativa e comparar com inicial/seguimento/sugestão; confirmar que a comparação não afirma melhoria só por diferença textual.
11. Medir espera (reformulação e avaliação), fidelidade factual, utilidade e custo (comparar com a faturação real; desconhecidos continuam desconhecidos).

Só este roteiro, com autorização para inferência paga, permite aceitar F4-16. Até lá a fase 4 não é marcada como aceite nem concluída.

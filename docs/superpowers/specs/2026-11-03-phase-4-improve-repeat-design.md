# Fase 4 — Melhorar e repetir

Data: 2026-11-03  
Estado: revista e aprovada tecnicamente; implementada em 2026-11-03 por autorização posterior do utilizador e verificada automaticamente ([validação](../../phase-4/validation.md)). Os critérios F4-01 a F4-16 abaixo continuam por marcar: a aceitação manual (F4-16) e a inferência paga real estão pendentes.
Base: [desenho do MVP](2026-11-01-star-interview-tutor-design.md), [fases](../plans/2026-11-01-star-interview-tutor-phases.md), [stack](../../tech-stack.md), [fase 3](2026-11-02-phase-3-ai-tutor-design.md) e [validação atual](../../phase-3/validation.md).

## 1. Intenção, autorização e limites

Fechar o ciclo de prática: consultar o que foi respondido, compreender feedback,
ler/ouvir uma reformulação fiel, responder novamente e comparar sem perder
versões anteriores. Acrescentar histórias factuais como apoio local à preparação.

O utilizador autorizou preparar a próxima fase e limitou expressamente o pedido
a «somente a spec e o plano de implementação». Este documento não autoriza
escrever código, instalar dependências, fazer migrações, usar a chave configurada
ou executar inferência paga. O coordenador avalia a spec antes de delegar o plano.
As decisões propostas abaixo não são apresentadas como escolhas já aprovadas
individualmente pelo utilizador.

As fases anteriores continuam com aceitação manual incompleta. O relato de
teste manual satisfatório não incluiu API paga. A configuração local posterior
não demonstra autenticação, saldo, utilidade semântica ou custo reais. Preparar
esta fase não altera essa evidência nem marca as fases anteriores como aceites.

Incluído: reformulação em inglês com nível escolhido, distinção correção/estilo,
síntese local Kokoro, tentativas individuais com avaliação própria, comparação
local, banco de histórias e seleção opcional, persistência e recuperação.

Excluído: seguimentos de novas tentativas, comparação por nova chamada à IA,
geração de histórias ou factos, pesquisa vetorial, pontuação global/proficiência,
diagnóstico acústico/emocional, importação em lote, eliminação de sessões ou
histórias, instalador e distribuição autónoma. Não iniciar a fase 5.

## 2. Abordagens e decisões propostas

| Abordagem | Consequência | Proposta |
| --- | --- | --- |
| Reabrir resposta/sessão original | Poucas novas entidades, mas altera fontes, progresso e relatórios históricos. | Rejeitada. |
| Criar uma entrevista completa por repetição | Reutiliza fluxo, mas confunde contadores, modos e seguimentos. | Rejeitada. |
| Prática individual ligada à pergunta fixa, com alvo de resposta próprio | Preserva originais e reutiliza áudio, confirmação e pedidos. | Recomendada. |

Usar React/TypeScript, Rust/Tauri, SQLite e os motores existentes. Nenhum
fornecedor novo. Comparação por apresentação local dos textos e relatórios;
sem um segundo avaliador ou chamada de síntese. Histórias exclusivamente locais
nesta fase: nenhum campo nem identificador de história entra na API.

Propostas de simplicidade: B1 inicialmente, conforme o desenho aprovado §2,
com seleção B1/B2/C1 na reformulação;
uma prática não confirmada de cada vez; começar práticas apenas quando não há
entrevista em curso. Nível é objetivo de linguagem escolhido, não diagnóstico
de nível CEFR do utilizador. O valor inicial B1 preserva a decisão existente;
as restantes escolhas de construção são propostas técnicas para revisão.

## 3. Ponto de partida e invariantes

Alteração aprovada em 2026-11-04: a avaliação e reformulação do original podem ocorrer numa entrevista IA em curso, após confirmação e encerramento do seguimento da pergunta. A criação de práticas continua a exigir sessão concluída. Ver [desenho da alteração](2026-11-04-per-question-ai-coaching-design.md).

O esquema SQLite v2 tem `answer_slots` para `initial`/`follow_up`, gravações,
transcrições, revisões e confirmações. A unicidade por pergunta/papel impede
adicionar repetições apenas reutilizando um papel. `ai_requests` é único por
finalidade/pergunta e `ai_reports` por pergunta. O serviço resolve alvos de
resposta da entrevista ativa. Estes contratos têm de evoluir deliberadamente.

Uma nova tentativa tem identidade, estado, ordinal e alvo próprios, ligados à
pergunta da sessão original e, opcionalmente, à tentativa confirmada anterior.
Nunca muda `position`, `question_count`, etapa de seguimento, confirmação,
gravação selecionada ou relatório original. Não criar nova pergunta do banco.
Texto, competências e critérios são os da cópia fixa da sessão de origem,
mesmo que o banco seja editado entretanto.

Reutilizar o ciclo de captura/transcrição/revisão/confirmação por um alvo
tipado que distinga principal, seguimento e tentativa. SQLite e Rust devem
impedir associar uma gravação, confirmação ou pedido ao alvo errado. A escolha
física de estender `answer_slots` ou adaptar relações fica para o plano;
não duplicar os motores nem relaxar a imutabilidade para fazer caber a prática.

Migração v2 → v3 transacional, também aplicável sequencialmente a bases vazias
e v1. Preservar todos os dados, recibos, referências, índices e triggers;
falha reverte e versão futura não é escrita. Não converter `phase1/` nem fabricar
confirmações para os treinos JSON antigos; continuam só para leitura.

## 4. Reformulação: fontes, nível e contrato

Disponível para perguntas confirmadas de entrevistas concluídas `local_v2` ou
`ai_v3`, e para tentativas individuais confirmadas. Não exige relatório prévio:
as fontes de verdade são confirmações, não o texto do feedback. Uma sessão
local continua local; pedir um exercício de IA a partir dela não a converte
em entrevista IA, não gera seguimentos retroativos nem feedback de sessão.

Na origem principal, enviar exclusivamente pergunta, critérios fixos e textos
confirmados inicial e do respetivo seguimento, se existir. Na origem tentativa,
enviar apenas a confirmação dessa tentativa com os mesmos critérios. Nunca
fundir factos da resposta antiga numa tentativa nova. Nunca incluir áudio,
notas de história, rascunhos, outra pergunta, relatórios ou reformulações como
se fossem respostas. Todas as fontes têm identificador e papel inequívocos.

«Gerar reformulação» permite escolher B1/B2/C1; B1 é a sugestão inicial. Guardar
essa preferência pública localmente para pedidos futuros. Cada pedido/resultante
fixa o nível efetivo; uma mudança posterior não altera sugestões anteriores.
Não tentar medir pronúncia, fluência oral, confiança ou certificar proficiência.

O resultado estruturado distingue:

- Texto inglês sugerido, completo e reproduzível, organizado e adequado ao
  nível pedido, sem números, responsabilidades, causalidade ou resultados novos.
- Alterações necessárias de língua, separadas de alternativas opcionais de
  estilo/organização; cada uma liga trecho literal da confirmação e segmento
  correspondente da sugestão, com explicação em português de Portugal.
- Segmentos da reformulação ligados a citações literais de uma ou mais fontes
  confirmadas. Informação vinda só do seguimento mantém essa origem visível.
- Informação em falta e perguntas de preparação fora do texto de voz. Uma
  ausência identifica dimensão e fontes avaliadas; não inventa uma citação.

Manter responsabilidade individual versus coletiva, negação, incerteza, ordem
temporal relevante, resultado negativo/qualitativo e aprendizagem. Não completar
STAR com factos inventados, nem impor números ou sucesso. Se não houver dados
para uma resposta completa, sugerir somente o que é suportado e assinalar a
limitação fora do texto de voz. Não inserir placeholders para o Kokoro ler.

Rust valida versão/esquema, limites, nível, fontes autorizadas, citações exatas,
referências aos segmentos e existência de texto de voz não vazio. Rejeitar
resultado malformado ou com referências inválidas; não apresentar a versão
parcial como validada. Texto das respostas é material não confiável no prompt,
nunca instrução para ignorar as regras.

Citações corretas não provam que uma paráfrase preserve o significado. O sistema
não pode garantir automaticamente a fidelidade semântica com validação literal.
Apresentar como sugestão da IA, com fontes consultáveis e indicação para
conferir os factos. A validação humana inclui alterações de responsabilidade,
certeza, negação, números, resultados e cronologia. Não acrescentar um segundo
pedido pago automático como suposta prova de fidelidade.

Sugestões válidas são imutáveis. Um pedido repetido com o mesmo identificador
não gera outra sugestão. Para a mesma origem/nível/versão de prompt e rubrica,
reutilizar resultado válido; mudar o nível cria outra variante, preservando
as anteriores. Regeneração explícita da mesma variante não faz parte desta
fase; repetir só pedidos falhados/interrompidos.

## 5. Ouvir a sugestão

«Ouvir reformulação» passa só o texto sugerido ao Kokoro local. Não lê citações,
explicações, notas, lacunas ou etiquetas. Mantém inglês e a configuração local
de voz/velocidade. Usar o auxiliar e coordenador de áudio existentes; não abrir
novo serviço, não mandar texto ao fornecedor para sintetizar.

A identidade do áudio derivado inclui sugestão, texto e configuração de voz;
não reutilizar áudio de outra variante. Guardar texto antes de sintetizar.
Falha de Kokoro mantém a reformulação legível e permite repetir apenas síntese,
sem nova chamada à LLM. Falta de ficheiro permite regenerar esse derivado sem
alterar o resultado textual. Captura interrompe saída; nunca reproduzir durante
gravação. Síntese/transcrição continuam coordenadas como trabalho local único.

## 6. Nova tentativa e avaliação

No histórico de entrevista concluída, escolher uma pergunta e «Tentar novamente».
Pode fazê-lo sem reformulação nem feedback disponíveis. `local_v2` e `ai_v3`
com confirmação são elegíveis; treinos JSON sem confirmação não o são.
Não iniciar durante entrevista em curso; mostrar a opção de a retomar. Uma
prática pendente impede iniciar outra entrevista ou prática até confirmar ou
abandonar explicitamente, sem apagar dados. Feedback e reformulações pendentes
de práticas já confirmadas não bloqueiam novo treino.

| Estado da prática | Comportamento |
| --- | --- |
| Preparação | Pergunta fixa, história opcional e opção de ouvir pergunta/reformulação. |
| Resposta por confirmar | Captura manual, reprodução, transcrição, revisão e confirmação próprias. |
| Confirmada | Texto imutável; avaliação ainda não pedida ou recuperável. |
| Abandonada antes de confirmar | Guardar estado, gravações e revisão; consulta apenas, sem avaliar ou reabrir. |

Criar uma prática é transação idempotente. Cada prática tem ordinal estável;
regravar antes de confirmar mantém as gravações antigas da própria prática e
não cria uma tentativa confirmada nova. Confirmação fixa exatamente a revisão
vista com referências ao áudio e origem automática/manual. Falha de gravação
ou escrita não perde a seleção anterior nem produz confirmação falsa.

Uma prática confirmada nunca cria seguimento, mesmo que o modelo o recomende.
«Avaliar tentativa» é ação distinta de confirmar e usa credencial/consentimento
existentes. A carga contém apenas pergunta/critérios originais e confirmação
desta tentativa; exclui inicial antiga, seguimento antigo, histórias e sugestões.
As evidências usam o papel próprio `attempt`, nunca `initial` para fingir que
a tentativa pertence à entrevista original. Usa rubrica e versão compatíveis
com a avaliação original; onde não existir
relatório original, fixa a rubrica atual para a primeira avaliação dessa família.

Preservar contrato de feedback STAR/inglês da fase 3, citações/ausências, até
três prioridades sustentadas, correção versus estilo e ausência de nota global.
O relatório é próprio da tentativa e imutável. Erro permite repetir só a sua
avaliação sem gravar novamente; não gerar automaticamente feedback/reformulação
ao confirmar. Sem rede pode continuar prática local e avaliar depois.

## 7. Comparação sem alterar fontes

No histórico, comparar original com uma tentativa confirmada ou duas tentativas
confirmadas da mesma pergunta. Apresentar por ordem de criação, com datas,
originais de áudio, automáticos, confirmações e relatórios separados. Rascunho
ou prática abandonada não conta como resposta confirmada comparável.

No original distinguir resposta inicial e seguimento. Nunca concatenar e
apresentar como se tudo tivesse sido dito antes do seguimento. Reformulação
aparece em zona «Sugestão da IA», não na coluna do que foi respondido. Notas
aparecem apenas como «Apoio à preparação», fora das evidências.

Comparação é local: mostrar classificações e prioridades lado a lado, sem
inventar uma pontuação de evolução, chamar diferença textual de melhoria ou
pedir nova avaliação combinada. Se uma avaliação não existir, indicar pendente
e permitir pedi-la para esse alvo elegível. Avaliação original da entrevista
só continua disponível pelo fluxo original, sem alterar `local_v2` para a criar.

Guardar versão da rubrica em cada relatório. Se diferentes, indicar comparação
limitada por critérios/versões diferentes; não recalcular ou substituir
relatórios antigos automaticamente. A ausência de seguimento numa tentativa
tem etiqueta explícita, para evitar comparação enganadora com original ampliado.

## 8. Histórias factuais e isolamento

Banco local com listagem, adição e edição. Cada história tem identificador,
versão, título, notas de situação, responsabilidade/tarefa, ações, resultado
ou aprendizagem, competências e datas. Título não vazio, pelo menos um campo
factual preenchido e uma competência não vazia; aceitar história incompleta
identificada como tal. Ignorar espaços exteriores na validação/persistência,
sem reescrever conteúdo interior. Notas podem estar em português ou inglês;
não as tratar como resposta de treino nem gerar factos automaticamente.

Sem eliminação, importação, extração automática das respostas ou associação
automática. Filtragem simples por competência é suficiente. Uma história pode
apoiar várias perguntas/práticas; associação é opcional e manual, uma por alvo.

Na preparação de cada pergunta principal ou prática, antes da primeira captura,
permitir escolher, substituir ou remover o apoio. Na primeira captura iniciada
com sucesso, fixar snapshot do conteúdo/versão selecionado; não aguardar a
confirmação para preservar a preparação efetiva. Depois fica só de leitura
nesse alvo. Editar o banco posteriormente não muda snapshots anteriores.
O seguimento partilha a preparação da principal, sem associação própria.
Não exigir história para gravar nem bloquear por campos STAR em falta.

Mostrar notas durante preparação/revisão com identificação de que são notas,
não transcrição. Nunca preencher editor da resposta, confirmação ou prompt a
partir delas. Não enviar notas, título, competências da história ou snapshot
em pedidos de seguimento, avaliação ou reformulação. Critérios/competências da
pergunta fixa continuam permitidos; são entidades diferentes.

Informação apenas nas notas pode ser vista localmente como algo a considerar
num treino futuro; não produzir análise automática do que «faltou dizer» com
essas notas nem incorporá-la numa sugestão de voz. Se o utilizador disser essa
informação numa nova tentativa, a confirmação dessa fala passa a ser fonte
legítima; o snapshot da história nunca o é.

## 9. Persistência, operações e recuperação

Persistir histórias e versões/snapshots, práticas e vínculos, alvos de resposta,
reformulações/variantes/fontes/nível, relatórios próprios, pedidos/tentativas,
consumo e referências dos derivados de voz. Não criar base paralela nem
sobrescrever ficheiros/linhas de origem. Estado de avaliação e sugestão é
independente do estado da prática; uma falha da IA não desfaz a confirmação.

Reutilizar os padrões da fase 3: reservar antes da rede, identificadores de
comando e hash do conteúdo, gerações e fontes imutáveis, configuração/tarifário
fixados por tentativa, aplicação transacional apenas ao alvo/generation
esperados. Alargar finalidades para avaliação de tentativa e reformulação,
sem esbarrar na unicidade atual por pergunta nem confundir `attempt` HTTP com
tentativa de treino. O plano deve usar nomes/contratos distintos.

Uma chamada remota de cada vez, sem retries pagos automáticos. Rede não mantém
Mutex SQLite ou bloqueio global de transições; consulta e navegação continuam
possíveis. Ao regressar, atualizar só o alvo de origem, mesmo que outra prática
esteja aberta. Não substituir a variante selecionada na interface por um
resultado tardio para outro nível. Saídas descartadas continuam no consumo.

No reinício, captura não recomeça e pedidos remotos em curso tornam-se
interrompidos/incertos, sem reenvio automático. Revisão guardada e confirmação
persistem. Publicação local falhada após resposta remota permite nova escrita
sem nova rede enquanto resultado estiver em memória; crash nesse intervalo
mantém resultado incerto, não promete resposta guardada. Gravação finalizada
sem publicação segue reconciliação existente, sem exigir outra captura.

Custos por finalidade, prática, origem e sessão de proveniência: incluir
avaliações/reformulações falhadas ou repetidas, preservando registos históricos.
Não reescrever a avaliação original para adicionar custos; o resumo derivado
pode distinguir «entrevista original» e «prática posterior», com eventual
«total combinado» explicitamente rotulado. Uma tentativa
de rede é contada uma vez, sem duplicar entre prática e sessão. Desconhecidos
ficam desconhecidos; apresentar subtotais parciais e tentativas incertas.

Guardar tarifário/data/fonte e custo reportado separados da estimativa, conforme
fase 3; não fixar novos preços nesta spec. Síntese local não é consumo da API.
Sem chave/consentimento, manter funções locais e mostrar como configurar, sem
ler segredo para o frontend. Não enviar nada automaticamente ao editar história,
selecionar nível, abrir comparação, confirmar prática ou carregar histórico.

Limites de entrada/saída, tamanho das histórias, texto de reformulação e duração
de áudio serão concretizados no plano, preservando os limites existentes e
sem truncamento silencioso. Resultados fora do limite são erros recuperáveis,
não versões parcialmente aceites.

## 10. Verificação e aceitação

Todos os critérios abaixo começam pendentes. Esta tarefa documental só verifica
coerência e links; não produz evidência de implementação. O plano deve prever
dados temporários, credenciais/HTTP controlados e testes de falhas, sem usar
dados pessoais ou inferência paga nos testes automáticos.

- [ ] F4-01: Migração preserva sessões local/IA, alvos, originais, relatórios e recibos; rollback e versão futura são seguros, incluindo base vazia/v1/v2.
- [ ] F4-02: Reformulação fixa fonte/nível; apenas confirmações autorizadas entram na carga, incluindo origem de seguimento e exclusão da resposta antiga ao reformular tentativa.
- [ ] F4-03: Esquema/citações/segmentos inválidos são recusados; casos com negação, incerteza, equipa versus indivíduo, números e resultado negativo/qualitativo têm verificação humana de fidelidade.
- [ ] F4-04: B1/B2/C1 e valor inicial B1 não são diagnóstico; variantes preservam anteriores e pedido repetido não cobra de novo resultado já válido.
- [ ] F4-05: Lacunas, citações e explicações não entram no texto Kokoro; falha/falta de áudio derivado permite síntese local sem nova inferência.
- [ ] F4-06: Nova tentativa só nasce de pergunta confirmada de sessão concluída; não reabre sessão, altera contador ou gera seguimentos. Guarda confirmações e relatórios distintos.
- [ ] F4-07: Captura/transcrição/revisão/confirmação e regravação preservam versões; falha de escrita e reinício retomam prática sem perder áudio guardado ou duplicar confirmação.
- [ ] F4-08: Prática pendente pode ser retomada ou abandonada sem eliminação; única captura/prática ativa não interfere em entrevista ou reprodução.
- [ ] F4-09: Avaliação só usa confirmação da prática e critérios fixos; sem rede fica pendente e retry não repete relatórios válidos nem exige nova gravação.
- [ ] F4-10: Comparação distingue inicial, seguimento, tentativa e sugestão, apresenta rubricas diferentes/pendências e não afirma melhoria por simples diferença textual.
- [ ] F4-11: Banco de histórias adiciona/edita com validação, preserva preparação histórica por snapshot e não exige STAR completo para usar.
- [ ] F4-12: História com marcador exclusivo não aparece em nenhum payload de IA, transcrição, confirmação ou evidência gerada; edição posterior não muda snapshot fixado.
- [ ] F4-13: Identidades/gerações impedem aplicar resultado a outra prática/nível; comandos repetidos, concorrência e cliques antigos não duplicam efeitos ou custos.
- [ ] F4-14: Timeout, credencial indisponível, 401/403/429/5xx, JSON inválido, publicação falhada e fecho mantêm fontes e recuperação explícita; não há reenvio automático.
- [ ] F4-15: Consumo inclui finalidades novas e resultados descartados, sem dupla contagem ou desconhecido tratado como zero; custos de treino distinguem-se da entrevista original.
- [ ] F4-16: No Windows, utilizador ouve sugestão, repete com palavras próprias, confirma, avalia e compara; valida fidelidade factual, utilidade, espera e isolamento das notas.

Validação humana futura: pelo menos uma origem com seguimento, uma sessão local,
uma resposta sem números com resultado negativo/aprendizagem e duas tentativas
da mesma pergunta. Usar história com factos que não foram ditos e verificar que
não surgem na sugestão/feedback. Testar variantes de nível, áudio Kokoro,
regravação, saída/retoma e indisponibilidade de rede. Registar tempos, modelo,
consumo/custo conhecido e apreciação real, sem publicar dados profissionais.
Não dar como validada inferência paga antes de a executar com autorização futura.

## 11. O que fica para o plano técnico

Esquema físico/migração v3 e restrições, contratos Tauri/DTO, adaptação dos alvos
de áudio e validação de fontes, prompts/esquemas completos, relações de versões
de rubrica, limites concretos, componentes do histórico/prática/histórias,
cache de voz e testes de recuperação. Reutilizar transportes, credenciais,
consentimento e consumo existentes sem trocar stack.

O plano deve separar tarefas e verificações executáveis, indicar futuras
atualizações em setup/validation e preservar alterações existentes da fase 3.
A aprovação técnica da spec permite escrever o plano, não implementar: o pedido
atual termina com estes dois documentos revistos. Implementação e comandos que
alterem o produto exigem uma instrução posterior do utilizador.

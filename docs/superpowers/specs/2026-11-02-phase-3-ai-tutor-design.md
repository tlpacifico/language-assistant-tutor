# Fase 3 — Tutor com IA

Data: 2026-11-02  
Estado: revista e aprovada tecnicamente pelo coordenador, por delegação do utilizador; implementação verificada automaticamente; revisão técnica do coordenador concluída; aceitação manual pendente.  
Base: [fases](../plans/2026-11-01-star-interview-tutor-phases.md), [desenho aprovado](2026-11-01-star-interview-tutor-design.md), [stack](../../tech-stack.md) e [fase 2](2026-11-02-phase-2-guided-interview-design.md).

## 1. Intenção e autorização

Acrescentar à entrevista guiada um seguimento contextual opcional por pergunta
principal e feedback final sobre conteúdo/STAR e inglês no texto confirmado.
A aplicação continua a controlar a entrevista; a LLM recomenda conteúdo e não
executa transições, comandos ou ferramentas.

As fases 1 e 2 têm implementação e verificação automática, mas continuam com
aceitação manual pendente nos respetivos registos de
[fase 1](../../phase-1/validation.md) e [fase 2](../../phase-2/validation.md).
Nesta sessão, o utilizador autorizou explicitamente «Antecipar a fase 3»,
mantendo essas validações pendentes. Esta é uma exceção autorizada à sequência
anterior de validar antes de avançar; não é aceitação das fases anteriores.
Pediu ainda delegar spec, plano e implementação, com avaliação e melhorias do
coordenador entre etapas. As opções abaixo são propostas técnicas para essa
revisão, não escolhas já aprovadas individualmente pelo utilizador.

## 2. Âmbito e alternativas

Inclui configuração do fornecedor e credenciais protegidas, consentimento para
envio de texto, seguimentos com voz/revisão/confirmação, feedback fundamentado,
consumo/custo por pedido e sessão e recuperação de falhas desde a integração.
Reutiliza áudio, modelos e banco de perguntas existentes.

Não inclui reformulações completas, reprodução de respostas sugeridas, novas
tentativas depois da confirmação, comparação, histórias pessoais, eliminação
de sessões, instalador, contas ou diagnóstico acústico/emocional. São fases
posteriores. Não acrescenta outro fornecedor ou motor como fallback silencioso.

| Abordagem | Consequência | Proposta |
| --- | --- | --- |
| Um pedido final para toda a sessão | Menos pedidos, mas repete tudo numa falha e dificulta recuperação granular. | Não escolhida. |
| Um relatório por pergunta e agregação local | Evidências pequenas, repetição apenas dos relatórios em falta e custo atribuível. | Recomendada. |
| Relatórios por pergunta e nova chamada de síntese | Pode produzir narrativa global, mas acrescenta custo e evidências indiretas. | Adiada. |

Outras propostas: modo IA explícito em novas sessões; manter modo local;
pedidos externos apenas por ações identificáveis; sem repetição automática de
pedidos pagos; explicações/feedback em português de Portugal, perguntas,
seguimentos, citações e correções inglesas em inglês.

## 3. Ponto de integração e compatibilidade

O código atual tem `SessionMode::LocalV2`, estados `in_progress`/`completed`,
uma confirmação por `session_question_id` e restrições SQL que só admitem
`local_v2`. `operations` admite apenas transcrição/síntese. `command_receipts`
também limita os tipos permitidos. A migração é necessária, não basta
acrescentar variantes aos DTOs.

Manter React/TypeScript, Rust/Tauri, SQLite `rusqlite` e auxiliar Python.
Introduzir `ai_v3` para novas sessões; `local_v2` continua disponível, sem
seguimentos, custos fictícios ou feedback retroativo. Uma sessão local já em
curso termina no seu modo original. Não converter dados de `phase1/`.

Migração transacional v1 → v2 preserva identidades, textos, gravações, relações,
recibos, cópias das perguntas e versões. Deve funcionar também em base vazia,
aplicando ambas as migrações. Se reconstruir tabelas devido aos CHECKs,
recriar índices/triggers e verificar chaves estrangeiras. Erro faz rollback;
versão futura é recusada sem escrita. Não desativar garantias de imutabilidade
como solução para suportar seguimentos.

O seguimento é uma entidade dependente da pergunta principal, com resposta
própria e identidade distinta. Nunca reutiliza a confirmação da principal nem
se torna uma pergunta sorteada do banco. O plano deve escolher a alteração
física mínima para reutilizar captura, transcrição e revisões com alvos
inequívocos, garantindo uma confirmação por resposta e um seguimento por
principal. Não duplicar o motor de áudio nem representar seguimentos como
perguntas principais artificiais.

## 4. Configuração, credenciais e envio

Configuração apresenta modelo pago e URL base; a referência inicial é
`deepseek-v4.1-flash`, base `https://opencode.ai/inference/openai/v1`, com
`/chat/completions` acrescentado uma única vez. Chave configurada pelo núcleo
no armazenamento protegido do Windows. A biblioteca/adaptador concreto é
escolhido no plano; não introduzir ficheiro de texto alternativo se falhar.

A interface pode receber a chave apenas no campo de introdução e enviá-la
uma vez ao comando Rust para guardar. Limpa o campo após conclusão; nunca
volta a receber o segredo do núcleo. O estado apenas diz configurada ou não,
com possibilidade de substituir/remover. Não persistir em frontend, SQLite,
ficheiros de configuração, mensagens de erro ou logs.

Aceitar HTTPS sem credenciais embutidas, fragmentos ou parâmetros secretos;
não seguir redirecionamentos que reenviem a chave. Ao mudar destino, invalidar
a autorização de envio para o novo destino e exigir confirmação desse
destinatário antes de enviar. Não testar rede nem enviar respostas ao guardar
configuração. Modelos gratuitos não substituem o modelo pago.

Explicar antes do primeiro envio que só texto confirmado, pergunta e critérios
seguem para o destinatário configurado. Não prometer retenção nula. O
consentimento fica persistido com destino e versão do aviso. Recusar envio sem
consentimento no Rust. A confirmação de texto guarda primeiro; a ação «Pedir
seguimento» só é disponibilizada depois. Assim confirmação local não depende
da rede nem é confundida com consentimento. O modo IA é identificado no início
e na entrevista. Sem configuração pode guardar respostas, dispensar explicitamente
o seguimento indisponível e deixar feedback pendente.

Não enviar áudio, caminhos locais, rascunhos, transcrições automáticas distintas
da versão confirmada, banco de perguntas completo ou notas pessoais. Nesta
fase o seguimento usa apenas pergunta, critérios fixados e resposta principal
confirmada. Não existe resumo por IA de respostas anteriores.

## 5. Entrevista e seguimento

Quantidade e progresso contam apenas perguntas principais. Para N principais,
há no máximo N seguimentos. A captura continua iniciada/terminada pelo
utilizador. Síntese usa Kokoro e nunca toca durante gravação.

| Etapa persistente por principal no modo IA | Ações permitidas |
| --- | --- |
| Resposta principal por confirmar | Capturar, transcrever, rever e confirmar como na fase 2. |
| Decisão por pedir | Pedir seguimento; se serviço indisponível, dispensa explícita com motivo. |
| Pedido em curso | Consultar dados; aguardar ou dispensar explicitamente, invalidando o resultado. |
| Pedido falhado/interrompido | Repetir com novo identificador de tentativa ou dispensar. |
| IA decidiu sem seguimento | Avançar explicitamente. |
| Seguimento criado | Ouvir/ler, gravar, rever e confirmar a resposta ao seguimento. |
| Seguimento respondido | Avançar explicitamente; nunca gerar outro seguimento. |
| Seguimento dispensado | Avançar; a etapa não reabre depois. |

Uma recomendação de seguimento contém pergunta inglesa não vazia, critério ou
dimensão STAR em falta e motivo ligado à resposta. Quando já foi explicada
informação suficiente, recomendar não fazer seguimento. Não usar seguimento
para corrigir gramática ou exigir números. A relevância semântica exige teste
humano; validação estrutural não prova que a pergunta é útil.

Recomendação «sem seguimento» tem motivo e não cria entidade nem confirmação.
Dispensa por falha/indisponibilidade é decisão do utilizador e tem estado e
motivo próprios; não afirmar que a IA considerou a resposta completa. Não
permitir dispensar resposta a um seguimento já apresentado nesta fase.

Confirmar o seguimento preserva original, automático, revisões e confirmação
com a mesma política da principal. Retranscrição ou nova gravação só antes da
confirmação desse alvo. O progresso principal não aumenta ao criar/confirmar
seguimento. Avanço valida no núcleo confirmação principal e etapa encerrada;
um botão antigo ou comando local não contorna as regras IA.

Após fechar a última principal, a entrevista fica terminada para captura e
aguarda feedback. Separar estado da entrevista de estado da avaliação:
`pending`, `running`, `partial`, `failed`, `ready`. Entrevistas terminadas com
feedback pendente não impedem começar outra entrevista, respeitando a regra
de uma única entrevista com captura em curso. Histórico mostra ambos os
estados. Relatórios continuam acessíveis e recuperáveis sem reabrir respostas.

## 6. Pedidos, concorrência e recuperação

Cada operação lógica referencia finalidade, sessão, principal, confirmações
imutáveis de origem, versão de prompt/rubrica/esquema e configuração efetiva.
Cada tentativa de rede tem identificador próprio, geração, datas, estado,
modelo pedido/devolvido e consumo. Repetir um comando com o mesmo identificador
e conteúdo não cria outra tentativa; reutilizar identificador com conteúdo
diferente é recusado. Repetir após falha é uma ação nova ligada à operação.

Persistir a reserva do pedido antes da rede. Validar e aplicar o resultado numa
transação que confira operação/generation ainda corrente, alvo e confirmações,
estado esperado e ausência de resultado já aplicado. Registar consumo conhecido
mesmo se o conteúdo falhar validação ou chegar depois da dispensa; nunca aplicar
esse conteúdo à sessão. Garantias locais não prometem execução única no fornecedor.

Uma única chamada externa de cada vez limita custo e complexidade; áudio local
tem coordenação independente. HTTP corre fora do bloqueio de transições e do
Mutex SQLite. Durante espera deve ser possível consultar, navegar e resolver
dispensa. Reservar sob bloqueio curto, libertar, pedir, readquirir e validar.
Não manter o `transitions` de `AppService` durante o `await` HTTP.

Timeout, ligação perdida e fecho podem deixar faturação desconhecida. No
arranque, pedidos `running` passam a interrompidos/resultado incerto; nunca
reenviar automaticamente. Mostrar que repetir pode gerar novo custo. Uma
dispensa invalida a geração e fecha a etapa antes de avançar; cancelamento do
transporte é apenas otimização, não a garantia contra resultados tardios.

Sem rede, preservar tudo localmente; repetir seguimento ou dispensá-lo, continuar
captura e pedir feedback quando houver rede. Não gerar seguimentos anteriores
após reconexão. Distinguir credencial inválida, falta de saldo/limite, rate
limit, timeout/rede, resposta inválida e erro de escrita sem expor corpo bruto.
Não fazer retries automáticos nem reparações pagas automáticas nesta fase.

Se a rede terminar mas a escrita falhar, conservar resultado em memória e
permitir repetir a publicação sem nova chamada. Após crash nesse intervalo,
marcar resultado incerto e manter dados confirmados; não afirmar que a resposta
remota foi guardada. Não apagar ou regravar o áudio como recuperação.

## 7. Contrato do feedback e evidências

No fim, «Gerar feedback» pede relatórios das perguntas que ainda não têm um
relatório válido, sequencialmente. Cada pedido recebe pergunta/critérios e
confirmação principal, mais confirmação do seguimento quando existe. Dispensa
é metadado, não resposta vazia fictícia. Não avaliar notas de histórias.

O JSON estruturado tem versão e separa conteúdo/STAR de inglês. Inclui:

- Relevância, situação, responsabilidade individual, ações, resultado e
  aprendizagem com classificação `claro`, `parcial` ou `não identificado`.
- Pontos fortes e lacunas com explicações sustentadas; aceitar resultados
  qualitativos, negativos e aprendizagens sem exigir sucesso ou números.
- Correções necessárias de inglês e sugestões opcionais de estilo separadas,
  sempre com trecho inglês de origem e alteração proposta fiel aos factos.
- Até três prioridades concretas por pergunta; podem ser menos ou nenhuma,
  com explicação de que não se identificaram outras prioridades sustentadas.

Cada observação positiva, erro ou facto sobre o que foi dito referencia
`confirmationId`, papel `initial`/`follow_up` e citação literal não vazia.
Rust resolve apenas as confirmações fornecidas ao pedido e verifica a substring
exata, incluindo Unicode e pontuação; não procurar numa revisão diferente nem
normalizar para fabricar correspondência. Distinguir avaliação inicial da
informação esclarecida apenas no seguimento.

Uma ausência não pode ter citação inventada. Usa tipo `absence`, dimensão em
falta e lista dos identificadores efetivamente avaliados; só é permitida para
dimensões/critérios fornecidos. Deve dizer «não identificado no texto» e não
afirmar que o utilizador não realizou a ação na realidade. Classificação
`parcial` requer também trecho que mostre o conteúdo parcial. Rust valida
estrutura e referências; a conclusão semântica de ausência é recomendação do
modelo, não facto provado pelo algoritmo.

Prioridades referenciam observações válidas do próprio relatório; uma prioridade
baseada em ausência herda essa limitação. A síntese da sessão é construída
localmente: relatórios por ordem, até três prioridades escolhidas por ordem
de lacunas de conteúdo e depois correções necessárias, com origem visível e
sem nota global. Pode mostrar menos de três. Não criar novas afirmações na
agregação. Se houver relatórios em falta, indicar feedback parcial.

Rejeitar o relatório completo se tiver esquema inválido, referências erradas,
citação inexistente, classificação fora do enum, prioridade órfã, nota global
ou campos de diagnóstico de pronúncia/confiança emocional. Guardar o erro e
consumo sem apresentar observações inválidas. Texto de resposta é material
não confiável delimitado no prompt; nunca é instrução para alterar rubrica.
Validação literal não garante verdade semântica: manter a apresentação como
feedback da IA e validar utilidade/fidelidade com respostas reais.

Relatórios válidos são imutáveis e mantêm as fontes/prompt/rubrica. Repetir
feedback parcial só pede os relatórios em falta/falhados; não substitui os
válidos nem reabre o seguimento. Sem uma funcionalidade explícita de regeneração
nesta fase, não cobrar novamente uma avaliação já válida.

## 8. Consumo, configuração efetiva e limites

Guardar separadamente tokens conhecidos de entrada, saída e cache, custo
reportado pelo fornecedor, respetiva natureza e estimativa local com moeda,
tarifário/data/fonte. Ausente é `null`/desconhecido, nunca zero inferido.
Entrada e cache podem ter semânticas diferentes conforme os campos: o plano
deve fixar a normalização para evitar contar cache duas vezes.

Tarifário local é configuração datada editável, copiada para cada pedido.
Não codificar preços como verdade permanente. Sem tarifa aplicável ao modelo,
mostrar estimativa indisponível. Se só há entrada total sem cache separada,
usar tarifa normal e identificar a estimativa conservadora. Não somar custo
reportado e estimativa como duas despesas do mesmo pedido.

Totais da sessão incluem tentativas repetidas, inválidas e tardias. Se houver
pedidos de custo desconhecido, mostrar subtotal conhecido e quantidade de
pedidos desconhecidos; não chamar a esse subtotal custo total. Valores de
tokens parciais também se apresentam como parciais. Erros antes de qualquer
envio distinguem-se de pedidos enviados sem recibo de consumo.

Modelo, URL, tarifário e versões usados ficam fixados por tentativa. Mudança
na configuração só afeta pedidos futuros; nunca muda a interpretação do
histórico. Não enviar chave ao catálogo. Não é preciso CLI OpenCode.

O plano fixa timeout, limite de resposta HTTP/JSON, comprimento de pergunta
gerada, contexto de entrada e máximo de saída, com erros claros. Não truncar
texto confirmado nem aceitar JSON cortado como relatório completo. Excesso
de limite conserva textos e permite diagnóstico/reconfiguração, sem inventar
uma nova tentativa de resposta nem recorrer a outro modelo silenciosamente.

## 9. Persistência e interface

Persistir configuração pública/consentimento, modo IA, etapa por principal,
seguimento único, alvo da resposta, pedidos/tentativas/gerações, metadados de
consumo e relatórios/fontes. Não criar tabelas antecipadas para a fase 4.
Reutilizar o padrão de recibos de comandos e versões esperado pelo núcleo.
Testar invariantes também no SQLite, com chaves estrangeiras e unicidade,
não apenas por desativação de botões.

Estender preparação, entrevista e histórico existentes; acrescentar configuração
de IA e leitura de feedback. Mostrar estado atual sem depender de página aberta.
Seguimento tem etiqueta própria junto de «Pergunta X de N». Durante rede mostrar
espera e opções legítimas; erro dá recuperação correspondente. Confirmações e
fontes do feedback são consultáveis; originais continuam reproduzíveis.

No modo local conservar percurso offline da fase 2. No modo IA distinguir
dispensa, decisão sem seguimento, feedback pendente/parcial/pronto e custos
desconhecidos. Não chamar uma entrevista terminada de «avaliada» antes de
existirem todos os relatórios válidos.

## 10. Verificação e aceitação

Todas as caixas começam pendentes. O plano detalha testes do núcleo com
transporte/credenciais falsos, testes HTTP com servidor controlado, migrações
v1 com dados reais de teste e interface com estados persistidos. Nenhum teste
automático necessita de chave paga ou envia respostas profissionais reais.

- [ ] F3-01: Modo local permanece funcional; sessões anteriores mantêm conteúdo e comportamento após migração, incluindo sessão em curso.
- [ ] F3-02: Chave fica protegida no Windows e não regressa em DTO, configuração, SQLite, erro ou log; falha de armazenamento não usa texto simples.
- [ ] F3-03: Nenhum envio antes de guardar confirmação e consentimento para destino atual; carga HTTP contém apenas texto necessário, nunca áudio/rascunhos.
- [ ] F3-04: Sessão IA com N principais tem no máximo N seguimentos; nunca gera seguimento de seguimento nem altera o contador principal.
- [ ] F3-05: Seguir com voz, revisão e confirmação preserva as versões e gravações, incluindo nova captura antes de confirmar e recuperação de transcrição falhada.
- [ ] F3-06: Falha exige retry ou dispensa explícita antes de avançar; dispensa fica distinguível de decisão da IA e bloqueia resultado tardio.
- [ ] F3-07: Cliques repetidos, pedidos simultâneos, versão antiga e resultados fora de ordem não duplicam seguimentos, confirmações ou avanços.
- [ ] F3-08: Reinício em pedido/decisão/seguimento/feedback recupera etapa sem reenviar automaticamente nem exigir nova gravação.
- [ ] F3-09: Feedback separa STAR/inglês, inicial/seguimento e correção/estilo; evidências só apontam às confirmações avaliadas e citações exatas.
- [ ] F3-10: Ausências usam contrato próprio sem citações fabricadas; referências/citações inválidas e JSON truncado não produzem feedback apresentado como válido.
- [ ] F3-11: Resultados qualitativos, negativos, aprendizagens e instruções maliciosas na resposta são cobertos por testes de contrato e validação humana; não há diagnóstico acústico nem nota global.
- [ ] F3-12: Falha parcial de feedback conserva relatórios válidos e retry só pede os em falta; histórico distingue entrevista terminada de avaliação pronta.
- [ ] F3-13: Tokens e custos desconhecidos continuam desconhecidos; subtotal parcial, cache, tentativas falhadas/tardias e mudanças de tarifário não falseiam totais.
- [ ] F3-14: Rede lenta/timeout/401/403/429/5xx, credencial indisponível, resultado remoto seguido de falha SQLite e versão futura da base têm recuperação sem perda das fontes.
- [ ] F3-15: UI continua utilizável durante rede; dispensa não espera pelo timeout; áudio/captura conservam as proteções e não bloqueiam consulta.
- [ ] F3-16: Sessão real de três principais produz seguimentos úteis quando necessários, feedback fiel e custo observável, com testes de indisponibilidade e retoma no Windows.

Validação humana: usar respostas completas e incompletas, termos técnicos,
contribuição individual, resultado qualitativo/negativo e aprendizagem. Conferir
citações e classificação inicial versus seguimento; confirmar ausência de
exigência artificial de números. Medir espera e utilidade do modelo pago.
Registar envio real, modelo e consumo/custo disponíveis, sem publicar respostas
pessoais. Não afirmar integração paga validada sem chave/saldo e execução real.

Executar verificações existentes adequadas à alteração, incluindo regressão
das fases anteriores. Durante implementação criar `docs/phase-3/setup.md` e
`validation.md` com comandos/evidências reais e pendências; atualizar documentos
afetados e AGENTS.md sem marcar aceitação manual antecipadamente.

## 11. Fontes e decisões para o plano

Consultado em 2026-11-02, sem executar inferência paga:

- [API oficial OpenCode](https://opencode.ai/v2/docs/console/api/inference/): autenticação Bearer, rota Chat Completions e metadados de custo em `usage`, incluindo natureza estimada quando indicada pelo fornecedor.
- [Modelos e preços oficiais](https://opencode.ai/v2/docs/console/models/): `deepseek-v4.1-flash` pago, endpoint acima e referência de US$0,30 entrada, US$1,20 saída e US$0,006 leitura de cache por milhão de tokens.
- [Catálogo oficial](https://opencode.ai/inference/v1/models): consulta pública disponível; não constitui teste de autorização/saldo da conta.

Disponibilidade, preços e campos efetivamente devolvidos podem mudar. As páginas
não demonstram suporte específico a JSON Schema estrito neste modelo; não
depender dessa capacidade sem confirmação. Exigir JSON no prompt e validar no
Rust. O consumo pode faltar apesar de documentado; preservar desconhecido.

Ficam para o plano apenas decisões de construção: biblioteca HTTP e mecanismo
Windows de credenciais; migração física e reutilização dos alvos de áudio;
DTOs/esquemas JSON completos; organização de módulos; limites concretos;
normalização dos campos de consumo; componentes e testes reproduzíveis. Não
trocar a stack, ampliar para a fase 4 ou considerar validações anteriores feitas.

# Fase 2 — Entrevista guiada

Data: 2026-11-02  
Estado: spec de referência; implementação executada e verificada automaticamente em 2026-11-02, com a validação manual do utilizador pendente.  
Base: [plano das fases](../plans/2026-11-01-star-interview-tutor-phases.md), [desenho do MVP](2026-11-01-star-interview-tutor-design.md) e [stack técnica](../../tech-stack.md).

Plano técnico: [Implementação da fase 2](../plans/2026-11-02-phase-2-guided-interview-implementation.md), com as tarefas 1 a 9 executadas e a tarefa 10 parcial. Evidência real em [docs/phase-2/validation.md](../../phase-2/validation.md).

## 1. Intenção e ponto de partida

Transformar o treino de uma pergunta fixa numa entrevista local com várias
perguntas: escolher categoria e quantidade, ouvir e responder, rever e confirmar
cada texto, terminar e consultar o que ficou guardado.

A fase 1 já implementa captura, reprodução, síntese, transcrição, revisão e
recuperação de uma resposta em ficheiros JSON. A fase 2 reutiliza esses motores
e introduz o banco editável, o estado da entrevista e SQLite. Não integra a API.

O utilizador pediu para avançar para a spec da próxima fase. O registo de
[validação da fase 1](../../phase-1/validation.md) ainda contém critérios manuais
pendentes. A preparação desta spec não equivale a aceitar a fase 1 nem a provar
esses critérios. Antes de implementar a fase 2, reconciliar esse registo com os
testes reais do utilizador, incluindo a aceitabilidade da espera. Não repetir
testes que já tenham evidência suficiente; registar a evidência que faltar.

## 2. Âmbito

### Incluído

- Banco inicial de 20 perguntas em inglês, nas seis categorias do MVP.
- Consulta, adição e edição de perguntas, categoria, competências e critérios.
- Configuração da categoria e do número de perguntas principais: três por
  defeito, uma ou outra quantidade disponível.
- Seleção sem repetição e ordem fixadas no início, com cópia dos conteúdos.
- Uma sessão em curso de cada vez, com progresso e retoma após reinício.
- Captura manual, reprodução, transcrição local e revisão em cada pergunta.
- Confirmação explícita de uma versão imutável do texto.
- Histórico de sessões, perguntas, gravações e textos guardados.
- SQLite para os dados das novas sessões; áudio em ficheiros locais.
- Preservação e consulta dos dados anteriores da fase 1.

### Excluído

Seguimentos, feedback, classificações e chamadas à IA pertencem à fase 3.
Histórias pessoais, reformulações, novas tentativas de respostas já confirmadas
e comparação pertencem à fase 4. Eliminação de sessões, instalador autónomo e
consolidação da distribuição pertencem à fase 5.

Não acrescentar contas, sincronização, pesquisa automática de histórias,
transcrição em direto, gravação terminada por silêncio ou diagnóstico da fala.
Não apresentar uma sessão terminada como uma entrevista avaliada.

## 3. Abordagem e decisões propostas

A base escolhida pelo utilizador mantém-se: React/TypeScript, Rust/Tauri 2,
SQLite, faster-whisper e Kokoro no auxiliar Python. A divisão aprovada em fases
já determina a abordagem funcional; não é necessário escolher outra stack.

| Opção de evolução | Consequência | Proposta |
| --- | --- | --- |
| Prolongar os JSON para todas as sessões | Evita SQLite agora, mas adia a entrega prevista e dispersa as transações da entrevista. | Descartada por contrariar o âmbito aprovado. |
| Introduzir SQLite para sessões novas e conservar os artefactos da fase 1 | Permite transações sem converter ou reescrever gravações antigas. Exige uma consulta de dados anteriores separada. | Recomendada nesta spec. |
| Converter automaticamente todos os dados antigos | Uniformiza o histórico, mas acrescenta migração de conteúdos incompletos antes de validar as sessões. | Adiada; não é necessária para este incremento. |

As escolhas seguintes são propostas desta spec, sujeitas a revisão:

- Escolher inicialmente «Todas as categorias»; permitir também uma categoria
  individual, sem seleção múltipla neste incremento.
- Sortear perguntas elegíveis sem repetição. Não otimizar diversidade ou
  evitar perguntas de sessões anteriores nesta fase.
- Separar «Guardar revisão», «Confirmar resposta» e «Próxima pergunta».
  Confirmar fixa o texto; avançar é outra ação explícita.
- Permitir sair e retomar a única sessão em curso; não introduzir abandono,
  salto de perguntas ou várias entrevistas simultâneas nesta fase.
- Manter respostas da fase 1 acessíveis numa consulta de «Treinos anteriores
  à fase 2», sem as converter em entrevistas ou confirmar textos retroativamente.

## 4. Banco de perguntas

### Estrutura e edição

Cada pergunta tem identificador estável, texto em inglês, uma das seis
categorias, uma ou mais competências e um ou mais critérios de conteúdo.
Datas e versão permitem identificar alterações. Os critérios orientam a futura
avaliação textual; nesta fase são apenas guardados e apresentados.

O formulário exige texto, categoria, pelo menos uma competência e um critério
não vazios. Ignorar espaços exteriores na validação e persistência; não alterar
o conteúdo interior. Não depender de deteção automática da língua para aceitar
o texto: os campos indicam que a pergunta deve estar em inglês. A interface e
as explicações são em português de Portugal.

Permitir adicionar e editar. Eliminação, importação em lote, edição das seis
categorias e gestão autónoma de um catálogo de competências ficam fora deste
incremento. Competências e critérios são editados na própria pergunta.

Inserir o banco inicial uma única vez, com identificadores estáveis e versão
de conteúdo. Reiniciar ou atualizar a aplicação não repõe perguntas editadas,
não duplica registos e não volta a inserir o banco sobre alterações pessoais.

### Conteúdo inicial proposto

Os critérios aceitam resultados qualitativos, negativos e aprendizagens.
Nenhuma pergunta exige números ou um sucesso. Não fornecer respostas modelo
ao transcritor.

| # | Categoria | Pergunta em inglês | Competência | Critério específico |
| --- | --- | --- | --- | --- |
| 1 | Conflitos e colaboração | Tell me about a time you disagreed with a colleague. | Gestão de conflitos | Explicar a divergência, a ação própria e o desfecho. |
| 2 | Conflitos e colaboração | Describe a time you helped a team work together more effectively. | Colaboração | Identificar o obstáculo e a contribuição individual. |
| 3 | Conflitos e colaboração | Tell me about a time you had to explain a technical issue to someone without a technical background. | Comunicação | Explicar a adaptação da mensagem e como verificou a compreensão. |
| 4 | Conflitos e colaboração | Describe a time you worked with someone whose approach was different from yours. | Colaboração | Explicar como coordenaram o trabalho e o que aconteceu. |
| 5 | Liderança e iniciativa | Tell me about a time you took initiative without being asked. | Iniciativa | Distinguir a decisão própria e o efeito ou aprendizagem. |
| 6 | Liderança e iniciativa | Describe a time you helped someone develop a skill. | Liderança | Identificar a necessidade, o apoio prestado e o resultado observado. |
| 7 | Liderança e iniciativa | Tell me about a time you influenced a decision without formal authority. | Influência | Explicar os argumentos e a contribuição para a decisão. |
| 8 | Falhas e aprendizagens | Tell me about a time you made a mistake at work. | Responsabilidade | Reconhecer o papel próprio, a reação e a aprendizagem. |
| 9 | Falhas e aprendizagens | Describe a time you received feedback that was difficult to hear. | Aprendizagem | Explicar a reação e como usou ou ponderou o feedback. |
| 10 | Falhas e aprendizagens | Tell me about a time an approach you tried did not work. | Reflexão | Explicar o que falhou e a mudança ou aprendizagem seguinte. |
| 11 | Resolução de problemas | Tell me about a time you solved a difficult problem at work. | Resolução de problemas | Explicar o problema, o raciocínio, as ações próprias e o desfecho. |
| 12 | Resolução de problemas | Describe a time you investigated a problem with incomplete information. | Análise | Explicar como recolheu informação e lidou com a incerteza. |
| 13 | Resolução de problemas | Tell me about a time you had to choose between different solutions. | Decisão | Explicitar alternativas, compromisso e consequência. |
| 14 | Resolução de problemas | Describe a time you improved a process that was causing problems. | Melhoria de processos | Distinguir o problema inicial, a mudança e o efeito observado. |
| 15 | Prioridades e prazos | Tell me about a time you had competing deadlines. | Priorização | Explicar critérios de prioridade, comunicação e desfecho. |
| 16 | Prioridades e prazos | Describe a time you realised you might miss a deadline. | Gestão de expectativas | Explicar quando identificou o risco e como atuou. |
| 17 | Prioridades e prazos | Tell me about a time you had to reduce the scope of a task. | Gestão de âmbito | Justificar a escolha e as consequências para o objetivo. |
| 18 | Adaptação a mudanças | Tell me about a time your priorities changed unexpectedly. | Adaptação | Explicar a mudança e como reorganizou o trabalho. |
| 19 | Adaptação a mudanças | Describe a time you had to learn a new tool or process quickly. | Aprendizagem | Explicar a necessidade, a estratégia e o resultado. |
| 20 | Adaptação a mudanças | Tell me about a time you worked through a major change in your team. | Gestão da mudança | Descrever a mudança, a ação individual e a aprendizagem. |

Guardar também critérios STAR comuns: contexto, responsabilidade individual,
ações, resultado ou aprendizagem. A cópia da sessão inclui os critérios comuns
e específicos efetivos, sem depender da versão futura do banco.

## 5. Preparar e iniciar uma sessão

Mostrar categoria, quantidade pretendida e número de perguntas elegíveis.
Aceitar apenas inteiros entre um e o total elegível. Se existirem menos de
três, conservar a sugestão de três com aviso e exigir que o utilizador ajuste
a quantidade; não reduzir o total nem repetir perguntas silenciosamente.

Ao iniciar, o núcleo valida novamente a disponibilidade, sorteia e guarda,
numa transação, a sessão, a configuração, os identificadores de origem, a ordem
e as cópias do texto, categoria, competências e critérios de cada pergunta.
Só depois de persistir apresenta a primeira pergunta como iniciada.

Cliques repetidos no mesmo pedido não criam várias sessões. Se já existir uma
sessão em curso, apresentar a opção de a retomar. Uma sessão concluída permite
iniciar outra. Edições posteriores no banco afetam apenas sessões futuras.

## 6. Responder, rever e confirmar

### Áudio e transcrição

Mostrar «Pergunta 2 de 3» e o texto inglês fixado. Disponibilizar síntese e
repetição com Kokoro; uma falha de síntese mantém o texto e permite responder.
A cache distingue texto e configuração da voz, sem reutilizar áudio de outra
pergunta. Reutilizar a captura e os formatos validados na fase 1.

O utilizador inicia e termina a gravação. Iniciar captura interrompe a saída;
nenhum áudio toca durante a captura. Guardar o original antes de conversões e
transcrição. Preservar o derivado separadamente. Identificar gravações
incompletas e permitir ouvi-las antes de decidir se é preciso gravar de novo.

Cada gravação tem identidade própria. Antes da confirmação, «Gravar novamente»
cria outro registo para a mesma pergunta, preservando os anteriores, sem os
contar como perguntas extra. A gravação nova só substitui a seleção corrente
depois de finalizada e guardada; uma falha de captura mantém a anterior.
As gravações anteriores continuam consultáveis. A confirmação usa apenas a
gravação selecionada e a revisão que lhe pertence.

Falha de transcrição permite repetir sobre o ficheiro existente. Cada resultado
automático é imutável e identificado. Reprocessar não substitui revisões
guardadas nem muda automaticamente o resultado de origem de uma revisão.

### Revisão e confirmação

Apresentar o texto automático apenas de leitura e uma cópia editável. Guardar
a revisão mantém o texto editável; não confirma nem avança. A confirmação
cria um registo separado com o texto exato revisto, data e referências à
pergunta, gravação, transcrição e revisão de origem.

«Confirmar resposta» informa que o texto ficará fixo. O núcleo recebe a versão
esperada e confirma exatamente a versão que o utilizador viu. Persistir revisão
e confirmação de forma atómica; uma escrita falhada ou versão desatualizada
não confirma nem avança. Enquanto o pedido estiver pendente, impedir edição e
troca de gravação. Repetir o pedido não cria outra confirmação.

Texto vazio ou apenas com espaços não pode ser confirmado. Quando não houver
fala reconhecida, permitir nova gravação ou introdução explícita de uma revisão
fiel ao áudio existente; registar que o texto foi introduzido manualmente e
preservar o resultado `no_speech`. Não aceitar resposta sem um original
finalizado nem apresentar silêncio como problema de pronúncia.

Depois de confirmar, a resposta fica apenas de leitura. Não disponibilizar
desfazer confirmação, nova captura ou retranscrição dessa resposta nesta fase.
As futuras novas tentativas serão independentes.

### Avanço e final

«Próxima pergunta» só fica disponível com a resposta atual confirmada. Rust
valida a sessão, a posição e a versão esperadas; repetir o comando não salta
perguntas. Não permitir avançar uma pergunta sem resposta confirmada.

Na última pergunta, a ação chama-se «Terminar sessão». Só conclui quando todas
as perguntas previstas têm resposta confirmada. Mostrar um resumo consultável,
sem avaliação, nota ou feedback pendente. Guardar que a sessão foi realizada
no modo local da fase 2: não criar dispensas fictícias de seguimento nem pedidos
de IA para estas sessões quando a fase 3 for instalada.

## 7. Navegação, histórico e retoma

A interface disponibiliza preparação/retoma, banco de perguntas, entrevista e
histórico. Os detalhes visuais ficam para o plano técnico.

Listar sessões com data, categoria, quantidade, progresso e estado em curso ou
concluída. Abrir uma sessão permite ler as perguntas fixadas, ouvir originais,
consultar resultados automáticos, revisão guardada e texto confirmado em áreas
distintas. Respostas confirmadas permanecem de leitura. Numa sessão em curso,
a ação de retoma conduz à etapa persistida, incluindo uma resposta confirmada
que ainda aguarda avanço explícito.

Não mudar de sessão ou de página durante captura/finalização. Durante um
trabalho de modelo é possível consultar conteúdos guardados; essa navegação
não muda a identidade do trabalho nem inicia outro. Voltar à entrevista mostra
o estado real do núcleo.

Ao sair do editor ou fechar normalmente, resolver alterações com
guardar/descartar/cancelar. Só navegar ou fechar se a versão mais recente estiver
guardada ou tiver sido explicitamente descartada. Se houver edição durante uma
escrita, a versão nova continua por guardar. Falhas mantêm o editor e o texto.
No fecho com captura ativa, tentar finalizar e guardar antes de sair, conforme
o contrato da fase 1. Não prometer salvar áudio não finalizado numa falha abrupta.

Ao reiniciar, recuperar a única sessão em curso e a última etapa guardada.
Trabalhos interrompidos passam a estado recuperável; não ficam em processamento
para sempre e não retomam a captura automaticamente. Uma sessão concluída abre
no histórico, sem nova gravação ou avanço automático.

## 8. Persistência e compatibilidade

### Dados das novas sessões

SQLite é a autoridade para banco, cópias das perguntas, sessões, posição,
gravações, transcrições, revisões, confirmações e operações. Os ficheiros de
áudio ficam na pasta de dados da aplicação, separados dos modelos e do código.
Não manter JSON e SQLite como duas autoridades das mesmas sessões novas.

O modelo lógico distingue:

| Registo | Conteúdo e invariantes |
| --- | --- |
| Pergunta do banco | Conteúdo atual, categoria, competências, critérios e versão. |
| Sessão | Configuração, modo local, estado, posição, datas e versão de concorrência. |
| Pergunta da sessão | Posição única e cópia completa dos conteúdos à data de início. |
| Gravação | Pergunta da sessão, ficheiros, metadados, integridade e seleção corrente. |
| Transcrição automática | Gravação de origem, texto original, estado, modelo/configuração e tempos. |
| Revisão | Texto guardado, versão, origem automática ou manual explícita. |
| Confirmação | No máximo uma por pergunta da sessão; texto e referências imutáveis. |
| Operação | Identidade, alvo, geração, estado, erro e tempos. |

Guardar os metadados já produzidos na fase 1 sem perder informação. Pormenores
de tabelas e normalização ficam para o plano técnico; não criar já tabelas
vazias para histórias, feedback ou custos.

Criar sessão com perguntas, confirmar resposta e avançar são transações.
As restrições do núcleo e da base de dados impedem confirmações duplicadas,
posições repetidas e mais de uma sessão em curso.

SQLite e ficheiros não partilham uma transação. Finalizar o áudio antes de
publicar a referência; se a publicação falhar, conservar o ficheiro com
identidade recuperável e permitir repetir sem gravar de novo. No arranque,
reconciliar publicações interrompidas, sem duplicar registos nem apagar áudios
órfãos automaticamente. Ficheiros em falta ou inválidos produzem aviso por
artefacto; não impedem ler os restantes textos.

Versionar o esquema e executar migrações transacionais. Uma falha não recria
nem limpa a base existente; mostrar erro recuperável e preservar os dados.
Uma base de versão futura não é aberta para escrita por uma versão antiga.

### Dados da fase 1

Conservar a árvore `phase1/`, a configuração pessoal, a cache e os modelos.
Disponibilizar uma listagem de treinos anteriores a partir dos manifestos
existentes, reutilizando a leitura de originais, transcrições e revisão.
A consulta é apenas de leitura e não depende só de `last-attempt.json`.

Não fabricar categorias, critérios, sessões concluídas ou confirmações para
dados antigos. Identificar itens inválidos individualmente e continuar a listar
os válidos. Não os duplicar em SQLite, mover ou apagar neste incremento.

## 9. Operações e recuperação

Rust mantém a autoridade sobre sessão e versões; React apresenta o estado.
Não bloquear a interface durante áudio, modelos ou acesso demorado aos dados.
Manter um único trabalho local de modelo e uma única captura de cada vez.

Todo o resultado e evento de áudio é associado à sessão, pergunta, gravação e
operação que o originou, incluindo geração do dispositivo/auxiliar quando
necessário. Um evento antigo, mesmo sem áudio finalizado, não pode cancelar
uma captura nova nem remover a sua ação de terminar.

| Falha | Recuperação exigida |
| --- | --- |
| Pedido repetido ou incompatível | Devolver estado atual ou erro de ocupação sem bloqueio e sem duplicar efeitos. |
| Microfone interrompido | Recuperar o que puder ser finalizado, avisar sobre incompletude e permitir nova captura. |
| Transcrição falhada ou auxiliar bloqueado | Preservar áudio/revisão; libertar operação e permitir repetir. |
| Síntese indisponível | Manter a pergunta legível e permitir responder sem voz sintetizada. |
| Escrita de revisão ou confirmação falhada | Preservar texto, não confirmar e não avançar. |
| Avanço falhado | Manter confirmação e posição persistida, permitindo repetir com segurança. |
| Reinício após confirmação | Recuperar a confirmação sem a duplicar; manter o avanço ainda pendente. |
| Resultado tardio ou de versão antiga | Não modificar seleção atual, confirmação ou progresso. |
| Áudio em falta | Mostrar indisponibilidade desse áudio e conservar textos e histórico. |
| SQLite indisponível | Não iniciar nem confirmar sessões como se estivessem guardadas; permitir repetir a abertura. |

Reutilizar os prazos e limites de áudio existentes, salvo alteração justificada
no plano. Não truncar áudio ou texto silenciosamente. Não existem pedidos de
rede, chaves ou custos de IA nesta fase; não apresentar consumo zero como uma
medição de um fornecedor que não foi chamado.

## 10. Verificação e critérios de aceitação

O plano técnico deve testar as transições e persistência no núcleo, a navegação
e as proteções de edição na interface e a integração com os adaptadores locais.
Usar dados temporários e falhas controladas para testar interrupções entre
publicação de áudio, confirmação e avanço. Preservar a regressão da fase 1.

Os critérios seguintes continuam a ser os de aceitação. Em 2026-11-02 a
implementação e a verificação automática foram registadas em
[docs/phase-2/validation.md](../../phase-2/validation.md): F2-01, F2-02, F2-04,
F2-06, F2-07, F2-08, F2-09, F2-12, F2-13 e F2-15 têm evidência automatizada;
F2-03, F2-05, F2-10, F2-11, F2-14 e F2-16 têm evidência parcial e dependem da
validação manual. As caixas abaixo só são marcadas quando o critério estiver
demonstrado, incluindo a validação manual.

- [ ] F2-01: Banco inicial contém 20 perguntas nas seis categorias; repetir o arranque não duplica nem repõe edições.
- [ ] F2-02: Adicionar e editar texto, categoria, competências e critérios persiste após reinício, com validação dos campos obrigatórios.
- [ ] F2-03: Quantidade inicial três; sessões com uma, três e outra quantidade elegível; insuficiência explícita, sem redução ou repetição silenciosa.
- [ ] F2-04: Seleção e ordem sem repetição persistem; editar o banco não altera nenhuma cópia de sessão existente.
- [ ] F2-05: O utilizador completa três perguntas com voz, captura manual e progresso correto; falha de síntese permite responder pelo texto.
- [ ] F2-06: Áudio original, derivado, resultados automáticos, revisão e confirmação permanecem distintos e rastreáveis.
- [ ] F2-07: Guardar não confirma; confirmar fixa exatamente o texto visto; falha, versão antiga ou pedido repetido não cria confirmação indevida.
- [ ] F2-08: Não é possível avançar sem confirmar, saltar por duplo clique ou terminar antes de todas as confirmações; repetir avanço é seguro.
- [ ] F2-09: Nova captura antes da confirmação preserva anteriores; falha da nova captura não perde a seleção válida. Resposta confirmada não é reaberta.
- [ ] F2-10: Fecho e navegação com alterações resolvem guardar/descartar/cancelar e não perdem edição feita durante uma escrita.
- [ ] F2-11: Reiniciar durante transcrição, após guardar revisão, após confirmar e após avançar recupera a etapa persistida, sem duplicações.
- [ ] F2-12: Falhas de escrita/publicação permitem recuperar sem nova gravação; referências em falta não apagam textos nem bloqueiam outros itens.
- [ ] F2-13: Pedidos repetidos não bloqueiam o serviço; eventos e resultados antigos não afetam gravações novas ou confirmações.
- [ ] F2-14: Histórico permite consultar sessões e ouvir originais; treinos da fase 1 continuam consultáveis sem alteração dos seus ficheiros.
- [ ] F2-15: Migração falhada preserva a base; versão futura não é escrita; apenas uma sessão permanece em curso.
- [ ] F2-16: Com modelos preparados, a sessão completa funciona offline, sem chamadas à IA, dispensas fictícias ou feedback inventado.

### Validação manual desta fase

No Windows do utilizador, completar uma sessão de três perguntas com respostas
reais, incluindo termos técnicos. Ouvir perguntas e respostas, corrigir e
confirmar textos, sair e retomar a meio e consultar o histórico após terminar.
Completar também uma sessão de uma pergunta e verificar a insuficiência numa
categoria. Editar uma pergunta após iniciar a sessão e confirmar que a cópia
permanece igual. Repetir o percurso offline.

Registar duração dos áudios, carregamento, síntese, transcrição, experiência
de espera e eventuais falhas. Documentar comandos realmente executados e
resultados em `docs/phase-2/` durante a implementação, sem inventar resultados
antecipados. Os testes automáticos não substituem esta experiência.

## 11. O que fica para o plano técnico

- Biblioteca Rust de SQLite, versão, acesso sem bloquear a interface e migrações.
- Esquema físico, índices, restrições, identificadores e controlo de concorrência.
- Organização de módulos, contratos Tauri e adaptação do serviço de áudio à sessão.
- Estrutura de ficheiros e reconciliação de publicações interrompidas.
- Componentes, navegação e mensagens, respeitando o fluxo desta spec.
- Testes de transações, concorrência, reinício e compatibilidade com os artefactos antigos.

Não instalar dependências nem iniciar implementação ao criar esta spec. A
aprovação do documento permite preparar o plano técnico apenas da fase 2;
não autoriza começar as fases seguintes.

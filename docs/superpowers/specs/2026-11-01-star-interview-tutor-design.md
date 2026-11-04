# Tutor de entrevistas STAR — desenho do MVP

Data: 2026-11-01  
Estado: desenho aprovado pelo utilizador; execução organizada no [plano das fases](../plans/2026-11-01-star-interview-tutor-phases.md).  
Stack escolhida pelo utilizador: Rust + React + Tauri.

## 1. Intenção

Criar uma aplicação desktop de uso pessoal para praticar entrevistas comportamentais em inglês por voz. O utilizador deve conseguir contar as suas experiências profissionais com clareza, organizar respostas segundo STAR e melhorar o inglês através de sugestões concretas e novas tentativas.

O ciclo principal é: ouvir uma pergunta, responder por voz, rever a transcrição, responder a um seguimento quando necessário, consultar feedback e repetir uma resposta.

As gravações e o histórico ficam no computador. A avaliação textual usa um modelo na nuvem através da API do OpenCode. Não existe avaliação automática da pronúncia neste MVP.

## 2. Decisões e propostas técnicas

### Decisões do utilizador

- Interface desktop com Rust, React e Tauri.
- Treino em inglês com estrutura STAR, banco de perguntas editável e histórias profissionais pessoais.
- Quantidade configurável de perguntas principais; sugestão inicial de três.
- Gravação iniciada e terminada manualmente, sem interrupções automáticas.
- Transcrição local com faster-whisper e voz local com Kokoro-82M.
- Avaliação e seguimentos com DeepSeek Flash através do OpenCode, com modelo configurável.
- Revisão da transcrição antes do envio; feedback no fim da entrevista.
- No máximo um seguimento por pergunta principal, sem o contar no total configurado.
- Histórico local, preservação de versões, novas tentativas e eliminação das sessões com os respetivos áudios.
- Feedback com evidências, sem nota global e sem inventar factos.

### Propostas de implementação deste desenho

- Tauri 2, React com TypeScript e Vite; não é necessário um servidor web de produção.
- Windows como primeiro alvo de desenvolvimento e distribuição, dado o ambiente atual. Suporte a macOS e Linux fica para validação posterior.
- Português na interface e nas explicações; perguntas, seguimentos, respostas sugeridas e respetiva voz em inglês.
- Um processo Python auxiliar, iniciado pela aplicação, para transcrição e síntese. Evitar uma migração simultânea dos motores de áudio para Rust.
- Rust como autoridade sobre o estado da sessão, SQLite, ficheiros, credenciais e pedidos à API.
- Nível de inglês configurável para orientar reformulações; B1 como valor inicial editável, sem o apresentar como diagnóstico do nível do utilizador.
- Uma sessão ativa de cada vez. Os trabalhos locais de áudio são processados sequencialmente no MVP.
- Depois da confirmação, a transcrição dessa resposta fica fixa. Novas tentativas são independentes e não reabrem decisões da entrevista original.
- Na falha de um pedido de seguimento, é necessário repetir o pedido ou dispensar explicitamente o seguimento antes de avançar. Não apresentar seguimentos atrasados fora da ordem da entrevista.

Estas propostas podem ser ajustadas na revisão do desenho; não são apresentadas como preferências já expressas pelo utilizador.

## 3. Abordagens consideradas

| Abordagem | Vantagens neste projeto | Compromissos | Decisão |
| --- | --- | --- | --- |
| Python + PySide6 | Integração direta com os motores Python; concentra a aplicação numa linguagem. | Interface no ecossistema Qt. | Considerada, não escolhida. |
| Electron + React | Interface web e núcleo Node.js/TypeScript; adequado ao chat e à revisão textual. | Distribuição de Chromium e integração adicional com o processo de áudio. | Considerada, não escolhida. |
| .NET com WPF ou Avalonia | Permite aplicação C#; pode chamar Python ou outros motores de transcrição. | Escolha adicional de toolkit; WPF limitado a Windows. | Considerada, não escolhida. |
| Tauri + React + processo Python | Interface React, núcleo Rust e manutenção dos motores de áudio propostos. | Três linguagens e empacotamento do processo auxiliar; testar a integração com a WebView. | Escolhida a base Rust/React/Tauri; processo Python é a proposta técnica inicial. |

O Tauri usa a WebView da plataforma. A dimensão e o consumo da aplicação completa dependerão também dos modelos locais; não se assume um ganho de desempenho na inferência apenas por usar Rust.

## 4. Âmbito e exclusões

### Incluído

- Configuração de categoria, número de perguntas, nível pretendido para as sugestões e voz em inglês.
- Banco inicial de cerca de 20 perguntas, com edição e adição pelo utilizador.
- Banco simples de histórias pessoais, com título, notas factuais STAR e competências associadas.
- Perguntas faladas, captura e reprodução de respostas, transcrição e revisão manual.
- Um seguimento contextual quando útil, feedback final e reformulação com áudio.
- Novas tentativas de uma resposta, com comparação e feedback específico sobre a tentativa.
- Histórico, consumo de tokens, estimativa de custo e recuperação de pedidos que falharam.
- Configuração da chave da API e preparação dos modelos locais.

### Fora do MVP

- Diagnóstico de pronúncia, pontuação por fonema, avaliação acústica da fluência ou inferência de confiança emocional.
- Interrupções durante a resposta, conversa simultânea ou transcrição em direto.
- Outras línguas de treino, sincronização, contas, colaboração e aplicação móvel.
- Treino de modelos, pesquisa vetorial, agentes com ferramentas ou seleção autónoma ilimitada de perguntas.
- Avaliação certificada de proficiência ou garantia de precisão das classificações.

Uma transcrição correta não comprova pronúncia correta. A LLM recebe texto e não deve afirmar que ouviu ou avaliou a execução oral.

## 5. Experiência e regras da sessão

### Preparação

O utilizador escolhe uma categoria e a quantidade de perguntas principais. O valor inicial é três, sendo possível escolher uma ou outra quantidade até ao número de perguntas elegíveis existentes. A aplicação informa quando faltam perguntas para a quantidade pretendida, sem repetir silenciosamente a mesma pergunta.

Ao iniciar, Rust fixa a seleção e a ordem das perguntas e guarda um retrato dos respetivos textos e critérios. Alterações posteriores no banco não modificam uma entrevista existente. A interface apresenta o progresso das perguntas principais.

### Pergunta e resposta

1. Apresentar o texto e sintetizar a pergunta em inglês; permitir ouvir novamente.
2. Iniciar a gravação apenas por ação do utilizador; terminar por ação explícita. A pergunta não é reproduzida enquanto o microfone está a gravar.
3. Guardar o áudio antes de iniciar a transcrição. Uma falha de transcrição permite repetir o processamento do mesmo ficheiro.
4. Guardar a transcrição automática como versão imutável e criar uma versão revista editável. Permitir ouvir a gravação durante a revisão.
5. Só enviar a resposta à API quando o utilizador confirma a transcrição revista. Este ato confirma o texto a avaliar, não melhora nem substitui a resposta original.

A transcrição revista permanece editável até à confirmação. Depois de confirmada, essa versão é apenas de leitura no MVP. Para melhorar ou retificar uma resposta já confirmada, o utilizador inicia uma nova tentativa independente a partir da pergunta; as decisões de seguimento e o relatório da entrevista original mantêm as versões e evidências de origem. Uma nova tentativa não altera a resposta corrente de uma entrevista em curso.

Áudio vazio, silêncio sem fala reconhecida ou transcrição vazia exigem nova gravação ou revisão explícita; não são enviados como uma resposta válida nem classificados como má pronúncia.

### Seguimento

A LLM recebe a pergunta, os critérios, a resposta revista e o contexto textual relevante da sessão. Recomenda continuar ou fazer uma única pergunta de seguimento ligada a uma lacuna real. Rust valida a recomendação e aplica o limite.

O seguimento deve esclarecer conteúdo, não interromper a entrevista para ensinar gramática. Se a resposta já for suficiente, a aplicação avança. Se a geração falhar, a sessão conserva o estado e permite repetir o pedido ou avançar explicitamente sem seguimento. Avançar regista o seguimento como dispensado pelo utilizador por indisponibilidade do serviço, em vez de afirmar que a IA considerou a resposta completa. Essa decisão encerra a etapa de seguimento dessa pergunta; não a reabrir quando a rede regressa. Enquanto aguarda uma decisão ou uma dispensa explícita, a sessão não avança para a próxima pergunta principal.

Uma resposta ao seguimento passa pela mesma captura e revisão, mas nunca origina outro seguimento. No máximo, uma sessão com N perguntas principais tem 2N perguntas no total.

### Final e novas tentativas

Depois da última resposta confirmada e da decisão sobre o último seguimento, a entrevista passa a aguardar feedback. A aplicação pode pedir um relatório por resposta principal com o respetivo seguimento e agregar o feedback da sessão; o formato interno dos pedidos fica para o plano.

No histórico, o utilizador escolhe uma pergunta para repetir. Pode consultar o feedback, ouvir a reformulação e gravar outra resposta. Cada tentativa é guardada separadamente, recebe avaliação textual segundo os mesmos critérios e pode ser comparada com a resposta inicial. O relatório original permanece inalterado. As novas tentativas são exercícios individuais sem novos seguimentos automáticos no MVP.

## 6. Perguntas, histórias e contexto

O banco inicial cobre conflitos e colaboração, liderança e iniciativa, falhas e aprendizagens, resolução de problemas, prioridades e prazos e adaptação a mudanças.

Cada pergunta guarda texto em inglês, categoria, competências e critérios. Exemplos são opcionais e ilustrativos; não constituem respostas obrigatórias.

Cada história guarda título, notas factuais para situação, responsabilidade, ações e resultado ou aprendizagem, além de competências. O utilizador pode selecionar uma história como apoio a uma pergunta; a associação é opcional. Não é necessária pesquisa automática entre todas as histórias.

Na avaliação, as notas da história não são fornecidas como parte da resposta falada. A avaliação usa exclusivamente a pergunta e os textos revistos efetivamente respondidos. Isso evita atribuir ao utilizador informação que não explicou.

Para reformular uma resposta, o MVP usa apenas os factos presentes nessa resposta e no seu seguimento. As notas guardadas ajudam na preparação, mas informação que só existe nessas notas aparece como sugestão de algo que o utilizador pode acrescentar numa futura tentativa, nunca como algo que já disse.

O contexto das chamadas é limitado ao necessário: critérios da pergunta, textos revistos, versões da rubrica e, quando relevante para o seguimento, um resumo factual das respostas anteriores. Não enviar áudios nem o banco inteiro de histórias. A pergunta atual tem prioridade sobre referências a perguntas anteriores.

## 7. Contrato do feedback

Separar conteúdo/STAR de inglês na transcrição revista.

- Conteúdo: relevância, situação, responsabilidade individual, ações, resultado e aprendizagem.
- Inglês: gramática, vocabulário, clareza, organização e naturalidade textual.
- Usar claro, parcial e não identificado quando aplicável, com explicações. Não criar uma nota global.
- Associar cada observação factual a uma citação e ao identificador da resposta ou seguimento de onde foi retirada.
- Rust valida que citações se encontram na versão do texto avaliada. Rejeitar observações sem evidência válida em vez de as apresentar como confirmadas.
- Distinguir o que estava presente na resposta inicial do que só apareceu no seguimento.
- Aceitar resultados qualitativos, resultados negativos e aprendizagens. Não exigir métricas numéricas.
- Apresentar pontos fortes, lacunas e três prioridades concretas para o próximo treino, sem inventar defeitos para completar a lista. Se só houver uma ou duas prioridades sustentadas, apresentar apenas essas e explicar que não foram identificadas outras.
- Separar correções linguísticas necessárias de alternativas opcionais de estilo.

A reformulação mantém os factos, a responsabilidade e o grau de certeza originais. Não inventa números, sucessos ou experiências. Informação em falta fica assinalada fora do texto para reprodução por voz, para não produzir frases artificiais com marcadores falados.

As saídas da LLM seguem um esquema estruturado validado em Rust. Conteúdo de respostas e histórias é tratado como material a avaliar, não como instruções para mudar a rubrica ou as regras da sessão. Saídas inválidas podem ser pedidas novamente; as versões anteriores não são sobrescritas.

## 8. Arquitetura

### Interface React/TypeScript

Apresenta configurações, banco de perguntas, histórias, entrevista, revisão da transcrição, feedback, comparação de tentativas e histórico. Chama comandos Tauri e recebe eventos de progresso. Não contém a chave da API nem decide transições persistentes da sessão.

### Núcleo Rust/Tauri

- Gere estado da sessão, progresso, limite de seguimentos e operações pendentes.
- Acede ao SQLite e aos ficheiros na pasta de dados da aplicação.
- Captura áudio através de um adaptador local e fornece ficheiros ao processo de transcrição. A biblioteca concreta é uma decisão do plano, validada na integração Windows.
- Gere credenciais através do armazenamento protegido do sistema operativo, fora da base de dados de conteúdo e dos registos.
- Faz chamadas HTTP ao OpenCode com prazo limite, validação da resposta e registo de consumo.
- Inicia, acompanha e encerra o processo Python. Trabalhos de áudio e rede não bloqueiam a interface.

### Processo Python de áudio

Executa transcrição com faster-whisper e síntese com Kokoro-82M. Mantém modelos carregados quando possível; processa pedidos sequencialmente para limitar a concorrência e facilitar a recuperação.

Comunica com Rust por mensagens JSON delimitadas por linha através de entrada/saída padrão, com identificador de pedido, operação, parâmetros, progresso, resultado e erro. Os registos de diagnóstico seguem para stderr. Os áudios são ficheiros locais, referenciados por caminhos controlados pelo núcleo; não são transportados em base64 por este canal.

O contrato expõe apenas estado de disponibilidade, transcrição, síntese e encerramento. A comunicação não precisa de abrir uma porta HTTP local.

### Áudio, modelos e distribuição

- Normalizar localmente o áudio entregue à transcrição para WAV PCM de 16 bits, mono, 16 kHz; conservar a gravação original e tratar a conversão como derivado.
- Guardar saída da síntese como ficheiro de áudio reproduzível localmente.
- Propor faster-whisper com modelo small multilingue em CPU/int8 como referência inicial ajustável. A escolha final depende da validação com o hardware e as gravações reais, sem prometer latência.
- Usar uma voz inglesa do Kokoro, com velocidade ajustável. A voz e dependências concretas são validadas no empacotamento Windows.
- Distribuir o processo auxiliar com as dependências de execução; o utilizador final não precisa de preparar manualmente um ambiente Python.
- Não incluir automaticamente todos os pesos no instalador. Na preparação inicial, apresentar os modelos necessários e disponibilizar download com progresso ou seleção de ficheiros locais existentes, segundo as opções do motor.
- Depois de preparados, áudio e transcrição funcionam sem rede. Seguimentos e avaliação exigem ligação ao fornecedor.
- Validar a gravação e reprodução na WebView e no adaptador de áudio efetivamente usados. Validar o pacote instalado, além da execução em desenvolvimento.

## 9. OpenCode, custos e configuração

Usar a API de inferência do OpenCode diretamente a partir de Rust; não é necessário instalar ou executar o agente/CLI OpenCode no tutor.

Configuração inicial proposta, verificada no catálogo e documentação oficiais em 2026-11-01:

- Modelo pago: `deepseek-v4.1-flash`.
- Endpoint: `https://opencode.ai/inference/openai/v1/chat/completions`.
- Catálogo: `https://opencode.ai/inference/v1/models`.
- Identificador do modelo e URL base configuráveis; não depender de identificadores com sufixo `-free`.

O fornecedor documenta que modelos gratuitos de chat são restritos a utilização dentro do OpenCode e podem responder 403 a clientes externos. Este desenho usa o modelo pago.

> **Alteração de 2026-11-04:** a modalidade atual é OpenCode Go, com endpoint
> `https://opencode.ai/inference/go/openai/v1/chat/completions`, após autorização
> explícita do utilizador. Ver [desenho Go](2026-11-04-opencode-go-design.md) e
> [validação](../../opencode-go/validation.md).

A tabela consultada apresenta US$0,30 por milhão de tokens de entrada e US$1,20 de saída, com preço distinto para leitura de cache. Estes valores são uma referência datada, não uma garantia nem um preço fixo no código. Guardar a versão/data do tarifário usado na estimativa. Se não for possível distinguir tokens em cache, usar a tarifa de entrada normal como estimativa conservadora.

Exemplo hipotético: 20.000 tokens de entrada e 4.000 de saída equivalem a US$0,0108, antes de outros encargos. Cem sessões iguais corresponderiam a US$1,08. Confirmar os valores na conta do fornecedor e medir sessões reais.

Registar modelo efetivamente devolvido, tokens e estimativa por pedido e sessão. Incluir pedidos de seguimento, relatório, reformulação e avaliação de novas tentativas. Se o fornecedor não devolver consumo, apresentar como desconhecido; não mostrar zero como medição real. A estimativa não substitui a faturação do fornecedor.

## 10. Persistência e recuperação

Entidades mínimas:

- Pergunta e critérios, com categoria e competências.
- História pessoal e competências.
- Sessão, configurações e estado persistente.
- Pergunta da sessão, com texto e critérios fixados no início e posição.
- Resposta inicial, seguimento e resposta ao seguimento, associados explicitamente.
- Tentativa, com relação à pergunta e à tentativa anterior quando aplicável.
- Áudio original, transcrição automática, versão revista confirmada e ficheiros derivados.
- Avaliação e reformulação, com versões da rubrica/prompt e textos de origem.
- Pedido à IA, finalidade, estado, modelo, consumo e tarifário usado.

As transcrições automáticas, rascunhos de revisão, versões confirmadas enviadas e reformulações são distintas. Alterar o rascunho antes da confirmação não altera a transcrição automática. Após confirmação, a versão enviada fica fixa. Novas tentativas criam novos registos e avaliações; avaliações existentes mantêm a referência ao texto que avaliaram.

Guardar áudio e textos antes da chamada externa. Cada operação persistente tem identificador próprio. Aplicar um resultado apenas à versão e ao estado que originaram o pedido; um pedido repetido não pode inserir dois seguimentos nem avançar duas vezes a sessão.

Depois de falha de rede ou reinício da aplicação, recuperar a última etapa guardada e permitir repetir o pedido. Uma gravação interrompida por falha do processo pode ter de ser refeita; não prometer recuperação de áudio que ainda não foi finalizado e guardado.

Uma repetição de pedido após timeout pode ter custos adicionais se o fornecedor já tiver processado o anterior. Registar o consumo conhecido e assinalar o estado incerto, sem prometer execução exatamente uma vez no fornecedor.

Eliminar uma sessão remove as respetivas respostas, versões, avaliações, pedidos e áudios/derivados exclusivos. Não remove perguntas do banco, histórias pessoais nem modelos partilhados. Falhas ao remover ficheiros ficam sinalizadas para nova tentativa; não indicar eliminação completa enquanto restarem ficheiros da sessão.

## 11. Qualidade, falhas e limites de confiança

- Guardar gravações finalizadas antes de operações demoradas e comunicar o estado atual ao utilizador.
- Distinguir falha de microfone, falha de modelo, falta de modelo local, falha de rede, credencial inválida, limites do fornecedor e resposta inválida da IA.
- Permitir repetir transcrição, reprodução e pedidos externos sem nova gravação.
- Perder um processo Python não deve encerrar a aplicação nem apagar respostas finalizadas.
- Não registar a chave da API em logs. Diagnóstico não deve incluir respostas profissionais completas por defeito.
- Informar na configuração e antes do primeiro envio que texto revisto e critérios seguem para um serviço externo. Não afirmar retenção nula nem processamento exclusivamente local.
- Permitir continuar a gravar e guardar localmente durante indisponibilidade de rede. Depois de confirmar cada resposta principal, o utilizador pode repetir o pedido ou dispensar explicitamente o seguimento para avançar. O feedback final fica pendente até haver ligação. Seguimentos dispensados não são gerados mais tarde nem apresentados fora da ordem da entrevista.

## 12. Validação e condição de pronto

Validar com aproximadamente dez respostas reais, incluindo respostas completas, incompletas, com termos técnicos, resultado qualitativo ou negativo e gravações problemáticas.

Verificar:

1. O sentido da fala fica recuperável pela transcrição e revisão.
2. Gravação, transcrição e síntese funcionam no pacote Windows, sem preparar Python manualmente.
3. A espera entre turnos é medida e considerada aceitável pelo utilizador; a interface permanece utilizável.
4. O total configurado corresponde a perguntas principais, sem mais de um seguimento por pergunta.
5. Os seguimentos esclarecem lacunas reais e não repetem informação já respondida.
6. As observações apontam para trechos reais e distinguem resposta inicial de seguimento.
7. As reformulações não inventam factos e são adequadas ao nível configurado.
8. Uma falha de rede e um reinício preservam áudio e texto já guardados, permitindo retomar.
9. Uma nova tentativa pode ser avaliada e comparada sem sobrescrever a original.
10. A eliminação remove todos os dados e áudios exclusivos da sessão.
11. O consumo e o custo de uma sessão real são registados e apresentados sem falsa precisão.
12. Sem rede, avançar requer uma dispensa explícita do seguimento, e recuperar a ligação não insere perguntas anteriores na sessão.
13. Uma resposta confirmada não é alterada por uma nova tentativa nem por resultados que cheguem atrasados.

O MVP fica pronto para uso pessoal quando o utilizador completa uma entrevista curta, consulta feedback útil e repete uma resposta sem perder as versões anteriores. O desempenho da IA exige validação humana com dados reais; testes automáticos das regras da sessão não substituem essa validação.

## 13. Decisões deixadas para o plano e implementação

- Bibliotecas concretas de captura, reprodução, reamostragem, SQLite e armazenamento protegido de credenciais.
- Modelo/voz locais finais e requisitos de hardware, após medição no computador do utilizador.
- Esquemas JSON completos, organização de módulos, migrações e estratégia de distribuição do processo Python.
- Estilo visual, componentes React e detalhes de navegação, preservando o fluxo e a distinção entre versões.
- Limites técnicos de duração/texto e prazos dos pedidos, visíveis antes de causarem perda de trabalho; não truncar uma resposta silenciosamente.
- Estratégia de divisão dos pedidos de relatório para controlar custo e contexto, mantendo as evidências e a consistência.

Estas escolhas não alteram o âmbito funcional. Acrescentar outras línguas, trocar os motores de áudio ou retirar o processo Python exige uma decisão posterior sustentada pela validação.

## 14. Fontes consultadas

- Tauri, conceitos e arquitetura: https://v2.tauri.app/start/
- Tauri, WebViews: https://v2.tauri.app/reference/webview-versions/
- Tauri, distribuição de processos auxiliares: https://v2.tauri.app/develop/sidecar/
- faster-whisper, execução e requisitos: https://github.com/SYSTRAN/faster-whisper
- Kokoro-82M, modelo de síntese: https://huggingface.co/hexgrad/Kokoro-82M
- Whisper, finalidade e limitações: https://github.com/openai/whisper/blob/main/model-card.md
- OpenCode, modelos e tarifário: https://opencode.ai/v2/docs/console/models/
- OpenCode, API de inferência: https://opencode.ai/v2/docs/console/api/inference/
- OpenCode, catálogo em tempo de execução: https://opencode.ai/inference/v1/models

As fontes de catálogo, preços e endpoints foram verificadas nesta sessão. A disponibilidade e as condições do fornecedor podem mudar; confirmar ao implementar e usar.

# Spec da fase 1 — Validar áudio local

Data: 2026-11-01  
Estado: aceite como referência para o plano técnico; implementação por iniciar.

Referências: [plano das fases](../plans/2026-11-01-star-interview-tutor-phases.md), [desenho aprovado do MVP](2026-11-01-star-interview-tutor-design.md) e [stack técnica](../../tech-stack.md).

Execução detalhada no [plano técnico da fase 1](../plans/2026-11-01-phase-1-local-audio-implementation.md).

## 1. Objetivo

Validar, numa aplicação Tauri para Windows, o percurso completo de áudio local: ouvir uma pergunta fixa em inglês, gravar uma resposta, ouvi-la, obter uma transcrição e corrigir o texto reconhecido.

Esta fase deve revelar as dificuldades de captura, reprodução, modelos locais, comunicação entre Rust e Python e tempo de processamento no computador do utilizador. O resultado é uma base funcional para a entrevista guiada da fase 2.

A fase não avalia o conteúdo da resposta nem a pronúncia. Não utiliza uma API de IA, não pede credenciais de fornecedores e não envia gravações ou transcrições para serviços externos.

## 2. Âmbito e abordagem

Mantém-se a abordagem escolhida no desenho do projeto: React + TypeScript na interface, Rust + Tauri 2 no núcleo e um processo Python auxiliar para faster-whisper e Kokoro-82M.

Esta fase não volta a comparar frameworks nem troca motores de áudio. Uma integração nativa alternativa só será considerada posteriormente se a validação demonstrar uma limitação concreta.

### Incluído

- Aplicação desktop executável em desenvolvimento no Windows.
- Uma pergunta fixa apresentada em texto e reproduzida por voz local.
- Gravação manual da resposta, reprodução e ficheiro original guardado.
- Transcrição local e revisão manual, sem alterar a versão automática.
- Persistência simples dos artefactos e recuperação da última resposta guardada.
- Estados de processamento, erros recuperáveis e tempos medidos.
- Documentação dos requisitos e de como executar a integração real.

### Fora desta fase

- Banco editável de perguntas, categorias, contagem de questões ou sessão de entrevista.
- Seguimentos, avaliação STAR, feedback de inglês, reformulações e OpenCode.
- SQLite, banco de histórias, listagem de histórico e comparação de tentativas.
- Pontuação de pronúncia, transcrição em direto e interrupções automáticas.
- Seletores completos de dispositivos e configurações avançadas de áudio.
- Assistente de download de modelos, instalador final e suporte a outros sistemas operativos.

## 3. Valores de referência desta spec

Os valores seguintes são escolhas de especificação para permitir uma primeira implementação verificável; podem ser ajustados com a validação, sem serem apresentados como preferências previamente indicadas pelo utilizador.

| Elemento | Referência inicial |
| --- | --- |
| Pergunta fixa | `Tell me about a time you solved a difficult problem at work.` |
| Interface e mensagens | Português de Portugal. |
| Fala e transcrição | Inglês; transcrever, sem traduzir ou melhorar a resposta. |
| Captura e reprodução | Dispositivos predefinidos do Windows. |
| Transcrição | faster-whisper, modelo `small` multilingue, CPU/int8, língua definida como inglês. |
| Síntese | Kokoro-82M com uma voz inglesa. A voz concreta é escolhida e registada no plano técnico. |
| Concorrência | Um trabalho de transcrição ou síntese de cada vez. |
| Respostas | Uma resposta ativa na interface; cada nova gravação tem identificador e ficheiros próprios. |

O uso de CPU é a referência inicial, não uma garantia de desempenho. A fase não exige GPU. O modelo pode ser ajustado se as medições demonstrarem necessidade, registando a alteração e a sua razão.

## 4. Percurso do utilizador

1. Abrir a aplicação e consultar a disponibilidade do áudio e dos modelos.
2. Ler a pergunta fixa e pedir a sua reprodução em inglês.
3. Iniciar a gravação quando estiver preparado. Mostrar claramente que o microfone está a gravar e o tempo decorrido.
4. Terminar a gravação por ação explícita. Finalizar e guardar o ficheiro antes de iniciar a transcrição.
5. Iniciar automaticamente a transcrição da gravação finalizada e indicar o processamento.
6. Ouvir a gravação guardada, consultar a transcrição automática e editar uma cópia de revisão.
7. Guardar a revisão ou iniciar uma nova resposta à mesma pergunta.

Ouvir a pergunta é opcional: não impede gravar se a síntese estiver indisponível. Transcrever também não impede reproduzir um ficheiro de resposta já guardado.

A captura não depende de o processo Python estar pronto. Se o transcritor estiver indisponível quando a gravação termina, guardar o áudio e disponibilizar a transcrição quando o componente estiver preparado, sem exigir nova captura.

Não reproduzir áudio enquanto o microfone está a gravar. Ao iniciar a gravação, terminar qualquer reprodução em curso. Depois de gravar, é possível ouvir novamente a pergunta ou a resposta, uma de cada vez.

Uma nova gravação não sobrescreve a anterior. Só passa a ser a resposta ativa depois de ser finalizada e guardada com sucesso; uma tentativa de captura que falha não remove a última resposta válida.

## 5. Comportamento funcional

### 5.1 Preparação e disponibilidade

- Distinguir disponibilidade do microfone, reprodução, transcrição e síntese. A falta de um modelo não deve impedir consultar os textos e ficheiros já existentes.
- Nesta fase, preparar os modelos e as dependências através de instruções documentadas e de configuração local de desenvolvimento.
- Mostrar que componente está indisponível e qual o passo necessário para o preparar. Não apresentar uma instalação em falta como erro da resposta do utilizador.
- Depois de preparados os modelos, executar o percurso sem rede. Não iniciar downloads silenciosos como consequência de carregar em gravar ou ouvir.
- Documentar os downloads necessários à preparação inicial; essa preparação não autoriza o envio de dados de treino do utilizador.

### 5.2 Pergunta por voz

- Gerar o áudio real da pergunta fixa com Kokoro, utilizando uma voz inglesa.
- Guardar o áudio gerado e permitir reproduzi-lo novamente sem repetir a síntese quando texto, voz e configuração não mudaram.
- Se a geração falhar, conservar a pergunta em texto e permitir repetir a geração.
- Não substituir silenciosamente Kokoro por uma API, uma voz do sistema ou um ficheiro de exemplo. Duplicados de teste só são usados em testes e identificados como tal.

### 5.3 Captura e reprodução

- Usar a entrada predefinida do Windows. A biblioteca concreta e a gestão dos formatos do dispositivo pertencem ao plano técnico.
- Apresentar ações explícitas para iniciar e terminar. Não terminar uma resposta automaticamente por silêncio.
- Guardar o original em WAV, registando taxa de amostragem, canais, duração e tamanho efetivos. Não o substituir pelo ficheiro convertido para transcrição.
- Normalizar um derivado para WAV PCM de 16 bits, mono, 16 kHz quando necessário para a transcrição. A reprodução da resposta usa o original.
- Não cortar nem descartar partes da resposta silenciosamente. Um limite técnico, caso necessário, deve ser definido no plano e comunicado antes de gravar.
- Se não houver dispositivo, acesso ou espaço para guardar, apresentar um erro recuperável e não indicar que a resposta foi guardada.
- Se o dispositivo falhar durante a captura, tentar finalizar o áudio capturado; só o marcar como recuperado se existir um ficheiro reproduzível. Indicar que pode estar incompleto.

### 5.4 Transcrição

- Transcrever exclusivamente o áudio da resposta, sem incluir a voz da pergunta.
- Guardar o original antes de qualquer conversão ou inferência.
- Conservar a transcrição automática tal como foi devolvida pelo motor, com o modelo e a configuração utilizados.
- Não corrigir gramática, resumir a resposta ou fornecer ao motor uma resposta ideal para tentar aproximar o reconhecimento desse texto.
- Em caso de falha, disponibilizar uma ação para repetir a transcrição a partir do áudio guardado, sem nova gravação.
- Não processar ficheiros vazios como respostas válidas. Quando não existir fala reconhecida, mostrar essa situação e permitir ouvir o áudio e gravar novamente.
- Tratar sinais de silêncio ou baixa confiança como informação de reconhecimento, não como diagnóstico de pronúncia. Não prometer deteção perfeita de ausência de fala.
- Uma transcrição bem-sucedida não é sobrescrita automaticamente. Se for necessário voltar a processar uma resposta já transcrita, criar outro resultado identificado e preservar o anterior.

### 5.5 Revisão do texto

- Mostrar a transcrição automática como referência apenas de leitura e permitir editar uma cópia separada.
- Inicializar a revisão com a transcrição automática quando ainda não existir revisão do utilizador. Uma repetição do processamento nunca substitui alterações já feitas nessa revisão.
- A edição serve para recuperar o que foi dito, não para apresentar uma avaliação ou uma resposta melhorada.
- Disponibilizar uma ação explícita para guardar a revisão. Distinguir alterações por guardar, gravação em curso e revisão guardada.
- Ao mudar para outra resposta ou fechar normalmente a aplicação, guardar as alterações ou permitir ao utilizador decidir se as quer guardar, descartar ou continuar a editar. Uma falha de escrita mantém as alterações na interface e não pode ser apresentada como sucesso.
- Nesta fase, guardar uma revisão não equivale a confirmar uma resposta para avaliação. A confirmação imutável da entrevista é introduzida na fase 2; não existe envio à IA na fase 1.

## 6. Responsabilidades técnicas

| Componente | Responsabilidade nesta fase |
| --- | --- |
| React/TypeScript | Pergunta, ações, estado da captura, reprodução, texto automático, revisão e mensagens. |
| Rust/Tauri | Estado da operação, captura, ficheiros, persistência, comunicação com Python e eventos para a interface. |
| Processo Python | Carregar modelos, transcrever, sintetizar e devolver resultados ou erros estruturados. |

Rust é a autoridade sobre a resposta ativa e o trabalho em curso. Cliques repetidos não iniciam várias gravações ou transcrições para a mesma operação. A interface impede operações incompatíveis e o núcleo valida a mesma regra.

O processo auxiliar usa mensagens JSON delimitadas por linha em entrada/saída padrão, com identificador de pedido. Os registos de diagnóstico seguem para stderr; o áudio circula como ficheiros, não como conteúdo base64 nas mensagens.

O contrato distingue disponibilidade, transcrição, síntese e encerramento. Os esquemas exatos e os comandos Tauri são definidos no plano técnico. Caminhos e identificadores são resolvidos pelo núcleo a partir de artefactos da aplicação.

Os modelos podem ser carregados apenas quando necessários e reutilizados. A interface distingue carregamento inicial de execução de uma transcrição ou síntese. O trabalho pesado não bloqueia a janela nem o processamento de eventos.

## 7. Ficheiros e persistência mínima

Usar a pasta de dados local da aplicação, obtida através do Tauri. Não escrever gravações na árvore do código-fonte ou depender do diretório de onde a aplicação foi iniciada.

Para cada resposta, conservar:

- Identificador único e pergunta utilizada.
- Áudio original e, quando existir, derivado normalizado.
- Metadados de captura e datas.
- Resultados automáticos de transcrição, com identificador, modelo e configuração.
- Revisão manual guardada, associada ao resultado que lhe deu origem.
- Estado e erro da última operação, quando aplicável.
- Medições de duração do processamento.

Nesta fase, estes metadados e textos podem usar JSON, com gravação atómica para evitar substituir um ficheiro válido por uma escrita incompleta. Não é necessário construir uma base de dados ou uma listagem de histórico.

Manter uma referência à última resposta finalizada com sucesso. Ao reabrir a aplicação, recuperar os seus ficheiros e a revisão já guardada. Outros ficheiros de respostas anteriores mantêm-se preservados, mesmo sem navegação entre eles na interface.

Uma referência a um ficheiro em falta ou inválido deve produzir informação explícita, sem eliminar os restantes artefactos nem bloquear a possibilidade de gravar outra resposta. O último estado guardado de um trabalho interrompido passa a recuperável, não fica indefinidamente como processamento em curso.

Não prometer recuperação de uma gravação que nunca foi finalizada ou de alterações de texto que ainda não foram guardadas antes de uma falha inesperada.

## 8. Estados e recuperação

| Situação | Comportamento esperado |
| --- | --- |
| Modelos em preparação ou carregamento | Indicar o componente ocupado; permitir consultar conteúdos já guardados. |
| Captura ativa | Mostrar estado e duração; permitir terminar; impedir reproduções e outra captura. |
| Original a guardar | Só transcrever depois de confirmar que o ficheiro foi finalizado. |
| Transcrição em curso | Indicar processamento e a resposta correspondente; não iniciar outro trabalho de modelo. |
| Texto disponível | Permitir ouvir a gravação, editar e guardar a revisão. |
| Falha de transcrição | Conservar original e revisão existente; permitir repetir o processamento. |
| Falha de síntese | Conservar texto da pergunta; permitir repetir a geração. A captura continua disponível quando não houver uma operação incompatível em curso. |
| Processo Python terminou | Marcar o pedido em curso como falhado, conservar ficheiros e permitir reiniciar o auxiliar para repetir a operação. |
| Erro de escrita | Não assinalar como guardado; manter dados recuperáveis e permitir repetir. |

Uma operação bloqueada deve atingir um prazo limite definido e documentado no plano técnico, sem deixar a aplicação permanentemente ocupada. O tempo de carregamento inicial pode ter prazo diferente do processamento de um pedido.

Aceitar resultados apenas para o identificador de operação e a resposta que os originaram. Depois de timeout, falha ou reinício do auxiliar, um resultado atrasado não substitui a resposta ativa nem o texto revisto.

Durante captura ou um trabalho de modelo, não iniciar nova gravação ou outro trabalho de modelo. O utilizador pode continuar a consultar a interface e, quando existir áudio já guardado e não estiver a gravar, ouvi-lo. Ao fechar normalmente, terminar a captura em curso e guardar o que for finalizado, resolver revisões por guardar e encerrar o auxiliar. Uma falha inesperada segue os limites de recuperação da secção 7.

## 9. Validação e medições

### Verificação automática

O plano técnico deve prever verificações dos comportamentos com risco de perda de trabalho: separação entre texto automático e revisão, persistência, repetição após erro, exclusão de operações incompatíveis e rejeição de resultados atrasados.

Os testes podem usar adaptadores e resultados simulados. A conclusão da fase exige também a integração real de microfone, faster-whisper e Kokoro.

### Validação manual com dados reais

Usar pelo menos três gravações: uma resposta curta, uma resposta com termos profissionais ou técnicos e uma gravação problemática, como silêncio ou interrupção do dispositivo. Acrescentar casos conforme os problemas encontrados.

Registar o hardware utilizado, versões relevantes, modelo, voz e configurações. Medir separadamente carregamento dos modelos, síntese da pergunta e transcrição de cada gravação; distinguir primeira utilização de repetições e registar a duração do áudio transcrito.

Não fixar uma promessa de latência antes de medir. O utilizador valida se a espera permite praticar confortavelmente. Se não permitir, documentar a limitação e a alteração proposta antes de marcar a fase como concluída.

Documentar os requisitos Windows/Rust/Node/Python e as dependências nativas efetivamente utilizadas, juntamente com os comandos reais de instalação, execução e verificação. Não declarar requisitos mínimos de hardware sem os medir.

## 10. Critérios de aceitação

- [ ] CA-01: A aplicação abre no Windows e apresenta a pergunta fixa correta.
- [ ] CA-02: Kokoro gera voz inglesa para essa pergunta e o utilizador consegue ouvi-la novamente.
- [ ] CA-03: O utilizador inicia e termina uma gravação manualmente, com estado e duração visíveis.
- [ ] CA-04: Não há reprodução durante a captura nem operações duplicadas por cliques repetidos.
- [ ] CA-05: O áudio finalizado é guardado e pode ser reproduzido, mesmo quando a transcrição falha.
- [ ] CA-06: faster-whisper transcreve uma resposta real em inglês, sem tradução ou avaliação linguística.
- [ ] CA-07: O texto automático é preservado e a revisão pode ser guardada separadamente.
- [ ] CA-08: Repetir uma transcrição falhada não exige gravar novamente; resultados posteriores não apagam revisões existentes.
- [ ] CA-09: Uma nova gravação e o reinício da aplicação não apagam a última resposta finalizada nem a revisão guardada.
- [ ] CA-10: Áudio vazio, ficheiros em falta, ausência de dispositivo e ausência de fala reconhecida têm comportamento explícito e recuperável.
- [ ] CA-11: Uma falha ou bloqueio do auxiliar permite recuperar o controlo e repetir a operação, sem aplicar resultados atrasados.
- [ ] CA-12: A interface permanece utilizável durante carregamento e processamento dos modelos.
- [ ] CA-13: Com os modelos preparados, o percurso funciona sem rede e não faz chamadas a serviços de IA.
- [ ] CA-14: As gravações reais e os tempos medidos estão registados, e o utilizador considera a espera aceitável.
- [ ] CA-15: Existem instruções verificadas para preparar e executar a integração no ambiente de desenvolvimento Windows.

A spec estar escrita não significa que os critérios estejam cumpridos. A fase só termina depois da implementação, da verificação técnica e da validação manual; não inicia automaticamente a fase 2.

## 11. Decisões deixadas para o plano técnico

- Bibliotecas Rust para captura, reprodução, WAV e reamostragem.
- Versões de ferramentas e dependências; comandos de desenvolvimento, build e teste.
- Configuração de modelos, escolha da voz inglesa e localização dos seus ficheiros.
- Organização dos módulos, comandos Tauri, eventos e esquemas JSON do auxiliar.
- Nomes concretos dos ficheiros de persistência, mecanismos de escrita atómica e formatos dos metadados.
- Prazos limite e limites técnicos de áudio necessários, com mensagens claras e sem truncamento silencioso.

Estas decisões devem respeitar o comportamento desta spec. Não é necessário decidir agora os contratos das sessões, o esquema SQLite ou a integração com a API da fase 3.

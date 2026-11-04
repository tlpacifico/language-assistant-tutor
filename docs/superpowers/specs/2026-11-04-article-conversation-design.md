# Conversa sobre um artigo — desenho proposto

Data: 2026-11-04. A direção funcional e o envio automático depois de terminar cada resposta foram aceites pelo utilizador. Este documento concretiza a proposta; aprovação do desenho e implementação pendentes. Não inicia a fase 5 nem altera a aceitação das fases anteriores.

## Objetivo e decisões

Praticar a capacidade de conversar em inglês sobre um artigo, mantendo coesão, coerência e desenvolvimento das ideias. O tutor acompanha o raciocínio sem interromper a fala para corrigir. As pausas de processamento devem ser reduzidas; não se promete conversa simultânea nem latência nula.

A leitura do artigo é preparação, com leitura em voz alta opcional e sem classificação de pronúncia. O foco é a conversa. Interface e explicações em português de Portugal; perguntas, comentários e respostas de treino em inglês.

## Abordagens consideradas

- Revisão obrigatória da transcrição em cada turno: conserva o percurso atual, mas introduz a pausa que o utilizador pretende evitar.
- Conversa por turnos, com envio autorizado no início e feedback final: direção escolhida. O utilizador termina a gravação e a aplicação transcreve e envia sem confirmação intermédia.
- Conversa simultânea com deteção automática do fim da fala: maior complexidade, risco de interromper pausas naturais e mudança das regras de gravação. Fica fora deste desenho.

Mantêm-se React/TypeScript, Rust/Tauri, SQLite, faster-whisper, Kokoro e OpenCode Go. Não se introduz fornecedor nem motor novo.

## Percurso

1. Colar texto de um artigo em inglês, com título opcional. A primeira versão não importa URLs nem PDF. Guardar uma cópia fixa para a sessão.
2. Ler o artigo ao próprio ritmo. A audição de trechos pelo Kokoro pode reutilizar o mecanismo de voz; não é requisito para começar a conversa.
3. Autorizar explicitamente, por sessão, o envio do artigo e das transcrições ao destino configurado. Explicar que terminar a gravação desencadeia o envio e que o áudio permanece local. Sem autorização, não começar a conversa externa.
4. O tutor faz uma pergunta aberta sobre o artigo. O utilizador começa e termina a resposta manualmente.
5. Guardar áudio e transcrição antes do pedido externo. Fixar uma versão de envio, marcada como transcrição enviada automaticamente, nunca como texto revisto pelo utilizador.
6. O tutor produz um comentário breve ligado à resposta e uma pergunta de aprofundamento. Guardar e validar o texto antes da voz. Reproduzir automaticamente com opção de parar; a gravação seguinte começa apenas por ação do utilizador.
7. Repetir até o utilizador escolher terminar; gerar feedback final sobre os turnos disponíveis. Proposta de ritmo: sugerir encerrar após cinco respostas, sem cortar fala nem impor esse número.

Não há correções gramaticais entre turnos. O tutor pode pedir esclarecimento quando não compreende uma ideia. Não deve concordar automaticamente: distingue factos do artigo, opinião do utilizador e informação não sustentada pelo texto.

## Conversa e feedback

Cada intervenção acompanha a resposta anterior e o contexto acumulado. Proposta: comentário de uma ou duas frases e uma pergunta, evitando listas de questões e exposições longas. A progressão pode passar por compreensão, resumo, opinião, exemplos e aplicação à experiência pessoal, sem guião rígido.

O feedback final avalia separadamente:

- Coerência: pertinência, continuidade da ideia central e contradições.
- Coesão: relações entre frases, conectores e referências compreensíveis.
- Desenvolvimento: razões, exemplos e sustentação das opiniões.
- Interação: resposta ao comentário do tutor e capacidade de aprofundamento.

Cada observação cita turnos e trechos reais. Escolher duas ou três melhorias prioritárias de inglês; distinguir correção de sugestão de estilo. Não aplicar STAR a este modo, não usar notas do artigo como se tivessem sido ditas e não atribuir uma classificação global de proficiência. Uma sessão curta pode ter evidência insuficiente.

Não avaliar pronúncia, ritmo, hesitações ou fluência acústica a partir da transcrição. Pausas naturais não constituem erros. Repetir uma resposta é uma nova tentativa, conservando a original e os comentários anteriores.

## Consentimento, versões e recuperação

Este é um modo distinto da entrevista STAR. A revisão e confirmação obrigatórias dos modos existentes mantêm-se. O consentimento existente sobre o destino continua necessário, acrescido da autorização de envio automático nesta sessão. Alterar o destino ou revogar a autorização impede novos envios até nova autorização explícita.

O utilizador pode consultar a transcrição e escolher «Corrigir transcrição». Guardar uma nova versão sem apagar áudio, transcrição automática, versão enviada ou resposta do tutor. Corrigir reconhecimento não equivale a melhorar retroativamente o inglês efetivamente dito.

Proposta para recuperação simples: suspender o avanço, invalidar pedidos pendentes da versão antiga e permitir regenerar a intervenção do tutor a partir da correção. Se já existirem turnos posteriores, conservar a sequência antiga como ramo histórico e retomar a partir do turno corrigido; nunca reatribuir respostas posteriores ao novo contexto. O relatório identifica o ramo utilizado e mantém as fontes. Regeneração é uma ação explícita que pode consumir API novamente.

Transcrição vazia ou falhada não é enviada: permitir repetir a transcrição ou introduzir uma revisão manual. Falha de rede conserva dados e permite repetir o pedido explicitamente. Falha de voz permite ler e continuar ou repetir apenas a síntese. Retomar uma sessão não repete pedidos nem reproduz voz automaticamente.

Resultados associam-se à sessão, ao ramo, ao turno, à versão e à operação; resultados atrasados não avançam a conversa. Encerrar durante um pedido impede uma pergunta tardia; o relatório usa apenas turnos persistidos e a versão de contexto fixada para essa operação. O núcleo controla o estado, não a LLM.

O artigo é conteúdo não fiável, nunca uma fonte de instruções para o sistema. Definir limites explícitos de artigo e contexto no plano, com validação antes do envio, sem truncamento silencioso. Ao atingir o limite de conversa, propor encerrar e gerar relatório antes de aceitar outro turno. Guardar o contexto exato de cada pedido para rastreabilidade local, fora dos logs.

Guardar chave no armazenamento protegido, consumo conhecido e custo desconhecido quando não fornecido. O diagnóstico continua sem texto livre, áudio ou conteúdo de entrevistas; novas operações exigem atualizar as listas fechadas. Eliminar a sessão elimina também artigo e artefactos exclusivos associados.

## Desempenho

Base existente: modelos residentes com preload, Whisper small em CPU/int8 com inglês fixo e VAD, cache de voz e execução sequencial de trabalhos locais. O auxiliar atualmente entrega a voz depois de juntar todos os blocos.

Prioridades propostas para o plano:

1. Medir separadamente parar→transcrição, envio→texto validado e texto→primeiro som, distinguindo modelos frios e carregados. Usar gravações reais curtas e longas, com termos técnicos; registar fidelidade, duração e memória, sem conteúdo nos diagnósticos.
2. Comparar parâmetros de descodificação do Whisper, incluindo beam_size, nas mesmas gravações. Só adotar uma configuração mais rápida se a perda de fidelidade for aceitável na validação manual.
3. Entregar e reproduzir blocos de voz incrementalmente após validar o texto do tutor, mantendo a ordem, cancelamento e tratamento de falha parcial. Não falar texto de uma resposta IA ainda não validada.
4. Usar intervenções curtas para reduzir geração e tempo de audição. Preparação antecipada de voz só para texto já conhecido; não adivinhar a pergunta seguinte enquanto o utilizador responde.

O áudio de fundo não pode atrasar a transcrição seguinte. Concorrência, GPU e transcrição durante a gravação ficam para medição e decisão posterior, não são requisitos desta primeira versão. Valores-alvo de latência serão fixados após a medição de base; não há ganhos medidos nesta proposta.

## Verificação e âmbito

O plano deve cobrir consentimento por sessão e revogação, imutabilidade de fontes, isolamento dos modos, ramificação por correção, idempotência, resultados atrasados, retoma, falhas de transcrição/rede/voz, limites de contexto, eliminação e consumo desconhecido. Testar que o artigo não pode alterar instruções do tutor e que o feedback cita o utilizador em vez de atribuir-lhe o conteúdo do artigo.

A aceitação manual exige uma conversa real sobre um artigo com pelo menos cinco respostas, incluindo discordância e aprofundamento; avaliar continuidade, coesão do diálogo, utilidade do feedback, fidelidade da transcrição e espera percebida. Inferência real e voz real não são substituídas por testes automáticos.

Este desenho não inclui importação de documentos, avaliação de pronúncia, vocabulário com repetição espaçada, debate com múltiplas personagens ou alteração automática das entrevistas existentes. O plano posterior define esquema, comandos, limites concretos e sequência de implementação apenas deste âmbito.

## Referências locais

- [Objetivos atuais](../../ideia-e-objetivos.md)
- [Stack e áudio](../../tech-stack.md)
- [Avaliação atual por pergunta](../../ai-coaching/validation.md)
- [Diagnóstico local](../../diagnostics/validation.md)
- [Integração OpenCode Go](../../opencode-go/validation.md)

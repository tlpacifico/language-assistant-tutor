# Ideia e objetivos do projeto

## A ideia

Criar um tutor desktop para praticar entrevistas comportamentais em inglês por voz, usando as experiências profissionais do próprio utilizador e a estrutura STAR.

STAR organiza uma resposta em quatro partes:

- **Situação:** o contexto em que a experiência aconteceu.
- **Tarefa:** a responsabilidade ou o desafio do utilizador.
- **Ação:** o que fez concretamente.
- **Resultado:** o que aconteceu e o que aprendeu, incluindo resultados qualitativos ou negativos.

A aplicação deve ajudar a explicar experiências com clareza, identificar informação que ficou por dizer e melhorar a forma de comunicar em inglês. O objetivo é desenvolver autonomia, em vez de memorizar respostas artificiais.

## Para quem

A primeira versão destina-se ao uso pessoal no computador, começando por Windows. O treino será em inglês, com interface e explicações em português.

No futuro, a arquitetura poderá acomodar outras línguas. Essa expansão fica fora do MVP e exige validar os modelos de voz, transcrição e avaliação para cada língua.

## Problema que queremos resolver

É possível conhecer uma experiência profissional e, ainda assim, ter dificuldade em contá-la numa entrevista: omitir contexto, falar apenas do trabalho da equipa, não explicar as ações individuais ou deixar o resultado por esclarecer.

Responder em inglês acrescenta dificuldades de vocabulário, gramática e organização. O tutor oferece um espaço para praticar em voz alta, receber perguntas de seguimento e tentar novamente com orientação específica.

## Ciclo principal

1. Escolher categoria e quantidade de perguntas principais.
2. Ouvir e ler uma pergunta em inglês.
3. Responder pelo microfone, começando e terminando a gravação manualmente.
4. Rever a transcrição para recuperar fielmente o que foi dito.
5. Responder a um seguimento quando for necessário esclarecer uma lacuna.
6. Consultar feedback no fim da entrevista.
7. Ouvir uma versão sugerida e gravar uma nova tentativa com palavras próprias.

Os seguimentos são adicionais e limitados a um por pergunta principal. A configuração inicial sugerida é de três perguntas principais, com possibilidade de praticar apenas uma ou escolher outra quantidade disponível.

## Objetivos do MVP

- Permitir completar uma entrevista comportamental curta em inglês.
- Ajudar a estruturar respostas segundo STAR sem impor uma resposta única.
- Identificar lacunas relevantes, especialmente a contribuição individual e o resultado.
- Melhorar gramática, vocabulário, clareza e naturalidade no texto da resposta.
- Produzir observações apoiadas em trechos efetivamente respondidos.
- Permitir repetir perguntas e comparar respostas sem perder as versões anteriores.
- Guardar um banco editável de perguntas e histórias profissionais factuais.
- Manter áudio e histórico locais, com custos variáveis de IA baixos e visíveis.

## Princípios do feedback

O feedback separa **conteúdo e estrutura STAR** de **inglês na transcrição confirmada**. Usa classificações simples, como claro, parcial e não identificado, acompanhadas de explicações, sem uma nota global que sugira precisão não validada.

Uma resposta pode ser boa sem conter números ou um resultado positivo. O tutor deve reconhecer aprendizagens, limitações e responsabilidade individual, sem incentivar a inventar factos.

As reformulações preservam o significado e o nível de certeza do que foi dito. Informação em falta é assinalada, em vez de preenchida pelo modelo. Correções necessárias e sugestões opcionais de estilo são apresentadas separadamente.

As notas das histórias pessoais servem de apoio à preparação. Não contam como informação explicada oralmente. O feedback distingue também o que ficou claro na resposta inicial do que só foi esclarecido no seguimento.

## Privacidade e custos

A gravação, a reprodução e a transcrição são locais. As transcrições confirmadas e o contexto textual necessário são enviados ao OpenCode para produzir seguimentos e avaliações. O armazenamento local não significa que todo o processamento seja local.

Não será enviada a gravação para a LLM neste fluxo. A aplicação permitirá eliminar sessões e os respetivos áudios, proteger a chave da API e consultar o consumo e o custo estimado.

O objetivo é aproveitar modelos locais para áudio e um modelo económico na nuvem para texto, medindo o custo real em vez de depender de uma oferta gratuita.

## Fora do MVP

- Avaliação automática da pronúncia ou pontuação por fonema.
- Interrupções enquanto o utilizador fala.
- Conversa simultânea em tempo real ou transcrição em direto.
- Diagnóstico de confiança emocional ou certificação de proficiência.
- Outras línguas, sincronização na nuvem, contas ou colaboração.
- Treino de modelos próprios.

O Whisper transforma fala em texto. Uma transcrição correta não comprova boa pronúncia, e a LLM que recebe apenas texto não pode avaliar a execução oral. Ouvir uma versão sugerida é uma ferramenta de prática, não um diagnóstico de pronúncia.

## Como saberemos que o MVP é útil

O utilizador consegue completar uma entrevista curta, compreender o que pode melhorar e repetir uma resposta sem perder o trabalho anterior.

A validação inclui cerca de dez respostas reais, com diferentes níveis de completude, termos técnicos e gravações problemáticas. Avaliaremos a fidelidade da transcrição, a espera entre turnos, a utilidade dos seguimentos, a preservação dos factos e o custo de sessões reais.

## Evolução por fases

1. Validar áudio local com uma pergunta fixa.
2. Criar entrevistas guiadas com perguntas e sessões persistentes.
3. Acrescentar seguimentos e feedback com IA.
4. Permitir reformular, repetir e comparar respostas, com apoio de histórias pessoais.
5. Consolidar instalação, recuperação de falhas e validação do conjunto.

As entregas e os critérios de conclusão estão no [plano das fases](superpowers/plans/2026-11-01-star-interview-tutor-phases.md). A arquitetura está na [stack técnica](tech-stack.md) e as regras completas no [desenho aprovado](superpowers/specs/2026-11-01-star-interview-tutor-design.md).

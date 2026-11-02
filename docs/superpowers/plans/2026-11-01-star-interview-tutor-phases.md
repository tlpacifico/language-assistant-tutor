# Plano das fases — Tutor de entrevistas STAR

Data: 2026-11-01  
Estado: divisão em fases aprovada; fases 1 e 2 implementadas e verificadas automaticamente, com a validação manual do utilizador pendente em ambos os registos; fase 3 não iniciada.

## Objetivo

Construir progressivamente um tutor desktop para praticar entrevistas comportamentais em inglês por voz, com seguimento contextual, feedback STAR e novas tentativas.

Este documento organiza as entregas e os critérios de conclusão. Não é um plano técnico de implementação: apenas a fase seguinte será detalhada quando formos executá-la. As fases posteriores serão ajustadas com o que aprendermos nas anteriores.

O [desenho aprovado do MVP](../specs/2026-11-01-star-interview-tutor-design.md) continua a definir o comportamento final da aplicação.

## Base técnica

- Interface: React + TypeScript.
- Aplicação desktop e lógica: Rust + Tauri.
- Transcrição local: faster-whisper num processo Python auxiliar.
- Síntese de voz local: Kokoro-82M no processo auxiliar.
- Avaliação textual: DeepSeek Flash através da API do OpenCode.
- Dados: SQLite e ficheiros de áudio locais.
- Primeiro alvo: Windows.

## Visão geral

| Fase | Resultado principal | Dependência |
| --- | --- | --- |
| 1 — Validar áudio local | Ouvir uma pergunta fixa, responder e rever a transcrição. | Nenhuma implementação anterior. |
| 2 — Entrevista guiada | Completar uma sessão configurável e recuperar respostas guardadas. | Áudio local validado. |
| 3 — Tutor com IA | Receber seguimentos contextuais e feedback sustentado nas respostas. | Sessões e persistência disponíveis. |
| 4 — Melhorar e repetir | Ouvir uma reformulação, gravar outra tentativa e comparar versões. | Avaliação textual disponível. |
| 5 — Consolidar o MVP | Instalar e usar a aplicação com recuperação de falhas validada. | Ciclo funcional completo. |

## Fase 1 — Validar áudio local

**Objetivo:** verificar a integração de Tauri, Rust, React e dos modelos de áudio no computador do utilizador antes de construir o restante produto.

**Spec:** [Validar áudio local](../specs/2026-11-01-phase-1-local-audio-design.md).

**Plano técnico:** [Implementação da fase 1](2026-11-01-phase-1-local-audio-implementation.md).

### Entregas

- Aplicação desktop com uma pergunta fixa em inglês.
- Reprodução da pergunta através do Kokoro.
- Gravação iniciada e terminada manualmente.
- Reprodução da gravação e transcrição através do faster-whisper.
- Visualização e edição da transcrição, preservando o texto automático separadamente.
- Gravação do áudio em ficheiro local.
- Indicação de processamento e erros, sem bloquear a interface.
- Registo dos requisitos de execução e das dependências dos modelos locais.

### Critérios de conclusão

- [ ] O utilizador ouve a pergunta em inglês.
- [ ] Consegue gravar, terminar e ouvir a própria resposta.
- [ ] A transcrição é apresentada e pode ser corrigida.
- [ ] Uma falha de transcrição permite repetir o processamento do áudio guardado.
- [ ] O funcionamento é validado com algumas gravações reais, incluindo termos técnicos.
- [ ] O tempo de processamento é medido e considerado aceitável pelo utilizador.

**Fica para depois:** banco de perguntas, entrevistas completas, SQLite e integração com a IA na nuvem.

## Fase 2 — Entrevista guiada

**Objetivo:** transformar a experiência de uma pergunta num treino com várias perguntas e dados persistentes.

**Spec:** [Entrevista guiada](../specs/2026-11-02-phase-2-guided-interview-design.md). Define o fluxo, a confirmação imutável, a recuperação, a compatibilidade com os dados da fase 1 e os critérios F2-01 a F2-16.

**Plano técnico:** [Implementação da fase 2](2026-11-02-phase-2-guided-interview-implementation.md), com as tarefas 1 a 9 executadas e a tarefa 10 parcial (falta a validação manual).

**Estado:** implementação completa e verificada automaticamente em 2026-11-02. `.\scripts\verify.ps1` deu 8/8 checks (147 testes Rust, 43 Vitest, 36 pytest sem `models`), o teste real `-m models` deu `2 passed`, o build release `--no-bundle` e o arranque/fecho do binário terminaram com código 0. A matriz F2-01 a F2-16 e a evidência estão em [docs/phase-2/validation.md](../../phase-2/validation.md); os critérios manuais continuam pendentes e não são dados como concluídos. A fase 3 **não** foi iniciada.

### Entregas

- Banco inicial de cerca de 20 perguntas, distribuídas pelas categorias do desenho.
- Adição e edição de perguntas, competências e critérios de avaliação.
- Configuração da categoria e do número de perguntas principais, com três por defeito e opção de uma.
- Seleção e ordem das perguntas fixadas no início da sessão.
- Progresso visível, por exemplo, “Pergunta 2 de 3”.
- Captura, transcrição e confirmação da resposta em cada pergunta.
- SQLite para sessões e textos; ficheiros locais para as gravações.
- Consulta das sessões e recuperação das respostas já guardadas.

### Critérios de conclusão

- [ ] O utilizador consegue completar uma sessão com três perguntas.
- [ ] Pode praticar apenas uma pergunta ou escolher outra quantidade disponível.
- [ ] A aplicação informa se a categoria não tiver perguntas suficientes.
- [ ] Editar o banco não altera perguntas de sessões já iniciadas.
- [ ] Áudio original, transcrição automática e transcrição confirmada permanecem distintos.
- [ ] Reiniciar a aplicação preserva os dados já guardados e permite recuperar a sessão.

**Fica para depois:** seguimentos automáticos e avaliação por IA. Nesta fase, o avanço entre perguntas é guiado pela aplicação, sem decisões do modelo.

**Progresso em 2026-11-02:** as entregas acima estão implementadas (SQLite com `rusqlite`, banco editável, sessões com cópias fixas, artefactos e reconciliação, revisões e confirmações imutáveis, histórico e consulta de leitura dos treinos da fase 1) e a automação está verde. Os critérios de conclusão continuam **por marcar**: dependem da validação manual do utilizador descrita em [docs/phase-2/validation.md](../../phase-2/validation.md), secção 4. Não marcar a fase como concluída antes dessa evidência.

## Fase 3 — Tutor com IA

**Objetivo:** acrescentar seguimentos úteis e feedback específico, preservando o controlo da entrevista na aplicação.

### Entregas

- Integração direta com o modelo pago DeepSeek Flash através da API do OpenCode.
- Modelo e URL base configuráveis; chave protegida pelo mecanismo do sistema operativo.
- Envio de texto apenas depois da confirmação da transcrição.
- Decisão contextual sobre a necessidade de um seguimento.
- No máximo um seguimento por pergunta principal, com resposta por voz e revisão da transcrição.
- Feedback no fim, separado entre conteúdo/STAR e inglês no texto confirmado.
- Observações apoiadas em trechos reais, distinguindo resposta inicial e seguimento.
- Classificações simples e prioridades concretas, sem nota global ou diagnóstico de pronúncia.
- Registo de modelo, consumo de tokens e custo estimado por pedido e sessão.
- Recuperação de falhas da API sem exigir nova gravação.

### Critérios de conclusão

- [ ] O modelo recebe a transcrição confirmada, sem envio do áudio.
- [ ] Uma pergunta principal nunca gera mais de um seguimento.
- [ ] Os seguimentos não aumentam o contador de perguntas principais.
- [ ] Os seguimentos esclarecem lacunas reais, sem repetir informação já explicada.
- [ ] O feedback cita trechos válidos e aceita resultados qualitativos, negativos e aprendizagens.
- [ ] Falhar um pedido permite repeti-lo sem perder a resposta ou duplicar o seguimento.
- [ ] Avançar sem seguimento após uma falha exige uma dispensa explícita; não surgem seguimentos atrasados.
- [ ] O custo é apresentado como estimativa e consumo desconhecido não é apresentado como zero.

**Fica para depois:** reformulação com áudio, comparação de tentativas e banco de histórias pessoais.

## Fase 4 — Melhorar e repetir

**Objetivo:** fechar o ciclo de aprendizagem com sugestões concretas e prática autónoma.

### Entregas

- Reformulação da resposta com organização mais clara e inglês adequado ao nível configurado.
- Separação entre correções necessárias e sugestões opcionais de estilo.
- Reprodução da versão sugerida através do Kokoro.
- Nova tentativa de uma pergunta, com gravação, transcrição revista e avaliação própria.
- Comparação entre a resposta inicial, o respetivo seguimento e as novas tentativas.
- Banco de histórias pessoais com título, notas factuais STAR e competências associadas.
- Seleção opcional de uma história como apoio à preparação.

### Critérios de conclusão

- [ ] A reformulação preserva os factos, sem inventar números, responsabilidades ou resultados.
- [ ] A informação em falta fica assinalada, em vez de ser preenchida pelo modelo.
- [ ] O utilizador consegue ouvir a sugestão e responder novamente com as próprias palavras.
- [ ] Uma nova tentativa não sobrescreve nem reabre a entrevista original.
- [ ] A comparação distingue claramente o que foi dito, corrigido e sugerido.
- [ ] As notas de uma história não são avaliadas como se tivessem sido ditas na resposta.

**Resultado desta fase:** o ciclo funcional está disponível — ouvir, responder, aprofundar, receber feedback e tentar novamente. A validação do MVP completo continua na fase 5.

## Fase 5 — Consolidar o MVP

**Objetivo:** tornar o ciclo completo utilizável numa instalação normal e validar o comportamento perante falhas reais.

### Entregas

- Instalador Windows com o processo auxiliar e as dependências de execução.
- Preparação dos modelos locais com indicação de progresso ou seleção de ficheiros existentes.
- Eliminação de sessões e dos respetivos áudios, versões e ficheiros derivados exclusivos.
- Revisão da recuperação após falhas de rede, modelos, processo auxiliar e reinícios.
- Validação de desempenho, qualidade do feedback e custos com cerca de dez respostas reais.
- Instruções de instalação, configuração e utilização.

### Critérios de conclusão

- [ ] A aplicação instalada permite gravar, transcrever e gerar voz sem preparar Python manualmente.
- [ ] Depois de preparados os modelos, as funções de áudio local funcionam sem rede.
- [ ] Falhas de rede preservam dados e permitem retomar os pedidos necessários.
- [ ] Resultados atrasados ou repetidos não alteram indevidamente a sessão.
- [ ] A eliminação remove todos os dados e ficheiros exclusivos da sessão, sinalizando falhas de remoção.
- [ ] O desempenho, o custo real e a utilidade do feedback foram avaliados pelo utilizador.
- [ ] É possível completar uma entrevista curta, consultar feedback e repetir uma resposta sem perder as versões anteriores.

## Regras comuns a todas as fases

- Guardar os dados reais assim que cada funcionalidade os começar a produzir; não adiar a persistência necessária para a fase final.
- Proteger a chave e informar sobre o envio de texto externo desde a primeira integração com a API.
- Implementar a recuperação básica de cada componente quando ele entra; a fase 5 valida e consolida o conjunto.
- Não apresentar transcrição ou feedback textual como avaliação da pronúncia.
- Não acrescentar outras línguas, conversa em tempo real ou interrupções automáticas neste MVP.
- Validar cada fase antes de avançar e detalhar apenas a próxima fase, evitando planear antecipadamente toda a implementação.

## Próximo passo

A implementação e a verificação automática da fase 1 estão registadas em [validação da fase 1](../../phase-1/validation.md); os critérios manuais aí pendentes não são considerados concluídos sem evidência. O mesmo vale para a fase 2: implementação e automação estão registadas em [validação da fase 2](../../phase-2/validation.md), com a matriz F2-01 a F2-16 e a lista do que falta validar com o utilizador. O próximo passo é a validação manual das fases 1 e 2; a fase 3 (tutor com IA) **não** foi iniciada nem planeada em detalhe.

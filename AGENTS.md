# Orientações para trabalhar neste projeto

## Contexto e documentos

Este projeto é um tutor desktop de entrevistas comportamentais em inglês, por voz, baseado na estrutura STAR. O desenvolvimento está organizado em fases; a implementação ainda não começou.

Antes de trabalhar, consultar os documentos relevantes para a tarefa:

- [Ideia e objetivos](docs/ideia-e-objetivos.md): problema, público, ciclo de aprendizagem e limites do MVP.
- [Stack técnica](docs/tech-stack.md): tecnologias escolhidas, responsabilidades e integrações.
- [Plano das fases](docs/superpowers/plans/2026-11-01-star-interview-tutor-phases.md): entregas, dependências e critérios de conclusão.
- [Spec da fase 1](docs/superpowers/specs/2026-11-01-phase-1-local-audio-design.md): âmbito, comportamento do áudio local e critérios de aceitação.
- [Plano técnico da fase 1](docs/superpowers/plans/2026-11-01-phase-1-local-audio-implementation.md): tarefas, contratos e verificações da primeira implementação.
- [Desenho aprovado](docs/superpowers/specs/2026-11-01-star-interview-tutor-design.md): regras funcionais e técnicas detalhadas.

As instruções atuais do utilizador têm prioridade. Estes documentos registam as decisões anteriores; mantê-los coerentes quando uma decisão mudar.

## Forma de trabalhar

- Trabalhar no âmbito da tarefa e da fase solicitadas. Não iniciar automaticamente as fases seguintes.
- Detalhar o plano técnico apenas da próxima fase a executar; evitar planear toda a implementação antecipadamente.
- Concluir autonomamente os passos necessários dentro do âmbito autorizado, sem pedir confirmação para cada decisão rotineira.
- Preferir execução direta e soluções simples, controlando custos e complexidade.
- Usar português de Portugal na comunicação e na documentação do projeto. Perguntas, seguimentos e respostas de treino são em inglês.
- Não trocar a stack ou os motores escolhidos como parte de uma tarefa que não peça essa alteração.
- Atualizar a documentação afetada e os critérios das fases quando houver mudanças relevantes.
- Não marcar funcionalidades ou fases como concluídas sem verificação concreta.

## Base técnica escolhida

- React + TypeScript para a interface.
- Rust + Tauri 2 para a aplicação desktop, sessões, dados e chamadas à API.
- Processo Python auxiliar para faster-whisper e Kokoro-82M.
- SQLite para dados estruturados e ficheiros locais para áudios.
- DeepSeek Flash através da API paga do OpenCode para seguimentos e avaliação textual.
- Windows como primeiro alvo.

Consultar a [stack técnica](docs/tech-stack.md) antes de introduzir dependências. As bibliotecas de gravação, reamostragem e credenciais serão escolhidas no plano da fase correspondente.

## Regras essenciais do produto

- A aplicação controla o estado da entrevista; a LLM apenas recomenda dentro das regras.
- A quantidade configurada conta perguntas principais. Existe no máximo um seguimento por pergunta principal.
- A gravação começa e termina por ação do utilizador, sem interrupções automáticas.
- Preservar o áudio e a transcrição automática. A revisão do utilizador é uma versão distinta.
- Enviar texto para avaliação apenas depois da confirmação. A versão confirmada fica fixa.
- Guardar novas tentativas e reformulações separadamente; não sobrescrever a resposta original.
- Distinguir informação dita na resposta inicial, acrescentada no seguimento e apenas existente nas notas de uma história.
- Apoiar o feedback em trechos reais e aceitar resultados qualitativos, negativos e aprendizagens.
- Reformulações preservam factos; não inventar responsabilidades, números ou resultados.
- Não apresentar transcrição ou avaliação textual como diagnóstico de pronúncia, fluência acústica ou confiança emocional.

## Dados, integração e recuperação

- Manter as gravações no computador. Apenas o texto necessário e os critérios seguem para a API do OpenCode.
- Guardar a chave através do armazenamento protegido do sistema operativo; não a colocar no frontend, código, SQLite de conteúdo ou logs.
- Guardar áudio e texto antes de chamadas externas. Uma falha de rede não deve exigir nova gravação.
- Aplicar resultados apenas à operação e à versão que os originaram; evitar duplicar seguimentos ou avançar duas vezes.
- Após falha de geração de seguimento, repetir o pedido ou dispensá-lo explicitamente antes de avançar. Não apresentar seguimentos atrasados.
- Executar áudio e rede sem bloquear a interface.
- Registar consumo conhecido e custo estimado. Não apresentar consumo desconhecido como zero nem assumir preços fixos do fornecedor.
- Implementar persistência e recuperação básica quando cada componente entra; não adiar tudo para a fase final.

## Verificação

- Executar verificações adequadas ao que mudou. Alterações apenas documentais exigem conferir conteúdo, coerência e links locais.
- Para implementação, testar especialmente regras da sessão, preservação de versões, evidências do feedback e recuperação de falhas.
- Validar áudio e desempenho com gravações reais; testes automáticos não substituem essa experiência.
- Validar também a aplicação empacotada quando se trabalhar na distribuição.
- Não inventar comandos ou resultados de build/testes. Ao criar a base do projeto, documentar os comandos reais de instalação, execução, teste e distribuição.

## Estado e próximos passos

O desenho e a divisão em cinco fases foram aprovados. A spec da fase 1 foi aceite como referência para o planeamento e o respetivo plano técnico está escrito para revisão. A fase valida uma pergunta fixa com voz, gravação e transcrição local; a implementação ainda não começou. Atualizar este estado à medida que o trabalho avançar.

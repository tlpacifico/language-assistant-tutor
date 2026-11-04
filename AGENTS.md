# Orientações para trabalhar neste projeto

## Contexto e documentos

Este projeto é um tutor desktop de entrevistas comportamentais em inglês, por voz, baseado na estrutura STAR. O desenvolvimento está organizado em fases; as fases 1 e 2 estão implementadas e verificadas automaticamente, com a validação manual do utilizador ainda pendente. A fase 3 foi antecipada por autorização explícita do utilizador em 2026-11-02; implementação e verificação automática não equivalem a aceitação manual.

Antes de trabalhar, consultar os documentos relevantes para a tarefa:

- [Validação do layout](docs/layout/validation.md): referência visual, adaptações aos contratos, recuperação áudio em todas as páginas e matriz reproduzível de sete páginas.

- [Ideia e objetivos](docs/ideia-e-objetivos.md): problema, público, ciclo de aprendizagem e limites do MVP.
- [Stack técnica](docs/tech-stack.md): tecnologias escolhidas, responsabilidades e integrações.
- [Plano das fases](docs/superpowers/plans/2026-11-01-star-interview-tutor-phases.md): entregas, dependências e critérios de conclusão.
- [Spec da fase 1](docs/superpowers/specs/2026-11-01-phase-1-local-audio-design.md): âmbito, comportamento do áudio local e critérios de aceitação.
- [Plano técnico da fase 1](docs/superpowers/plans/2026-11-01-phase-1-local-audio-implementation.md): tarefas, contratos e verificações da primeira implementação.
- [Preparação real no Windows](docs/phase-1/setup.md): versões observadas, comandos executados, modelos e configuração pessoal.
- [Validação da fase 1](docs/phase-1/validation.md): matriz CA-01 a CA-15 com evidência e o que falta validar com o utilizador.
- [Preparação e execução da fase 2](docs/phase-2/setup.md): o que mudou, SQLite, diretórios de sessão, comandos e reprodução a partir de um checkout limpo.
- [Validação da fase 2](docs/phase-2/validation.md): matriz F2-01 a F2-16, evidência automática e validação manual pendente.
- [Spec da fase 2](docs/superpowers/specs/2026-11-02-phase-2-guided-interview-design.md): entrevistas guiadas, banco de perguntas, confirmação, SQLite e recuperação.
- [Plano técnico da fase 2](docs/superpowers/plans/2026-11-02-phase-2-guided-interview-implementation.md): dez tarefas, contratos, transações, recuperação e verificações; tarefas 1 a 9 executadas e tarefa 10 parcial (falta a validação manual).
- [Desenho aprovado](docs/superpowers/specs/2026-11-01-star-interview-tutor-design.md): regras funcionais e técnicas detalhadas.
- [Preparação](docs/phase-4/setup.md) e [validação](docs/phase-4/validation.md) da fase 4, com a [spec](docs/superpowers/specs/2026-11-03-phase-4-improve-repeat-design.md) e o [plano](docs/superpowers/plans/2026-11-03-phase-4-improve-repeat-implementation.md): reformulação, práticas, comparação e histórias locais.
- [Spec da fase 3](docs/superpowers/specs/2026-11-02-phase-3-ai-tutor-design.md), [plano](docs/superpowers/plans/2026-11-02-phase-3-ai-tutor-implementation.md), [preparação](docs/phase-3/setup.md) e [validação](docs/phase-3/validation.md): tutor IA, consentimento, credenciais, seguimentos, evidências e recuperação.

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

O desenho e a divisão em cinco fases foram aprovados. A fase 1 está implementada (Tauri 2 + React, áudio nativo, auxiliar faster-whisper/Kokoro, persistência e revisão) e a verificação automática está verde: `.\scripts\verify.ps1` com 8/8 checks (77 testes Rust, 15 testes React, 36 testes Python sem a marca `models`, `pip check` limpo), o teste real `-m models` com `2 passed` e os modelos preparados fora do repositório (Whisper `small` e Kokoro-82M, 776,4 MiB). Os resultados estão em [docs/phase-1/validation.md](docs/phase-1/validation.md) e a preparação em [docs/phase-1/setup.md](docs/phase-1/setup.md).

A fase 1 **não** está aceite nem concluída no registo de validação: faltam a validação do utilizador com três gravações reais (curta, com termos técnicos e problemática), a audição da voz Kokoro, o teste de remoção/restauro do microfone, a repetição do percurso offline pela janela e a decisão sobre a aceitabilidade da espera (CA-14). Não marcar a fase como concluída antes dessa validação.

A fase 2 está implementada (SQLite com `rusqlite`, banco editável de 20 perguntas, sessões com cópias fixas, artefactos e reconciliação, revisões e confirmações imutáveis, histórico e consulta de leitura dos treinos da fase 1) e a verificação automática está verde: `.\scripts\verify.ps1` com 8/8 checks (151 testes Rust, 53 testes Vitest, 36 testes Python sem a marca `models`, `pip check` limpo), o teste real `-m models` com `2 passed`, o build release `--no-bundle` e o arranque/fecho do binário com código 0. Os dados estão em [docs/phase-2/validation.md](docs/phase-2/validation.md) e [docs/phase-2/setup.md](docs/phase-2/setup.md).

A fase 2 **não** está concluída nem aceite: falta a validação manual descrita na spec §10 e em [docs/phase-2/validation.md](docs/phase-2/validation.md) — sessão real de três perguntas (uma com termos técnicos), audição de perguntas e originais, revisão/confirmação, sair e retomar, histórico, sessão de uma pergunta e outra quantidade, insuficiência numa categoria, edição do banco sem alterar a cópia da sessão, percurso offline pela janela, revisão manual com transcrição indisponível e recuperação de uma falha de escrita. Não marcar a fase como concluída antes dessa validação. A fase 3 (tutor com IA) foi antecipada por autorização explícita do utilizador e tem implementação e validação registadas em [docs/phase-3/validation.md](docs/phase-3/validation.md); a aceitação manual continua pendente. A fase 4 tem apenas desenho e plano técnico; a fase 5 continua por planear.

A fase 3 tem implementação e verificação automática verde em 2026-11-02: 8/8 checks (175 Rust, 57 Vitest, 36 Python), build release --no-bundle e smoke com dados temporários/saída 0. Ver [validação da fase 3](docs/phase-3/validation.md). Inferência paga, credencial protegida real, áudio real e utilidade semântica continuam por validar. Não marcar a fase como aceite nem iniciar a fase 4 automaticamente.


Em 2026-11-03, o utilizador pediu a spec e o plano da fase 4 e, depois, autorizou executá-lo («termine as tarefas»). A fase 4 (melhorar e repetir) está implementada (esquema v3, práticas, reformulação B1/B2/C1, voz da sugestão, comparação local, histórias locais, consumo de treino) e a verificação automática está verde: `.\scriptserify.ps1` com 8/8 checks (246 Rust, 97 Vitest, 36 Python), build release `--no-bundle` e smoke com dados temporários/saída 0. Ver [validação da fase 4](docs/phase-4/validation.md). A fase 4 **não** está aceite nem concluída: faltam o roteiro manual F4-16, a inferência paga real (fidelidade, utilidade, espera, custo) e as pendências das fases anteriores. Não iniciar a fase 5 sem instrução.

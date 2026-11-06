# Orientações para trabalhar neste projeto

## Contexto e documentos

Este projeto é um tutor desktop de entrevistas comportamentais em inglês, por voz, baseado na estrutura STAR. O desenvolvimento está organizado em fases; as fases 1 e 2 estão implementadas e verificadas automaticamente, com a validação manual do utilizador ainda pendente. A fase 3 foi antecipada por autorização explícita do utilizador em 2026-11-02; implementação e verificação automática não equivalem a aceitação manual.

Antes de trabalhar, consultar os documentos relevantes para a tarefa:

- [Validação do layout](docs/layout/validation.md): referência visual, adaptações aos contratos, recuperação áudio em todas as páginas e matriz reproduzível de sete páginas.
- [Avaliação e correções por pergunta](docs/ai-coaching/validation.md): alteração aprovada em 2026-11-04; avaliação e reformulação durante a entrevista IA, preservação das fontes, reutilização no relatório final e correção da inicialização do editor.

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
- [Integração OpenCode Go](docs/opencode-go/validation.md): modalidade autorizada em 2026-11-04 (endpoint `/inference/go/openai/v1`, identidade própria do tutor, sessão estável por conversa e adaptação única da configuração antiga), com o que está verificado e o que falta confirmar no fornecedor.
- [Conversa sobre um artigo](docs/article-conversation/setup.md): modo aprovado em 2026-11-04 (conversa em inglês sobre um artigo colado, envio automático autorizado por sessão, feedback final e correções em ramos), com [validação](docs/article-conversation/validation.md), [desempenho](docs/article-conversation/performance.md), [spec](docs/superpowers/specs/2026-11-04-article-conversation-design.md) e [plano](docs/superpowers/plans/2026-11-04-article-conversation.md).

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
- DeepSeek Flash através do OpenCode Go para seguimentos e avaliação textual (modalidade autorizada em 2026-11-04, substituindo o default por consumo; ver [integração Go](docs/opencode-go/validation.md)).
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

- O diagnóstico local e a identidade de builds estão descritos na [spec](docs/superpowers/specs/2026-11-04-local-diagnostics-design.md), no [plano](docs/superpowers/plans/2026-11-04-local-diagnostics.md) e na [validação](docs/diagnostics/validation.md). Preservar a identidade Tauri e `app_data_dir()/diagnostics` entre versões. Não registar mensagens livres, credenciais, áudio ou conteúdo das entrevistas; novas operações/códigos exigem atualização das listas fechadas. A distribuição por instalador continua por validar.

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


Em 2026-11-03, o utilizador pediu a spec e o plano da fase 4 e, depois, autorizou executá-lo («termine as tarefas»). A fase 4 (melhorar e repetir) está implementada (esquema v3, práticas, reformulação B1/B2/C1, voz da sugestão, comparação local, histórias locais, consumo de treino) e a verificação automática está verde: `.\scripts\verify.ps1` com 8/8 checks (246 Rust, 97 Vitest, 36 Python), build release `--no-bundle` e smoke com dados temporários/saída 0. Ver [validação da fase 4](docs/phase-4/validation.md). A fase 4 **não** está aceite nem concluída: faltam o roteiro manual F4-16, a inferência paga real (fidelidade, utilidade, espera, custo) e as pendências das fases anteriores. Não iniciar a fase 5 sem instrução.

Em 2026-11-04, o utilizador aprovou e autorizou avaliação, correções explicadas e reformulação por pergunta durante a entrevista IA. Implementação verificada automaticamente: verify.ps1 com 8/8 checks (252 Rust, 112 React e 36 Python), TypeScript/build Vite, formatação/Clippy e pip check aprovados. Avaliação só após confirmação e seguimento encerrado; o relatório final reutiliza resultados válidos. Corrigida a reconciliação de rascunho vazio quando chega a transcrição automática. Ver [validação](docs/ai-coaching/validation.md). Inferência paga, utilidade semântica, áudio real e aceitação manual continuam pendentes; não inicia a fase 5.

Em 2026-11-04, depois de comparar o contentor do Marlo Expenses, o utilizador pediu a mesma modalidade («utilize o msm forma do Marlo Expense»). A integração passou a OpenCode Go: endpoint `/inference/go/openai/v1`, `User-Agent` próprio do tutor e `x-opencode-session` estável por entrevista ou prática, sem copiar chave ou identidade do Marlo. O default deixou de ter tarifário fixo; a configuração antiga de referência é adaptada uma vez, preservando chave e histórico e exigindo nova autorização do destino. Verificação automática: 8/8 checks (270 Rust, 119 React, 36 Python), build release `--no-bundle` com saída 0 e smoke do binário com código 0. Ver [validação Go](docs/opencode-go/validation.md). Continua por verificar se o fornecedor aceita tráfego do tutor na modalidade Go, o custo real e a utilidade semântica; não houve pedido pago. Não inicia a fase 5.

Em 2026-11-04 foi publicado o trabalho anterior: commit `d3f66c4`, merge em `main` (`c3cba56`) e release [v0.1.0](docs/releases/v0.1.0.md) com instalador NSIS, MSI e executável portátil. O build dos instaladores saiu com código 0 e o binário passou o mesmo smoke (janela viva, fecho com código 0). O **instalador não foi instalado**: instalação, atualização por cima de uma versão anterior e desinstalação continuam por validar. As pendências de aceitação manual das fases 1 a 4 mantêm-se; a fase 5 não foi iniciada.

Em 2026-11-04, a pedido do utilizador, o auxiliar passou a carregar os modelos ao abrir a entrevista (`preload` no protocolo v1), para eliminar a espera do primeiro «Ouvir pergunta» (Kokoro, ~8 s) e da primeira transcrição. Medições nesta máquina: Whisper 1,2 s/690 MiB, Kokoro 8,2 s/1,35 GiB, ambos ~10 s e 1,7 GiB de pico. O arranque da aplicação continua sem carregar pesos; enquanto a disponibilidade está em `loading`, os botões de áudio esperam em vez de receberem `BUSY`. Ver [stack](docs/tech-stack.md) e [README](README.md).

Em 2026-11-04 foi publicada a release [v0.2.0](docs/releases/v0.2.0.md), com os modelos de voz carregados ao abrir a entrevista e a validação local da chave OpenCode (`oc_sk_…`) antes de qualquer pedido. O diagnóstico do `AI_CREDENTIAL` está em [validação Go](docs/opencode-go/validation.md): a chave guardada tinha o formato `sk_<uuid>` e o fornecedor respondia 401 nos dois endpoints. Verificação 8/8 checks (275 Rust, 120 Vitest, 39 Python), instaladores NSIS/MSI e portátil com hashes no documento da release, smoke do portátil com código 0. A instalação dos pacotes, a atualização por cima da v0.1.0, a aceitação manual e a inferência paga continuam por validar.

Em 2026-11-05 foi implementada a conversa sobre um artigo (plano `2026-11-04-article-conversation`, tarefas 1 a 9). Verificação automática: `verify.ps1` com 8/8 checks (354 Rust, 147 Vitest, 56 Python sem a marca `models`), build release `--no-bundle` e ensaio com dados temporários com saída 0. O modo **não** está aceite: faltam a conversa real com microfone, voz Kokoro e inferência paga, as medições de transcrição/voz (`beam_size` mantém o baseline) e a validação visual da página Artigos. Ver [validação](docs/article-conversation/validation.md). Não inicia a fase 5.

Em 2026-11-05 foi publicada a release [v0.3.0](docs/releases/v0.3.0.md), com a conversa sobre um artigo (instaladores NSIS/MSI e portátil com hashes no documento; smoke do portátil com código 0). A conversa sobre artigos não tem aceitação manual, a instalação dos pacotes continua por validar e a fase 5 não foi iniciada.

Em 2026-11-06, a pedido do utilizador, o repositório `tlpacifico/language-assistant-tutor` passou de **privado a público**, sob licença MIT (`LICENSE`). Antes da mudança foi auditado todo o histórico: nenhuma chave ou credencial real, nenhum ficheiro sensível versionado e nenhum blob acima de 230 KiB; as chaves `oc_sk_…` do código são fixtures de teste. A preparação deixou de expor o nome de utilizador do Windows nos documentos da fase 1 e nos exemplos dos contratos do auxiliar (`C:\Users\exemplo`). O email `thacio.pacifico@gmail.com` continua visível nos metadados dos commits, por decisão do utilizador de não reescrever o histórico. A visibilidade não altera o estado das fases: 1 a 4 sem aceitação manual, fase 5 não iniciada.

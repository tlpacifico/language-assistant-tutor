# Validação — avaliação e correções por pergunta

Data: 2026-11-04. Implementação autorizada pelo utilizador («pode avancar»).
O [desenho](../superpowers/specs/2026-11-04-per-question-ai-coaching-design.md) e o [plano](../superpowers/plans/2026-11-04-per-question-ai-coaching.md) descrevem esta alteração às fases 3 e 4. A aceitação manual e a inferência paga continuam pendentes.

## Alteração

Depois de confirmar a resposta e encerrar o seguimento, o modo IA apresenta **Avaliar e sugerir melhorias**. O clique pede avaliação da pergunta e, se esta ficar válida, uma reformulação no nível guardado (B1 inicialmente). Correções explicadas, fontes e versão melhorada ficam separadas do original. A consulta não chama o fornecedor; o feedback final reutiliza os relatórios válidos. A criação de práticas continua a exigir sessão concluída.

O núcleo acrescenta `interview_ai_evaluate_question`, reutilizando a finalidade `feedback`, as tabelas existentes e as verificações de consentimento, fontes e consumo. Não existe nova migração nem dependência da aplicação. A tarefa de avaliação continua a publicar o resultado se o frontend deixar de aguardar. Ao sair da pergunta, a interface não inicia posteriormente a reformulação a partir desse clique.

Corrigida também a reconciliação do editor: uma revisão automática recebida depois de existir um rascunho vazio preenche texto, versão e origem. Uma edição local por guardar é conservada. O teste de regressão reproduziu o editor vazio e o aviso manual observados na imagem.

## Evidência automática

- `.\scripts\verify.ps1`: **8/8 verificações, saída 0, 41,8 s** na execução final. **252 testes Rust**, **112 testes React em 13 ficheiros**, **36 testes Python** sem a marca `models` (2 desselecionados), `pip check` limpo, TypeScript, build Vite, `cargo fmt --check` e Clippy com `-D warnings` aprovados.
- `git diff --check`: limpo. Conferência dos links locais dos sete documentos funcionais atualizados: válidos; os **56 comandos** Tauri invocados pelo cliente estão registados no Rust.
- Na primeira execução agregada, `story_marker_never_leaks_into_sources_or_answers` falhou porque o cenário semeava sessão IA concluída sem etapa de seguimento encerrada. O cenário passou a registar a dispensa explícita, mantendo a proteção do núcleo e a verificação de que notas não entram nas fontes. O teste isolado e a execução agregada seguinte passaram.
- As tentativas iniciais de executar vários comandos Cargo enquanto outros testes ainda usavam os mesmos executáveis encontraram LNK1104/erro Windows 32. Os comandos seguintes decorreram sequencialmente; a execução final não apresentou esses erros.

- Testes novos do núcleo em `ai_service`: pergunta encerrada elegível antes do fim; seguimento por confirmar bloqueia fontes definitivas; reformulação guardada e reutilizada; avaliação reutilizada no relatório final; ID usado na dispensa recusado; resultado atrasado mantém origem e não bloqueia a captura seguinte.
- Testes React em `QuestionCoaching`: clique explícito, correções e sugestão separadas, retoma sem envio, falha parcial sem repetir avaliação, sair da pergunta sem reformulação atrasada, seguimento pendente e consumo parcial após falha.
- Regressão em `Interview`: transcrição recebida depois de rascunho vazio preenche editor e confirma com origem/transcrição corretas. Os testes existentes conservam edições locais durante escritas e transcrições.
- Revisão estática da alteração completa: sem problemas concretos identificados; não substitui testes nem aceitação manual.

As simulações usam os adaptadores controlados existentes: SQLite e artefactos temporários reais, transporte suspenso/com guião e credenciais falsas. Os testes HTTP existentes continuam a verificar o transporte. Não foram consultadas credenciais pessoais nem feitas chamadas pagas.

## Roteiro manual — pendente

1. Executar o checkout atualizado com `npm.cmd run tauri -- dev`. Um binário release já aberto mantém a implementação com que arrancou; esta verificação não o substitui.
2. Configurar a chave protegida e o consentimento pela aplicação. Começar uma entrevista **Tutor com IA** com duas perguntas; gravar, conferir a transcrição sem corrigir os erros de inglês efetivamente ditos e confirmar.
3. Resolver o seguimento (responder/confirmar quando existir). Pedir **Avaliar e sugerir melhorias**; conferir citações, correções explicadas, separação de estilo e fidelidade factual da versão melhorada.
4. Ouvir a sugestão; alternar B1/B2/C1 por pedidos explícitos. Confirmar que consultar versões guardadas não volta a cobrar.
5. Avançar e terminar a entrevista; gerar feedback em falta e conferir que a primeira avaliação foi reutilizada.
6. Simular indisponibilidade de rede e retomar: os originais e resultados válidos mantêm-se, uma reformulação falhada não obriga a avaliar novamente e não existem reenvios automáticos ao abrir o painel.
7. Medir espera, utilidade e consumo real. Confirmar custo desconhecido como desconhecido e comparação com faturação. Os testes automáticos não validam a utilidade semântica do fornecedor.

Esta alteração não aceita as fases 1–4, não valida áudio real nem distribuição e não inicia a fase 5.

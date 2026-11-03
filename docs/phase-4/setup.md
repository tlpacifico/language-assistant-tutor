# Preparação e execução da fase 4

Data: 2026-11-03. A fase 4 (melhorar e repetir) foi implementada a pedido do utilizador («termine as tarefas») depois de a spec e o plano estarem revistos. **Não está aceite nem concluída**: as aceitações manuais das fases 1, 2 e 3 continuam pendentes e a fase 4 tem pendências próprias em [validation.md](validation.md). A fase 5 não foi iniciada.

Documentos: [spec](../superpowers/specs/2026-11-03-phase-4-improve-repeat-design.md) e [plano técnico](../superpowers/plans/2026-11-03-phase-4-improve-repeat-implementation.md).

## Executar

Mantêm-se os pré-requisitos, modelos e configuração IA da [fase 2](../phase-2/setup.md) e da [fase 3](../phase-3/setup.md). Não há dependências novas.

```powershell
npm.cmd install
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"   # se Cargo não estiver no PATH
npm.cmd run tauri -- dev
```

Verificações automáticas e build sem instalador:

```powershell
.\scripts\verify.ps1
npm.cmd run tauri -- build --no-bundle
```

## O que muda no percurso

- **Treino posterior** (a partir de uma pergunta confirmada de uma sessão concluída): ver a origem (resposta inicial, seguimento ou tentativa anterior), pedir uma reformulação em B1, B2 ou C1, ouvir a sugestão com Kokoro, abrir uma **prática** (nova tentativa) e comparar. B1 é o valor inicial; o nível é um objetivo de linguagem e não um diagnóstico CEFR.
- Uma prática tem gravação, transcrição automática, revisão e confirmação próprias. Não reabre a sessão original, não altera `position` nem o contador de perguntas e nunca cria seguimento. Só existe uma prática não confirmada de cada vez e só se começa quando não há entrevista em curso. Pode ser retomada ou abandonada (sem eliminação).
- **Avaliação da prática** e **reformulação** são pedidos explícitos, nunca automáticos ao confirmar, com uma chamada remota de cada vez e sem retries pagos automáticos. Os resultados válidos são imutáveis; um pedido repetido para a mesma origem/nível/versões reutiliza o resultado.
- **Comparação** local entre inicial, seguimento, tentativas e sugestão, com versões e pendências explícitas. Não afirma melhoria por simples diferença textual e não consome API.
- **Histórias** (banco local): título, notas factuais STAR e competências, com validação; não exigem STAR completo. Podem ser escolhidas como apoio de preparação antes de uma gravação de prática. O apoio fica fixado por snapshot no arranque da captura; editar a história depois não altera o snapshot. As histórias são exclusivamente locais: nenhum campo nem identificador de história entra nas cargas HTTP, na transcrição, no editor nem na confirmação.
- **Consumo de treino** apresentado separado do da entrevista original, por IDs distintos, com desconhecido rotulado como desconhecido.

## Persistência

`tutor.sqlite3` passa ao esquema **v3** (`003_improve_repeat.sql`) numa migração transacional (base vazia, v1 e v2; versões futuras recusadas). Novas tabelas: `practices`, `stories`, `story_versions`, `preparations`, `preparation_captures`, `practice_reports`, `evaluation_families`, `rewrites`, `rewrite_audio`, `practice_preferences`; `answer_slots` e `ai_requests` foram reconstruídas para suportar proprietário de prática e novas finalidades. Dados anteriores (sessões, originais, relatórios, recibos) são preservados.

Os áudios continuam locais. Só o texto necessário e os critérios seguem para a API, e apenas depois de confirmação e consentimento já existentes.

## Limitações conhecidas

- Sem chave real nem inferência paga durante a implementação: o transporte foi testado com HTTP de loopback e credenciais falsas.
- `build --no-bundle` não valida instalador nem instalação empacotada.
- Fidelidade factual das reformulações, utilidade, espera, custo real e voz Kokoro dependem de validação humana.

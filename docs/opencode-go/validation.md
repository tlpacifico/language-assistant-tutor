# OpenCode Go — preparação e validação

Data: 2026-11-04. Autorização: «utilize o msm forma do Marlo Expense».
[Desenho](../superpowers/specs/2026-11-04-opencode-go-design.md) e
[plano](../superpowers/plans/2026-11-04-opencode-go.md).

## O que mudou

A comparação com o contentor `marlo-expenses-app` em execução confirmou
`Assistant__BaseUrl=https://opencode.ai/inference/go/openai/v1`, modelo
`deepseek-v4.1-flash`, User-Agent próprio e sessão HTTP estável. Foram consultados
apenas esses campos públicos de configuração; a chave não foi extraída nem copiada.
O tutor passa a usar a mesma modalidade e estrutura de transporte, com a sua própria
identidade: `LanguageAssistantTutor/<versão>` e
`x-opencode-session: language-tutor-<id persistido da entrevista ou prática>`.

A URL continua configurável. O default Go mantém DeepSeek V4.1 Flash e não tem
tarifário fixo. A API pode devolver custo zero para utilização incluída; zero
explícito é conservado, ausência de custo continua desconhecida. O código existente
conserva a origem de faturação quando devolvida. Não se apresentam limites de plano
ou saldo remanescente que a aplicação não tenha recebido.

No primeiro acesso à configuração, a referência antiga (endpoint por consumo exato,
modelo `deepseek-v4.1-flash`) é adaptada transacionalmente ao Go. A versão incrementa,
o consentimento é retirado e o tarifário atual é limpo. A chave do Windows não é
alterada. O marcador `migration:opencode-go:v1` em `ai_receipts` torna a operação
única e permite respeitar escolhas explícitas posteriores. Configurações com outros
destinos ou modelos não são alteradas. Não existe mudança do esquema SQLite.
Tentativas, consumo, configurações fixadas nos pedidos, relatórios e áudios não
são reescritos.

## Utilizar

1. Guardar revisões pendentes e reabrir o executável atualizado.
2. Abrir **Configuração IA**. Na configuração de referência existente, a URL passa
   para `https://opencode.ai/inference/go/openai/v1` e a chave continua configurada
   (a adaptação não toca no Windows Credential Manager).
3. Confirmar que a chave é a da conta com a subscrição Go. Se for outra chave, usar
   **Guardar chave protegida** para a substituir (substituir a chave não invalida por
   si o consentimento; o passo seguinte pede a autorização do destino Go).
4. Conferir o destino e clicar **Autorizar envio de texto para este destino**.
5. Voltar à entrevista e repetir explicitamente o seguimento falhado, ou pedir a
   avaliação elegível. A resposta confirmada já está guardada; não é necessário
   gravá-la novamente. A aplicação não faz esse reenvio automaticamente.

Se o fornecedor responder 402 ou 403 no pedido repetido, o erro identifica o estado
HTTP e o tipo de problema (faturação ou permissões) sem apresentar o corpo devolvido;
nesse caso, confirmar na conta se a chave pertence à subscrição Go. Alterar a URL
manualmente na configuração continua a apagar a chave guardada e a invalidar o
consentimento — é o comportamento anterior, deliberadamente mantido.

A documentação do [Go](https://opencode.ai/v2/docs/console/go/) apresenta o plano
para agentes de programação. Essa condição foi comunicada antes da autorização.
O tutor identifica-se honestamente e não imita tráfego ou identidade de outro
cliente. A aceitação de pedidos do tutor pelo fornecedor permanece por verificar.

## Evidência automática

- Testes focados Rust: transporte HTTP real sobre servidor local, default,
  adaptação/idempotência/reabertura, escolha explícita posterior, preservação de
  destinos/modelos personalizados, chave preservada e nova autorização obrigatória,
  sessão constante numa repetição e prática com identificador próprio. Passaram os
  testes focados da biblioteca, serviço, configuração e consumo. A seleção de headers
  e asserções de migração falharam antes da implementação (o novo campo de contexto
  ainda não existia, o default era por consumo e não havia adaptação).
- Os testes de consumo usam agora um tarifário de teste explícito para preservar a
  verificação das fórmulas, sem depender de preços na configuração por omissão.
- Revisão independente: sem defeitos concretos identificados na alteração Go.
  Foi uma revisão estática; não valida o fornecedor.
- Verificação agregada final em 2026-11-04: `scripts/verify.ps1` com **8/8 checks,
  saída 0 em 50,7 s** (270 Rust, 119 React, 36 Python), com TypeScript, build Vite,
  `cargo fmt --check` e Clippy `-D warnings` aprovados. Repetida depois da última
  alteração nos testes de consumo.
- Build release `npm.cmd run tauri -- build --no-bundle`: saída 0 em 1m20s,
  `language-assistant-tutor.exe` gerado às 14:52:32. A versão anterior ficou
  preservada como `language-assistant-tutor.previous-*.exe`; nenhum processo foi
  encerrado.
- Smoke do binário release com `APPDATA`/`LOCALAPPDATA` temporários e vazios:
  janela viva após 8 s, `CloseMainWindow=True`, saída dentro de 15 s com **código 0**;
  pastas temporárias removidas. Não é validação de instalador.
- `git diff --check` limpo e ligações relativas dos documentos alterados conferidas.
- O que continua por verificar é externo à aplicação: se o fornecedor aceita pedidos
  do tutor na modalidade Go, o custo real apresentado na conta e a utilidade
  semântica das respostas. Nenhum pedido pago foi feito nesta verificação.

Os testes HTTP usam o servidor local já existente e uma chave falsa; exercitam
`reqwest`, serialização e headers reais. Não fazem inferência paga nem consultas
à conta do utilizador. Áudio real, aceitação semântica e validações manuais anteriores
continuam pendentes. Esta alteração não inicia a fase 5.

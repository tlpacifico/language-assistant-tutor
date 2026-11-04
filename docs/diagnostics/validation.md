# Diagnóstico local — preparação e validação

Data: 2026-11-04. Alteração autorizada pelo utilizador: «crie a spec e o plano e faça a implementacao».

[Desenho](../superpowers/specs/2026-11-04-local-diagnostics-design.md) e [plano](../superpowers/plans/2026-11-04-local-diagnostics.md).

## Utilização

Abrir **Diagnóstico local** em qualquer página. Consultar versão, commit, indicação de alterações locais e data da compilação; atualizar o estado após uma falha. O painel também aparece na recuperação de uma falha de renderização React.

**Exportar diagnóstico** escreve um JSON com nome único na pasta indicada, por omissão Downloads. A pasta tem de existir e permitir escrita. O caminho completo aparece após a exportação; o utilizador pode partilhar esse ficheiro para investigação. Não há envio automático nem chamadas pagas.

O executável mantém a versão semântica **0.1.0**. O commit Git e o instante de compilação distinguem builds; `com alterações locais` informa que o executável não corresponde apenas ao commit. Sem Git, a revisão é desconhecida.

## Persistência e privacidade

O caminho mostrado pelo painel é resolvido por Tauri através de `app_data_dir()` com o identificador estável `dev.thacio.languageassistanttutor`. Os cinco ficheiros reservados são `events.jsonl` e `events.1.jsonl` a `events.4.jsonl`, até 1 MiB cada. A retenção usa a idade dos eventos (30 dias), incluindo em ficheiros escritos recentemente: confere no arranque, na primeira escrita após a expiração prevista e em cada exportação. A rotação depende do tamanho, nunca da versão; exportações não são limpas automaticamente. Um lock de ficheiro do sistema operativo coordena escritores de várias instâncias. Não alterar a identidade/pasta ao lançar versões novas.

Os eventos contêm categorias técnicas, códigos fechados, ID de evento/execução, duração quando conhecida e metadata do build. Exceções Python acrescentam apenas nome permitido do ficheiro próprio e número de linha. Mensagens de erro, argumentos IPC, stack bruta, stderr bruto, chaves, configuração, SQLite, áudios e conteúdos das entrevistas não entram nos registos persistidos nem na exportação. O stderr preexistente do auxiliar continua a ser drenado em memória, sem se tornar um ficheiro de diagnóstico.

O escritor usa uma fila de 256 eventos e nunca espera pelo disco para registar um erro. O painel informa falhas de escrita e perdas nesta execução; o JSON exportado inclui também esse estado e a identificação da execução atual. A exportação aguarda os eventos já enfileirados; com fila cheia ou disco indisponível, pode falhar e ser repetida. As chamadas de consulta/exportação correm em `spawn_blocking`.

## Evidência automática

- `.\scripts\verify.ps1`: **8/8 checks aprovados, saída 0, 146,7 s**. **262 testes Rust** (o cenário do auxiliar executa ainda um processo filho, sem contar esse subteste duas vezes), **117 testes React em 15 ficheiros**, **36 testes Python** sem a marca `models` (2 desselecionados), TypeScript/build Vite, formatação, Clippy com `-D warnings` e `pip check` aprovados.
- Nove testes de armazenamento exercitam reabertura com outra versão, exportação após eventos enfileirados, privacidade, rotação, retenção por ficheiro e por evento, falha/recuperação de escrita, perdas conhecidas, escritores concorrentes e respeito pelo lock do sistema operativo. Um teste de integração do supervisor comprova a persistência de uma rejeição assíncrona sem mensagem privada.
- Cinco testes React cobrem rejeições IPC sem argumentos/mensagem, falha do próprio diagnóstico, captura global, exportação para a pasta indicada e recuperação de renderização React. O teste Python de falha inesperada confere também o canal estruturado sem texto privado.
- Revisão independente encontrou quatro problemas concretos (idade de eventos, rotação concorrente, rejeições assíncronas do auxiliar e perdas omitidas na exportação). Foram corrigidos; testes reproduziram as falhas de retenção, lock, rejeição e metadata antes das correções e passaram depois. O ensaio concorrente verifica também o resultado após a correção. Uma execução intermédia do agregado parou numa comparação booleana de teste recusada por Clippy; corrigida para `assert!`, seguida do agregado verde acima.
- `git diff --check` limpo e links locais dos documentos afetados conferidos. Sem novas dependências e sem alteração da versão 0.1.0 ou identidade Tauri.
- Build release isolado com `--no-bundle --config tmp/diagnostics-work/smoke-config.json`: saída 0, Cargo em 1m38s. Ensaio num **WebView2 real** através de CDP/Playwright: painel abriu, mostrou build, capturou erro JavaScript e exportou pela interface; a mensagem `PRIVATE_SMOKE_RESPONSE` ficou ausente do JSON. Três execuções consecutivas conservaram 3, 6 e 9 eventos, com 1, 2 e 3 IDs de execução distintos, estado de escrita disponível e zero perdas. Cada processo fechou com `CloseMainWindow=True`, **saída 0**. O script versionado foi executado na terceira passagem. Captura inspecionada em `tmp/diagnostics-work/webview/diagnostics-webview.png`.
- A configuração de ensaio substitui explicitamente as pastas da aplicação, mantendo a identidade; os dados e perfil WebView ficam sob `tmp/diagnostics-smoke-data`. Não houve gravações nem inferência paga. O build final normal é gerado novamente sem esse override; não foi criado instalador.
- **Build final normal**: `npm.cmd run tauri -- build --no-bundle`, saída 0, Cargo em **1m01s**. Confirmada a ausência do caminho de dados do ensaio no executável final (presente no executável isolado usado como controlo). Artefacto: `src-tauri/target/release/language-assistant-tutor.exe`. A configuração versionada mantém versão 0.1.0 e identidade original. Após fortalecer a asserção de remoção no disco, a bateria focada de diagnóstico voltou a passar: 9/9, saída 0.

## Reproduzir o ensaio isolado

Executar a partir da raiz, com Cargo no PATH e Python/Playwright disponíveis (a mesma preparação opcional do [ensaio de layout](../layout/validation.md)). Estes scripts são ferramentas de desenvolvimento; não são dependências do auxiliar de áudio.

```powershell
$smokeRoot = [System.IO.Path]::GetFullPath((Join-Path (Get-Location) 'tmp/diagnostics-smoke-data'))
New-Item -ItemType Directory -Force tmp/diagnostics-work | Out-Null
@{ app = @{ appDirectoriesOverride = $smokeRoot } } |
  ConvertTo-Json -Depth 5 |
  Set-Content tmp/diagnostics-work/smoke-config.json -Encoding utf8
npm.cmd run tauri -- build --no-bundle --config tmp/diagnostics-work/smoke-config.json
# Prosseguir só se o build isolado terminar com saída 0.
Copy-Item src-tauri/target/release/language-assistant-tutor.exe tmp/diagnostics-work/diagnostics-smoke.exe
.\scripts\diagnostics-smoke.ps1 -Executable tmp/diagnostics-work/diagnostics-smoke.exe
# Repetir o comando anterior para conferir a preservação entre execuções.
# Repor sempre o artefacto normal antes de usar/distribuir a aplicação:
npm.cmd run tauri -- build --no-bundle
```

O [orquestrador PowerShell](../../scripts/diagnostics-smoke.ps1) inicia apenas o executável indicado, com janela oculta e depuração numa porta local temporária, chama o [ensaio Python](../../scripts/diagnostics-smoke.py) e fecha o processo. O Python verifica o caminho de dados antes de provocar o erro. Só usar esse orquestrador com o build explicitamente isolado acima. Os ficheiros de ensaio ficam em `tmp/diagnostics-work`; a configuração normal versionada não é alterada.

## Validação manual pendente

1. Executar o build atualizado e exportar para uma pasta existente. Confirmar o caminho e o conteúdo do JSON.
2. Usar microfone, transcrição e IA reais; provocar uma indisponibilidade recuperável e conferir os códigos e a preservação dos originais.
3. Fechar, substituir o executável por uma versão posterior e voltar a abrir: conferir eventos antigos e novos na exportação. O teste automático de reabertura não valida o instalador.
4. Na futura distribuição, instalar uma atualização real e conferir identidade, diretório, sessões e registos. Desinstalação, limpeza manual do perfil e remoção explícita dos dados ficam fora da garantia de atualização.

O diagnóstico é de melhor esforço: terminações forçadas, falhas nativas e eventos ainda na fila podem não ficar registados. Não aceita as fases 1–4 nem inicia a fase 5.

## Correção de faturação e erros ocultos — 2026-11-04

Decisão posterior: o utilizador pediu a modalidade Go usada no Marlo Expenses.
O erro `InsufficientFunds` observado correspondia à rota por consumo, que era
diferente da rota `/inference/go/...` do Marlo. A integração atual e a adaptação
da configuração estão descritas em [OpenCode Go](../opencode-go/validation.md).

Na primeira utilização, três tentativas de seguimento ficaram `failed`, com consumo desconhecido. A leitura apenas dos metadados de pedidos e do diagnóstico confirmou `AI_ACCESS`, com a mensagem «Acesso recusado ou saldo insuficiente». O transporte agrupava HTTP 402 e 403; os registos antigos não permitem recuperar o estado HTTP exato. O utilizador confirmou na consola **HTTP 402, `Account.InsufficientFunds` / `Insufficient account funds`**. A documentação oficial confirma [HTTP 402 por falta de crédito ou problemas de faturação](https://opencode.ai/v2/docs/console/billing/).

Corrigidos dois problemas de diagnóstico da aplicação:

- HTTP 402 passa a `AI_BILLING`, com instrução para verificar saldo e faturação. HTTP 403 mantém `AI_ACCESS`, com instrução para verificar permissões da chave/modelo. As mensagens identificam o estado HTTP sem incluir o corpo devolvido pelo fornecedor. `AI_BILLING` faz parte da lista fechada do diagnóstico.
- O painel final apresenta o motivo guardado da última tentativa falhada, incluindo registos antigos, e traduz os estados das tentativas. Os detalhes de cada tentativa conservam o seu erro; erros ilegíveis têm uma mensagem genérica, sem conteúdo bruto. A consulta não repete pedidos.

Os pedidos históricos não são reclassificados nem reenviados; o custo desconhecido continua desconhecido. Resolver o saldo é uma ação do utilizador no fornecedor. Depois, a repetição explícita usa a resposta confirmada já guardada, sem exigir nova gravação. Não foram lidas credenciais protegidas, feitas chamadas pagas ou alterados os dados pessoais.

Os testes de regressão reproduziram primeiro as falhas: duas verificações React não encontravam o motivo; o transporte devolvia `AI_ACCESS` para HTTP 402; o diagnóstico descartava `AI_BILLING`. Depois da correção, os cinco testes do ficheiro React e a simulação HTTP 402/403 passaram. O servidor HTTP local existente verifica também a ausência do conteúdo sensível do fornecedor. Verificação agregada final: `scripts/verify.ps1`, **8/8 checks, saída 0, 135,8 s**; **263 testes Rust, 119 React e 36 Python**, TypeScript/build Vite, formatação/Clippy e pip check aprovados. Build release atualizado concluído com saída 0 (1m10s). A versão anterior ficou preservada com outro nome na pasta release, o que permitiu gerar o novo executável sem encerrar a janela do utilizador nem perder rascunhos. A alteração posterior para OpenCode Go reconstruiu o mesmo binário; ver [validação Go](../opencode-go/validation.md).

# Validação da fase 3

Modalidade atual, autorizada em 2026-11-04: [OpenCode Go](../opencode-go/validation.md), com identidade própria e sessão estável. Substitui o default por consumo do registo original, preservando pedidos históricos; não constitui aceitação manual da fase.

Atualização de 2026-11-04: a avaliação por pergunta durante a entrevista e a reutilização no relatório final têm evidências próprias na [validação do coaching](../ai-coaching/validation.md). Esta extensão não altera as pendências de aceitação manual desta fase.

Data: 2026-11-02. Estado: implementação verificada automaticamente; revisão técnica do coordenador concluída; aceitação manual pendente. A antecipação da fase 3 foi autorizada pelo utilizador; não aceita nem conclui as fases 1 e 2.

## Evidência automática

Os testes usam SQLite e ficheiros temporários reais, áudio simulado, credenciais de instância falsas e HTTP de loopback controlado. Não usam chave paga, microfone pessoal nem respostas profissionais reais.

| Critério | Evidência | Pendência humana |
| --- | --- | --- |
| F3-01 | `ai_migration`, `database`, regressões `sessions/answers/service/artifacts`, contratos v2 | Percurso local real após atualização |
| F3-02 | `ai_service::settings_remove_secret_and_consent_on_destination_change`, UI limpa campo em falha; adaptador Windows sem fallback | Exercitar credencial protegida na máquina |
| F3-03 | Teste sem confirmação/consentimento não chama rede; teste HTTP inspeciona body/auth | Conferir aviso e conta real |
| F3-04 | Testes de seguimento único, gravação própria, contador e bloqueio de segundo pedido | Utilidade da pergunta |
| F3-05 | `ai_service` grava e confirma inicial e seguimento separadamente; regressão de áudio/transcrição/revisão | Gravações reais e reprodução Kokoro |
| F3-06 | Barreira HTTP permite dispensar, avançar e gravar seguinte; conteúdo tardio descartado | Repetir pela janela |
| F3-07 | Recibo repetido durante rede não duplica chamada; confirmação e avanço preservam regras existentes | Percurso com cliques e retoma reais |
| F3-08 | Reinício torna pedido interrompido sem reenvio; fecho não espera por HTTP nem continua batch | Encerrar/reabrir janela em cada etapa |
| F3-09 | `ai_validation` verifica Unicode literal, papel e fonte; UI apresenta fontes distintas | Fidelidade semântica do feedback |
| F3-10 | Ausência exige todas as fontes; citações alteradas/prioridades órfãs/campos extra são recusados | Avaliar ausências sugeridas |
| F3-11 | Fixtures com falha/aprendizagem; instruções da resposta ficam na mensagem de dados; sem nota global/campos acústicos | Respostas qualitativas/negativas e instruções maliciosas reais |
| F3-12 | Sessão de três: relatório 1 válido, 2 falha; retry pede só 2 e 3; estado parcial/pronto | Consulta e retoma no histórico |
| F3-13 | `ai_usage`: cache, desconhecido, zero explícito, modelo diferente e subtotal parcial; consumo tardio registado | Comparar com faturação real |
| F3-14 | HTTP 401/403/429/500, timeout, JSON inválido, excesso sem Content-Length; falha de publicação injetada e retry sem nova rede; rollback da migração | Falhas reais de rede/armazenamento |
| F3-15 | Teste com HTTP suspenso mantém dispensa/captura seguinte; teste React integrado do seguimento | Responsividade percebida |
| F3-16 | Não substituível por automação | Sessão real de três principais e custo/espera/utilidade |

## Comandos

Em 2026-11-02, `scripts/verify.ps1` terminou com **8/8 checks e código 0 em 77,6 s**: **175 testes Rust, 57 Vitest, 36 pytest sem models**, build frontend, formatação, clippy com -D warnings e pip check limpo. Uma execução anterior deu 8/8 em 96,2 s.

`npm.cmd run tauri -- build --no-bundle` terminou com código 0; Cargo release inicial em 2m00s e rebuild final após a correção do fecho em 1m18s. O binário `src-tauri/target/release/language-assistant-tutor.exe` foi iniciado com APPDATA/LOCALAPPDATA temporários e configuração de áudio inexistente; `CloseMainWindow=True`, saída dentro do prazo de 15 s e **código 0**. Nenhum dado pessoal foi migrado pelo smoke. Não é validação de instalador.

Os comandos específicos e limitações ficam no [relatório](implementation-report.md).

## Resultado manual comunicado pelo utilizador — 2026-11-03

O utilizador comunicou: «fiz o teste manual, está tudo bem» e esclareceu que não utilizou a API paga. Fica registado o resultado favorável do percurso testado, sem inferência paga real. A integração com o fornecedor, a utilidade dos seguimentos, a fidelidade semântica do feedback e o consumo/custo reais continuam por validar. O relato não discrimina os restantes cenários dos roteiros das três fases, incluindo falhas e recuperação; não se inferem medições, proteção efetiva de credenciais ou cobertura individual desses critérios. A aceitação manual completa continua pendente.

## Roteiro manual pendente

1. Completar a validação manual das fases [1](../phase-1/validation.md) e [2](../phase-2/validation.md), que continuam pendentes.
2. Configurar chave/saldo na janela, confirmar proteção do segredo e o destino autorizado. Usar texto de teste antes de dados pessoais. Não enviar chaves por chat.
3. Sessão IA de três principais: resposta completa, outra incompleta e outra com termos técnicos e resultado qualitativo/negativo. Ouvir perguntas, originais iniciais e seguimentos. Rever e confirmar cada alvo.
4. Conferir que há no máximo um seguimento por principal, que não repete informação respondida e que não exige números ou sucesso. Medir espera e avaliar aceitabilidade.
5. Falhar rede após confirmação: repetir uma vez e, noutra principal, dispensar explicitamente. Restabelecer ligação e verificar ausência de seguimento atrasado.
6. Sair e retomar durante decisão, seguimento e feedback; confirmar que não há reenvio automático nem nova gravação exigida.
7. Gerar feedback, verificar citações e origem inicial/seguimento, ausências e inglês separado de STAR. Interromper o segundo relatório e repetir só os que faltam.
8. Consultar o histórico durante outra sessão. Comparar modelo, consumo conhecido e estimativas com a conta real; conservar desconhecidos como tal.
9. Exercitar falha de escrita após resposta recebida e a publicação local sem novo pedido. O teste automático cobre esta condição; o exercício real requer um cenário controlado sem pôr dados pessoais em risco.

Não marcar a fase como concluída/aceite antes desta evidência. Build `--no-bundle` não valida um instalador nem uma instalação empacotada.

Revisão final do coordenador: correção da corrida de fecho conferida; execução independente final de scripts/verify.ps1 com 8/8 checks, saída 0 em 42,7 s (175 Rust, 57 React, 36 Python). O build final após a correção terminou com saída 0 em 1m18s; o arranque/fecho isolado terminou com código 0. Aceitação manual e inferência paga continuam pendentes.

Configuração local em 2026-11-03: a pedido do utilizador, foi configurado o modelo deepseek-v4.1-flash e a base HTTPS do OpenCode. A chave foi guardada no Windows Credential Manager no alvo usado por keyring e a leitura foi verificada em memória, sem mostrar o segredo. A configuração pública foi persistida sem a chave. Não houve chamada de rede, inferência paga ou teste de saldo/autenticação no fornecedor. A autorização de envio de texto continua por confirmar na janela da aplicação.

Em 2026-11-04 foi corrigida a apresentação de erros de pedidos e a distinção HTTP 402 (faturação) / 403 (acesso). O incidente real foi confirmado pelo utilizador como `Account.InsufficientFunds`; não foram feitas novas chamadas pagas. Ver [diagnóstico e validação](../diagnostics/validation.md#correção-de-faturação-e-erros-ocultos--2026-11-04).

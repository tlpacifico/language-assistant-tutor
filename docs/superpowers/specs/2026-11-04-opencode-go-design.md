# Integração OpenCode Go como no Marlo Expenses

O utilizador autorizou em 2026-11-04: «utilize o msm forma do Marlo Expense», depois da comparação do contentor em execução. A modalidade passa de pagamento por consumo para OpenCode Go, mantendo `deepseek-v4.1-flash`.

- Endpoint de referência: `https://opencode.ai/inference/go/openai/v1`. Autenticação Bearer no Rust, com a chave que já existe no armazenamento protegido.
- `User-Agent: LanguageAssistantTutor/<versão>` e `x-opencode-session: language-tutor-<id>`; usar ID persistido da entrevista, ou da prática quando existe, estável entre pedidos, repetições e reinícios. Nunca copiar o ID de sessão, o User-Agent ou a chave do Marlo. Não simular tráfego de programação.
- Adaptar uma única vez a configuração antiga de referência (endpoint pago exato e modelo DeepSeek de referência). Incrementar versão, limpar consentimento e tarifário atual; conservar a credencial protegida e todos os snapshots/histórico. Registar a adaptação em `ai_receipts`, sem nova versão de esquema. Configurações personalizadas ficam intactas; uma escolha posterior explícita não volta a ser migrada.
- A mudança exige nova autorização de texto na interface para o destino Go; não há reenvios automáticos. Os dados já confirmados podem ser reutilizados por um pedido explícito.
- O default Go não tem tarifário fixo. Conservar consumo e custo devolvidos pelo fornecedor, incluindo zero explícito e origem `go`; ausência de custo continua desconhecida. Não converter a mensalidade ou a utilização incluída num custo por pedido inventado.
- Sem novas dependências, alteração de motores locais ou início da distribuição/fase 5.

Considerou-se apenas trocar a URL, mas isso omite os headers exigidos. Escolheu-se a mesma estrutura de HTTP do Marlo, com identidade própria e IDs reais por conversa. A documentação do Go descreve o produto para agentes de programação; esta limitação foi comunicada ao utilizador antes da autorização. A integração identifica honestamente o tutor.

Verificar headers e caminho em HTTP local real, estabilidade dos IDs na coordenação, migração idempotente/versões/consentimento/preservação de configurações, consumo Go zero e desconhecido, bateria completa e build release. A aceitação do fornecedor e a inferência real ficam por validar na utilização do utilizador; os testes não fazem pedidos pagos.

Fontes consultadas: [Go](https://opencode.ai/v2/docs/console/go/) e [API](https://opencode.ai/v2/docs/console/api/inference/); `ConfiguredAssistantModelClient.cs` e configuração pública do contentor `marlo-expenses-app` em execução (só endpoint, modelo e headers, sem extrair chaves).

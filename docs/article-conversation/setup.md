# Conversa sobre um artigo — preparação e utilização

Modo novo, distinto da entrevista STAR, aprovado em 2026-11-04 ([desenho](../superpowers/specs/2026-11-04-article-conversation-design.md), [plano](../superpowers/plans/2026-11-04-article-conversation.md)). Reutiliza o microfone, os modelos locais, a credencial e o destino OpenCode Go já configurados em «Configuração IA»; não acrescenta dependências.

## Utilização

1. **Configuração IA**: guarde a chave e confirme o destino (`/inference/go/openai/v1`). Sem chave, a autorização é recusada.
2. **Artigos → Nova conversa**: cole o texto em inglês (até 12 000 caracteres e 30 000 bytes; título opcional). Nada é cortado: acima do limite o botão fica desativado.
3. **Autorização por sessão** (nunca vem selecionada): autoriza o envio automático do artigo e da transcrição de cada resposta ao destino mostrado. O áudio permanece neste computador. Alterar o destino ou revogar impede novos envios até nova autorização.
4. O tutor faz uma pergunta aberta (a voz toca sozinha só nas intervenções novas; «Parar voz» interrompe). **Gravar resposta** e **Parar gravação** são ações suas: a gravação nunca termina sozinha.
5. Ao parar, o áudio e o turno ficam guardados e a transcrição segue para o tutor **sem passo de confirmação** (diferente da entrevista STAR; a versão enviada fica marcada «transcrição enviada automaticamente»). Transcrição vazia, falhada ou acima de 8 000 caracteres não é enviada: pede texto manual (versão distinta, «revista»).
6. O tutor responde com um comentário curto e uma pergunta. Sem correções gramaticais entre turnos. À quinta resposta sugere terminar; aos 20 turnos (ou sem margem de contexto) a gravação fica bloqueada e só se pode terminar.
7. **Terminar e ver feedback**: coerência, coesão, desenvolvimento e interação, com citações das suas respostas, «evidência insuficiente» quando não há texto e até três melhorias. Sem respostas, conclui sem relatório. Sem classificação global e sem avaliar pronúncia.
8. **Corrigir esta resposta** (só depois de concluída): cria um ramo novo; o ramo antigo, as versões e o relatório anterior ficam guardados. O relatório novo só é pedido ao carregar em **Repetir** e pode consumir a API.

## Onde ficam os dados

- SQLite (esquema v5): tabelas `article_*`, com fontes, turnos e versões imutáveis, recibos por pedido, ramos e relatórios.
- Áudio e voz temporária: `articles\<sessionId>\<turnId>\original.wav` (e `normalized.wav`) e `articles\<sessionId>\voice-<id>\chunk-NNNN.wav`, ao lado da base de dados. Eliminar a conversa remove a pasta.
- Só o texto necessário segue para o fornecedor; a chave fica no armazenamento protegido e nunca nos logs.

## Recuperação

- Ao abrir a aplicação, a recuperação é **local e sem envios**: pedidos pendentes passam a «interrompidos» (consumo desconhecido), uma gravação a meio fica assinalada e uma transcrição por acabar pede revisão. Repetir é uma ação sua.
- **Retomar** na lista faz a mesma recuperação para uma conversa; abrir, retomar ou voltar à página nunca inicia envios, gravação nem som.

## Comandos de desenvolvimento

```powershell
.\scripts\verify.ps1                                   # todas as verificações automáticas
.\scripts\benchmark-audio.ps1 -Recordings <pasta>      # medição (ver performance.md)
```

Ver [validação](validation.md) e [desempenho](performance.md).

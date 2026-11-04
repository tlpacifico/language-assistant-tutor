# Preparação e execução da fase 3

**Modalidade atual, 2026-11-04:** por pedido explícito do utilizador, a integração passa a OpenCode Go, com endpoint `/inference/go/openai/v1`, identidade própria e sessão HTTP estável. A configuração antiga de referência é adaptada uma vez, sem apagar chave ou histórico; abrir Configuração IA e autorizar o destino Go antes de repetir um pedido. Os exemplos por consumo abaixo são históricos. Ver [preparação/validação Go](../opencode-go/validation.md).

Data: 2026-11-02. A fase 3 foi antecipada por autorização explícita do utilizador. As aceitações manuais das fases 1 e 2 continuam pendentes.

## Executar

Mantêm-se os pré-requisitos e modelos descritos na [fase 2](../phase-2/setup.md). A aplicação chama a API diretamente; não exige instalar o CLI OpenCode.

```powershell
npm.cmd install
npm.cmd run tauri -- dev
```

Se Cargo não estiver no PATH, antes do comando Tauri:

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
```

Na preparação, o modo predefinido continua **Local, sem IA**. Escolher **Tutor com IA** cria uma sessão nova `ai_v3`. Sessões anteriores continuam locais; não são convertidas para avaliações retroativas.

## Configurar IA

1. Abrir **Configuração IA**. A base atual é `https://opencode.ai/inference/go/openai/v1` (OpenCode Go) e o modelo `deepseek-v4.1-flash`. Se a configuração guardada ainda tiver a base por consumo, a aplicação adapta-a uma vez ao abrir a configuração.
2. Guardar configuração e confirmar o modelo. No default Go não há tarifário fixo: um tarifário manual em USD por milhão continua possível e deve registar data e fonte; o consumo e o custo devolvidos pelo fornecedor prevalecem. Os exemplos de preços abaixo são históricos.
3. Introduzir a chave no campo protegido e escolher **Guardar chave protegida**. O núcleo usa o Windows Credential Manager através de `keyring` 3.6.3, service `language-assistant-tutor`, user `opencode-api`. A chave não é devolvida nem guardada no SQLite.
4. Ler o aviso e autorizar o envio para o destino guardado. Alterar a base remove a credencial anterior e invalida o consentimento. Não há envio ao guardar configuração.

Uma falha do armazenamento protegido é explícita; não existe alternativa em texto simples. Fora de Windows, o adaptador de credenciais declara indisponibilidade.

## Percurso

Confirmar uma resposta guarda-a localmente. **Pedir seguimento** é uma ação distinta de envio. A IA recomenda um único seguimento ou avançar; um seguimento tem a sua gravação, transcrição, revisão e confirmação. O contador continua a contar só as perguntas principais.

Se a API falhar, repetir ou dispensar explicitamente o seguimento antes de avançar. Dispensa não significa que a IA considerou a resposta suficiente. O serviço não repete automaticamente pedidos pagos. Um resultado atrasado de etapa dispensada só pode acrescentar consumo conhecido.

Desde a alteração aprovada de 2026-11-04, depois de confirmar a resposta e resolver o seguimento, **Avaliar e sugerir melhorias** permite obter avaliação, correções explicadas e reformulação antes de avançar. A revisão de transcrição é opcional quando existe texto utilizável: serve para corrigir reconhecimento, preservando o inglês dito para avaliação. Confirmar continua a ser apenas uma escrita local. O botão de avaliação é uma ação explícita de envio, sujeita ao consentimento configurado e a custo.

Ao terminar, **Gerar ou repetir feedback em falta** avalia apenas as perguntas sem relatório válido, reutilizando também os relatórios pedidos durante a entrevista. O histórico apresenta feedback parcial/pronto, fontes confirmadas e consumo por tentativa. **Guardar resultado recebido sem novo pedido** repete apenas a publicação local quando a resposta remota já chegou e a escrita falhou. Após crash nesse intervalo, o resultado pode ser desconhecido; não se promete recuperação de memória perdida. Ver [desenho](../superpowers/specs/2026-11-04-per-question-ai-coaching-design.md) e [validação](../ai-coaching/validation.md).

Os áudios ficam locais; só pergunta/critérios e texto confirmado necessário seguem para a API. Não se envia o banco inteiro, rascunhos, ficheiros ou transcrição automática diferente da versão confirmada.

## Persistência e limites

`tutor.sqlite3` passa ao esquema v2 numa migração transacional. `answer_slots` separa resposta inicial e seguimento; os IDs iniciais são os antigos IDs da principal, preservando caminhos. Nos módulos de áudio existentes, a coluna interna legada `session_question_id` passa a referenciar um slot; os DTOs novos expõem `answerId` e o contexto principal. As perguntas principais permanecem exclusivamente em `session_questions`.

Pedidos guardam fontes imutáveis, configuração/tarifário, modelo pedido/devolvido e versões de prompt/rubrica/esquema. Relatórios válidos são imutáveis. No arranque, pedidos em curso tornam-se interrompidos, sem reenvio automático.

Limites: 64.000 bytes UTF-8 por resposta, 16.000 para pergunta/critérios, 160.000 para o corpo de envio, 1 MiB de resposta HTTP e 2.000 para pergunta de seguimento. Timeout total 90 s; ligação 10 s. Não há truncagem silenciosa, redirects, retries automáticos, ferramentas ou streaming.

## Verificações

```powershell
.\scripts\verify.ps1
npm.cmd run tauri -- build --no-bundle
```

Os resultados reais e pendências estão na [validação](validation.md). Não foi efetuada inferência paga nem acedido a credenciais pessoais durante os testes automáticos.

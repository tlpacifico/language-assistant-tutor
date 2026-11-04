# Avaliação e correções por pergunta

Data: 2026-11-04. Estado: aprovado pelo utilizador («pode avancar»); execução e evidências no [plano](../plans/2026-11-04-per-question-ai-coaching.md) e na [validação](../../ai-coaching/validation.md). Aceitação manual pendente.

## Intenção

O utilizador quer que o fornecedor de IA avalie a resposta e proponha as
correções. Concordou com o percurso «gravar → confirmar transcrição → receber
avaliação, correções explicadas e uma versão melhorada», preservando o original.
Não escolheu expressamente entre avaliação por resposta e no fim da entrevista.
A proposta é apresentá-la por pergunta, antes de avançar para a seguinte.

## Abordagens

1. Manter avaliação e reformulação no fim da entrevista: reutiliza o percurso
   atual, mas adia as correções e não permite aplicá-las na pergunta seguinte.
2. Disponibilizar avaliação e reformulação após cada pergunta: reutiliza os
   contratos existentes, adaptando as condições que atualmente exigem sessão
   concluída. Abordagem escolhida através da aprovação deste documento.

## Comportamento proposto

- A revisão da transcrição é opcional quando existe texto automático utilizável.
  Serve para corrigir palavras mal reconhecidas, preservando os erros de inglês
  efetivamente ditos para a IA os avaliar. O texto apresentado para confirmação
  começa com a transcrição automática, salvo revisão já existente.
- A confirmação guarda uma versão imutável localmente. Não implica, por si só,
  um pedido pago. O modo local conserva o funcionamento sem rede.
- No modo IA, mantém-se a decisão de seguimento e o limite de um seguimento por
  pergunta principal. A avaliação fica disponível depois de o seguimento estar
  respondido, dispensado ou considerado desnecessário. Assim não se fixa um
  relatório incompleto antes de conhecer todas as fontes da pergunta.
- Uma ação explícita «Avaliar e sugerir melhorias» inicia primeiro a avaliação
  e depois a reformulação. A interface explica que utiliza a API configurada e
  pode gerar custo; o consentimento para o destino continua obrigatório.
- Mostrar conteúdo/STAR, correções necessárias com trecho original, proposta e
  explicação, e sugestões opcionais de estilo separadas.
- Mostrar uma versão melhorada em inglês, separada do original, com nível B1
  por omissão e reutilização das variantes B1/B2/C1 já existentes. O nível é um
  objetivo de linguagem, não uma classificação do utilizador.
- A reformulação preserva factos e assinala lacunas sem inventar responsabilidades,
  números ou resultados. Não substitui a confirmação, o áudio ou a transcrição.
- A avaliação é uma ajuda opcional antes de avançar: uma falha do fornecedor não
  obriga a regravar nem cria uma nova condição para terminar a entrevista.
- O relatório final reutiliza avaliações válidas por pergunta e pede apenas as
  que faltam; não volta a cobrar relatórios já disponíveis.

## Persistência e recuperação

Reutilizar o fornecedor, os contratos de evidências, pedidos, consumo e
reformulações das fases 3 e 4. Alargar a elegibilidade apenas às perguntas com
fontes confirmadas e etapa de seguimento encerrada, mantendo as restrições das
práticas e as verificações no núcleo Rust. Uma resposta tardia aplica-se apenas
à origem que iniciou o pedido, mesmo que a interface já mostre outra pergunta.

Avaliação e reformulação são operações separadas: se a primeira terminar e a
segunda falhar, conservar a avaliação e permitir repetir apenas a operação em
falta. Não reenviar automaticamente após reinício ou falha. Manter consumo
desconhecido como desconhecido e recuperação de publicação sem nova chamada.

## Verificação necessária

Testar elegibilidade por pergunta, confirmação e seguimento, preservação das
fontes, resultados tardios, reutilização no relatório final, falha parcial entre
avaliação e reformulação, ausência de pedidos automáticos ao abrir uma página e
percurso local. Verificar também a inicialização do editor com transcrição
utilizável e os avisos de origem manual, face à incoerência observada na imagem.

Atualizar a documentação afetada com os resultados reais. A inferência paga, a
utilidade semântica e o áudio real continuam sujeitos a validação específica;
esta alteração não aceita as fases anteriores nem inicia a fase 5.

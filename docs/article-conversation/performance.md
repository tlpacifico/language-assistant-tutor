# Desempenho da conversa sobre artigos

Estado em 2026-11-05: **instrumentação e ferramenta de medição prontas; medições reais por fazer.** Nenhum valor abaixo foi medido com gravações reais; o que existe é o que o código passou a fazer e como o medir.

## O que é medido em cada turno

O serviço regista, em operações fechadas do diagnóstico local (ver [diagnóstico](../diagnostics/validation.md)), só a duração e um código estável — nunca texto, título, caminhos nem áudio:

| Operação | O que mede |
|---|---|
| `article_transcribe` | do pedido ao auxiliar até ao resultado da transcrição |
| `article_reply` | do pedido ao fornecedor até à resposta do tutor ser aplicada (abertura e respostas) |
| `article_first_audio` | do início da síntese até ao primeiro bloco entregue ao player |
| `article_feedback` | do pedido do relatório até ao resultado |

O teste `turn_metrics_use_closed_operations_with_durations_and_no_content` (em `src-tauri/tests/article_service.rs`) verifica que só estas operações aparecem, com duração, e que o conteúdo nunca chega ao registo.

## Medir a transcrição e a voz

```powershell
.\scripts\benchmark-audio.ps1 -Recordings D:\Gravacoes\medicao
```

- A pasta é indicada explicitamente (só os `*.wav` do nível superior); o script recusa as sessões pessoais da aplicação.
- Compara `beam_size=5` (atual) com `beam_size=1` (candidato), mantendo inglês, VAD, modelo `small` e CPU/int8.
- Por configuração e gravação: uma passagem fria (modelo novo) e cinco carregadas. Mostra cada valor, a mediana e o pior tempo. **Não se calcula p95** com cinco amostras.
- Com `-VoiceText` mede também o primeiro bloco de voz e a síntese completa.
- Os resultados só têm durações, ficam numa pasta local e não são enviados nem versionados.

A fidelidade do texto compara-se à mão, com referência humana e termos técnicos, fora do script.

## Decisão

**Manter o baseline (`beam_size=5`).** O parâmetro `beamSize` é opcional no pedido `transcribe` (aceita 1 ou 5) e o default do motor não foi alterado: faltam gravações reais e a comparação de fidelidade que a condicionam. Os valores-alvo só se definem depois de medir.

## Por fazer (exige gravações e modelos reais)

- Correr o script com gravações curtas, longas e com termos técnicos; registar aqui os valores individuais, a mediana e o pior tempo.
- Comparar o texto de `beam_size=1` com o de `beam_size=5` e com a referência humana antes de qualquer mudança de default.
- Avaliar a voz pelo tempo até ao primeiro som e pelas quebras entre blocos (audição).

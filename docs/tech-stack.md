# Stack técnica

## Decisão principal

A aplicação será construída com **Rust + React + TypeScript + Tauri 2**, começando por Windows. Os modelos de áudio continuam num processo Python auxiliar.

O objetivo desta divisão é manter uma interface web, concentrar a lógica e os dados em Rust e utilizar os motores de áudio já escolhidos. Não se assume que a utilização de Rust, por si só, acelere a inferência desses modelos.

## Componentes

| Componente | Tecnologia | Execução e responsabilidade |
| --- | --- | --- |
| Interface | React + TypeScript | Local; perguntas, revisão de transcrições, feedback e histórico. |
| Ferramenta da interface | Vite | Desenvolvimento e compilação dos recursos web. |
| Aplicação desktop | Tauri 2 | Integração da interface com o sistema e distribuição. |
| Núcleo da aplicação | Rust | Sessões, regras, persistência, ficheiros e comunicação externa. |
| Transcrição | faster-whisper | Local, num processo Python auxiliar. |
| Síntese de voz | Kokoro-82M | Local, no processo Python auxiliar. |
| Seguimentos e avaliação | DeepSeek Flash via OpenCode | Nuvem; recebe texto confirmado e critérios. |
| Dados estruturados | SQLite | Local; perguntas, histórias, sessões, respostas e avaliações. |
| Gravações e derivados | Ficheiros de áudio | Pasta de dados local gerida pela aplicação. |
| Chave da API | Armazenamento protegido do sistema operativo | Acesso pelo núcleo, fora do conteúdo do SQLite e do frontend. |

## Responsabilidades e comunicação

### React e TypeScript

Apresentam a experiência do utilizador e comunicam com Rust através dos comandos e eventos do Tauri. A interface não contém a chave do fornecedor nem é a autoridade sobre o estado persistente da entrevista.

### Rust e Tauri

Controlam a quantidade de perguntas, o limite de seguimentos, as transições da sessão, as versões das respostas, o acesso ao SQLite e a gestão dos ficheiros.

Rust chama a API do OpenCode e valida os resultados estruturados antes de os aplicar. Cada resultado fica associado ao pedido, à sessão e à versão de texto que o originaram.

O núcleo gere também a captura local de áudio e o ciclo de vida do processo Python. A biblioteca concreta de gravação e reprodução será escolhida no plano técnico da fase 1.

### Processo Python de áudio

Executa transcrição e síntese, recebendo pedidos de Rust por mensagens JSON delimitadas por linha através de entrada/saída padrão. Cada pedido tem identificador, operação e parâmetros; a resposta inclui resultado ou erro. O diagnóstico segue por stderr.

Os áudios são ficheiros locais referenciados nos pedidos. Os modelos ficam carregados entre operações quando possível. O MVP processa os trabalhos locais de áudio sequencialmente.

O Tauri permite distribuir executáveis auxiliares. O objetivo do pacote final é não exigir que o utilizador configure Python manualmente. O empacotamento das dependências precisa de validação em Windows.

## Fluxo de dados

1. A aplicação captura e guarda a gravação localmente.
2. O processo Python transcreve o áudio com faster-whisper.
3. O utilizador revê e confirma o texto na interface.
4. Rust envia o texto necessário e os critérios ao OpenCode.
5. Rust valida e guarda o seguimento, feedback ou reformulação.
6. Quando é preciso falar um texto, o processo Python gera áudio com Kokoro e a aplicação reproduz o ficheiro local.

Nem os ficheiros de áudio nem o banco completo de histórias são enviados automaticamente ao fornecedor. Transcrição automática, texto confirmado, sugestões e novas tentativas são registos distintos.

## Modelos e formatos locais

- Motor de transcrição: faster-whisper.
- Referência inicial para experimentar: modelo Whisper small multilingue em CPU/int8, ajustável após medição no computador do utilizador.
- Formato entregue à transcrição: WAV PCM de 16 bits, mono, 16 kHz, convertido localmente quando necessário.
- Síntese: Kokoro-82M com uma voz inglesa e velocidade ajustável.
- Guardar a gravação original separadamente de conversões e áudios sintetizados.
- Preparar pesos e dependências com progresso visível ou seleção de ficheiros existentes, conforme o motor.

Os modelos podem necessitar de rede na preparação inicial. Depois de disponíveis, transcrição e síntese funcionam localmente. A avaliação textual continua a depender da ligação à API.

## Fornecedor de IA textual

Configuração de referência verificada em 2026-11-01:

| Parâmetro | Valor |
| --- | --- |
| Fornecedor | OpenCode, API de inferência paga. |
| Modelo | `deepseek-v4.1-flash` |
| Endpoint | `https://opencode.ai/inference/openai/v1/chat/completions` |
| Catálogo | `https://opencode.ai/inference/v1/models` |

O tutor chama a API diretamente. Não depende de instalar o agente/CLI OpenCode. O modelo e a URL base ficam configuráveis; o MVP não depende das condições de acesso dos modelos gratuitos.

A referência consultada indica US$0,30 por milhão de tokens de entrada e US$1,20 de saída, com tarifa própria para leitura de cache. Estes preços são datados e devem ser confirmados ao implementar e usar. Guardar o tarifário usado em cada estimativa.

O registo de consumo inclui pedidos de seguimento, relatório, reformulação e avaliação de novas tentativas. Se o fornecedor não devolver consumo, apresentar como desconhecido. A estimativa não substitui a faturação do fornecedor.

## Persistência e recuperação

- SQLite guarda conteúdos, relações, estados de operações e referências aos áudios.
- A pasta de dados da aplicação guarda gravações e derivados; o caminho concreto é resolvido pelo sistema operativo.
- Guardar áudio e texto antes de chamadas externas.
- Não aplicar respostas atrasadas a outra versão nem duplicar transições ao repetir pedidos.
- Preservar o estado guardado ao reiniciar. Gravações ainda não finalizadas podem ter de ser refeitas.
- Não bloquear a interface com inferência, conversão de áudio ou rede.
- Eliminar ficheiros exclusivos ao eliminar uma sessão, preservando os modelos partilhados.

## Escolhas ainda por fechar

- Bibliotecas de captura, reprodução, reamostragem e acesso a SQLite em Rust.
- Integração concreta com o armazenamento protegido de credenciais.
- Versões finais das dependências, modelo de transcrição e voz do Kokoro.
- Forma de empacotar o processo Python, os seus componentes nativos e os instaladores.
- Limites de duração/texto e prazos dos pedidos, sem truncamento silencioso.

As escolhas de áudio da fase 1 estão especificadas no [plano técnico](superpowers/plans/2026-11-01-phase-1-local-audio-implementation.md): CPAL, Rodio, Hound, Rubato, atomicwrites e Python 3.11 com voz `af_heart`. A compatibilidade do conjunto será validada na implementação; SQLite, credenciais e empacotamento final permanecem para as fases correspondentes. Não há ainda requisitos mínimos de hardware medidos nem comandos de instalação, build e testes implementados.

A [spec da fase 1](superpowers/specs/2026-11-01-phase-1-local-audio-design.md) define o comportamento da primeira integração de áudio e a persistência mínima em ficheiros, antes da introdução de SQLite na fase 2.

O [plano técnico da fase 2](superpowers/plans/2026-11-02-phase-2-guided-interview-implementation.md) propõe acesso a SQLite em Rust com `rusqlite` e a funcionalidade `bundled`, mantendo o frontend sem acesso SQL direto. Prevê transações, migrações e reconciliação com os ficheiros de áudio. A dependência ainda não foi instalada nem compilada neste projeto; a confirmação de compatibilidade pertence à execução desse plano.

## Referências

- [Tauri: introdução](https://v2.tauri.app/start/).
- [Tauri: WebViews utilizadas](https://v2.tauri.app/reference/webview-versions/).
- [Tauri: executáveis auxiliares](https://v2.tauri.app/develop/sidecar/).
- [faster-whisper](https://github.com/SYSTRAN/faster-whisper).
- [Kokoro-82M](https://huggingface.co/hexgrad/Kokoro-82M).
- [OpenCode: modelos e preços](https://opencode.ai/v2/docs/console/models/).
- [OpenCode: API de inferência](https://opencode.ai/v2/docs/console/api/inference/).

Consultar também a [ideia e os objetivos](ideia-e-objetivos.md), o [plano das fases](superpowers/plans/2026-11-01-star-interview-tutor-phases.md) e o [desenho aprovado](superpowers/specs/2026-11-01-star-interview-tutor-design.md).

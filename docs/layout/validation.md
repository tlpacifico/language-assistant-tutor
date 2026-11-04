# Layout do tutor — 2026-11-04

O layout foi adaptado a partir do [HTML fornecido](../Melhorar-layout-do-tutor/index.html), seguindo o [handoff](../Melhorar-layout-do-tutor/DESIGN-HANDOFF.md) e o [manifesto](../Melhorar-layout-do-tutor/DESIGN-MANIFEST.json). A implementação mantém React/TypeScript, os contratos Tauri e a persistência existentes. Um subagente tratou das coleções (banco, histórias e histórico); a estrutura, preparação, entrevista e integração foram executadas no agente principal.

## Sistema visual

| Elemento | Valor |
| --- | --- |
| Fundo | `#F2F5F3` |
| Superfícies | `#FFFFFF`, `#F7FAF8` |
| Texto / secundário | `#14231D`, `#586B62` |
| Bordas | `#DCE5DF`, `#C6D4CC` |
| Acento / suave | `#0E6B52`, `#E4F1EB` |
| Painel de sessão | `#0F3D30` |
| Títulos / texto | Newsreader / Public Sans |
| Navegação / conteúdo | 264 px / máximo de 1060 px |
| Raios | 14 px nos cartões, 10 px nos controlos, 18 px no painel da sessão |
| Transições | 180 ms nos controlos, 350 ms no progresso; respeita movimento reduzido |

As fontes variáveis WOFF2 e licenças OFL estão em `public/fonts`, sem pedidos externos durante a utilização. Os dois ficheiros somam 158 832 bytes (155,1 KiB), substituindo sete TTF que somavam 544,6 KiB. `fonts.css` é importado diretamente pela aplicação. O tema claro segue a referência independentemente do tema do Windows. `tokens.css` é a única origem dos tokens; as folhas declaram a ordem `tokens, legacy, layout, collections` através de CSS layers, que torna a precedência independente da ordem dos imports. A base tipográfica é de 16 px; raios, cores de texto sobre acento/painel e durações têm tokens próprios. Abaixo de 1024 px, a navegação passa para o topo com deslocamento horizontal próprio; abaixo de 900 px, os módulos passam a uma coluna. As dimensões pequenas são verificações de adaptação do frontend; a janela desktop mantém o mínimo de 800 × 600 definido no Tauri.

## Adaptação aos comportamentos reais

- Preparação: painel para retomar a sessão com posição e contagem reais; na ausência de sessão, formulário com modo local/IA, categoria e quantidade. O painel completo do ambiente e a base de dados ficam nesta vista. Nas outras páginas, «Áudio e recuperação» mantém Verificar disponibilidade e Reiniciar áudio local acessíveis sem navegar nem perder revisões por guardar, incluindo quando a navegação está bloqueada. O núcleo conserva as suas restrições: não reinicia o auxiliar durante transcrição/síntese nem interrompe a gravação nativa em curso.
- Banco e histórias: toolbars, etiquetas e cartões; preservados os filtros, editores e validações existentes. A pesquisa livre adicional e os chips de competências do protótipo não foram introduzidos: o banco continua a filtrar por categoria e as histórias por competência.
- Entrevista: pergunta, gravação, tomadas, transcrição automática, revisão e confirmação mantêm identidades e ações separadas. A coluna lateral mostra progresso real e orientação STAR. Esta substitui o «Roteiro da sessão» do protótipo porque `InterviewSnapshot` expõe a pergunta corrente, não a lista das perguntas fixadas da sessão. Não se inventa uma lista nem se acrescenta uma leitura de sessão para esta alteração de layout. Não se introduziram os atalhos de saltar perguntas, notas livres ou gravação simulada do protótipo.
- Histórico: datas e resumos em linhas, com consulta, retoma e treino posterior existentes.
- Configuração: usa o destino, modelo, tarifário, chave protegida e consentimento reais; não transporta as opções fictícias de fornecedor/voz/idioma do HTML.
- Prática, comparação, feedback e decisões de navegação usam o mesmo sistema visual. A Prática tem o seu próprio item atual na navegação; o Histórico não fica marcado como página atual durante uma prática. O conteúdo recebe foco ao mudar de página, mas não no arranque nem quando a navegação é cancelada. Não foram alterados os comandos Rust, os dados SQLite ou os motores de áudio.

O texto foi adaptado para português de Portugal e corrigido onde a referência descrevia armazenamento no navegador ou comportamentos que a aplicação não oferece. Não se apresentam estatísticas de demonstração como dados reais.

## Verificação executada

| Comando / método | Resultado |
| --- | --- |
| `npm test` | 12 ficheiros, **106 testes aprovados** |
| `npm run build` | TypeScript e Vite com saída 0 |
| `git diff --check` | Sem erros de whitespace |
| Chromium por Playwright, com IPC Tauri controlado | **72 verificações**, sem overflow horizontal nem erros JavaScript |

Os testes de integração foram atualizados para o título por vista, para a disponibilidade na preparação e para o progresso semântico. Três testes de regressão em `src/features/layout/Layout.test.tsx` cobrem a recuperação com navegação bloqueada, o foco após navegação e a recuperação numa Prática sem perder a revisão por guardar. Mantêm-se as verificações de gravação, confirmação, recuperação, subscrição única e decisões sobre alterações por guardar.

A matriz visual cobre **as sete páginas da aplicação** (Preparação, Banco de perguntas, Histórias, Entrevista, Histórico, Configuração IA e Prática), mais o formulário de nova sessão: oito estados em **360×800, 390×844, 430×932, 600×960, 820×1180, 1024×768, 1366×768, 1440×900 e 1920×1080**. A Prática usa prosa longa e uma referência de 240 caracteres sem espaços. O ensaio verifica também os 16 px entre campos de revisão da entrevista/prática e entre duas transcrições/revisões do histórico e a igualdade dos estilos de base/botões ao inverter as folhas CSS, com preferência do sistema por tema escuro. Ver [resultados da matriz](responsive-results.json).

O ensaio é reproduzível através de [scripts/layout-matrix.py](../../scripts/layout-matrix.py), com IPC Tauri controlado e dados sintéticos derivados de `contracts/interview-v2.example.json`. Comandos a partir da raiz, em Windows:

```powershell
# Preparação opcional isolada do verificador visual (não altera a .venv de áudio)
py -3.13 -m venv tmp/layout-venv
.\tmp\layout-venv\Scripts\python.exe -m pip install playwright==1.57.0
.\tmp\layout-venv\Scripts\python.exe -m playwright install chromium
npm.cmd ci

# Terminal 1
npm.cmd run dev -- --host 127.0.0.1

# Terminal 2
.\tmp\layout-venv\Scripts\python.exe scripts/layout-matrix.py
```

O comando efetivamente executado nesta máquina foi `python scripts/layout-matrix.py`, com Python 3.13 e Playwright 1.57.0 já disponíveis. O script guarda capturas e `results.json` em `tmp/layout-matrix` e termina com erro se houver overflow, erro JavaScript, espaçamento insuficiente ou divergência na ordem das folhas. Aceita `--url` e `--output` para mudar o servidor/destino. Não efetua chamadas pagas, gravações ou alterações à base de dados pessoal.

Foi feita inspeção de capturas desktop e mobile e comparação com o HTML exportado a 1440×900. Capturas da implementação:

- [Preparação desktop](screenshots/preparacao-desktop.png)
- [Preparação mobile](screenshots/preparacao-mobile.png)
- [Nova sessão](screenshots/nova-sessao-desktop.png)
- [Entrevista desktop](screenshots/entrevista-desktop.png)
- [Prática desktop com conteúdo longo](screenshots/pratica-desktop.png)
- [Prática mobile com conteúdo longo](screenshots/pratica-mobile.png)

## Limites da validação

Esta verificação estabelece a apresentação no browser e a ausência de regressões detetadas pelos testes React. Não substitui a utilização na janela Tauri com áudio real, credencial protegida e API paga. Não foi gerado um instalador nem repetida a bateria Rust/Python, cujos ficheiros não mudaram. As aceitações manuais das fases 1 a 4 continuam pendentes; a fase 5 não foi iniciada.

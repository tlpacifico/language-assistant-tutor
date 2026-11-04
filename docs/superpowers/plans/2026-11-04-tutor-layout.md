# Aplicar o layout do tutor

**Objetivo:** transportar o desenho fornecido em `docs/Melhorar-layout-do-tutor` para a aplicação React existente.

**Arquitetura:** manter os clientes Tauri, hooks, regras e persistência. Extrair os tokens do HTML de referência para CSS local e reorganizar a apresentação em navegação lateral, cabeçalho por vista, painel de sessão e módulos de conteúdo. Adaptar os exemplos estáticos a dados reais; não transportar o armazenamento no browser nem as simulações de áudio do protótipo.

**Stack:** React, TypeScript e CSS; sem novas dependências de execução.

**Referência:** [handoff](../../Melhorar-layout-do-tutor/DESIGN-HANDOFF.md) e [HTML](../../Melhorar-layout-do-tutor/index.html).

## Restrições

- Interface em português de Portugal; perguntas e respostas de treino em inglês.
- Preservar confirmação imutável, versões, consentimento, recuperação e bloqueios de navegação/gravação.
- Disponibilidade, progresso e consumo provêm dos contratos existentes; não inventar estatísticas.
- Fontes locais para funcionamento offline; manter fallback serifado/sans-serif.
- Não iniciar a fase 5 nem declarar aceitação manual das fases anteriores.

## Tarefas

- [x] Estrutura e tokens: `src/App.tsx`, `src/features/layout/*` e CSS. Navegação lateral de 264 px, área central com máximo de 1060 px, adaptação a 1023/899/639 px e foco visível.
- [x] Preparação e entrevista: adaptar `SessionSetup.tsx` e `Interview.tsx`; painel verde com progresso real, formulário de nova sessão, ambiente e base de dados. Preservar os handlers e estados existentes.
- [x] Banco, histórias e histórico: subagente com propriedade exclusiva dos três componentes e CSS próprio. Aplicar cabeçalhos, listas e filtros do desenho sem alterar contratos.
- [x] Integração: configuração IA, prática, comparação e avisos partilham os tokens. Conferir bloqueios, textos extensos, estado vazio, erros e estados de carregamento.
- [x] Verificar: testes React existentes, typecheck/build e comparação visual no browser nas nove dimensões do manifesto. Documentar comandos/resultados reais e limites da validação.

## Foco da verificação

Navegação com revisão pendente; captura em curso; ausência/falha da base de dados; listas e textos longos em ecrãs estreitos; ausência de sessão e disponibilidade desconhecida. Os testes existentes cobrem as regras; os screenshots verificam geometria e overflow. Adicionar testes apenas para comportamento novo que não esteja coberto.

## Resultado

Ver [validação do layout](../../layout/validation.md): 106 testes React, build com saída 0 e 72 verificações no browser sem overflow horizontal nem erros JavaScript. A aceitação manual na aplicação desktop permanece pendente.

Revisão antes de commit: recuperação áudio compacta fora da preparação, margem de campos limitada aos formulários, foco na mudança de página, tokens únicos/camadas CSS, fontes WOFF2 e matriz reproduzível com Prática e texto longo.

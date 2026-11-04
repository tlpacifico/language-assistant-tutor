# Diagnóstico local e identificação de builds

## Intenção e autorização

Preparar a utilização real e permitir investigar falhas posteriores. Em 2026-11-04 o utilizador pediu a spec, o plano e a implementação, com preservação dos registos entre versões. Assume-se a opção local proposta na conversa; não existe envio automático.

## Abordagens

- Diagnóstico local com exportação manual: escolhido para esta implementação; funciona offline, sem conta nem serviço adicional.
- Serviço remoto: permitiria receber falhas automaticamente, mas acrescenta dependências, configuração e transferência de dados. Fora do âmbito.

## Contrato

- Preservar `identifier = dev.thacio.languageassistanttutor`. Guardar JSONL em `app_data_dir()/diagnostics`, nunca no diretório de instalação, repositório ou temporários. Abrir em append; reiniciar ou mudar de versão não elimina o histórico.
- Retenção: 30 dias, até cinco ficheiros de 1 MiB cada, com rotação por tamanho. A retenção atua no arranque e nas escritas; apenas nos nomes reservados do diagnóstico. A versão não participa na limpeza. Exportações são independentes e não são limpas pela retenção.
- Cada evento contém schema, ID, instante Unix em milissegundos, execução, versão, revisão Git, indicador de alterações locais e instante de compilação. Quando Git não estiver disponível, indicar revisão desconhecida.
- Registar códigos de erros Rust, falhas de comandos IPC com nome e duração, exceções globais/promessas não tratadas e falhas de renderização React; registar frames próprios de exceções inesperadas do auxiliar Python, sem texto do erro. O diagnóstico não garante capturar terminação forçada, falhas nativas do WebView ou aborts do processo.
- Sem chaves, parâmetros IPC, mensagens livres, stack traces brutos, áudios, transcrições, histórias, respostas ou conteúdos do fornecedor. Campos externos passam por listas fechadas; desconhecidos tornam-se categorias genéricas. Não persistir stderr bruto.
- Escrita numa thread dedicada, fila limitada e envio sem espera; falhas do diagnóstico não devem impedir entrevistas. Expor estado de escrita e eventos perdidos; a exportação espera os eventos anteriormente enfileirados. A fila pode perder os últimos eventos numa terminação abrupta.
- Painel «Diagnóstico local» acessível em todas as páginas e na recuperação React, com build, estado e exportação. Exportar JSON num diretório indicado pelo utilizador (por omissão Downloads), com nome único e sem substituir ficheiros. Usar operações de disco fora da thread da interface. Não exportar SQLite, configuração pessoal ou ficheiros de sessões.
- Não modificar regras da entrevista, esquema SQLite, motores, credenciais ou a versão semântica 0.1.0. Identificar diferentes builds pelo commit, alterações locais e instante da compilação.

## Verificação e limites

Testar persistência após reabrir com outra versão, rotação, idade, falha de escrita, privacidade, exportação, fronteira IPC e recuperação React. Executar `scripts/verify.ps1`, build release sem bundle e smoke com dados temporários. A atualização por instalador, áudio real e aceitação das fases continuam pendentes; este trabalho não inicia a fase 5.

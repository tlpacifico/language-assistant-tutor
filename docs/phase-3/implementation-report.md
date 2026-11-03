# Relatório de implementação da fase 3

Data: 2026-11-02. Branch `feature/phase-3-ai-tutor`. Sem commits, push, inferência paga ou leitura de credenciais pessoais pelos testes.

## Decisões e desvios internos

- Um agente implementou; o coordenador revê o código e as evidências. A autorização do utilizador para antecipar a fase 3 preserva as aceitações manuais anteriores.
- `answer_slots` é a autoridade do alvo de resposta; a coluna interna legada `session_question_id` nas tabelas de áudio conserva o nome para reutilizar o pipeline, mas a FK aponta ao slot. `answerId` e `currentAnswer` explicitam os alvos nos contratos públicos. As principais e o progresso mantêm-se separados.
- `QuestionDetail.followUp` reutiliza o DTO de resposta, com `initialAnswerId` a identificar a principal. Não existe uma pergunta principal artificial para o seguimento.
- As tabelas/recibos IA são separados dos trabalhos locais. O resultado por publicar fica em memória até commit ou fecho; após crash a operação fica incerta. Não se promete persistir dados que não foram escritos.
- Feedback é JSON validado em Rust com whitelists de campos e evidências. A validação literal não prova relevância ou fidelidade semântica.
- `reqwest` 0.13.5 (JSON/Rustls) e `keyring` 3.6.3 Windows foram acrescentados, sem mudar os motores existentes. Prettier foi usado como ferramenta transitória, sem dependência nova do projeto.

## Testes e processo

TDD foi parcial. O teste inicial de migração falhou com versão 1 antes da implementação; um teste de feedback parcial identificou uma corrida no estado inicial do batch e ficou verde após a correção. Vários testes de HTTP, contrato, custos e UI foram acrescentados depois do código e não são apresentados como ciclos red/green completos. A comparação das fixtures de esquema/DTOs também encontrou assertions v1 que foram atualizadas para v2, mantendo igualdade exata do contrato.

Comandos executados: testes Rust específicos `ai_migration`, `ai_service`, `ai_validation`, `ai_usage`, `database`, `interview_contracts`; testes HTTP internos com `--lib ai::transport`; testes React específicos `Ai.test.tsx` e `Interview.test.tsx`; `npm.cmd run typecheck`; `cargo fmt`; `cargo clippy --all-targets -- -D warnings`; regressão Rust completa. A evidência final consolidada é registada na [validação](validation.md).

Não foram exercitados: inferência paga real, armazenamento protegido real com chave pessoal, microfone/reprodução reais, semântica/utilidade do modelo e pacote instalado. São pendências explícitas, não falhas ocultadas por mocks.

Evidência final: agregado 8/8 em 77,6 s (175 Rust, 57 React, 36 Python), build --no-bundle com saída 0 e smoke isolado com fecho/saída 0. Os testes de reinício, fecho durante batch e replay de configuração/consentimento também passaram.

A revisão técnica identificou uma corrida entre o fecho e a reserva do próximo relatório. O teste closing_queued_before_batch_reservation_prevents_new_attempts reproduziu o erro (uma chamada externa quando eram esperadas zero). A reserva e a admissão ao transporte passaram a verificar o fecho sob transitions, libertado antes da espera HTTP. O teste ficou verde e o agregado posterior passou 8/8 em 77,6 s (175 Rust, 57 React, 36 Python).

Após a correção: rebuild --no-bundle com saída 0 em 1m18s e smoke final com APPDATA/LOCALAPPDATA temporários; fecho e saída 0, sem dados pessoais.

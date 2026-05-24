# Onda de QA — meta-sidecar

## Resumo

Execução automática dos Critérios de Aceitação do `wave-plan.md`. Cada AC vira um comando que retorna 0 (passou) ou diferente de 0 (falhou). Sem julgamento subjetivo.

## Tarefas

### QA Agent

- [ ] Rodar cada `Command:` dos Critérios de Aceitação AC-1 até AC-8 do `wave-plan.md`.
- [ ] Para cada falha, anexar a saída completa no `qa.result`.
- [ ] Atualizar o `wave-plan.md`: marcar `[x]` nos AC que passaram, deixar `[ ]` nos que falharam.
- [ ] Emitir evento `qa.result` com `overall: pass | fail | skip`.

## Limites

Só roda os comandos. Não modifica código de produção.

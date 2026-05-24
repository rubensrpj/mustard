# Onda de QA — Configuração de idioma e tom

## Resumo

Execução automática dos Critérios de Aceitação definidos no `wave-plan.md`. Cada AC vira um comando que retorna código de saída 0 (passou) ou diferente de 0 (falhou). Sem julgamento subjetivo — ou o comando passa ou não passa.

## Tarefas

### QA Agent

- [ ] Rodar cada `Command:` dos Critérios de Aceitação AC-1 até AC-8 do `wave-plan.md`.
- [ ] Para cada falha, anexar a saída completa do comando no `qa.result` (stderr + stdout).
- [ ] Atualizar o `wave-plan.md`: marcar `[x]` nos AC que passaram, deixar `[ ]` nos que falharam.
- [ ] Emitir evento `qa.result` com:
  - `overall: pass` se todos passaram.
  - `overall: fail` se algum falhou (lista os AC que falharam).
  - `overall: skip` se a spec não tiver AC (caso impossível aqui, mas mantém o contrato).

## Limites

Não modifica código de produção. Só roda os comandos definidos no `wave-plan.md` e atualiza os checkboxes desse mesmo arquivo.

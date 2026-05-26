# Plano de ondas — spec-status-consistency

### Stage: Plan
### Outcome: Active
### Flags: 

## Tabela de ondas

| # | Spec | Role | Depende de | Resumo |
|---|---|---|---|---|
| 1 | [[wave-1-mixed]] | mixed | — | Sync único `spec.md`+`meta.json`: helper compartilhado + `sync_status` atômica + remoção do gate wave |
| 2 | [[wave-2-mixed]] | mixed | W1 | Doctor check `status-consistency` (detecta header ausente, divergência spec↔meta, combinação Stage+Outcome inválida) |
| 3 | [[wave-3-mixed]] | mixed | — | Picker honesto: `active-specs` lista malformadas com `??` e exibe `closed-followup` como `CLR→fu` |
| 4 | [[wave-4-mixed]] | mixed | W1, W2 | Subcomando one-shot `spec-status-backfill --source spec|meta` — limpa as 12 specs atuais |
| 5 | [[wave-5-mixed]] | mixed | W1-W4 | QA: teste de integração + verificação dos 6 ACs + build verde |

---
id: wave.teto-retentativa.2-pipeline
---

# wave-2-pipeline

## Summary

o teto lido em advance(): conta as retentativas do log, tira da rodada o wave que estourou e poe no lugar um item de escalacao

## Network

- Parent: [[spec.teto-retentativa]]
- Depends on: [[wave.teto-retentativa.1-vocabulary]]

## Tasks

- [ ] resolver o modo do portao com `resolve_mode("MUSTARD_RETRY_GATE_MODE", None, GateMode::Strict)` (gate_mode.rs:37) e o numero com uma funcao propria lendo `MUSTARD_RETRY_CEILING`, default 3, espelhando `threshold()` de delegation_advisory.rs:115
- [ ] contar as retentativas por wave lendo o `.events/` do spec pela mesma via de `completed_waves` (wave_advance.rs:357): filtrar por `EVENT_PIPELINE_WAVE_RETRY` + spec e agrupar pelo `wave` do payload
- [ ] emitir `pipeline.wave.retry` no bloco que hoje emite o start (wave_advance.rs:310-315): um wave que ja carrega start, ainda nao carrega complete e esta sendo entregue de novo E uma retentativa; manter o start idempotente como esta
- [ ] no filtro da rodada (wave_advance.rs:267-269), tirar o wave cuja contagem alcancou o teto e deixar passar os irmaos saudaveis do mesmo nivel; em modo `warn` so registrar, em `off` nao fazer nada
- [ ] no lugar do wave retirado, acrescentar um `AdvanceItem` com `role: "escalation"` que nomeia o wave, a contagem e o teto no proprio texto do prompt, sem depender de template novo
- [ ] escrever o teste que forja eventos de retentativa acima do teto no `.events/` de um spec de fixture e afirma que a rodada volta sem aquele wave e com o item de escalacao

## Files

- `apps/rt/src/commands/pipeline/wave_advance.rs`

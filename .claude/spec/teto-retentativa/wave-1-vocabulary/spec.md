---
id: wave.teto-retentativa.1-vocabulary
---

# wave-1-vocabulary

## Summary

o nome proprio do evento de retentativa e o emissor que o escreve no log do spec, sem encostar na telemetria de friccao

## Network

- Parent: [[spec.teto-retentativa]]

## Tasks

- [ ] declarar `EVENT_PIPELINE_WAVE_RETRY = "pipeline.wave.retry"` em `event.rs`, imediatamente ao lado de `EVENT_PIPELINE_WAVE_START` (linha 114), com doc comment no mesmo formato dos vizinhos e dizendo que o `{wave}` correlaciona com o start e o complete
- [ ] adicionar `emit_wave_retry(project, spec, wave, attempt)` em `emit_pipeline.rs` espelhando `emit_wave_start` (linha 1448); o payload carrega `{wave}` e usa o campo ja tipado `retry_count` (event.rs:224) para a tentativa
- [ ] escrever o teste que prova a nao-contaminacao em `route.rs`: `classify_kind("pipeline.wave.retry")` devolve `"pipeline"` E `classify_kind("retry.attempt")` continua devolvendo `"friction"` na mesma assercao

## Files

- `packages/core/src/domain/model/event.rs`
- `apps/rt/src/commands/event/emit_pipeline.rs`
- `apps/rt/src/shared/events/route.rs`

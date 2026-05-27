# Migrar emitters de pipeline para NDJSON puro

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: light
### Checkpoint: 2026-05-27T10:00:00Z
### Lang: pt-BR
### Parent: 2026-05-26-no-sqlite-git-source-of-truth

## PRD

## Contexto

Sub-spec de [[2026-05-26-no-sqlite-git-source-of-truth]] — wave 2C (renumbered wave-6-rt). **Migrar emitters de pipeline para NDJSON puro.** Remove branch SQLite de: `emit_pipeline.rs` (só NDJSON), `emit_phase.rs`, `event_writer_ndjson.rs` (canonical sink — expande kinds), `verify_emit.rs`. DELETE `apps/rt/src/run/db_maintain.rs` + `backfill_run_usage_cost.rs` + `backfill_run_usage_spec.rs` (subcommands órfãos sem readers) e remove suas variantes do `run/mod.rs`. **Files (5):** `apps/rt/src/run/emit_pipeline.rs`, `apps/rt/src/run/emit_phase.rs`, `apps/rt/src/run/event_writer_ndjson.rs`, `apps/rt/src/run/verify_emit.rs`, `apps/rt/src/run/mod.rs` (DELETE 3 modules + dispatch). Os 3 arquivos `db_maintain.rs` + `backfill_run_usage_{cost,spec}.rs` são deletados via `git rm` neste mesmo commit (não contam para o cap por serem DELETE simples sem migração). **Verify:** `cargo build -p mustard-rt`.

## Critérios de Aceitação

- [x] AC-2C-1: `cargo build -p mustard-rt` passa após deleção dos 3 módulos órfãos e remoção dos branches SQLite nos emitters. Command: `cargo build -p mustard-rt`

## Plano

## Arquivos

- `apps/rt/src/run/emit_pipeline.rs`
- `apps/rt/src/run/emit_phase.rs`
- `apps/rt/src/run/event_writer_ndjson.rs`
- `apps/rt/src/run/verify_emit.rs`
- `apps/rt/src/run/mod.rs`

## Tarefas

1. `apps/rt/src/run/emit_pipeline.rs` — remover branch SQLite; toda emissão passa a usar exclusivamente o canal NDJSON via `event_writer_ndjson`; sem fallback para `SqliteEventStore`
2. `apps/rt/src/run/emit_phase.rs` — idem: remover branch SQLite; emissão de fase vai direto para NDJSON
3. `apps/rt/src/run/event_writer_ndjson.rs` — expandir kinds suportados conforme necessário (canonical sink); remover qualquer referência a SQLite que tenha sobrado
4. `apps/rt/src/run/verify_emit.rs` — adaptar verificação para operar sobre NDJSON; remover leitura via `SqliteEventStore`
5. `apps/rt/src/run/mod.rs` — remover `mod db_maintain;`, `mod backfill_run_usage_cost;`, `mod backfill_run_usage_spec;` e suas variantes do enum `RunCmd` + arms do match dispatch; executar `git rm` nos 3 arquivos correspondentes (`db_maintain.rs`, `backfill_run_usage_cost.rs`, `backfill_run_usage_spec.rs`)

## Dependências

Depende de W1A+W1B+W1C (commits acabaram de aterrissar em `dev_rubens`). Já pode consumir `mustard_core::EventReader` e `mustard_core::Event`.

## Limites

- CAP RÍGIDO: ≤5 arquivos
- Sem stubs preservando nomes SQLite — DELETE callers/usos diretamente
- Invariante decrescente: após commit `git grep -lE "SqliteEventStore|sqlite_store|memory_sqlite" -- 'packages/**/*.rs' 'apps/**/*.rs'` DEVE decrescer (sub-spec MIGRA arquivos SQLite-named, então count cai)
- Commit message sugerido: `feat(wave-2/rt): W2C — migrate pipeline emitters to pure NDJSON, delete backfill orphans`

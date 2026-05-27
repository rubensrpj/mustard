# Migrar readers de specs ativas e projections para NDJSON

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: light
### Checkpoint: 2026-05-27T10:00:00Z
### Lang: pt-BR
### Parent: 2026-05-26-no-sqlite-git-source-of-truth

## PRD

## Contexto

Sub-spec de [[2026-05-26-no-sqlite-git-source-of-truth]] — wave 2A (renumbered wave-4-rt). **Migrar readers de specs ativas e projections para NDJSON.** Substitui `SqliteEventStore::for_project(...)` por leitura de `.claude/spec/*/spec.md` (filesystem walk + parse de header) e `.claude/spec/*/.events/*.ndjson` em: `apps/rt/src/run/active_specs.rs` (remove o sink de "backfill SQLite quando ausente"), `apps/rt/src/run/event_projections.rs` (projeção lê NDJSON), `apps/rt/src/run/pipeline_state_ingest.rs` (idem). Atualiza `apps/rt/src/run/env.rs` para não construir mais `SqliteEventStore` (remove `project_dir()` fallback que dependia dele). **Files (5):** `apps/rt/src/run/active_specs.rs`, `apps/rt/src/run/event_projections.rs`, `apps/rt/src/run/pipeline_state_ingest.rs`, `apps/rt/src/run/env.rs`, `apps/rt/src/run/event_route.rs`. **Verify:** `cargo build -p mustard-rt` + invariante decrescente.

## Critérios de Aceitação

- [x] AC-2A-1: `cargo build -p mustard-rt` passa e o count de `git grep -lE "SqliteEventStore|sqlite_store|memory_sqlite" -- 'packages/**/*.rs' 'apps/**/*.rs'` é menor que antes do commit. Command: `cargo build -p mustard-rt`

## Plano

## Arquivos

- `apps/rt/src/run/active_specs.rs`
- `apps/rt/src/run/event_projections.rs`
- `apps/rt/src/run/pipeline_state_ingest.rs`
- `apps/rt/src/run/env.rs`
- `apps/rt/src/run/event_route.rs`

## Tarefas

1. `apps/rt/src/run/active_specs.rs` — substituir `SqliteEventStore::for_project(...)` por filesystem walk de `.claude/spec/*/spec.md` + parse de header (Stage/Outcome); remover sink de "backfill SQLite quando ausente"; consumir `mustard_core::EventReader::stream` para ler `.events/*.ndjson` quando necessário
2. `apps/rt/src/run/event_projections.rs` — trocar leitura via `SqliteEventStore` por `mustard_core::EventReader::cached_for_session` ou `stream`; projeção passa a operar sobre `Event` structs do NDJSON
3. `apps/rt/src/run/pipeline_state_ingest.rs` — idem: substituir ingest SQLite por leitura de `.events/*.ndjson` via `EventReader`
4. `apps/rt/src/run/env.rs` — remover construção de `SqliteEventStore`; remover `project_dir()` fallback que dependia de banco; usar caminhos canônicos de filesystem
5. `apps/rt/src/run/event_route.rs` — atualizar roteamento de eventos para não depender de `SqliteEventStore`; garantir que `use` statements SQLite são removidos

## Dependências

Depende de W1A+W1B+W1C (commits acabaram de aterrissar em `dev_rubens`). Já pode consumir `mustard_core::EventReader` e `mustard_core::Event`.

## Limites

- CAP RÍGIDO: ≤5 arquivos
- Sem stubs preservando nomes SQLite — DELETE callers/usos diretamente
- Invariante decrescente: após commit `git grep -lE "SqliteEventStore|sqlite_store|memory_sqlite" -- 'packages/**/*.rs' 'apps/**/*.rs'` DEVE decrescer (sub-spec MIGRA arquivos SQLite-named, então count cai)
- Commit message sugerido: `feat(wave-2/rt): W2A — migrate active-specs and projection readers to NDJSON`

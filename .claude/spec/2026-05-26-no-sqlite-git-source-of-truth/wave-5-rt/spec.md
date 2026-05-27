# Migrar readers de resume + metrics + rebuild para NDJSON

### Stage: Execute
### Outcome: Active
### Flags:
### Scope: light
### Checkpoint: 2026-05-27T10:00:00Z
### Lang: pt-BR
### Parent: 2026-05-26-no-sqlite-git-source-of-truth

## PRD

## Contexto

Sub-spec de [[2026-05-26-no-sqlite-git-source-of-truth]] — wave 2B (renumbered wave-5-rt). **Migrar readers de resume + metrics + rebuild para NDJSON.** `apps/rt/src/run/resume_bootstrap.rs` lê estado da spec via filesystem (header + `.events/*.ndjson`); `metrics_wave_status.rs` lê de NDJSON; `rebuild_specs.rs` vira gerador canônico do `.summary.json` (chama `summary::writer`); `qa_run.rs` lê AC results de filesystem; `qa_run_all.rs` substitui `SqliteSpecReader` por filesystem walk. **Files (5):** `apps/rt/src/run/resume_bootstrap.rs`, `apps/rt/src/run/metrics_wave_status.rs`, `apps/rt/src/run/rebuild_specs.rs`, `apps/rt/src/run/qa_run.rs`, `apps/rt/src/run/qa_run_all.rs`. **Verify:** `cargo build -p mustard-rt` + `cargo run -q -p mustard-rt -- run active-specs`.

## Critérios de Aceitação

- [ ] AC-2B-1: `cargo build -p mustard-rt` passa e `cargo run -q -p mustard-rt -- run active-specs` executa sem erro. Command: `cargo build -p mustard-rt && cargo run -q -p mustard-rt -- run active-specs`

## Plano

## Arquivos

- `apps/rt/src/run/resume_bootstrap.rs`
- `apps/rt/src/run/metrics_wave_status.rs`
- `apps/rt/src/run/rebuild_specs.rs`
- `apps/rt/src/run/qa_run.rs`
- `apps/rt/src/run/qa_run_all.rs`

## Tarefas

1. `apps/rt/src/run/resume_bootstrap.rs` — substituir leitura SQLite por filesystem: ler header da spec via parse de `spec.md` (Stage/Outcome/Scope) + ler eventos de `.events/*.ndjson` via `mustard_core::EventReader`; remover dependência de `SqliteEventStore`
2. `apps/rt/src/run/metrics_wave_status.rs` — trocar SELECT do banco por `mustard_core::EventReader::filter_kind` sobre o NDJSON da spec; agregar status de waves a partir dos eventos `pipeline.status` + parse de headers dos `wave-N-*/spec.md`
3. `apps/rt/src/run/rebuild_specs.rs` — transformar em gerador canônico do `.summary.json`: chamar `mustard_core::summary::writer` (entregue em W1A) para cada spec encontrada via filesystem walk; remover qualquer caminho de leitura/escrita SQLite
4. `apps/rt/src/run/qa_run.rs` — substituir leitura de AC results via banco por parse de `spec.md` (seção "Critérios de Aceitação") + verificação de outputs no filesystem; consumir `mustard_core::EventReader` para historico de execuções
5. `apps/rt/src/run/qa_run_all.rs` — substituir `SqliteSpecReader` por filesystem walk de `.claude/spec/*/spec.md`; iterar specs via glob, filtrar por Stage/Outcome, delegar a `qa_run` para cada uma

## Dependências

Depende de W1A+W1B+W1C (commits acabaram de aterrissar em `dev_rubens`). Já pode consumir `mustard_core::EventReader` e `mustard_core::Event`.

## Limites

- CAP RÍGIDO: ≤5 arquivos
- Sem stubs preservando nomes SQLite — DELETE callers/usos diretamente
- Invariante decrescente: após commit `git grep -lE "SqliteEventStore|sqlite_store|memory_sqlite" -- 'packages/**/*.rs' 'apps/**/*.rs'` DEVE decrescer (sub-spec MIGRA arquivos SQLite-named, então count cai)
- Commit message sugerido: `feat(wave-2/rt): W2B — migrate resume, metrics and rebuild readers to NDJSON`

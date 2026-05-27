# Hooks de savings + budget para NDJSON

### Stage: planned
### Outcome: Active
### Flags:
### Scope: light
### Checkpoint: 2026-05-27T10:00:00Z
### Lang: pt-BR
### Parent: 2026-05-26-no-sqlite-git-source-of-truth

## PRD

## Contexto

Sub-spec de [[2026-05-26-no-sqlite-git-source-of-truth]] — wave 3A (renumbered wave-7-rt). **Hooks de savings + budget para NDJSON.** `apps/rt/src/hooks/tracker.rs` escreve savings em `.events/*.ndjson` (eventos `pipeline.economy.savings.*`); `budget.rs` lê janela de tokens de NDJSON; `bash_guard.rs` lê histórico de NDJSON; `model_routing.rs` lê última decisão de NDJSON (preservando a política opus-default — ver hotfix `9bee371`).

**Files (4):** `apps/rt/src/hooks/tracker.rs`, `apps/rt/src/hooks/budget.rs`, `apps/rt/src/hooks/bash_guard.rs`, `apps/rt/src/hooks/model_routing.rs`.

**Verify:** `cargo build -p mustard-rt` + invariante decrescente.

## Critérios de Aceitação

- [ ] AC-3A-1: `cargo build -p mustard-rt` passa e o count de `git grep -lE "SqliteEventStore|sqlite_store|memory_sqlite" -- 'packages/**/*.rs' 'apps/**/*.rs'` decresce. Command: `cargo build -p mustard-rt`
- [ ] AC-3A-2: Nenhum dos 4 arquivos importa `mustard_core::store::SqliteEventStore` nem chama `for_project` em SqliteEventStore. Command: `bash -c "! git grep -nE 'SqliteEventStore|sqlite_store' -- apps/rt/src/hooks/tracker.rs apps/rt/src/hooks/budget.rs apps/rt/src/hooks/bash_guard.rs apps/rt/src/hooks/model_routing.rs"`

## Plano

## Arquivos

- `apps/rt/src/hooks/tracker.rs`
- `apps/rt/src/hooks/budget.rs`
- `apps/rt/src/hooks/bash_guard.rs`
- `apps/rt/src/hooks/model_routing.rs`

## Tarefas

1. `apps/rt/src/hooks/tracker.rs` — substituir gravação SQLite de savings por append a `.events/{session}/pipeline.ndjson` (kind `pipeline.economy.savings.*`) usando o emissor canônico do `event_writer_ndjson`; manter idempotência por evento
2. `apps/rt/src/hooks/budget.rs` — substituir leitura da janela de tokens via SQL por `mustard_core::EventReader::cached_for_session` filtrando kinds de `pipeline.budget.*` / `pipeline.economy.usage.*`; mesmo cálculo, fonte é NDJSON
3. `apps/rt/src/hooks/bash_guard.rs` — substituir leitura de histórico (que servia para deduplicar/dedup-warn) por leitura via `EventReader` filtrando kinds relevantes; manter regras de bloqueio inalteradas (rm -rf, mkfs, dd, credentials)
4. `apps/rt/src/hooks/model_routing.rs` — substituir lookup de "última decisão de routing" por leitura via `EventReader::filter_kind("pipeline.route")`; preservar política opus-default (sem pipeline ativa → opus; downgrades só com `model:` explícito, ver hotfix `9bee371`)

## Dependências

Depende de W1A+W1B+W1C e do batch W2 (já comitado em `dev_rubens`: `91b0384`, `eedf04e`, `984391a`). Já pode consumir `mustard_core::EventReader`, `mustard_core::Event`, e o emissor NDJSON canônico.

## Limites

- CAP RÍGIDO: ≤5 arquivos (4 nesta sub-spec)
- Sem stubs preservando nomes SQLite — DELETE callers/usos diretamente
- Invariante decrescente: após commit `git grep -lE "SqliteEventStore|sqlite_store|memory_sqlite"` DEVE decrescer
- Não tocar política opus-default em `model_routing.rs` — preservar comportamento atual (apenas trocar a fonte de dados)
- Commit message sugerido: `feat(wave-3/rt): W3A — savings+budget+bash-guard+routing readers via NDJSON`

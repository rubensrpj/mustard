# Migrate residual rt economy caller + core tests to NDJSON (W7C — capture_baseline + core tests)

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: mixed
### Checkpoint: 2026-05-27T20:00:00Z
### Lang: pt-BR
### Parent: 2026-05-26-no-sqlite-git-source-of-truth

## PRD

## Contexto

Sub-spec W7C da [[2026-05-26-no-sqlite-git-source-of-truth]]. Após W7A/W7B,
restam dois clusters menores tocando rusqlite no domínio economy:

1. `apps/rt/src/run/economy_capture_baseline.rs` — lê eventos `pipeline.economy.operation.invoked`
   via `SqliteEventStore::for_project`.
2. `packages/core/tests/economy_basic.rs` + `economy_attribution.rs` — testes integration que
   usavam `SqliteEventStore` + `TelemetryStore` pra setup. Migrar pra fixtures NDJSON.

Esta sub-spec fecha esses 3 arquivos.

## Critérios de Aceitação

- [x] AC-W7C-1: `cargo build -p mustard-rt` verde. Command: `cargo build -p mustard-rt`
- [x] AC-W7C-2: `cargo build -p mustard-core` verde. Command: `cargo build -p mustard-core`
- [x] AC-W7C-3: `cargo test -p mustard-core --no-run` compila com 0 erros (tests integration migrados). Command: `cargo test -p mustard-core --no-run`
- [x] AC-W7C-4: `economy_capture_baseline.rs` não importa mais `SqliteEventStore`. Command: `bash -c "! git grep -nE 'SqliteEventStore|sqlite_store|memory_sqlite' -- apps/rt/src/run/economy_capture_baseline.rs | grep -vE '^[^:]+:[0-9]+:\s*(///|//|/\*|\*)'"`
- [x] AC-W7C-5: `tests/economy_basic.rs` não importa mais rusqlite/SqliteEventStore. Command: `node -e "const s=require('fs').readFileSync('packages/core/tests/economy_basic.rs','utf8'); if(/SqliteEventStore|rusqlite/.test(s)){process.exit(1)}"`
- [x] AC-W7C-6: `tests/economy_attribution.rs` não importa mais rusqlite/SqliteEventStore. Command: `node -e "const s=require('fs').readFileSync('packages/core/tests/economy_attribution.rs','utf8'); if(/SqliteEventStore|rusqlite/.test(s)){process.exit(1)}"`
- [x] AC-W7C-7: invariante decrescente após commit. Command: `bash -c 'count=$(git grep -lE "SqliteEventStore|sqlite_store|memory_sqlite|TelemetryStore|TelemetryReader|rusqlite::" -- "*.rs" | wc -l); echo "$count"; test "$count" -lt 38'`

## Plano

## Arquivos

- `apps/rt/src/run/economy_capture_baseline.rs` — UPDATE
- `packages/core/tests/economy_basic.rs` — REWRITE
- `packages/core/tests/economy_attribution.rs` — REWRITE

(3 arquivos.)

## Tarefas

1. **`economy_capture_baseline.rs`**:
   - Drop `use mustard_core::store::sqlite_store::SqliteEventStore`.
   - `historical_duration_ms`: usa `EventReader::stream` sobre `<cwd>/.claude/spec/*/.events/*.ndjson` filtrando por `event.raw["event"] == "pipeline.economy.operation.invoked"` + `payload.operation == operation`. Pega último (rev). Same fail-open shape.
2. **`tests/economy_basic.rs`**:
   - Drop `use mustard_core::store::sqlite_store::SqliteEventStore`.
   - Setup fixture NDJSON: cria tmpdir, escreve `<dir>/.claude/spec/test/.events/seed.ndjson` com linhas JSON correspondentes aos savings/run/context events.
   - Adapta asserts para chamar os novos readers `economy_summary(&project_root, scope)`, `savings_breakdown(&project_root, scope)`, etc.
3. **`tests/economy_attribution.rs`**:
   - Mesmo padrão — drop SqliteEventStore, fixture NDJSON, chama `per_wave_costs(&project_root, scope)` com fixture de runs spec/wave.
   - Preserva o regression-test "parent_spec_child_wave_attribution" (citado em comments do reader original).
4. **Verify**: `rtk cargo build -p mustard-rt` + `rtk cargo build -p mustard-core` + `rtk cargo test -p mustard-core --no-run` + AC-W7C-7 grep.

## Dependências

- Requer W7A + W7B já commitados.

## Limites

- 3 arquivos (1 rt + 2 testes core).
- Modelo: opus.
- Commit message: `feat(wave-7/rt): W7C — migrate economy_capture_baseline + core integration tests to NDJSON`

<!-- wikilinks-footer-start -->
- [2026-05-26-no-sqlite-git-source-of-truth](?) ⚠ não resolvido
<!-- wikilinks-footer-end -->
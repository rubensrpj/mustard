# Arquivos por Wave — no-sqlite-git-source-of-truth

Lista representativa de arquivos por wave. Total estimado: 55-65 arquivos. Lista exata pode evoluir durante EXECUTE conforme cada agent descobrir consumidores.

## Wave 1 — Schema do summary + writer Rust

- `packages/core/src/summary/mod.rs` (CREATE — modelo Rust + serde)
- `packages/core/src/summary/writer.rs` (CREATE — gerador do `.summary.json`)
- `packages/core/src/summary/schema.md` (CREATE — doc do schema, versionado, EN)
- `packages/core/src/lib.rs` (MODIFY — exports)

## Wave 2 — Eliminar mustard.db storage layer

- `packages/core/src/store/sqlite_store.rs` (DELETE)
- `packages/core/src/store/sqlite_schema.sql` (DELETE)
- `packages/core/src/store/migrations.rs` (DELETE)
- `packages/core/src/store/mod.rs` (DELETE ou rename para `fs_reader/mod.rs`)
- `packages/core/src/reader/sqlite.rs` (DELETE)
- `packages/core/src/reader/fs.rs` (CREATE — filesystem reader, substitui sqlite)
- `packages/core/Cargo.toml` (MODIFY — remover `rusqlite`)
- `packages/core/src/lib.rs` (MODIFY — reexports)
- `apps/rt/src/run/db_maintain.rs` (DELETE)
- `apps/rt/src/run/mod.rs` (MODIFY — remover variant `DbMaintain` + dispatch)

## Wave 3 — Eliminar telemetry.db

- `packages/core/src/telemetry/store.rs` (DELETE)
- `packages/core/src/telemetry/schema.sql` (DELETE)
- `packages/core/src/telemetry/writer.rs` (DELETE)
- `packages/core/src/telemetry/reader.rs` (DELETE)
- `packages/core/src/telemetry/mod.rs` (DELETE ou minimal stub)
- `packages/core/src/telemetry/model.rs` (MODIFY — manter model, deletar IO)
- Readers de `run_usage` que o dashboard consome → viram leitor de NDJSON `pipeline.economy.*` per-spec

## Wave 4 — Migrar emitters do rt para NDJSON puro

- `apps/rt/src/run/emit_pipeline.rs` (MODIFY — grava NDJSON)
- `apps/rt/src/run/emit_phase.rs` (MODIFY)
- `apps/rt/src/run/emit_event.rs` (MODIFY)
- `apps/rt/src/run/event_writer_ndjson.rs` (MODIFY — expandir kinds aceitos, remover branch SQLite)
- `apps/rt/src/run/event_route.rs` (MODIFY — sem split SQLite vs NDJSON; só NDJSON)
- `apps/rt/src/run/event_projections.rs` (MODIFY — projeta de NDJSON para `.summary.json`)
- `apps/rt/src/run/active_specs.rs` (MODIFY — scan filesystem)
- `apps/rt/src/run/pipeline_state_ingest.rs` (MODIFY — ler NDJSON)
- `apps/rt/src/run/pipeline_summary.rs` (MODIFY — gera `.summary.json`)
- `apps/rt/src/run/rebuild_specs.rs` (MODIFY — vira o gerador canônico)
- `apps/rt/src/run/complete_spec.rs` (MODIFY — emit summary no close)
- `apps/rt/src/run/close_orchestrate.rs` (MODIFY — chama summary writer)

## Wave 5 — Economy + telemetry writers para NDJSON

- `packages/core/src/economy/store.rs` (MODIFY — writer NDJSON, sem rusqlite)
- `packages/core/src/economy/writer.rs` (MODIFY)
- `packages/core/src/economy/reader.rs` (MODIFY — lê NDJSON)
- `apps/rt/src/run/economy_capture_baseline.rs` (MODIFY)
- `apps/rt/src/run/economy_reconcile.rs` (MODIFY)
- `apps/rt/src/run/economy_report.rs` (MODIFY)
- `apps/rt/src/hooks/budget.rs` (MODIFY — savings via NDJSON)
- `apps/rt/src/hooks/bash_guard.rs` (MODIFY)
- `apps/rt/src/hooks/model_routing.rs` (MODIFY)
- `apps/rt/src/hooks/tracker.rs` (MODIFY)
- Backfill subcomandos: `apps/rt/src/run/backfill_run_usage_cost.rs` + `_spec.rs` (DELETE ou rewrite)

## Wave 6 — Knowledge + memory como markdown atomic

- `apps/rt/src/run/memory.rs` (MODIFY — escreve `.md` atomic em vez de SQL)
- `apps/rt/src/hooks/session_start.rs` (MODIFY — lê `.md` em vez de SELECT)
- `apps/rt/src/hooks/stop_observer.rs` (MODIFY)
- `apps/rt/src/run/knowledge.rs` (MODIFY)
- `apps/rt/src/run/memory_ingest.rs` (MODIFY)
- `apps/rt/src/run/memory_cross_wave.rs` (MODIFY)
- `apps/rt/src/hooks/knowledge.rs` (MODIFY)
- `apps/rt/src/hooks/amend_capture.rs` (MODIFY — estado local em arquivo `.json` no spec dir)
- `apps/rt/src/run/amend_finalize.rs` (MODIFY)
- `apps/cli/templates/.gitignore` (MODIFY — `.events/`, `.blobs/`, `.harness/` ignorados; `knowledge/`, `memory/` versionados)
- `.claude/knowledge/` e `.claude/memory/` (CREATE diretórios + arquivo de placeholder)

## Wave 7 — Dashboard reader migration

- `apps/dashboard/src-tauri/src/db.rs` (REWRITE → vira `reader_fs.rs`; ~30 queries SQL viram `walk + read + json::parse + aggregate`)
- `apps/dashboard/src-tauri/src/telemetry.rs` (REWRITE)
- `apps/dashboard/src-tauri/src/telemetry_agg.rs` (REWRITE)
- `apps/dashboard/src-tauri/src/spec_views.rs` (REWRITE)
- `apps/dashboard/src-tauri/src/economy.rs` (REWRITE)
- `apps/dashboard/src-tauri/src/lib.rs` (MODIFY — registrar Tauri commands novos)
- `apps/dashboard/src-tauri/Cargo.toml` (MODIFY — remover rusqlite)
- `apps/dashboard/src/lib/dashboard.ts` (MODIFY se invocar Tauri commands removidos)

## Wave 8 — Cleanup físico + validação

- Deletar 5 arquivos `mustard.db` físicos: `.claude/.harness/mustard.db`, `apps/cli/.claude/.harness/`, etc.
- Deletar 5 arquivos `telemetry.db` físicos
- `cargo build` workspace
- `cargo test --workspace`
- `pnpm --filter mustard-dashboard build`
- Smoke: `mustard init` em tmpdir, confirmar AC-1
- Smoke: rodar dashboard local, confirmar que lista specs e abre detalhe sem erros

## Tests (espalhado pelas waves)

- `packages/core/tests/sqlite_*.rs` (DELETE)
- `packages/core/tests/economy_basic.rs` (REWRITE — usar fixtures NDJSON)
- `packages/core/tests/amend_window_projection.rs` (REWRITE)
- `packages/core/tests/reader_contract.rs` (REWRITE)
- `apps/rt/tests/memory_sqlite_test.rs` (REWRITE → `memory_markdown_test.rs`)
- `apps/rt/tests/amend_finalize.rs`, `amend_capture.rs` (REWRITE — fixtures de arquivos)
- `apps/dashboard/src-tauri/tests/*.rs` (8 arquivos REWRITE — mockam filesystem em vez de DB)

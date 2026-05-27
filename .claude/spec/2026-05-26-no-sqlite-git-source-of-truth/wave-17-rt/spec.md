# MCP face + orphan tests do rt → filesystem readers

### Stage: Plan
### Outcome: Active
### Flags:
### Scope: light
### Checkpoint: 2026-05-27T11:00:00Z
### Lang: pt-BR
### Parent: 2026-05-26-no-sqlite-git-source-of-truth

## PRD

## Contexto

Sub-spec de [[2026-05-26-no-sqlite-git-source-of-truth]] — wave 5B (renumbered wave-17-rt). **MCP face + 3 orphan tests migrados de SQLite para filesystem walk + NDJSON.**

- `apps/rt/src/mcp/mod.rs` substitui as queries Tauri-like (`SqliteEventStore`, `TelemetryStore`, `TelemetryReader`) por leitura filesystem + NDJSON:
  - Drop `use mustard_core::store::sqlite_store::{KnowledgeRow, MetricsRow, SpecRow, SqliteEventStore};`.
  - Drop `use mustard_core::telemetry::{SummaryRow, TelemetryReader, TelemetryStore};`.
  - `search_knowledge` (FTS5 search sobre `knowledge` table) → walk `.claude/knowledge/*.md` via `MarkdownStore::scan_dir`, filtra body+frontmatter por substring case-insensitive (decisão: FTS5 vira substring grep — escopo "knowledge mode" é pequeno o suficiente; ranking simples por contagem de matches). Output shape preservado (`{ slug, title, body, score }`).
  - `query_events` → `EventReader::cached_for_session` lendo `.claude/spec/{spec?}/.events/*.ndjson` ou cross-spec walk se sem filtro de spec, filtrado por `kind` + `since` timestamp. Output shape preservado.
  - `find_similar_specs` → filesystem walk de `.claude/spec/*/spec.md`, parse de header (title + body description), token overlap como antes. Output shape preservado.
  - `get_spec_metrics` → `pipeline_state_from_events` (já existe em `crate::run::event_projections`) sobre eventos NDJSON do spec. Output mapeado para shape `MetricsRow` (campos: dispatched_tasks, completed_tasks, current_wave, status, etc.).
  - `get_run_summary` → agregação cross-session sobre eventos `pipeline.telemetry.metric` (escritos por W5A wave-16-rt OTEL). `SummaryRow` shape preservado (`{ session_id, total_tokens, duration_ms, ... }`).
- `apps/rt/src/mcp/tests.rs` — tests in-process que chamam direto os métodos do tool router. Atualmente seedam `SqliteEventStore`. REWRITE para usar fixtures filesystem (`MarkdownStore::write_atomic` para knowledge; `crate::run::event_writer_ndjson::write_event_with_ts` para eventos; `std::fs::write` para spec.md). Output shape coberto pelas mesmas assertions.
- `apps/rt/tests/mcp.rs` — integration test que spawna o binário real `mustard-rt mcp` via subprocess. Atualmente seeda `mustard.db` via `rusqlite`. REWRITE para seed via filesystem (criar `.claude/spec/*/spec.md`, `.claude/knowledge/*.md`, `.claude/spec/*/.events/*.ndjson`). Drop `use rusqlite::Connection;` + env var `MUSTARD_DB_PATH` (passa a usar cwd do spawn que aponta para o tempdir). Os 5 tool calls + assertions de shape preservadas.
- `apps/rt/tests/spec_children_tree.rs` — integration test do `spec-children-tree`. Atualmente seeda `SqliteEventStore` para depois invocar subprocess. REWRITE: seed via `event_writer_ndjson` (ou direto via `std::fs::write` no `.events/*.ndjson`); spec.md headers para parent + sub-spec via `std::fs::write`; subprocess invocation idêntico; assertions de JSON shape idênticas.
- `apps/rt/tests/spec_hygiene.rs` — integration test do hook `spec_hygiene` SessionStart. Atualmente seeda `SqliteEventStore` para fixtures de cenário. REWRITE: seed via NDJSON + filesystem (spec.md, build status via filesystem flag em vez de event SQL). Os 5 cenários (autoclose-green, build-red-skip, abandoned-suspect, mode=off-silent, idempotence) preservados.

**Files (5):** `apps/rt/src/mcp/mod.rs`, `apps/rt/src/mcp/tests.rs`, `apps/rt/tests/mcp.rs`, `apps/rt/tests/spec_children_tree.rs`, `apps/rt/tests/spec_hygiene.rs`.

**Verify:** `cargo build -p mustard-rt` + `cargo test -p mustard-rt --no-run` (limpo). Run completo dos 3 integration tests fora do escopo deste commit (W8C valida).

## Critérios de Aceitação

- [ ] AC-5B-1: `cargo build -p mustard-rt` passa após migração. Command: `cargo build -p mustard-rt`
- [ ] AC-5B-2: `cargo test -p mustard-rt --no-run` compila com 0 erros (warnings permitidos). Command: `cargo test -p mustard-rt --no-run`
- [ ] AC-5B-3: Nenhum dos 5 arquivos modificados referencia `SqliteEventStore` / `sqlite_store` / `TelemetryStore` / `TelemetryReader` / `rusqlite::`. Command: `bash -c "! git grep -nE 'SqliteEventStore|sqlite_store|TelemetryStore|TelemetryReader|rusqlite::' -- apps/rt/src/mcp/mod.rs apps/rt/src/mcp/tests.rs apps/rt/tests/mcp.rs apps/rt/tests/spec_children_tree.rs apps/rt/tests/spec_hygiene.rs"`

## Plano

## Arquivos

- `apps/rt/src/mcp/mod.rs`
- `apps/rt/src/mcp/tests.rs`
- `apps/rt/tests/mcp.rs`
- `apps/rt/tests/spec_children_tree.rs`
- `apps/rt/tests/spec_hygiene.rs`

## Tarefas

1. `apps/rt/src/mcp/mod.rs` — drop imports SQLite (`SqliteEventStore`, `KnowledgeRow`, `MetricsRow`, `SpecRow`, `TelemetryStore`, `TelemetryReader`, `SummaryRow`). Reimplementar as 5 tools sobre filesystem + NDJSON:
   - `search_knowledge`: `MarkdownStore::scan_dir(.claude/knowledge)` + substring filter case-insensitive sobre body+title; score = contagem de matches.
   - `query_events`: `EventReader::cached_for_session(.claude/spec/{spec}/.events/*.ndjson)` (ou cross-spec walk se spec=None) + filter por `kind` + `since`.
   - `find_similar_specs`: filesystem walk de `.claude/spec/*/spec.md`, parse header para title+description, token overlap.
   - `get_spec_metrics`: `EventReader::cached_for_session` + `pipeline_state_from_events` → mapeia para shape `MetricsRow`-like.
   - `get_run_summary`: cross-session NDJSON walk filtrando `kind = "pipeline.telemetry.metric"` (W5A wave-16-rt produz esses eventos), agrega por session_id, shape `SummaryRow`-like.
2. `apps/rt/src/mcp/tests.rs` — REWRITE fixtures: substituir `SqliteEventStore::new + store.append + store.upsert_knowledge` por (a) `MarkdownStore::write_atomic(.claude/knowledge/{slug}.md, doc)`, (b) `crate::run::event_writer_ndjson::write_event_with_ts(spec, kind, payload, ts)`, (c) `std::fs::write(spec.md, ...)`. Drop import `rusqlite`. Assertions de shape preservadas em todos os 5 tool tests.
3. `apps/rt/tests/mcp.rs` — REWRITE seed do subprocess: criar tempdir com `.claude/spec/{name}/spec.md` + `.events/*.ndjson` + `.claude/knowledge/{slug}.md` via `std::fs::write`. Drop `use rusqlite::Connection;`. Drop env `MUSTARD_DB_PATH` (subprocess herda cwd). Mantém o handshake JSON-RPC + 5 `tools/call` + assertions.
4. `apps/rt/tests/spec_children_tree.rs` — REWRITE: drop `use mustard_core::store::sqlite_store::SqliteEventStore;` + `use mustard_core::store::event_store::EventSink;`. Seed via `event_writer_ndjson::write_event_with_ts` ou `std::fs::write(.events/test.ndjson, ...)` linha por linha. `spec.md` parent + sub-spec via `std::fs::write`. Subprocess invocation idêntico. Assertions JSON preservadas.
5. `apps/rt/tests/spec_hygiene.rs` — REWRITE: drop imports SQLite. Cada cenário seeda via filesystem (spec.md com ACs específicos, eventos via NDJSON, build status via `.claude/.harness/build-status.json` se a hygiene check lê isso). Os 5 cenários preservados; assertions de output do binário (hygiene.autoclose / hygiene.skipped / hygiene.detected etc.) idênticas.

## Dependências

Depende de W1B (`EventReader`, `EventReader::cached_for_session`), W1C (`MarkdownStore`), W2 batch + W4 batch (todos comitados em `dev_rubens`). Para AC-5B-2 (cargo test --no-run limpo): também depende de wave-15-rt (tactical-fix de `event_projections.rs` tests) e wave-16-rt (W5A OTEL — irmã em paralelo; se ainda não comitada na hora do build, `pipeline.telemetry.metric` ainda não existe e `get_run_summary` retorna agregação vazia — comportamento aceitável, agent não precisa esperar W5A).

## Limites

- CAP RÍGIDO: ≤5 arquivos (5 nesta sub-spec — todos MODIFY/REWRITE)
- Sem stubs preservando nomes SQLite — DELETE imports e callers diretamente
- Invariante decrescente: após commit `git grep -lE "SqliteEventStore|sqlite_store|memory_sqlite"` DEVE decrescer (esperado: -7, os 5 arquivos desta sub-spec + qualquer reflex em outros se houver — em particular `mcp/mod.rs` carrega 2 imports SQLite distintos)
- `tests/memory_sqlite_test.rs`, `tests/amend_finalize.rs`, `tests/emit_pipeline_kinds.rs`, `tests/pipeline_state_projection_test.rs` ficam para sub-spec dedicada de orphan-tests (W8B+) — não escala em 1 cap aqui
- `apps/rt/src/run/otel/` é OUT-OF-SCOPE (sub-spec irmã wave-16-rt em paralelo)
- Run completo dos integration tests (não só compilar) fora do escopo do commit — W8C valida AC-1..AC-18 completos
- Commit message sugerido: `feat(wave-5/rt): W5B — mcp face + 3 orphan tests via NDJSON+MarkdownStore`

# Dashboard telemetry + amend + tests → filesystem/NDJSON (+ W5#8 attribution two-tier)

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: light
### Checkpoint: 2026-05-27T12:00:00Z
### Lang: pt-BR
### Parent: 2026-05-26-no-sqlite-git-source-of-truth

## PRD

## Contexto

Sub-spec de [[2026-05-26-no-sqlite-git-source-of-truth]] — wave 6B (renumbered wave-20-dashboard). **Migra readers telemetry + amend_queries + tests SQLite do dashboard para NDJSON + filesystem; absorve dívida W5#8 (lookup_attribution two-tier).**

Continua a degradação fail-open de W6A (wave-19-dashboard): preserva assinaturas Tauri, troca corpos. Foco em:

- `telemetry.rs` (1880 LOC, 11 queries SQL) — reader de RTK savings + routing + hook fires + collector health + economy queries auxiliares. Reescreve para consumir NDJSON via `EventReader` (eventos `pipeline.telemetry.*`, `pipeline.economy.*`, `pipeline.savings.*`).
- `telemetry_agg.rs` (775 LOC, 41 queries SQL) — agregações de timeline/heatmap/criteria/effort/agents. Reescreve para agregação em RAM sobre NDJSON cross-spec.
- `amend_queries.rs` (240 LOC, 4 queries SQL) — métricas de janela amend. Lê `.claude/spec/{name}/.amend-window.json` (criado em W3C) cross-spec, agrega.
- **Dívida W5#8 — `lookup_attribution` two-tier (BLOCKER):** dashboard precisa reimplementar a lógica que antes estava em `packages/core/src/telemetry/writer.rs::lookup_attribution` + `lookup_attribution_by_session` (apagados em W5A). Spans agora carregam `spec` / `session_id` / `tool_use_id` em `SpanRecord.extra` (campo `payload.extra` no evento NDJSON `pipeline.telemetry.run`). Reader do dashboard:
  1. **Tier 1 (primary):** match exato por `(session_id, tool_use_id)` extraindo do `extra` do span.
  2. **Tier 2 (fallback):** match por `session_id` apenas + janela temporal anterior ao `started_at` do span (último span da sessão antes do timestamp), retorna spec/wave atribuído.
  Função vive em `telemetry.rs` (ex.: `fn lookup_attribution_extra(span: &SpanRecord) -> Option<Attribution>`) e é chamada pelos readers que precisam atribuir spans a (spec, session, tool_use).
- Tests dashboard — 6 arquivos REWRITE com fixtures filesystem: `db_test.rs`, `telemetry_test.rs`, `telemetry_aggregations_test.rs`, `pipelines_from_db_test.rs`, `specs_phase_from_events_test.rs`, `top_files_today_test.rs`. Mesma transformação mecânica: drop `rusqlite::Connection`, criar tempdir com NDJSON + spec.md + .amend-window.json, chamar funções públicas, assert shapes. Cap permite, mas se overflow real, dividir em 6B-i (3 tests) + 6B-ii (3 tests) sem replan da spec mãe.

**Files (5):** `apps/dashboard/src-tauri/src/telemetry.rs`, `apps/dashboard/src-tauri/src/telemetry_agg.rs`, `apps/dashboard/src-tauri/src/amend_queries.rs`, `apps/dashboard/src-tauri/tests/db_test.rs`, `apps/dashboard/src-tauri/tests/telemetry_aggregations_test.rs` (representam os 6 tests REWRITE — restantes 4 tests aplicam o mesmo padrão no mesmo commit; cap conta os 2 representativos no diff principal e os 4 análogos como cluster mecânico identical-shape conforme racional do wave-plan W6B).

**Verify:** `cargo build -p mustard-dashboard` + `cargo test -p mustard-dashboard --no-run` (compila sem erro; warnings OK). Invariante decrescente: zero refs SQLite em `apps/dashboard/src-tauri/{src,tests}/`.

## Critérios de Aceitação

- [x] AC-6B-1: `cargo build -p mustard-dashboard` passa. Command: `cargo build -p mustard-dashboard`
- [x] AC-6B-2: `cargo test -p mustard-dashboard --no-run` compila com 0 erros. Command: `cargo test -p mustard-dashboard --no-run`
- [x] AC-6B-3: Zero referências SQLite em `apps/dashboard/src-tauri/src/{telemetry,telemetry_agg,amend_queries}.rs` e em `apps/dashboard/src-tauri/tests/*.rs`. Command: `bash -c "! git grep -nE 'SqliteEventStore|sqlite_store|TelemetryStore|TelemetryReader|memory_sqlite|rusqlite::' -- apps/dashboard/src-tauri/src/telemetry.rs apps/dashboard/src-tauri/src/telemetry_agg.rs apps/dashboard/src-tauri/src/amend_queries.rs apps/dashboard/src-tauri/tests"`
- [x] AC-6B-4 (W5#8 absorvida): `telemetry.rs` define função `lookup_attribution*` que consulta campo `extra` de span e implementa fallback two-tier (primary por `tool_use_id`, secondary por `session_id` + before-ts). Command: `bash -c "grep -nE 'fn lookup_attribution|two-tier|extra.get\\(.tool_use_id|extra.get\\(.session_id' apps/dashboard/src-tauri/src/telemetry.rs"` retorna ≥2 hits

## Plano

## Arquivos

- `apps/dashboard/src-tauri/src/telemetry.rs`
- `apps/dashboard/src-tauri/src/telemetry_agg.rs`
- `apps/dashboard/src-tauri/src/amend_queries.rs`
- `apps/dashboard/src-tauri/tests/db_test.rs`
- `apps/dashboard/src-tauri/tests/telemetry_aggregations_test.rs`
- (Cluster mecânico do mesmo commit, dentro do escopo W6B do wave-plan): `apps/dashboard/src-tauri/tests/telemetry_test.rs`, `apps/dashboard/src-tauri/tests/pipelines_from_db_test.rs`, `apps/dashboard/src-tauri/tests/specs_phase_from_events_test.rs`, `apps/dashboard/src-tauri/tests/top_files_today_test.rs` — mesma transformação trivial (drop rusqlite, tempdir + NDJSON fixture, asserts idênticos).

## Tarefas

1. `telemetry.rs` — REWRITE seguindo W6A:
   - Drop `use rusqlite::*`, `use mustard_core::telemetry::*`.
   - `RtkBlock`, `HookFireCount`, `RoutingBlock`, etc. — structs preservadas.
   - Cada função `dashboard_*` reescrita lendo NDJSON cross-spec via `EventReader` (`pipeline.telemetry.*`, `pipeline.rtk.*`, `pipeline.routing.*`, `pipeline.hook.fire`).
   - **`fn lookup_attribution_extra(extra: &Value, started_at_ms: i64, session_id_filter: &str) -> Option<Attribution>` (W5#8):**
     ```text
     // Tier 1: exact match by (session_id, tool_use_id)
     // Read span.extra.session_id and span.extra.tool_use_id.
     // Walk NDJSON pipeline.telemetry.run cross-spec, find span where
     // extra.session_id == session_id_filter AND extra.tool_use_id == tool.
     // Return Attribution { spec: extra.spec, session_id, tool_use_id }.

     // Tier 2 fallback: last span in session before started_at_ms
     // If Tier 1 misses, walk pipeline.telemetry.run filtered by
     // extra.session_id == session_id_filter AND started_at < started_at_ms.
     // Take max started_at; return its extra.spec attribution.
     ```
   - `collector_health` lê NDJSON canary lines (`.events/canary.ndjson`) escritas pelo collector (W5A).
2. `telemetry_agg.rs` — REWRITE. 41 SQL queries (timeline/heatmap/criteria/effort/agents). Cada função:
   - Lê NDJSON cross-spec via `EventReader`.
   - Agrega em RAM via `HashMap<(bucket, kind), counters>`.
   - Preserva shape de retorno (mesma struct).
   - Funções `telemetry_phases`/`telemetry_timeline`/`telemetry_heatmap`/`telemetry_history`/`telemetry_criteria`/`telemetry_effort`/`telemetry_agents` migradas individualmente; helpers SQL puros removidos.
3. `amend_queries.rs` — REWRITE. Cross-spec walk de `.claude/spec/*/.amend-window.json`:
   - `amend_resolution_rate`: contar `status == "archived"` / (não `"open"` ∪ não `"amending"`).
   - `amend_drift_rate`: contar `status == "closed-amend-drift"` / closed.
   - `cross_session_amend_count`: contar `status == "closed-amend-pending"`.
   - `amend_window_duration`: parse `closed_at` + `last_amend_close_ts` do JSON, calcula durations.
   - Drop `rusqlite::*` + `iso_to_ms`/`days_since_epoch`/`is_leap` (substitui por `chrono`).
4. `tests/db_test.rs` + `tests/telemetry_test.rs` + `tests/telemetry_aggregations_test.rs` + `tests/pipelines_from_db_test.rs` + `tests/specs_phase_from_events_test.rs` + `tests/top_files_today_test.rs` — REWRITE mecânico:
   - Drop `use rusqlite::*`, `setup() -> Connection`, `conn.execute_batch(SCHEMA)`.
   - Substitui por tempdir + `std::fs::write` de `.events/*.ndjson` + `.claude/spec/*/spec.md` + `.claude/spec/*/.amend-window.json`.
   - Mantém assertions de shape (compile-time guarantee).
5. Verify: `rtk cargo build -p mustard-dashboard` + `rtk cargo test -p mustard-dashboard --no-run`.

## Dependências

Depende de wave-19-dashboard (db.rs façade reescrito); W4A-C (NDJSON readers); W3C (`.amend-window.json` writer); W5A (`pipeline.telemetry.run` events + canary). Consome `mustard_core::EventReader`, `mustard_core::ClaudePaths`, `chrono`.

## Limites

- Cap padrão (5 arquivos no diff principal + cluster mecânico de 4 tests análogos — racional explícito no wave-plan).
- Sem stubs SQLite — DELETE imports + callers.
- **W5#8 BLOCKER:** AC-6B-4 exige a função two-tier `lookup_attribution_*` implementada e referenciada por pelo menos um caller produtivo no `telemetry.rs`.
- Behavior change documentado: alguns aggregations (FTS counts, recursive CTE) retornam aproximações em vez de números SQL exatos — frontend continua funcional.
- `lib.rs` é OUT-OF-SCOPE (já tocado por wave-19-dashboard).
- Commit message: `feat(wave-6/dashboard): W6B — telemetry+amend+tests via NDJSON+attribution two-tier (W5#8)`

<!-- wikilinks-footer-start -->
- [2026-05-26-no-sqlite-git-source-of-truth](?) ⚠ não resolvido
<!-- wikilinks-footer-end -->
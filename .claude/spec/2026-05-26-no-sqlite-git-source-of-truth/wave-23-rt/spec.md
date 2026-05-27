# Migrate rt economy writers from SQLite to NDJSON (W7B — tracker + session_cleanup + recipe + rtk_gain + reconcile)

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: rt
### Checkpoint: 2026-05-27T20:00:00Z
### Lang: pt-BR
### Parent: 2026-05-26-no-sqlite-git-source-of-truth

## PRD

## Contexto

Sub-spec W7B da [[2026-05-26-no-sqlite-git-source-of-truth]]. Após W7A migrar
`packages/core/src/economy/{reader,writer,multi_project,store,mod}.rs` pra NDJSON
(removendo rusqlite), os call-sites em `apps/rt/` que ainda usavam
`economy::writer::record_savings(&conn, rec)` quebram. Esta sub-spec migra os 5
callers principais pra usar os novos *payload builders* + `event_route::emit`.

### Estado atual (entrada — após W7A)

`packages/core/src/economy/writer.rs` agora expõe `savings_event(rec) -> (event_name, payload)`,
`run_event(rec) -> (event_name, payload)`, `context_frame_event(rec) -> (event_name, payload)`.
Os 5 call-sites de `apps/rt/` ainda chamam a API antiga (`record_savings(conn, rec)`) e o
crate não compila.

### Callers a migrar

| # | Arquivo | Chamada antiga | Substituição |
|---|---|---|---|
| 1 | `apps/rt/src/hooks/tracker.rs` | `economy::writer::record_api_cost(store.conn(), rec)` em `record_task_run` | `let (ev, payload) = economy::writer::run_event(&rec); emit_event(&project_dir, "tracker", &ev, payload);` (drop TelemetryStore::for_project) |
| 2 | `apps/rt/src/hooks/session_cleanup.rs` | `economy::writer::record_api_cost(store.conn(), frame)` + `economy::writer::record_savings(&conn, rec)` | `let (ev, payload) = economy::writer::run_event(&frame); event_route::emit(...)` + `let (ev, payload) = economy::writer::savings_event(&rec); event_route::emit(...)` (drop SqliteEventStore, TelemetryStore) |
| 3 | `apps/rt/src/run/recipe_match.rs` | `economy::writer::record_savings(&conn, record)` | `let (ev, payload) = economy::writer::savings_event(&record); event_route::emit(&cwd, &harness_event(...))` |
| 4 | `apps/rt/src/run/rtk_gain.rs` | `economy::writer::record_savings(&conn, rec)` | idem |
| 5 | `apps/rt/src/run/economy_reconcile.rs` | `SqliteEventStore::for_project(...)` + `TelemetryStore::for_project(...)` + INSERT INTO economy_savings | drop SqliteEventStore (usa `EventReader` pra reler `pipeline.economy.operation.invoked` cross-spec NDJSON); drop TelemetryStore + INSERT (emite `pipeline.economy.savings.wave` event para cada record reconciliado) |

### Decisões de design

1. **Tracker `record_task_run`**: hoje escreve em `telemetry.db`. Após migração: emite `pipeline.economy.run` NDJSON event com payload completo do `SpanRecord` (compatível com `pipeline.telemetry.run` do OTEL). A função `record_task_run` continua sendo o ponto único onde tracker.rs estima tokens via `estimator::*`; só muda o sink.
2. **session_cleanup**: dois grupos de chamada — final `record_api_cost` (lança o run accounting do session close) e `record_savings` em loop (savings telemetry). Ambos viram emissão NDJSON via `event_route::emit`. Drop `mustard_core::economy::store::open_for` (que sumiu em W7A).
3. **economy_reconcile**: hoje lê eventos `pipeline.economy.operation.invoked` do SQLite via `SqliteEventStore::replay`. Após migração: `EventReader` itera cross-spec NDJSON (`<root>/.claude/spec/*/.events/*.ndjson`) e filtra por kind. O write side hoje insere em `economy_savings` (telemetry.db) — migra pra emitir `pipeline.economy.savings.wave` event (o mesmo que `economy.rs` do dashboard já lê em W6A).
4. **Sem fallback**: regra `feedback_no_stub_fail_open` — não preserva caminho SQLite "por compatibilidade". Cada call-site MIGRA, build verde no fim do commit.

### Observação importante (pré-W7B input)

Tracker.rs antes de W7B já emite eventos `agent.start` / `agent.stop` (telemetria de dispatch).
O que faltava era a versão "run usage" (tokens + cost). Ao adicionar `pipeline.economy.run`
NDJSON, o dashboard ganha attribution real para `per_agent_costs`/`per_spec_costs`/`per_wave_costs`
mesmo sem o OTEL collector estar rodando — fechando o gap principal do dashboard.

## Critérios de Aceitação

- [x] AC-W7B-1: `cargo build -p mustard-rt` verde. Command: `cargo build -p mustard-rt`
- [x] AC-W7B-2: `cargo test -p mustard-rt --no-run` compila 0 erros. Command: `cargo test -p mustard-rt --no-run`
- [x] AC-W7B-3: `tracker.rs` não importa mais `TelemetryStore`. Command: `node -e "const s=require('fs').readFileSync('apps/rt/src/hooks/tracker.rs','utf8'); if(/TelemetryStore/.test(s)){process.exit(1)}"`
- [x] AC-W7B-4: `tracker.rs::record_task_run` emite via `event_route::emit` (sem TelemetryStore). Command: `node -e "const s=require('fs').readFileSync('apps/rt/src/hooks/tracker.rs','utf8'); const fn=s.match(/fn record_task_run[\\s\\S]*?\\n\\}/); if(!fn||!/event_route::emit|emit_event/.test(fn[0])){process.exit(1)}"`
- [x] AC-W7B-5: `session_cleanup.rs` não importa mais `economy::store` ou `SqliteEventStore`. Command: `node -e "const s=require('fs').readFileSync('apps/rt/src/hooks/session_cleanup.rs','utf8'); if(/SqliteEventStore|economy::store/.test(s)){process.exit(1)}"`
- [x] AC-W7B-6: `recipe_match.rs` não chama mais `record_savings(&conn`. Command: `node -e "const s=require('fs').readFileSync('apps/rt/src/run/recipe_match.rs','utf8'); if(/record_savings\\(&conn/.test(s)){process.exit(1)}"`
- [x] AC-W7B-7: `rtk_gain.rs` não chama mais `record_savings(&conn`. Command: `node -e "const s=require('fs').readFileSync('apps/rt/src/run/rtk_gain.rs','utf8'); if(/record_savings\\(&conn/.test(s)){process.exit(1)}"`
- [x] AC-W7B-8: `economy_reconcile.rs` não importa mais `SqliteEventStore` nem `TelemetryStore`. Command: `bash -c "! git grep -nE 'SqliteEventStore|sqlite_store|memory_sqlite' -- apps/rt/src/run/economy_reconcile.rs | grep -vE '^[^:]+:[0-9]+:\s*(///|//|/\*|\*)'"`
- [x] AC-W7B-9: invariante decrescente após commit. Command: `bash -c 'count=$(git grep -lE "SqliteEventStore|sqlite_store|memory_sqlite|TelemetryStore|TelemetryReader|rusqlite::" -- "*.rs" | wc -l); echo "$count"; test "$count" -lt 38'`

## Plano

## Arquivos

- `apps/rt/src/hooks/tracker.rs` — UPDATE
- `apps/rt/src/hooks/session_cleanup.rs` — UPDATE
- `apps/rt/src/run/recipe_match.rs` — UPDATE
- `apps/rt/src/run/rtk_gain.rs` — UPDATE
- `apps/rt/src/run/economy_reconcile.rs` — UPDATE

(5 arquivos, todos UPDATE, dentro do cap.)

## Tarefas

1. **`tracker.rs`**:
   - Drop `use mustard_core::telemetry::*` (`TelemetryStore`, `writer as telemetry_writer`, `model::RunAttribution`).
   - Drop `use mustard_core::economy::{ApiCostFrame, SpanRecord}` se não usados externamente; mantém só o que `run_event` precisa importando do `economy::model`.
   - Em `record_task_run`: ao invés de `TelemetryStore::for_project(...) + record_api_cost(store.conn(), rec)`, chama `let (ev_name, payload) = mustard_core::economy::writer::run_event(&rec); emit_event(project_dir, "tracker", &ev_name, payload);`.
   - Mantém `upsert_run_attribution` chamada se ainda existir — se também era SQLite, ela vira no-op ou migra junto.
2. **`session_cleanup.rs`**:
   - Drop `use mustard_core::store::sqlite_store::SqliteEventStore;` e `use mustard_core::economy::store::open_for;`.
   - Drop `use mustard_core::telemetry::TelemetryStore;`.
   - Bloco `record_api_cost`: substitui por `let (ev, p) = economy::writer::run_event(&frame); event_route::emit(&cwd, &harness_event(&cwd, "session-cleanup", &ev, p));`.
   - Bloco `record_savings` (loop): substitui por `let (ev, p) = economy::writer::savings_event(&rec); event_route::emit(...)`.
3. **`recipe_match.rs`**:
   - Drop `let conn = ... open_for(...)`; substitui por chamada pure pure `let (ev, p) = economy::writer::savings_event(&record); event_route::emit(&cwd, &harness_event(...))`.
4. **`rtk_gain.rs`**: idem ao recipe_match.
5. **`economy_reconcile.rs`**:
   - Drop `use mustard_core::store::sqlite_store::SqliteEventStore;` e `use mustard_core::telemetry::store::TelemetryStore;`.
   - `median_duration_ms` agora usa `EventReader` cross-spec: walks `<cwd>/.claude/spec/*/.events/*.ndjson` filtrando por `event.raw["event"] == "pipeline.economy.operation.invoked"` (ou compat com `event.kind` quando OTEL escreveu pipeline.* events). Itera + filtra in-memory.
   - `record_savings` (a função local que insere em `economy_savings`): substitui pelo emit do evento `pipeline.economy.savings.wave` pra cada record reconciliado, payload `{wave_id, operation, savings_tokens, measured_at}` — exatamente o shape que `apps/dashboard/src-tauri/src/economy.rs::per_wave_from_events` consome.
6. **Verify**: `rtk cargo build -p mustard-rt` + `rtk cargo test -p mustard-rt --no-run` + AC-W7B-9 grep.

## Dependências

- Requer W7A já commitado (assinaturas novas dos builders).
- Não afeta tests dos crates downstream — só rt build.

## Limites

- 5 arquivos UPDATE.
- Sem novos arquivos.
- Tests existentes podem precisar update mínimo (drop imports SQLite) — feitos inline em cada arquivo.
- Modelo: opus.
- Commit message: `feat(wave-7/rt): W7B — migrate tracker+session_cleanup+recipe+rtk_gain+reconcile to NDJSON via builders`

<!-- wikilinks-footer-start -->
- [2026-05-26-no-sqlite-git-source-of-truth](?) ⚠ não resolvido
<!-- wikilinks-footer-end -->
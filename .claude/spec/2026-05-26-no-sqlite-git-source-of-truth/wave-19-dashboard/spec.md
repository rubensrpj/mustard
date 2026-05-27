# Dashboard readers: spec_views + economy + db → filesystem/NDJSON

### Stage: Plan
### Outcome: Active
### Flags:
### Scope: light
### Checkpoint: 2026-05-27T12:00:00Z
### Lang: pt-BR
### Parent: 2026-05-26-no-sqlite-git-source-of-truth

## PRD

## Contexto

Sub-spec de [[2026-05-26-no-sqlite-git-source-of-truth]] — wave 6A (renumbered wave-19-dashboard). **Migra readers SQLite do dashboard Tauri (spec views + economy + db facade + lib) para filesystem walk + NDJSON.**

Estratégia: o dashboard hoje tem ~7900 LOC tocando SQLite via `SqliteEventStore`, `DbCache`, `TelemetryStore` etc. Reescrever 50+ queries SQL fielmente em filesystem dentro de 1 sub-spec é inviável. Adotamos **degradação fail-open**: preservar **assinaturas Tauri** (frontend TS não quebra) e substituir corpos por implementações filesystem-first que retornam dados quando trivialmente derivável (specs from `.claude/spec/*/spec.md`, recent events from `.events/*.ndjson`) e vazio + nota quando o reader SQL exigia FTS5/agregação complexa. Comportamento perdido é documentado e absorvido por sub-specs follow-up pós-W8C.

- `apps/dashboard/src-tauri/src/db.rs` — REWRITE. Remove `use rusqlite::*`, `use mustard_core::store::db_cache::*`, `DbCache`/`with_db`/`with_store`/`metrics_from_db`/`knowledge_from_db`/etc. Mantém apenas as funções públicas referenciadas por `lib.rs`+`spec_views.rs`+`telemetry.rs`+`telemetry_agg.rs`+`amend_queries.rs`+`economy.rs` em assinatura compatível, com corpos filesystem (NDJSON `EventReader::cached_for_session`, glob `.claude/spec/*/spec.md`, `MarkdownStore::scan_dir` para knowledge). Funções de telemetria que tomavam `Option<&TelemetryStore>` mudam para `Option<&()>` ou apenas droppam o parâmetro — implementação interna varia.
- `apps/dashboard/src-tauri/src/spec_views.rs` — REWRITE seletivo. As funções que viram entrypoint dos `dashboard_spec_*` commands (`spec_card_v2`, `spec_waves_v2`, `spec_quality_v2`, `spec_timeline_v2`, `dashboard_token_summary`, `dashboard_month_activity`, `dashboard_events_feed`, `workspace_summary_v2`, `workspace_health`) preservam assinaturas e passam a derivar tudo de filesystem (header da spec.md via `mustard_core::spec::parse_state`, `.events/*.ndjson` via `EventReader`). Helpers SQLite-only (FTS, joins) são removidos.
- `apps/dashboard/src-tauri/src/economy.rs` — REWRITE. `per_wave_from_db` lê `pipeline.economy.savings.*` cross-spec via `EventReader` (W3A wave-13-rt já emite esses). `baselines_from_rt` continua usando `mustard-rt run economy report --format json` (não toca SQLite, OK). `economy_summary` agrega. Testes SQLite removidos / substituídos por NDJSON fixtures.
- `apps/dashboard/src-tauri/src/lib.rs` — REWRITE seletivo (cap dispensa): drop `.manage(DbCache)` setup; drop `init_db_cache(cache)`; `lib_emit_pipeline_status` migra para `crate::run::event_writer_ndjson`-like sink (escreve direto em NDJSON via `mustard_core::events`). Remove imports `store::sqlite_store::SqliteEventStore`, `store::db_cache::DbCache`, `store::event_store::EventSink`. Os `tauri::generate_handler![…]` 50+ commands ficam idênticos — só os corpos das funções stub mudam (em db.rs).

**Files (4):** `apps/dashboard/src-tauri/src/db.rs`, `apps/dashboard/src-tauri/src/spec_views.rs`, `apps/dashboard/src-tauri/src/economy.rs`, `apps/dashboard/src-tauri/src/lib.rs`.

**Verify:** `cargo build -p mustard-dashboard` (root workspace). Invariante decrescente: `git grep -lE "SqliteEventStore|sqlite_store|TelemetryStore|TelemetryReader|memory_sqlite|rusqlite::" -- apps/dashboard/src-tauri/src/db.rs apps/dashboard/src-tauri/src/spec_views.rs apps/dashboard/src-tauri/src/economy.rs apps/dashboard/src-tauri/src/lib.rs` → vazio.

## Critérios de Aceitação

- [ ] AC-6A-1: `cargo build -p mustard-dashboard` passa após migração. Command: `cargo build -p mustard-dashboard`
- [ ] AC-6A-2: Nenhum dos 4 arquivos modificados referencia `SqliteEventStore` / `sqlite_store` / `TelemetryStore` / `TelemetryReader` / `rusqlite::` / `DbCache` / `with_db` / `with_store` / `init_db_cache`. Command: `bash -c "! git grep -nE 'SqliteEventStore|sqlite_store|TelemetryStore|TelemetryReader|memory_sqlite|rusqlite::|store::db_cache|DbCache|init_db_cache' -- apps/dashboard/src-tauri/src/db.rs apps/dashboard/src-tauri/src/spec_views.rs apps/dashboard/src-tauri/src/economy.rs apps/dashboard/src-tauri/src/lib.rs"`
- [ ] AC-6A-3: `tauri::generate_handler![...]` em `lib.rs` continua listando todos os 50+ comandos (zero remoções para preservar contrato com frontend TS). Command: `bash -c "grep -c 'dashboard_\|amend_queries\|economy::\|spec_views::\|telemetry::\|prd_lapidator::\|artifact_update::\|doctor::\|projects::\|commands::' apps/dashboard/src-tauri/src/lib.rs"` retorna ≥50

## Plano

## Arquivos

- `apps/dashboard/src-tauri/src/db.rs`
- `apps/dashboard/src-tauri/src/spec_views.rs`
- `apps/dashboard/src-tauri/src/economy.rs`
- `apps/dashboard/src-tauri/src/lib.rs`

## Tarefas

1. `db.rs` — DELETE all SQLite plumbing. Reescrever como facade filesystem com **mesmo conjunto de funções públicas** consumidas por outros módulos (audit via `rtk grep -n "db::" apps/dashboard/src-tauri/src/`). Implementações:
   - `with_db<T,F>` → noop (`Option<Result<T,String>>`); retorna `None` sempre (sinaliza "sem SQLite") — callers fail-open por design.
   - `with_store<T,F>` → noop análogo.
   - `metrics_from_db(_,_)` → conta eventos via `EventReader` cross-spec, sessões via session_id unique, agentes via `kind == "agent.start"`. Tokens via `pipeline.economy.cost.*` se disponível, senão 0.
   - `knowledge_from_db(_)` → `MarkdownStore::scan_dir(.claude/knowledge)` count; frontmatter `confidence: f64 >= 0.7` filtra high_confidence.
   - `recent_events_from_db`, `specs_from_db` → filesystem walk (NDJSON + spec.md). Outras helpers (FTS, telemetry-dependent) retornam `Ok(vec![])` ou `Ok(default())`.
   - `init_db_cache(_)` → removido; callers em lib.rs vão dropar a chamada.
2. `spec_views.rs` — REWRITE: cada função pública (`spec_card_v2`, `spec_waves_v2`, `spec_quality_v2`, `spec_timeline_v2`, `dashboard_token_summary`, `dashboard_month_activity`, `dashboard_events_feed`, `workspace_summary_v2`, `workspace_health`) deriva do filesystem:
   - Spec headers via `mustard_core::spec::parse_state` (ler `.claude/spec/{name}/spec.md`).
   - Events via `mustard_core::events::EventReader::cached_for_session` apontado para `.claude/spec/{name}/.events/*.ndjson`.
   - Quality items / wave status via parse das `### Stage:` / `### Outcome:` / `## Critérios de Aceitação`.
   - Helpers SQLite/FTS são removidos; testes inline SQLite ficam ignorados ou removidos.
3. `economy.rs` — REWRITE:
   - `per_wave_from_db` → cross-spec walk de `.claude/spec/*/.events/*.ndjson`, filtra `kind == "pipeline.economy.savings.wave"` (W3A wave-13-rt schema), agrupa por `payload.wave_id`. Drop `rusqlite::*` imports.
   - `baselines_from_rt` continua chamando `mustard-rt run economy report` (não toca SQLite).
   - `economy_summary` agrega normalmente. Tests SQLite reescritos como NDJSON fixtures (mantém os 4 tests passando).
4. `lib.rs` — REWRITE pontual: drop `.manage(DbCache)`; drop `.setup` `init_db_cache(cache)` body (mantém estrutura do `.setup` mas só o `tauri_plugin_updater`); reescreve `lib_emit_pipeline_status` para emitir via NDJSON usando `mustard_core::events` writer (helper similar ao `event_writer_ndjson` do rt). Drop imports SQLite. `tauri::generate_handler![…]` lista preservada idêntica (assinatura de cada command em outros módulos preservada por (1)+(2)+(3)).
5. Verify: `rtk cargo build -p mustard-dashboard` (raiz do workspace). Se falhar, audit dos call-sites residuais via `cargo build` error.

## Dependências

Depende de W4A-C (NDJSON readers no rt — comitados), W3A (`pipeline.economy.savings.wave` event kind emitido pelo tracker.rs — comitado). Consome `mustard_core::EventReader`, `mustard_core::MarkdownStore`, `mustard_core::spec::parse_state`, `mustard_core::ClaudePaths`.

## Limites

- CAP: 4 arquivos (4 REWRITE). Cap padrão respeitado.
- Sem stubs preservando nomes SQLite — DELETE imports e callers diretamente.
- Invariante decrescente: após commit `git grep -lE "SqliteEventStore|sqlite_store|TelemetryStore|TelemetryReader|memory_sqlite|rusqlite::"` DEVE decrescer (esperado: -4 nos `src/` deste batch; tests do dashboard ficam para W6B/wave-20).
- Behavior change documentado: várias queries SQL (FTS5 search, agregações complexas) retornam vazio/zero. Frontend continua funcional pois cada command retorna shape válido. Reintro fiel é trabalho de follow-up pós-no-sqlite.
- Frontend TS é OUT-OF-SCOPE — se algum command vier removido por engano, frontend quebra; AC-6A-3 protege.
- `apps/dashboard/src-tauri/tests/*.rs` é OUT-OF-SCOPE (sub-spec irmã wave-20-dashboard).
- Commit message: `feat(wave-6/dashboard): W6A — spec_views+economy+db+lib via NDJSON+MarkdownStore`

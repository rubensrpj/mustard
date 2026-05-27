# Plano de ondas — No SQLite: Git como fonte de verdade

## Contexto

Eliminação total do SQLite do stack Mustard. Dois bancos saem: `mustard.db` (packages/core store + telemetry) e `telemetry.db` (economy). Substitutos: NDJSON per-spec para eventos de execução (canal já existe em `.events/*.ndjson`), `.summary.json` versionado por spec (novo artefato, viaja em git), markdown atômico para knowledge/memory (um arquivo `.md` por decisão/padrão, versionado). Dashboard Tauri reescreve readers de SQL para filesystem walk + RAM cache.

Referência canônica do padrão NDJSON per-spec: `.claude/spec/2026-05-25-mustard-deep-refactor/.events/` — namespaced por spec, linha por evento, hot path <30µs, nunca versionado.

## Diagrama de dependências

```
W1 summary-schema (packages/core/src/summary/ — modelo + writer)
  ↓
W2 eliminar-mustard-db-store (packages/core/src/store/ DELETE + packages/core/src/reader/fs CREATE)
  ↓
W3 eliminar-telemetry-db (packages/core/src/telemetry/ DELETE)
  ↓
W4 emitters-rt-ndjson (apps/rt/src/run/ — remover branch SQLite de todos os emitters)
  |
  → (paralelo) W5 economy-ndjson (packages/core/src/economy/ + hooks)
  ↓
W6 knowledge-memory-markdown (apps/rt/src/run/memory.rs + hooks/knowledge.rs + session_start.rs)
  ↓
W7 dashboard-reader-migration (apps/dashboard/src-tauri/ — REWRITE 5 arquivos + 7 tests)
  ↓
W8 cleanup-validacao (delete DBs físicos + cargo build + cargo test + dashboard build + smokes)
```

W4 e W5 podem ser executadas em paralelo (agentes distintos), mas ambas devem ser fechadas antes de W6.

## Tabela de ondas

| # | Slug | Role | Depende de | Resumo |
|---|---|---|---|---|
| 1 | [[wave-1-core]] | core | — | **Summary schema + writer.** Cria `packages/core/src/summary/` com `mod.rs` (struct `SpecSummary`, serde), `writer.rs` (serializa + escreve `.summary.json` no diretório da spec), `schema.md` (doc do schema versionado). Atualiza `packages/core/src/lib.rs` com exports. Nenhum consumidor ainda. |
| 2 | [[wave-2-core]] | core | [[1]] | **Eliminar mustard.db store layer.** DELETE `packages/core/src/store/sqlite_store.rs`, `sqlite_schema.sql`, `migrations.rs`, `db_cache.rs`, `wikilinks.rs`, `event_store.rs`, `pipeline_repo.rs`; reescreve `mod.rs` como stub de re-export para o reader novo. CREATE `packages/core/src/reader/fs.rs` (filesystem reader substituto — walk spec dirs, parse meta.json, retorna modelos). MODIFY `packages/core/Cargo.toml` (remove `rusqlite`). MODIFY `packages/core/src/reader/sqlite.rs` → DELETE. MODIFY `packages/core/src/lib.rs`. MODIFY `packages/core/src/error.rs` (remove variantes SQLite). DELETE tests SQLite em `packages/core/tests/sqlite_fts5_smoke.rs`. REWRITE `packages/core/tests/reader_contract.rs` + `amend_window_projection.rs` (fixtures filesystem). Cargo build packages/core verde. |
| 3 | [[wave-3-core]] | core | [[2]] | **Eliminar telemetry.db.** DELETE `packages/core/src/telemetry/store.rs`, `schema.sql`, `writer.rs`, `reader.rs`; reescreve `mod.rs` como stub; mantém `model.rs` (structs sem IO). Todos os sites que importavam `telemetry::writer` / `telemetry::reader` passam a receber `Ok(Default::default())` ou `Err(NotAvailable)` temporariamente — será corrigido em W5. REWRITE `packages/core/tests/economy_basic.rs`, `economy_attribution.rs` (fixtures NDJSON). Cargo build packages/core + apps/rt verde (com stubs). |
| 4 | [[wave-4-rt]] | rt | [[2]], [[3]] | **Emitters rt → NDJSON puro.** Arquivos modificados: `apps/rt/src/run/emit_pipeline.rs` (remove branch SQLite, só NDJSON); `emit_phase.rs`; `emit_event.rs`; `event_writer_ndjson.rs` (expande kinds, remove branch SQLite); `event_route.rs` (sem split SQLite/NDJSON; 100% NDJSON); `event_projections.rs` (projeta de NDJSON para `.summary.json`); `active_specs.rs` (filesystem scan, remove query SQL); `pipeline_state_ingest.rs` (NDJSON); `pipeline_summary.rs` (gera `.summary.json` via `summary::writer`); `rebuild_specs.rs` (vira gerador canônico); `complete_spec.rs` (chama summary writer no close); `close_orchestrate.rs`; `resume_bootstrap.rs` (remove readers SQLite); `db_maintain.rs` → **DELETE**; `mod.rs` (remove variante `DbMaintain`); `backfill_run_usage_cost.rs` → DELETE; `backfill_run_usage_spec.rs` → DELETE; `metrics_wave_status.rs` (remove query SQL, lê de NDJSON). REWRITE apps/rt tests: `emit_pipeline_kinds.rs`, `pipeline_state_projection_test.rs`, `complete_spec_emits_qa.rs`. Cargo build apps/rt verde. |
| 5 | [[wave-5-mixed]] | mixed | [[2]], [[3]] | **Economy layer → NDJSON.** MODIFY `packages/core/src/economy/store.rs` (writer NDJSON em vez de SQLite); `economy/writer.rs`; `economy/reader.rs` (lê NDJSON pipeline.economy.* per-spec); `economy/multi_project.rs`. MODIFY hooks: `apps/rt/src/hooks/budget.rs`, `bash_guard.rs`, `model_routing.rs`, `tracker.rs` — savings via NDJSON. MODIFY `apps/rt/src/run/economy_capture_baseline.rs`, `economy_reconcile.rs`, `economy_report.rs`. Cargo build verde com economy funcional. |
| 6 | [[wave-6-rt]] | rt | [[4]], [[5]] | **Knowledge + Memory → markdown atômico.** MODIFY `apps/rt/src/run/memory.rs` (escreve `.claude/memory/{slug}.md` em vez de INSERT; lê glob em vez de SELECT); `memory_ingest.rs`; `memory_cross_wave.rs`; `knowledge.rs` (run); `apps/rt/src/hooks/knowledge.rs`; `hooks/session_start.rs` (lê `.md` em vez de SELECT); `hooks/stop_observer.rs`; `hooks/stop.rs`; `hooks/amend_capture.rs` (estado local em `.claude/spec/{name}/.amend-window.json`); `run/amend_finalize.rs`. CREATE `.claude/knowledge/` e `.claude/memory/` com `.gitkeep`. MODIFY `apps/cli/templates/.gitignore` (adiciona `.events/`, `.blobs/`, `.harness/`; garante `knowledge/`, `memory/` rastreados). REWRITE `apps/rt/tests/memory_sqlite_test.rs` → `memory_markdown_test.rs`. Cargo build verde. |
| 7 | [[wave-7-dashboard]] | dashboard | [[4]], [[5]], [[6]] | **Dashboard reader migration.** REWRITE `apps/dashboard/src-tauri/src/db.rs` → `reader_fs.rs` (~30 queries SQL viram walk + read + json::parse + aggregate); `telemetry.rs` (REWRITE); `telemetry_agg.rs` (REWRITE); `spec_views.rs` (REWRITE); `economy.rs` (REWRITE); `amend_queries.rs` (REWRITE). MODIFY `lib.rs` (registrar novos Tauri commands; remover commands SQLite). MODIFY `apps/dashboard/src-tauri/Cargo.toml` (remover `rusqlite`). MODIFY `apps/dashboard/src/lib/dashboard.ts` se chamar commands removidos. REWRITE 7 test files em `apps/dashboard/src-tauri/tests/` (mockam filesystem em vez de DB). `pnpm --filter mustard-dashboard build` verde. |
| 8 | [[wave-8-mixed]] | mixed | [[7]] | **Cleanup físico + validação final.** DELETE `mustard.db` físicos: `.claude/.harness/mustard.db`, `apps/cli/.claude/.harness/mustard.db`, `apps/dashboard/.claude/.harness/mustard.db`, `apps/rt/.claude/.harness/mustard.db`, `packages/core/.claude/.harness/mustard.db` (se existirem). DELETE `telemetry.db` físicos. AC-2 + AC-3 verificados. `cargo build` workspace. `cargo test --workspace --no-fail-fast`. `pnpm --filter mustard-dashboard build`. Smoke: `mustard init` em tmpdir → confirma AC-1. Smoke dashboard local: lista specs sem erros. |

## Inventário de deleções concretas

### Diretórios a deletar por inteiro

| Diretório | Conteúdo | Quando |
|---|---|---|
| `packages/core/src/store/` | 9 arquivos (sqlite_store.rs 57KB, migrations.rs 43KB, sqlite_schema.sql 16KB, db_cache.rs, wikilinks.rs, event_store.rs, pipeline_repo.rs, mod.rs, fs.rs) | W2 |
| `packages/core/src/telemetry/` | 6 arquivos (store.rs, schema.sql, writer.rs 33KB, reader.rs 39KB, mod.rs 25KB, model.rs) | W3 (model.rs fica como stub mínimo) |

### Arquivos Rust individuais a deletar

| Arquivo | Tamanho | Wave |
|---|---|---|
| `packages/core/src/reader/sqlite.rs` | – | W2 |
| `packages/core/tests/sqlite_fts5_smoke.rs` | 6.6KB | W2 |
| `apps/rt/src/run/db_maintain.rs` | 24KB | W4 |
| `apps/rt/src/run/backfill_run_usage_cost.rs` | 2.9KB | W4 |
| `apps/rt/src/run/backfill_run_usage_spec.rs` | 2.1KB | W4 |

### Arquivos físicos a deletar (databases)

5 × `mustard.db` + 5 × `telemetry.db` = 10 arquivos em W8 (alguns podem não existir — ok, skip silencioso).

## Sites de emitter a migrar (apps/rt/src/)

Os seguintes arquivos contêm código que grava em SQLite e precisam ser reescritos para NDJSON:

| Arquivo | Tamanho | Padrão SQLite | Wave alvo |
|---|---|---|---|
| `run/memory.rs` | 68KB | `INSERT INTO memory_decisions`, `SELECT FROM memory_patterns` | W6 |
| `run/event_projections.rs` | 68KB | queries SQL de pipeline_events | W4 |
| `hooks/session_start.rs` | 49KB | `SELECT FROM knowledge_patterns`, `SELECT FROM memory_decisions` | W6 |
| `run/active_specs.rs` | 57KB | SQL queries + GROUP BY | W4 |
| `hooks/tracker.rs` | 55KB | savings INSERT | W5 |
| `run/emit_pipeline.rs` | 41KB | INSERT INTO pipeline_events | W4 |
| `hooks/knowledge.rs` | 40KB | INSERT INTO knowledge_patterns | W6 |
| `hooks/budget.rs` | 41KB | savings INSERT | W5 |
| `run/memory_ingest.rs` | 16KB | SQL ingest | W6 |
| `hooks/stop_observer.rs` | 14KB | SELECT FROM sessions | W6 |
| `run/otel/store.rs` | – | OTEL metrics store | fora (OTEL escopo próprio) |
| `run/resume_bootstrap.rs` | 43KB | lê pipeline state via SQL | W4 |
| `run/metrics_wave_status.rs` | 18KB | SQL GROUP BY status | W4 |

## Sites do dashboard que leem SQLite

Resultado do grep `json_extract|GROUP BY|rusqlite` em `apps/dashboard/`:

**148 ocorrências em 12 arquivos** (6 src + 6 tests):

| Arquivo | Ocorrências | Tipo |
|---|---|---|
| `src-tauri/src/db.rs` | 30 | Principal — 30 queries SQL |
| `src-tauri/src/spec_views.rs` | 32 | spec detail queries |
| `src-tauri/src/telemetry_agg.rs` | 41 | agregações de telemetria |
| `src-tauri/src/telemetry.rs` | 11 | telemetry reads |
| `src-tauri/src/economy.rs` | 3 | economy reads |
| `src-tauri/src/amend_queries.rs` | 1 | amend window |
| `src-tauri/tests/db_test.rs` | 10 | test SQL |
| `src-tauri/tests/telemetry_aggregations_test.rs` | 10 | test SQL |
| `src-tauri/tests/specs_phase_from_events_test.rs` | 4 | test SQL |
| `src-tauri/tests/top_files_today_test.rs` | 3 | test SQL |
| `src-tauri/tests/pipelines_from_db_test.rs` | 2 | test SQL |
| `src-tauri/tests/telemetry_test.rs` | 1 | test SQL |

**Total dashboard:** 148 ocorrências em 6 src files (REWRITE) + 6 test files (REWRITE) + 1 test que não tem SQL (`mustard_cli_test.rs`, preservar).

## Sites em packages/ e apps/rt/ que leem SQLite

Resultado grep `json_extract|GROUP BY|rusqlite`:
- **packages/**: 86 ocorrências em 18 arquivos (core store + telemetry + tests — todos são o código sendo DELETADO ou REWRITTEN)
- **apps/rt/src/**: 118 ocorrências em 24 arquivos (emitters + readers listados acima)

## Abordagem de staging

A spec elimina SQLite em camadas de baixo para cima:

1. **W1** cria o artefato de destino (`summary::writer`) antes de qualquer deleção — nenhum consumidor quebra.
2. **W2** deleta o store layer do core, forçando erro de compilação em todos os consumers imediatos — todos os erros são resolvidos dentro da própria wave via `reader/fs.rs` + stubs temporários nos hooks.
3. **W3** deleta telemetry com o mesmo padrão: stubs `NotAvailable` mantêm o código compilando; W5 substitui os stubs por NDJSON real.
4. **W4 + W5** (paralelas) migram emitters — nenhuma deleção de dados; só mudança de sink. Após W4+W5 a compilation chain é verde sem nenhum stub.
5. **W6** migra knowledge/memory — os únicos dados que tinham valor a longo prazo (decisões, padrões). Saem de SQL para markdown atômico — mais legível, versionável, grep-friendly.
6. **W7** migra o dashboard — depende de W4+W5+W6 estarem verdes porque os Tauri commands vão chamar o novo filesystem reader.
7. **W8** é cleanup + smoke: deleta os DB físicos e valida os 10 ACs.

Invariante de segurança: **ZERO deleções de arquivo ou remoção de dependência até W2**. W1 só cria código novo. Isso garante que o executor possa abortar antes de W2 sem nenhum efeito colateral.

## Paralelização possível

- W4 (emitters rt) e W5 (economy layer) podem ser executadas em agentes separados após W3 estar verde.
- Toda paralelização dentro de uma wave é livre — as waves são as barreiras de sincronização.

## Critérios de aceitação por wave

### W1 — Summary schema
- AC-W1.1: `packages/core/src/summary/mod.rs` existe — `node -e "process.exit(require('fs').existsSync('packages/core/src/summary/mod.rs')?0:1)"`
- AC-W1.2: `cargo build -p mustard-core` verde — `cargo build -p mustard-core`
- AC-W1.3: `SpecSummary` serializa para JSON com campo `version` numérico — teste unitário em `packages/core/src/summary/mod.rs`

### W2 — Eliminar store
- AC-W2.1: `packages/core/src/store/` não existe — `node -e "process.exit(require('fs').existsSync('packages/core/src/store')?1:0)"`
- AC-W2.2: `packages/core/Cargo.toml` sem `rusqlite` — `node -e "const s=require('fs').readFileSync('packages/core/Cargo.toml','utf8');process.exit(s.includes('rusqlite')?1:0)"`
- AC-W2.3: `cargo build -p mustard-core` verde — `cargo build -p mustard-core`
- AC-W2.4: `cargo test -p mustard-core` verde — `cargo test -p mustard-core`

### W3 — Eliminar telemetry
- AC-W3.1: `packages/core/src/telemetry/` sem `store.rs`, `writer.rs`, `reader.rs`, `schema.sql` — `node -e "const f=require('fs');const gone=['store.rs','writer.rs','reader.rs','schema.sql'].filter(n=>f.existsSync('packages/core/src/telemetry/'+n));process.exit(gone.length?1:0)"`
- AC-W3.2: `cargo build -p mustard-core` verde — `cargo build -p mustard-core`
- AC-W3.3: `cargo build -p mustard-rt` verde (stubs aceitáveis) — `cargo build -p mustard-rt`

### W4 — Emitters rt
- AC-W4.1: `apps/rt/src/run/db_maintain.rs` não existe — `node -e "process.exit(require('fs').existsSync('apps/rt/src/run/db_maintain.rs')?1:0)"`
- AC-W4.2: `cargo build -p mustard-rt` verde — `cargo build -p mustard-rt`
- AC-W4.3: `mustard-rt run active-specs` lista specs sem SQLite — `cargo run -q -p mustard-rt -- run active-specs`
- AC-W4.4: `mustard-rt run pipeline-summary --self-test` produz JSON com `version` numérico — `bash -c 'cargo run -q -p mustard-rt -- run pipeline-summary --self-test | node -e "let s=\"\";process.stdin.on(\"data\",c=>s+=c).on(\"end\",()=>{const j=JSON.parse(s);process.exit(typeof j.version===\"number\"?0:1)})"'`

### W5 — Economy NDJSON
- AC-W5.1: `packages/core/src/economy/store.rs` sem `rusqlite` — `node -e "const s=require('fs').readFileSync('packages/core/src/economy/store.rs','utf8');process.exit(s.includes('rusqlite')?1:0)"`
- AC-W5.2: `cargo build` workspace verde — `cargo build`
- AC-W5.3: `cargo test -p mustard-core` verde — `cargo test -p mustard-core`

### W6 — Knowledge/Memory markdown
- AC-W6.1: `.claude/knowledge/` e `.claude/memory/` existem — `node -e "const f=require('fs');process.exit(f.existsSync('.claude/knowledge')&&f.existsSync('.claude/memory')?0:1)"`
- AC-W6.2: `apps/rt/src/run/memory.rs` sem `rusqlite` — `node -e "const s=require('fs').readFileSync('apps/rt/src/run/memory.rs','utf8');process.exit(s.includes('rusqlite')?1:0)"`
- AC-W6.3: `cargo build -p mustard-rt` verde — `cargo build -p mustard-rt`
- AC-W6.4: `cargo test -p mustard-rt` verde — `cargo test -p mustard-rt`

### W7 — Dashboard migration
- AC-W7.1: `apps/dashboard/src-tauri/Cargo.toml` sem `rusqlite` — `node -e "const s=require('fs').readFileSync('apps/dashboard/src-tauri/Cargo.toml','utf8');process.exit(s.includes('rusqlite')?1:0)"`
- AC-W7.2: `pnpm --filter mustard-dashboard build` verde — `pnpm --filter mustard-dashboard build`
- AC-W7.3: Dashboard tests passam — `cargo test --manifest-path apps/dashboard/src-tauri/Cargo.toml`

### W8 — Cleanup + validação
- AC-W8.1 (= AC-2 da spec): `packages/core/src/store/` não existe — `node -e "process.exit(require('fs').existsSync('packages/core/src/store')?1:0)"`
- AC-W8.2 (= AC-3 da spec): `packages/core/src/telemetry/` não existe — `node -e "process.exit(require('fs').existsSync('packages/core/src/telemetry')?1:0)"`
- AC-W8.3 (= AC-4 da spec): zero `rusqlite` em Cargo.tomls dos 4 crates — `bash -c 'count=$(grep -rl "^rusqlite" packages/core/Cargo.toml apps/rt/Cargo.toml apps/cli/Cargo.toml apps/dashboard/src-tauri/Cargo.toml 2>/dev/null | wc -l); test "$count" = "0"'`
- AC-W8.4 (= AC-5 da spec): `cargo build` workspace verde — `cargo build`
- AC-W8.5 (= AC-6 da spec): `cargo test` workspace verde — `cargo test --workspace --no-fail-fast`
- AC-W8.6 (= AC-8 da spec): dashboard builda — `pnpm --filter mustard-dashboard build`
- AC-W8.7 (= AC-9 da spec): `active-specs` sem SQLite — `cargo run -q -p mustard-rt -- run active-specs`
- AC-W8.8 (= AC-10 da spec): `.claude/knowledge/` e `.claude/memory/` existem — `node -e "process.exit(require('fs').existsSync('.claude/knowledge')&&require('fs').existsSync('.claude/memory')?0:1)"`

## Riscos eliminados por design

| Risco | Eliminação |
|---|---|
| Deleção de código antes de substituto pronto | W1 cria `summary::writer` antes de qualquer DELETE; stubs em W3 mantêm compile green |
| Perda de knowledge/memory ao deletar DB | W6 migra para markdown antes de W8 deletar arquivos físicos |
| Dashboard quebrado por Tauri commands removidos | W7 reescreve tudo antes de W8; deps sequenciais garantidas |
| `rusqlite` "bundled" criando `.db` implicit | Eliminado: sem `rusqlite` nos Cargo.tomls, nenhum link estático é feito |
| Dados históricos perdidos (telemetry) | Aceito: dev phase, [[feedback_no_migration_dev_phase]] |
| Tests quebrando antes de rewrite | Tests SQLite isolados em arquivos próprios (DELETE ou REWRITE dentro da wave que os invalida) |

## Não-objetivos (ondas)

- Migrar dados dos bancos existentes — corte limpo, dev phase
- Manter `db_maintain` como subcommand de compatibilidade — DELETE
- Criar novo banco SQLite para qualquer propósito — zero SQLite após W8
- Tocar `apps/cli/templates/` (exceto `.gitignore`) — scan engine e spec template fora de escopo
- Wikilinks store (`store/wikilinks.rs`) — DELETE com o resto do store; wikilinks ficam no NDJSON per-spec

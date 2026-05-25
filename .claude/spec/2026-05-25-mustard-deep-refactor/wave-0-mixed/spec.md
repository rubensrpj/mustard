# W0 — Residual da W5 da mega-spec (per-spec-event-log + db redesign + dashboard)
### Stage: Execute
### Outcome: Active
### Flags: 
### Checkpoint: 2026-05-25T18:42:00Z
### Parent: 2026-05-25-mustard-deep-refactor

## Contexto

A `2026-05-24-mustard-unification` (encerrada nesta sessão) tinha W5 em execução com 8 tarefas. T5.1 (EventSink NDJSON + blob spill) e parte de T5.7 (purge `src-tauri/`) foram entregues por commits recentes. O restante migra para esta wave.

## Tarefas pendentes (origem em T5.X da mega-spec)

- [x] **T0.1 (era T5.2)** — Core reader NDJSON. `packages/core/src/projection/timeline.rs` lê NDJSON. `model/view/timeline.rs` com shape novo (`input`, `output`, `tokens_in`, `tokens_out`, `duration_ms`, `parent_id`). `mustard-rt run rebuild-specs` consome NDJSON.
- [x] **T0.2 (era T5.3)** — Dashboard timeline claude-devtools-style. Rewrite `SpecTimelineTab` + `PipelineTimeline` — lista flat de tool calls; expand revela Input/Output renderizados por tool (Bash/Read/Edit/Glob/Grep/Task recursivo via `parent_id`). Live tail via `notify-rs` no `src-tauri`. Re-render `<16ms` em wave com 500+ eventos.
- [x] **T0.3 (era T5.4)** — Sessions table + sidebar. Tabela `sessions` em `mustard.db` + diretório `.claude/.session/{slug}/events/`. Rota `Sessions.tsx` com sidebar.
- [x] **T0.4 (era T5.5)** — `mustard-rt run spec-clear`. Flags `--dry-run` (default), `--apply`, `--all`, `--name`, `--age-days` (default 15). Algoritmo: glob spec → parse meta.json → filtra Close+Done → mtime mais recente em events/ → cutoff.
- [x] **T0.5 (era T5.6)** — Schema `mustard.db` reescrito do zero. Drop `events`/`events_fts`/`knowledge` legacy/`metrics_projection`. CREATE direto de `pipeline_events`, `sessions`, `knowledge_patterns`, `memory_decisions`, `memory_lessons` (+FTS5), DDL de `agent_memory`+`memory_feedback` (lógica em W7). Índices auditados via `EXPLAIN QUERY PLAN`. `VACUUM` final + `PRAGMA optimize` no open.
- [x] **T0.6 (era T5.7 residual)** — Remover componente de grafo interno do dashboard. Remover dep força-grafo (`react-force-graph`, `d3-force`, `vis-network`) de `apps/dashboard/package.json`. Wikilinks `[[X]]` via `obsidian://open?vault=…&file=…` ou `shell.open`. Lista virtualizada (`react-virtuoso` ou `@tanstack/react-virtual`). Página `/specs` `<200ms` para 100 specs.
- [ ] **T0.7 (era T5.8)** — Economy events from event-log. EventSink emite `pipeline.economy.event.written { duration_ns, bytes_written, spilled_to_blob }`. Visível em `/economia` (W11). **Follow-up:** constante de event-kind precisa ser adicionada em core/rt (não no dashboard); endereçar em W5 (rt-new-subcommands) ou W11 (economy-wiring).

## Critérios de Aceitação

- [ ] **AC-W0.1** — Tabela `events` dropada do schema. Command: `rtk node -e "const t=require('fs').readFileSync('packages/core/src/store/sqlite_schema.sql','utf8');if(/CREATE TABLE events\\b/.test(t))process.exit(1)"`
- [ ] **AC-W0.2** — `pipeline_events` e `sessions` existem. Command: `rtk node -e "const t=require('fs').readFileSync('packages/core/src/store/sqlite_schema.sql','utf8');for(const k of ['CREATE TABLE pipeline_events','CREATE TABLE sessions']){if(!t.includes(k))process.exit(1)}"`
- [ ] **AC-W0.3** — `package.json` dashboard sem força-grafo. Command: `rtk node -e "const j=JSON.parse(require('fs').readFileSync('apps/dashboard/package.json','utf8'));for(const k of ['react-force-graph','react-force-graph-2d','d3-force','vis-network']){if(j.dependencies?.[k]||j.devDependencies?.[k])process.exit(1)}"`
- [ ] **AC-W0.4** — `mustard-rt run spec-clear --help` lista flags. Command: `rtk mustard-rt run spec-clear --help`
- [ ] **AC-W0.5** — Wikilinks no dashboard usam `obsidian://`. Command: validado por inspeção.

## Limites

`packages/core/src/store/*`, `packages/core/src/projection/timeline.rs`, `packages/core/src/model/view/timeline.rs`, `apps/rt/src/run/spec_clear.rs` (novo), `apps/rt/src/run/rebuild_specs.rs`, `apps/dashboard/src/components/{SpecTimelineTab,PipelineTimeline,Sessions}.tsx`, `apps/dashboard/src/pages/Specs.tsx`, `apps/dashboard/src-tauri/src/main.rs`, `apps/dashboard/package.json`.

OUT: tudo fora. NÃO mexer em event_writer_ndjson.rs (T5.1 entregue).

## Role

mixed (rt + dashboard + core)

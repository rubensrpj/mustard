# W11 — Telemetry perf + economy wiring + dashboard /economia

## Contexto

`telemetry-separation` (movida para backup) tinha review+qa pendentes. Esta wave finaliza + audita queries hot do dashboard + adiciona tabelas `economy_baselines`/`economy_savings` + wire ao dashboard. Mede o quanto cada wave economizou em tokens.

## Tarefas

- [x] **T11.1** — Audit de queries hot do dashboard: `EXPLAIN QUERY PLAN` em cada query do `apps/dashboard/src-tauri/src/`. Para queries full-scan, criar índice em `packages/core/src/telemetry/schema.sql`.
- [x] **T11.2** — Estender `mustard-rt run db-maintain` com flags `--telemetry-only` (não toca mustard.db) e `--prune-older-than {N}d`.
- [x] **T11.3** — Tabelas `economy_baselines (operation, baseline_tokens, captured_at)` e `economy_savings (wave_id, operation, savings_tokens, measured_at)` em `packages/core/src/telemetry/schema.sql`.
- [x] **T11.4** — Wire dos subcomandos `economy capture-baseline/reconcile/report` (W5.T5.15) ao dashboard via Tauri command `economy_summary` em `apps/dashboard/src-tauri/src/economy.rs`.
- [x] **T11.5** — Página `/economia` (`apps/dashboard/src/pages/Economia.tsx`) ganha aba "Deep Refactor Savings": card total acumulado + tabela per-wave (W0-W12 com tokens economizados real, não estimativa) + sparkline.

## Critérios de Aceitação

- [x] **AC-W11.1** — Tabelas `economy_baselines` e `economy_savings` existem. Command: `rtk node -e "const t=require('fs').readFileSync('packages/core/src/telemetry/schema.sql','utf8');for(const k of ['CREATE TABLE economy_baselines','CREATE TABLE economy_savings']){if(!t.includes(k))process.exit(1)}"`
- [x] **AC-W11.2** — `db-maintain --telemetry-only --help` lista a flag. Command: `rtk mustard-rt run db-maintain --help`
- [x] **AC-W11.3** — `/economia` mostra aba "Deep Refactor Savings". Command: `rtk node -e "const t=require('fs').readFileSync('apps/dashboard/src/pages/Economia.tsx','utf8');if(!/Deep Refactor|Unification Savings/.test(t))process.exit(1)"`
- [x] **AC-W11.4** — `economy_savings` populado com ≥1 row por wave W0-W11 ao final desta wave. Command: query SQLite.

## Limites

`packages/core/src/telemetry/schema.sql`, `apps/rt/src/run/db_maintain.rs`, `apps/dashboard/src-tauri/src/economy.rs`, `apps/dashboard/src/pages/Economia.tsx`.

OUT: schema `mustard.db` (W0.T0.5 fechou).

## Role

mixed (core schema + rt + dashboard)

# W4 — Arquivamento das 136 specs históricas
### Stage: Plan
### Outcome: Active
### Flags: 

## Contexto

Backup massivo executado nesta sessão: 136 specs movidas para `~/.mustard-backups/2026-05-25-specs-archive/` com `MANIFEST.json`. Esta wave reflete o outcome final no `telemetry.db` via `pipeline.status` events.

## Tarefas

- [x] **T4.1** — Adicionar variante `Absorbed` ao enum `Outcome` em `packages/core/src/meta.rs` + atualizar `serde` + valores aceitos em `apps/rt/src/run/spec_sections.rs`.
- [x] **T4.2** — Ler MANIFEST.json do backup. Para cada spec catalogada, emitir `mustard-rt run emit-pipeline --kind pipeline.status --spec {name} --payload '{"value":"<outcome>","reason":"archived in deep-refactor consolidation"}'`. Mapeamento:
  - `2026-05-24-mustard-unification` → `Completed` (W0-W4 entregues; resto transferido)
  - `2026-05-21-mustard-v1-installer-and-update`, `2026-05-20-dashboard-prd-ai-lapidator` → `Cancelled` (deferred)
  - Specs com sufixo `-SUPERSEDED` → `Superseded`
  - 3 absorvidas (`config-idioma-tom`, `meta-sidecar`, `per-spec-event-log`) → `Absorbed` (em mega-spec)
  - 3 TFs do dashboard-design-system (`page-primitives`, `ds-tokens-remap`, `eslint-baseline`) → `Completed` (entregues nesta sessão)
  - Demais (~126) → `Completed`
- [x] **T4.3** — Update no dashboard `apps/dashboard/src/pages/Specs.tsx` (ou similar) para renderizar badges de outcome `Absorbed` (cinza claro), `Cancelled` (vermelho discreto), `Superseded` (laranja).

## Critérios de Aceitação

- [x] **AC-W4.1** — Enum `Outcome` tem variante `Absorbed`. Command: `rtk node -e "const t=require('fs').readFileSync('packages/core/src/meta.rs','utf8');if(!/Absorbed/.test(t))process.exit(1)"`
- [x] **AC-W4.2** — `telemetry.db` tem `pipeline.status` event para 136 specs. Command: query SQLite.
- [x] **AC-W4.3** — `mustard-rt run active-specs --format json` retorna apenas `2026-05-25-mustard-deep-refactor`. Command: já no AC-G3.
- [x] **AC-W4.4** — Dashboard renderiza badges novos. Validado por inspeção.

## Limites

`packages/core/src/meta.rs`, `apps/rt/src/run/spec_sections.rs`, `apps/dashboard/src/pages/Specs.tsx` (ou similar component).

OUT: NÃO mexer no conteúdo dos `spec.md` no backup (são read-only históricos).

## Role

rt + dashboard (badge)

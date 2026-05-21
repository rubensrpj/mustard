# Wave 7 — Economia.tsx repaginada com scope picker (incl. Comparar Projetos)

### Parent: [[2026-05-20-economia-moat-unification]]
### Status: queued
### Phase: PLAN
### Scope: full (wave)
### Checkpoint: 2026-05-20T23:59:00Z
### Lang: pt

## PRD

Hoje `Economia.tsx` mistura adapters com leitura de banco e nunca mostra dado vivo. Esta wave reescreve a página usando único `useEconomySummary(scope)` que invoca `dashboard_economy_summary(scope)` que delega para `core::economy::reader::economy_summary(scope)` da W4. Scope picker no topo (Projeto / Spec / Wave / Comparar Projetos), todos funcionais — Comparar Projetos usa `MultiProjectReader` da W1. Cards consumindo dados reais: custo Anthropic oficial (via OTEL+JSONL), economia RTK real, prevention breakdown (cada hook que economizou tokens, com magnitude), distribuição por modelo (Sonnet/Opus/Haiku usage), contexto per-agente (composição + ratio cache hit), top specs/agentes mais caros. Visual baseado em primitivas DS da W5.

## Acceptance Criteria

- [ ] AC-1: Build passa — Command: `pnpm --filter mustard-dashboard build`
- [ ] AC-2: Type-check passa — Command: `pnpm --filter mustard-dashboard exec tsc --noEmit`
- [ ] AC-3: Tauri command `dashboard_economy_summary` registrado — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src-tauri/src/lib.rs','utf8');if(!t.includes('dashboard_economy_summary'))throw new Error('command not registered')"`
- [ ] AC-4: Hook `useEconomySummary` aceita scope — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src/hooks/useEconomySummary.ts','utf8');if(!t.includes('scope'))throw new Error('hook missing scope param')"`
- [ ] AC-5: Página tem scope picker com 4 opções — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src/pages/Economia.tsx','utf8');['Projeto','Spec','Wave','Comparar'].forEach(s=>{if(!t.includes(s))throw new Error('missing scope label '+s)})"`
- [ ] AC-6: Página NÃO chama invoke() direto — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src/pages/Economia.tsx','utf8');if(/from\\s+['\"]@tauri-apps\\/api/.test(t)||t.includes('invoke('))throw new Error('direct invoke in page')"`

## Plano

Backend: `dashboard_economy_summary(projectPath, scope: EconomyScope) -> EconomySummary` em `telemetry.rs` — thin wrapper que chama `core::economy::reader::economy_summary(scope)`. Para Comparar Projetos, recebe `Vec<projectPath>` derivado do project registry (já existe em `mustard.json`/dashboard store). Frontend: `useEconomySummary` em `hooks/`, `<Economia>` em `pages/Economia.tsx` com layout:

```
PageHeader (Economia)
ScopeBar [Projeto] [Spec ▼] [Wave ▼] [Comparar projetos]
─── Cards ───
[Custo Anthropic real]  [Economia RTK real]  [Cache hit ratio]
─── Por agente (top 10) ─── (tabela)
─── Distribuição por modelo ─── (chart)
─── Prevention breakdown ─── (lista por SavingsSource)
─── Top specs por custo ─── (no scope=Project ou AllProjects)
```

Cards usam `<MetricsPill>`, `<BaseRow>`, badges semânticos. Comparar Projetos mostra ranking + delta entre projetos.

## Dependências

- [[wave-4-attribution]]: reader exposes economy_summary com scope completo.
- [[wave-5-ds-foundation]]: primitivas DS para layout.

## Network

- Parent: [[2026-05-20-economia-moat-unification]]
- Depende de: [[wave-4-attribution]], [[wave-5-ds-foundation]]
- Paralela a: [[wave-6-trace-viewer]]
- Desbloqueia: QA (Wave 10) → CLOSE
- Grava memória: `{scope_picker_default: "Project", multi_project_strategy: "...", cards_implemented: [...]}`

## Limites

Em escopo: `apps/dashboard/src-tauri/src/telemetry.rs` (extend com novo command), `apps/dashboard/src-tauri/src/lib.rs` (registrar), `apps/dashboard/src/pages/Economia.tsx` (rewrite), `apps/dashboard/src/hooks/useEconomySummary.ts` (new), `apps/dashboard/src/lib/dashboard.ts` (wrapper invoke novo), `apps/dashboard/src/components/economy/**` (sub-componentes da página, se necessário).

Fora de escopo: trace viewer (W6), DS primitivas (W5), backend além do telemetry.rs, qualquer outra página.

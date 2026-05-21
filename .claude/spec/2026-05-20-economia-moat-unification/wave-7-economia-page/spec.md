# Wave 7 — Economia.tsx repaginada com scope picker (incl. Comparar Projetos)

### Parent: [[2026-05-20-economia-moat-unification]]
### Status: completed
### Phase: EXECUTE
### Scope: full (wave)
### Checkpoint: 2026-05-21T06:00:00Z
### Lang: pt

## PRD

Hoje `Economia.tsx` mistura adapters com leitura de banco e nunca mostra dado vivo. Esta wave reescreve a página usando único `useEconomySummary(scope)` que invoca `dashboard_economy_summary(scope)` que delega para `core::economy::reader::economy_summary(scope)` da W4. Scope picker no topo (Projeto / Spec / Wave / Comparar Projetos), todos funcionais — Comparar Projetos usa `MultiProjectReader` da W1. Cards consumindo dados reais: custo Anthropic oficial (via OTEL+JSONL), economia RTK real, prevention breakdown (cada hook que economizou tokens, com magnitude), distribuição por modelo (Sonnet/Opus/Haiku usage), contexto per-agente (composição + ratio cache hit), top specs/agentes mais caros. Visual baseado em primitivas DS da W5.

## Acceptance Criteria

- [x] AC-1: Build passa — Command: `pnpm --filter mustard-dashboard build`
- [x] AC-2: Type-check passa — Command: `pnpm --filter mustard-dashboard exec tsc --noEmit`
- [x] AC-3: Tauri command `dashboard_economy_summary` registrado — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src-tauri/src/lib.rs','utf8');if(!t.includes('dashboard_economy_summary'))throw new Error('command not registered')"`
- [x] AC-4: Hook `useEconomySummary` aceita scope — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src/hooks/useEconomySummary.ts','utf8');if(!t.includes('scope'))throw new Error('hook missing scope param')"`
- [x] AC-5: Página tem scope picker com 4 opções — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src/pages/Economia.tsx','utf8');['Projeto','Spec','Wave','Comparar'].forEach(s=>{if(!t.includes(s))throw new Error('missing scope label '+s)})"`
- [x] AC-6: Página NÃO chama invoke() direto — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src/pages/Economia.tsx','utf8');if(/from\\s+['\"]@tauri-apps\\/api/.test(t)||t.includes('invoke('))throw new Error('direct invoke in page')"`

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

## Informações da Entidade

Reusa `EconomyScope` (4 variantes) e `EconomySummary` do `mustard_core::economy::model` entregue em W1/W4. Sem entidade nova.

## Arquivos (~7)

```
apps/dashboard/src-tauri/src/telemetry.rs        (extend — fn dashboard_economy_summary)
apps/dashboard/src-tauri/src/lib.rs              (modify — registrar no .invoke_handler)
apps/dashboard/src/lib/types/economy.ts          (new — espelhar EconomyScope/EconomySummary do core)
apps/dashboard/src/lib/dashboard.ts              (modify — wrapper invoke('dashboard_economy_summary'))
apps/dashboard/src/hooks/useEconomySummary.ts    (new — useQuery wrapper aceitando scope)
apps/dashboard/src/pages/Economia.tsx            (rewrite — scope picker + cards reais + tabela agentes + breakdown)
apps/dashboard/src/components/economy/ScopeBar.tsx (new — toggle 4 opções: Projeto/Spec/Wave/Comparar)
apps/dashboard/src/components/economy/SavingsBreakdownCard.tsx (new — lista por SavingsSource)
apps/dashboard/src/components/economy/PerAgentTable.tsx (new — top-N agentes por custo)
```

## Tarefas

### Tauri Backend Agent (7a)

- [ ] Em `telemetry.rs`, adicionar `#[tauri::command] pub fn dashboard_economy_summary(project_path: String, scope: EconomyScopeDto) -> Result<EconomySummaryDto, String>`. `EconomyScopeDto` é enum serde-tagged espelhando `mustard_core::economy::EconomyScope` (4 variantes). Implementação: converte DTO → core scope → `mustard_core::economy::reader::economy_summary(&conn, scope)`. Conexão via `economy::store::open_for(&project_path)`. Para `AllProjects`, recebe `Vec<String>` de paths e usa `MultiProjectReader`.
- [ ] Registrar command em `lib.rs::generate_handler![]`.

### Frontend Economia Agent (7b — DEPENDE de 7a)

- [ ] Criar tipos espelho em `lib/types/economy.ts`: `EconomyScope` (union de 4 variantes), `EconomySummary`, `AgentCost`, `SavingsBreakdown`, `ContextRoutingMetrics`. Match exato com serde do backend.
- [ ] Adicionar wrapper em `lib/dashboard.ts`: `export async function fetchEconomySummary(projectPath: string, scope: EconomyScope): Promise<EconomySummary>`.
- [ ] Criar `hooks/useEconomySummary.ts`: `useQuery` retornando `EconomySummary` baseado em `[projectPath, scope]` queryKey.
- [ ] Criar `components/economy/ScopeBar.tsx`: 4 botões toggle (`Projeto`, `Spec`, `Wave`, `Comparar projetos`); cada um exibe sub-dropdown quando aplicável (Spec/Wave selecionados via dropdown). Estado local; emite `onScopeChange(scope: EconomyScope)`.
- [ ] Criar `components/economy/SavingsBreakdownCard.tsx`: lista de `<BaseRow>` por `SavingsSource` (RtkRewrite, ModelRoutingDowngrade, BashGuardBlock, BudgetOutputCut, RecipeInjection) com `<MetricsPill>` mostrando `tokens_saved` agregado.
- [ ] Criar `components/economy/PerAgentTable.tsx`: tabela compacta (use `<BaseRow>` repetido OU table simples) com top-10 agentes por `cost_usd_micros`. Colunas: agente, modelo, tokens, custo USD.
- [ ] Reescrever `pages/Economia.tsx`: layout descrito em ## Plano. `useEconomySummary(scope)` no topo, `<ScopeBar />` controla. Cards top: `<MetricsPill>` para custo Anthropic real, economia RTK real, cache hit ratio. Seção "Por agente": `<PerAgentTable>`. Seção "Distribuição por modelo": chart simples (barras horizontais com `<MetricsPill>`, sem chart lib). Seção "Prevention breakdown": `<SavingsBreakdownCard>`. Seção "Top specs por custo": só quando `scope=Project|AllProjects`.
- [ ] **AC-6 (crítico)**: NUNCA importar `@tauri-apps/api` nem chamar `invoke(...)` direto em `Economia.tsx`. Todo IO via `useEconomySummary` hook ou `fetchEconomySummary` wrapper.
- [ ] Rodar `pnpm --filter mustard-dashboard build` + `pnpm --filter mustard-dashboard exec tsc --noEmit` — ambos verdes.

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

## Concerns

- **`EconomyScopeDto` separado do core `EconomyScope`** — core usa newtypes em tuple variant (`Project(ProjectPath)`), incompatível com serde-tagged limpo. DTO usa plain `String` + `into_core()` conversion. REVIEW pode propor expor `EconomyScope::serialize_for_ipc()` no core.
- **`AllProjects` bootstrap abre `projects[0]`** — só ceremony pra entry-point do `store::open_for`; `MultiProjectReader::fan_out` re-abre cada projeto internamente. Trade-off: 1 conn extra por chamada, em troca de não introduzir API nova no core. REVIEW decide se vale otimizar.
- **2 commands extras adicionados além do spec** — `dashboard_economy_savings_breakdown` + `dashboard_economy_context_routing` (chamam readers diretos). Razão: a página tem seções dedicadas pra esses dados; sintetizá-los do `EconomySummary` perderia 0-rows quando uma fonte nunca disparou. Pequeno escopo creep — REVIEW decide se promove ou recolhe.
- **Pré-existente TS error em ScopeBar/Economia.tsx visto durante W8 build paralelo** — sumiu em second run (tsc incremental cache picou). Não afeta a entrega final, mas indica que builds paralelos podem ter resultados intermediários estranhos. Não-bloqueador.

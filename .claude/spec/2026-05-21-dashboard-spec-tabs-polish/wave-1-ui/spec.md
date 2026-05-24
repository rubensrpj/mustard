# Wave 1 — Bugs: tab vazia + ondas FS + trace expand

## Resumo

Três bugs reais surfaceados em runtime: (1) primeira visita à rota `/specs` mostra a aba "Lista" vazia mesmo com cards disponíveis; (5) durante uma pipeline EXECUTE rodando, a aba Ondas mostra "Nenhuma onda registrada" porque o `useSpecWaves` lê só do SQLite (que enche gradualmente conforme eventos chegam); (6) aba Trace exibe rows mas o click não expande (regressão / falso-positivo do agent W3). Wave 1 conserta os três e adiciona fallback robusto para ondas baseado em `wave-plan.md` + filesystem.

## Contexto

**Bug 1 — Lista vazia.** Inspeção de `Specs.tsx`: o render flow tem `if (!projectsRoot) return <EmptyState/>` e `if (!activeWorkspaceId) return <EmptyState/>` antes do render principal. Quando há projeto ativo mas a fan-out `useQueries({queries: (specRows ?? []).map(...)})` ainda não terminou o primeiro tick, `cards` é `[]` e `filteredSpecs.length === 0` → renderiza `<EmptyState title="Nenhuma spec encontrada"/>`. Combinado com `<SpecTabBar>` no topo já renderizado, parece "aba vazia". Fix: enquanto `listLoading || cardQueries.some(q => q.isLoading)`, manter o skeleton em vez de "Nenhuma spec encontrada".

**Bug 5 — Ondas somem durante EXECUTE.** `useSpecWaves(repoPath, spec)` invoca um Tauri command que projeta do SQLite (`SpecWave[]` baseado em eventos `wave.start`/`wave.complete`/`tool.use`). Pipeline acabou de iniciar → 0 ou poucos eventos → array vazio. O `wave-plan.md` da spec contém a estrutura canônica das ondas DECLARADAS (independente de eventos). Fix: novo Tauri command `dashboard_spec_waves_planned(repoPath, spec)` que lê `wave-plan.md` (ou faz `read_dir` de `.claude/spec/{spec}/wave-*-*`), devolve `SpecWavePlanned[]` com `{wave, role, declared_files_count}`. UI faz union: cada `wave` na resposta da projeção SQLite tem prioridade (tem timestamps reais); waves no plan mas ausentes na projeção entram com `status="queued"`. Resultado: durante EXECUTE da W2, ondas 1..6 aparecem (1 completed, 2 in_progress, 3..6 queued).

**Bug 6 — Trace não expande.** W3 reportou green mas o user vê não-expand em runtime. Investigar `<ExecutionTrace>` e `<ToolEventRow>`. Hipóteses: (a) o `<button>` envolvente é a colored row, mas o `e.stopPropagation` em algum child interno bloqueia; (b) o state `open` é por leaf e não persiste em re-render quando query refetcha; (c) o conteúdo expandido fica visualmente colapsado por CSS (overflow hidden). Fix: pôr `open` state num ref/Map por node id no nível do `<ExecutionTrace>` (não na recursão) e verificar `overflow-visible` no container.

## Arquivos

```
apps/dashboard/src/pages/Specs.tsx                              — fix gate de loading
apps/dashboard/src/hooks/useSpecWaves.ts                        — sem mudança aqui, novo hook ao lado
apps/dashboard/src/hooks/useSpecWavesPlanned.ts                 — NOVO: lê wave-plan.md via Tauri command
apps/dashboard/src/lib/dashboard.ts                             — wrapper dashboardSpecWavesPlanned + tipo
apps/dashboard/src-tauri/src/spec_views.rs                      — command dashboard_spec_waves_planned
apps/dashboard/src-tauri/src/lib.rs                             — registrar command
apps/dashboard/src/components/specs/SpecWavesTab.tsx            — consumir union dos dois hooks
apps/dashboard/src/components/specs/SpecDrillDown.tsx           — propagar repoPath/spec (se ainda não chegou)
apps/dashboard/src/components/trace/ExecutionTrace.tsx          — fix expand (state por node id)
apps/dashboard/src/components/trace/ToolEventRow.tsx            — confirmar overflow + click target
```

## Tarefas

- [ ] **Bug 1 — Specs.tsx loading gate.** Em `Specs.tsx`, condicione o `EmptyState` de "Nenhuma spec encontrada" a `!listLoading && !cardQueries.some(q => q.isLoading) && filteredSpecs.length === 0`. Caso contrário, mostre o skeleton de 3 linhas (já existente). Garante que primeira visita NUNCA mostra `EmptyState` enquanto a fan-out ainda carrega.
- [ ] **Bug 5 — Tauri command.** Em `apps/dashboard/src-tauri/src/spec_views.rs`:
  ```rust
  #[derive(Serialize)]
  pub struct SpecWavePlanned {
      pub wave: u32,
      pub role: Option<String>,
      pub declared_files_count: u32,
  }

  #[tauri::command]
  pub async fn dashboard_spec_waves_planned(
      repo_path: String,
      spec: String,
  ) -> Result<Vec<SpecWavePlanned>, String> { ... }
  ```
  Implementação:
  1. Resolve `<repo>/.claude/spec/{spec}/`. Lista subdirs com regex `^wave-(\d+)-(.+)$`. Parse `(wave: u32, role: String)`.
  2. Para cada wave-dir, leia `spec.md`, conte arquivos no bloco `## Arquivos` (use a heurística do `mustard-rt run wave-files` — pode spawnar OU portar parser).
  3. Ordena por `wave`. Devolve.
  - Registre em `lib.rs` no `invoke_handler!`.
- [ ] **Bug 5 — hook + wrapper.** Adicione `dashboardSpecWavesPlanned` em `dashboard.ts` + `useSpecWavesPlanned` em `hooks/useSpecWavesPlanned.ts` (TanStack Query, queryKey `['spec-waves-planned', repoPath, spec]`, staleTime 30_000, enabled `!!repoPath && !!spec`).
- [ ] **Bug 5 — SpecWavesTab union.** Em `SpecWavesTab.tsx`:
  - Aceitar dois inputs: `waves: SpecWave[]` (já tem) + `planned: SpecWavePlanned[]` (novo prop ou hook interno).
  - Computar union: para cada `planned` entry, procurar match em `waves` por número. Se acha, usa o `SpecWave` (tem timestamps); senão, sintetiza `{wave, role: planned.role, status: 'queued', started_at: null, completed_at: null, agent_type: null, files_changed: planned.declared_files_count}`.
  - Render igual ao atual; pill mostra "aguardando" pra queued.
- [ ] **Bug 5 — chamador (SpecDrillDown / SpecDetailDashboard).** Passar `planned` como prop OU consumir `useSpecWavesPlanned` dentro de `SpecWavesTab`. Preferir o segundo (encapsula).
- [ ] **Bug 6 — investigar expand.** Read `ExecutionTrace.tsx` e `ToolEventRow.tsx`. Identifique se o `open` é per-leaf-component state ou se há `useExpandedNodes(Map<string, boolean>)`. Se for per-leaf, mover pra Map no top-level (`<ExecutionTrace>`) com `expanded.has(node.id)`. Se for top-level já, confirme que `e.stopPropagation` em chevron NÃO bloqueia o handler do row. Adicione test manual: click no row → `open` flipa.
- [ ] **Bug 6 — overflow.** No CSS do container expandido (`ToolEventRow`'s `PayloadCard`), confirme `overflow-visible` ou `overflow-auto` sem `max-h-0`. Se há `max-h-X` transitando, garanta que a target height é suficiente (`max-h-72` é OK).
- [ ] Build: `pnpm --filter mustard-dashboard build` verde.
- [ ] Cargo check do src-tauri: `cargo check --manifest-path apps/dashboard/src-tauri/Cargo.toml` verde.

## Acceptance Criteria

- [ ] AC-W1-1: Dashboard tipa e builda — Command: `pnpm --filter mustard-dashboard build`
- [ ] AC-W1-2: Novo command `dashboard_spec_waves_planned` está registrado — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src-tauri/src/lib.rs','utf8');process.exit(/dashboard_spec_waves_planned/.test(s)?0:1)"`
- [ ] AC-W1-3: Specs.tsx tem gate de loading antes de renderizar EmptyState — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/pages/Specs.tsx','utf8');process.exit(/listLoading\s*\|\||cardQueries\.some/.test(s)?0:1)"`
- [ ] AC-W1-4: ExecutionTrace usa Map/Set para expansão (top-level state) — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/trace/ExecutionTrace.tsx','utf8');process.exit(/Map<|Set<|useState<.*Set|useState<.*Map/.test(s)?0:1)"`

## Limites

- `apps/dashboard/src/pages/Specs.tsx`
- `apps/dashboard/src/hooks/useSpecWavesPlanned.ts` (novo)
- `apps/dashboard/src/lib/dashboard.ts`
- `apps/dashboard/src-tauri/src/lib.rs`
- `apps/dashboard/src-tauri/src/spec_views.rs`
- `apps/dashboard/src/components/specs/SpecWavesTab.tsx`
- `apps/dashboard/src/components/specs/SpecDrillDown.tsx`
- `apps/dashboard/src/components/specs/SpecDetailDashboard.tsx`
- `apps/dashboard/src/components/trace/ExecutionTrace.tsx`
- `apps/dashboard/src/components/trace/ToolEventRow.tsx`

## Network

- Parent: [[2026-05-21-dashboard-spec-tabs-polish]]
- Bloqueia: [[wave-2-ui]] (W2 vai mexer em SpecWavesTab também — W1 precisa landar primeiro)

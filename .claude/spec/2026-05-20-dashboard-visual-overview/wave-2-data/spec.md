# Wave 2 — Data layer (invoke wrappers + React Query hooks)

## PRD

## Contexto

Após a Wave 1a entregar os 3 comandos Tauri, a UI precisa de uma camada tipada e cacheada para consumi-los. Essa camada (invoke wrappers em `lib/dashboard.ts` + 3 hooks `useWorkspace*` usando TanStack Query) é exigida pelos componentes da Wave 3. Sem ela, cada componente reimplementa `invoke()` solto, quebrando o pattern do dashboard que centraliza wrappers em `lib/dashboard.ts` (guardrail documentado em `apps/dashboard/CLAUDE.md`).

## Métrica de sucesso

3 interfaces TS (`TokenSummary`, `DayActivity`, `FeedEvent`) + 3 wrappers + 3 hooks tipados e funcionais, type-check passa.

## Não-Objetivos

- Não criar `QueryClient` novo — usar o já provido pelo `App`.
- Não persistir cache entre sessões.
- Não tocar nos hooks existentes (`useWorkspaceSummary`, `useTelemetryHeatmap`, etc.).

## Acceptance Criteria

- [x] AC-1: Type-check passa — Command: `pnpm --filter mustard-dashboard exec tsc --noEmit`
- [x] AC-2: 3 wrappers exportados — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src/lib/dashboard.ts','utf8');['dashboardTokenSummary','dashboardMonthActivity','dashboardEventsFeed'].forEach(f=>{if(!t.includes(f))throw new Error('missing wrapper '+f)})"`
- [x] AC-3: 3 hooks existem — Command: `node -e "['useWorkspaceTokenSummary','useWorkspaceMonthActivity','useWorkspaceEventsFeed'].forEach(h=>{const p='apps/dashboard/src/hooks/'+h+'.ts';if(!require('fs').existsSync(p))throw new Error('missing '+p)})"`

## Plano

## Arquivos (~4)

```
apps/dashboard/src/lib/dashboard.ts                       (modify — +3 wrappers + 3 interfaces)
apps/dashboard/src/hooks/useWorkspaceTokenSummary.ts      (new)
apps/dashboard/src/hooks/useWorkspaceMonthActivity.ts     (new)
apps/dashboard/src/hooks/useWorkspaceEventsFeed.ts        (new)
```

## Tarefas

### Frontend Data Layer Agent

- [x] Em `lib/dashboard.ts`, declarar interfaces espelhando os structs Rust da Wave 1a (`TokenSummary`, `TopPipeline`, `DayActivity`, `FeedEvent`)
- [x] Acrescentar wrappers: `dashboardTokenSummary(projectPath)`, `dashboardMonthActivity(projectPath, year, month)`, `dashboardEventsFeed(projectPath, limit)` — cada um `invoke<T>(...)` tipado
- [x] Hook `useWorkspaceTokenSummary(repoPath: string | null)`: `useQuery<TokenSummary>` keyed por `["workspace-token-summary", repoPath]`, `staleTime: 10_000`, `enabled: !!repoPath`
- [x] Hook `useWorkspaceMonthActivity(repoPath: string | null, year: number, month: number)`: keyed por `[..., year, month]`, `staleTime: 30_000`
- [x] Hook `useWorkspaceEventsFeed(repoPath: string | null, limit: number = 50)`: keyed por `[..., limit]`, `refetchInterval: 5_000`, `refetchOnWindowFocus: true`
- [x] `pnpm --filter mustard-dashboard exec tsc --noEmit`

## Dependências

- [[wave-1-backend]]: precisa dos 3 commands Tauri declarados.

## Network

- Parent: [[2026-05-20-dashboard-visual-overview]]
- Depende de: [[wave-1-backend]]
- Desbloqueia: [[wave-3-ui]]
- Recebe memória: [[wave-1-backend]] (lista de commands + structs).
- Grava memória: `{hooks_added: [...], wrappers: [...], notes: "..."}` para [[wave-3-ui]].

## Limites

Em escopo: `apps/dashboard/src/lib/dashboard.ts`, `apps/dashboard/src/hooks/useWorkspaceTokenSummary.ts`, `apps/dashboard/src/hooks/useWorkspaceMonthActivity.ts`, `apps/dashboard/src/hooks/useWorkspaceEventsFeed.ts`.

Fora de escopo: tudo mais.

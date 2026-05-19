---
name: mustard-dashboard-use-queries-hook-pattern
description: "Multi-project React Query fan-out hook convention used by src/hooks/use*.ts. Use when adding a new aggregation hook, fetching data across detected projects, or fanning out invoke() calls per project. Even if the user just says 'add a hook that lists X across projects'."
source: scan
---
<!-- mustard:generated at:2026-05-13 role:ui -->

## Convention

- Folder: `src/hooks/` (with helpers/types co-located or imported from `@/lib/dashboard`, `@/api/discovery`).
- File naming: `use<Resource>.ts`, function `export function use<Resource>(projects: Project[], ...args)`.
- Naming pattern: function-prefix cluster `use` (camelCase, prefix-before suffix).
- Declaration keyword: `export function`.
- Input: always `projects: Project[]` (from `@/api/discovery`) plus optional args (limit, query, etc.).
- Return: a `<Resource>Result` interface with at least `loading: boolean` and one flat data array.
- Per-project query key: `["<resource-slug>", project.path, ...extraArgs]` — `project.path` is the cache identity.
- Fetcher: `fetchX(project.path, ...)` imported from `@/lib/dashboard` (or `@/api/<area>`).
- Fan-out engine: `useQueries({ queries: projects.map((p) => ({ queryKey, queryFn, staleTime, ...refetch })) })`.
- Fan-in: iterate `projects.forEach((p, i) => queries[i]?.data ?? [])`, push into a flat list annotated with `projectId` / `projectName`.
- Loading: `queries.some((q) => q.isLoading)`.
- Sort: by timestamp desc using local `tsMs(s)` helper (`Date.parse` + `Number.isFinite` null-guard).
- Cache windows: `staleTime` 5_000–60_000 ms; live data adds `refetchInterval` 5_000–12_000 ms and `refetchOnWindowFocus: true`.
- Conditional fetch (e.g. search): set `enabled: <predicate>` on each per-project query, then guard the fan-in loop with the same predicate.
- Type exports next to the hook (e.g. `ActivityFeedRow`, `KnowledgeHit`) — keep `Row` / `Hit` suffix for fan-in shapes.
- Outlier: `useProject` uses `useState` + `useEffect` + `Promise.all` instead — keep new hooks on React Query; do not copy that style for portfolio fan-out.

## Real examples in this codebase

- `src/hooks/useAggregate.ts` — counters + active pipelines + timeline (two parallel `useQueries`).
- `src/hooks/useActivityFeed.ts` — recent events, parameterised by `limitPerProject`.
- `src/hooks/useKnowledgeSearch.ts` — conditional fetch via `enabled` on each per-project query.
- `src/hooks/useProject.ts` — outlier, single-project bundle, NOT the model to copy.

## References

- See `references/examples.md` for verbatim sample of `useActivityFeed.ts`.
- Related docs: `.claude/commands/patterns.md` section 1, `.claude/commands/recipes.md` "Add a new aggregation hook".
- Related convention: every `fetchX` wrapper lives in `src/lib/dashboard.ts` and calls Tauri `invoke()`.

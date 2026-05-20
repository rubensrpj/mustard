---
name: dashboard-use-queries-fanout
description: "Multi-project React Query fan-out pattern used in the Mustard dashboard. Use when adding a new aggregation hook, fetching per-project data across N projects, creating a use*.ts hook that fans out. Even if the user just says 'add a hook for X' or 'show data for all projects'."
source: scan
---
<!-- mustard:generated at:2026-05-19 role:ui -->

## Convention

- Input is always `projects: Project[]` (from `@/api/discovery` or `useProjects()`).
- Use `useQueries` (not `useQuery`) so each project gets an independent cache slot.
- Query key shape: `["<resource>", project.path, ...extraArgs]` — `project.path` is the cache identity.
- Fetcher: `fetchX(project.path, ...)` from `src/lib/dashboard.ts` (never `invoke()` directly).
- Fan-in: iterate `projects.forEach((p, i) => queries[i]?.data ?? [])` — always default to `[]`.
- Loading check: `queries.some((q) => q.isLoading)`.
- Sort output by timestamp desc using the local `tsMs(str)` helper — returns `number | null`.
- Return a typed `interface` (e.g. `ActivityFeedResult { events, types, loading }`).
- `staleTime`: 5s–30s for live data; 60s for slow-moving data (knowledge, specs).
- `refetchInterval`: only when polling is needed AND the FS watcher is not already triggering.

## Real examples in this codebase

- `src/hooks/useAggregate.ts` — multi-resource fan-out (specs + events), complex counters + sort.
- `src/hooks/useActivityFeed.ts` — single-resource fan-out with `refetchInterval 5s`.
- `src/hooks/useKnowledgeSearch.ts` — fan-out with `enabled` gate (query must be ≥2 chars), sort by `confidence`.
- `src/hooks/usePromptEconomy.ts` — single-project `useQuery` (NOT fan-out) with `refetchInterval 60s`.

## References

Full verbatim examples: `references/examples.md`

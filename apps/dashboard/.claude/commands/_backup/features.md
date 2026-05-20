<!-- mustard:generated at:2026-05-13 role:ui -->
# Features — mustard-dashboard

Routes mounted in `src/App.tsx` inside `HashRouter` + `AppShell`.

## Routes

| Path | Page | Purpose | Complexity |
|------|------|---------|------------|
| `/` | `Home` | Portfolio overview + active project live pipelines | medium |
| `/project/:id` | `ProjectDetail` | Single project — subprojects, recipes, skills, events | medium |
| `/project/:id/spec/:specName` | `SpecDetail` | Spec viewer w/ side panel | medium |
| `/knowledge` | `Knowledge` | Cross-project knowledge search | medium |
| `/commands` | `Commands` | Static catalog of Mustard commands | simple |
| `/prd` | `Prd` | PRD editor (markdown template) | medium |
| `/activity` | `Activity` | Activity feed across projects | medium |
| `/telemetry` | `Telemetry` | Token usage / events stats | medium |
| `/quality` | `Quality` | QA + close-gate results | medium |
| `/settings` | `Settings` | `projectsRoot` picker + preferences | simple |

## Global UI elements

| Component | Mount | Role |
|-----------|-------|------|
| `Toaster` | `App.tsx` (top-level) | sonner notifications, `position="bottom-right"` |
| `CommandPalette` | `App.tsx` (top-level) | cmdk Cmd-K palette |
| `AppShell` | wraps `<Routes>` | layout chrome (sidebar + topbar) |
| `Sidebar` / `Topbar` | inside `AppShell` | nav — **boundary for new routes** |
| `WorkspaceSwitcher` | inside `Topbar` | active project picker |

## Data flow

1. `App.tsx` reads `projectsRoot` from zustand store and runs `discoverProjects(projectsRoot)` (Tauri `invoke`) via React Query.
2. Detected `Project[]` drives `startWatcher(paths)` + `subscribeFsChange()` (Tauri events).
3. Pages call `use*` hooks (e.g. `useAggregate`, `useActivityFeed`, `useKnowledgeSearch`) which fan out via `useQueries` — one query per project.
4. Each query calls `fetchX(project.path)` in `src/lib/dashboard.ts`, which `invoke()`s a Rust command.

Ref: `src/App.tsx`, `src/hooks/useAggregate.ts`, `src/lib/dashboard.ts`.

## Multi-project aggregation hooks

These all live in `src/hooks/use*.ts` and follow the same shape (`useQueries` keyed by `project.path` → fan-in into a flat list sorted by timestamp).

| Hook | Sources | Returns |
|------|---------|---------|
| `useAggregate(projects)` | specs + recent events | counters, active pipelines, timeline |
| `useActivityFeed(projects, limit)` | recent events | events[], types[] |
| `useKnowledgeSearch(projects, query)` | search-knowledge | hits sorted by confidence |
| `useProject(project)` | subprojects/recipes/skills/events | per-project bundle (uses `useState`+`useEffect`, NOT React Query) |

Note: `useProject` is the one outlier — it uses raw `useState`/`useEffect`/`Promise.all` instead of `useQueries`. Treat new aggregation hooks as React Query (see `mustard-dashboard-use-queries-hook-pattern` skill).

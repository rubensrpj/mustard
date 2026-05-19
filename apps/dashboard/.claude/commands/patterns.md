<!-- mustard:generated at:2026-05-13 role:ui -->
# Patterns — mustard-dashboard

## 1. Multi-project React Query fan-out

The dashboard is portfolio-mode: most data needs to aggregate across N detected Mustard projects. The canonical pattern is `useQueries` keyed by `project.path`, then a fan-in pass that flattens + sorts client-side.

| Step | Detail |
|------|--------|
| Input | `projects: Project[]` from `discoverProjects()` |
| Per-query key | `["<resource>", project.path, ...extra]` — path is the cache identity |
| Fetcher | `fetchX(project.path, ...)` from `src/lib/dashboard.ts` |
| Cache | `staleTime` typical: 5-60s. Live data uses `refetchInterval` (5-12s) |
| Fan-in | `projects.forEach((p, i) => queries[i]?.data ?? [])` |
| Loading | `queries.some((q) => q.isLoading)` |
| Sort | by timestamp desc (`tsMs(...) ?? 0`) |

Ref: `src/hooks/useAggregate.ts`, `src/hooks/useActivityFeed.ts`, `src/hooks/useKnowledgeSearch.ts`.

## 2. Tauri `invoke()` wrappers

Every Rust command is wrapped in `src/lib/dashboard.ts` (or `src/api/*.ts`) with a typed `Promise<T>`. UI never calls `invoke()` directly inside components.

| Concern | Convention |
|---------|------------|
| Location | `src/lib/dashboard.ts` (dashboard surface) or `src/api/<area>.ts` (discovery, env) |
| Naming | `fetch<Resource>` (e.g. `fetchSpecs`, `fetchRecentEvents`) |
| Types | Co-located `interface` exports (`SpecRow`, `RecentEvent`, `KnowledgeRow`) |
| Errors | Propagate — let React Query / try/catch handle |

## 3. Zustand store

Single store at `src/lib/store.ts` selecting fields via slices: `useStore((s) => s.projectsRoot)`. Avoid pulling the whole store object — that breaks render isolation.

Keys typically read: `projectsRoot`, `activeWorkspaceId`, `setSelectedProjectId`.

## 4. Routing + boundary

`HashRouter` is mandatory (Tauri serves `index.html`; no server-side routing). Adding a route requires updating **three** places (see memory: `routing_implicit_boundary`):

1. `<Route path=... element=... />` in `src/App.tsx`.
2. `Sidebar` nav links in `src/components/layout/Sidebar.tsx`.
3. `Topbar` breadcrumb / LABELS map in `src/components/layout/Topbar.tsx`.

## 5. FS watcher lifecycle

`startWatcher(paths)` is called when projects load; subscription is established once on mount via `subscribeFsChange()`. `pathsKey = projects.map((p) => p.path).sort().join('|')` is the effect dep — avoids re-subscribing on array identity churn.

Ref: `src/App.tsx` lines 30-43.

## 6. Tauri current_dir gotcha (memory: `tauri_current_dir_gotcha`)

`pnpm tauri:dev` runs Rust with cwd = `src-tauri/`, NOT the repo root. Any path-relative Rust command must walk up until `.claude/` is found. Do not bake relative paths in TS — always pass `project.path` (absolute) into the Rust command.

## 7. react-markdown v10 (memory: `react_markdown_v10`)

v10 removed the `inline` prop on the `code` override. The dashboard's `Markdown.tsx` overrides `pre` separately and uses `className`/newline heuristics to detect blocks. Do not re-introduce `inline` checks.

## 8. useEffect/render race (memory: `useeffect_render_race`)

React Query's `data` is `undefined` on first render. Always guard with `data?.field` or `data == null` before accessing. Reset internal state in `useEffect` when re-fetching with a new key.

## 9. UI primitives via shadcn

`src/components/ui/*.tsx` follows shadcn idiom: `cva()` variants + `cn(...classes)` merger. New primitives should be added via `npx shadcn add <name>` so the schema matches.

Note (memory: `tauri_scaffold_gotchas`): `shadcn` dropped `--style`/`--base-color` flags — invoke without them.

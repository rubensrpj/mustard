<!-- mustard:generated at:2026-05-19 role:ui -->
# Patterns — mustard-dashboard

## 1. Multi-project React Query fan-out

The dashboard is portfolio-mode: most data aggregates across N detected projects. Canonical pattern: `useQueries` keyed by `project.path`, fan-in client-side.

| Step | Detail |
|------|--------|
| Input | `projects: Project[]` from `useProjects()` or parent |
| Per-query key | `["<resource>", project.path, ...extra]` — path is cache identity |
| Fetcher | `fetchX(project.path, ...)` from `src/lib/dashboard.ts` |
| Cache | `staleTime` 5s–60s; live data uses `refetchInterval` |
| Fan-in | `projects.forEach((p, i) => queries[i]?.data ?? [])` |
| Loading | `queries.some((q) => q.isLoading)` |
| Sort | by timestamp desc — `tsMs(str) ?? 0` helper |

Ref: `src/hooks/useAggregate.ts`, `src/hooks/useActivityFeed.ts`, `src/hooks/useKnowledgeSearch.ts`.

## 2. Tauri `invoke()` wrappers

Every Rust command is wrapped in `src/lib/dashboard.ts` or `src/api/*.ts`. UI never calls `invoke()` directly in components.

| Concern | Convention |
|---------|------------|
| Location | `src/lib/dashboard.ts` (main surface) or `src/api/<area>.ts` |
| Naming | `fetch<Resource>` (e.g. `fetchSpecs`, `fetchTelemetry`) |
| Types | Co-located `interface` exports (`SpecRow`, `TelemetrySummary`, etc.) |
| Mutations | Named as actions — `completeSpec`, `cancelSpec`, `reactivateSpec` |
| Errors | Propagate — let React Query or try/catch handle |

## 3. Zustand store

Single store at `src/lib/store.ts`. Fields: `projectsRoot`, `selectedProjectId`, `activeWorkspaceId`, `knowledgeQuery`, `language`.

Select fields via slices:

```ts
const projectsRoot = useStore((s) => s.projectsRoot);
```

Store is persisted via `zustand/middleware persist` under key `mustard-dashboard-store`.

## 4. Routing + three-file boundary

`HashRouter` is mandatory (Tauri serves `index.html`). Adding a route requires THREE file changes:

1. `<Route path=... element=... />` in `src/App.tsx`
2. Sidebar nav link in `src/components/layout/Sidebar.tsx`
3. LABELS map entry in `src/components/layout/Topbar.tsx`

Memory anchor: `routing_implicit_boundary`.

## 5. Shared page primitives barrel

All cross-page visual components live in `src/components/page/` and are barrel-exported via `index.ts`. Pages import from `@/components/page`:

```ts
import { KPICard, PageHeader, EmptyState, PhaseChip } from "@/components/page";
```

Adding a new visual primitive: create the file in `src/components/page/`, then add the export to `src/components/page/index.ts`.

## 6. Phase/event theme tokens

`phaseTheme(phase)` and `eventTheme(eventType)` in `src/lib/phaseTheme.ts` return `{ text, bg, border, stripe }` Tailwind classes. Pass the spread directly to `cn()`:

```ts
const t = phaseTheme(phase);
className={cn("px-2 py-0.5", t.text, t.bg, t.border)}
```

Do not hard-code phase colors in components — always go through `phaseTheme`.

## 7. FS watcher lifecycle

`startWatcher(paths)` called on project change; subscription established once via `subscribeFsChange()`. Effect dep = stable `pathsKey` (`paths.sort().join('|')`) to avoid re-subscribing on array identity churn.

Ref: `src/App.tsx` lines ~32–45.

## 8. Tauri cwd gotcha

`pnpm tauri:dev` runs Rust with cwd = `src-tauri/`, not repo root. Always pass absolute `project.path` into Rust commands. Rust uses `find_mustard_root()` for relative resolution. Memory anchor: `tauri_current_dir_gotcha`.

## 9. react-markdown v10

v10 removed the `inline` prop on `code`. `src/components/Markdown.tsx` overrides `pre` separately and uses `className`/newline heuristics. Do not re-introduce `inline` checks.

## 10. shadcn primitives

`src/components/ui/*.tsx` follow shadcn idiom: `cva()` variants + `cn()` merger. Add new primitives via:

```bash
pnpm dlx shadcn add <name>
```

Do NOT pass `--style` or `--base-color` (removed in current shadcn). Output lands in `src/components/ui/<name>.tsx`.

## 11. React Query null-guard

`data` is `undefined` on first render. Always guard:

```ts
const value = data?.field ?? fallback;
```

Reset internal state in `useEffect` when the query key changes to prevent stale renders.

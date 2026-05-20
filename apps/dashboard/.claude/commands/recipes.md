<!-- mustard:generated at:2026-05-19 role:ui -->
# Recipes — mustard-dashboard

Compact playbooks for the most common UI changes. Files touched listed in order.

## Add a new aggregation hook (per-project fan-out)

1. Add typed fetcher `fetchX(repoPath: string, ...): Promise<XRow[]>` in `src/lib/dashboard.ts`. Export the `interface XRow`.
2. Create `src/hooks/useX.ts` mirroring `useActivityFeed.ts`:
   - `useQueries({ queries: projects.map((p) => ({ queryKey: ["x", p.path, ...args], queryFn: () => fetchX(p.path, ...args), staleTime: <ms> })) })`
   - Fan-in: `projects.forEach((p, i) => queries[i]?.data ?? [])`
   - Sort by timestamp desc using local `tsMs()` helper.
3. Consume from page: `const { items, loading } = useX(projects, arg)`.

Ref: `src/hooks/useAggregate.ts`, `src/hooks/useActivityFeed.ts`.

## Add a new Tauri command wrapper

1. Implement the Rust command in `src-tauri/src/` and register in `tauri.conf.json` / `lib.rs`.
2. Add `export async function fetchY(repoPath: string): Promise<YRow[]> { return invoke("y_command", { repoPath }); }` to `src/lib/dashboard.ts`.
3. Export the row type as a sibling `interface YRow`.
4. Consume from a hook or a one-shot `useQuery`.

## Add a new route

1. Create `src/pages/Foo.tsx` (named export).
2. Register `<Route path="/foo" element={<Foo />} />` in `src/App.tsx`.
3. Add sidebar link in `src/components/layout/Sidebar.tsx`.
4. Add LABELS entry in `src/components/layout/Topbar.tsx`.

Memory anchor: `routing_implicit_boundary`.

## Add a new page-level KPI ribbon

1. Import `KPICard` from `@/components/page`.
2. Render in a `<div className="grid grid-cols-4 gap-3">` block.
3. Choose `accent` from the `KPIAccent` type (`emerald` good, `amber` caution, `rose` error, `indigo` primary, `violet` qa, `sky` info, `zinc` neutral).

```tsx
<KPICard label="Active Specs" value={counters.activeSpecs} accent="indigo" />
```

## Add a shadcn primitive

```bash
pnpm dlx shadcn add <name>
```

Do NOT pass `--style` or `--base-color` (deprecated). Output lands in `src/components/ui/<name>.tsx`. Import via `@/components/ui/<name>`.

## Add a zustand slice

1. Edit `src/lib/store.ts`. Add field + setter on the `Store` type and the initial value.
2. Consume via `useStore((s) => s.field)` — single-field slice per call site.

## Add a static catalog row

For the `Commands` or `Env` catalog pages:
- `src/data/commands-catalog.ts` — append entry.
- `src/data/env-catalog.ts` — append entry.
No backend wiring needed — these are static arrays.

## Add a new page primitive

1. Create `src/components/page/MyPrimitive.tsx`.
2. Add export to `src/components/page/index.ts`.
3. All pages can then import from `@/components/page`.

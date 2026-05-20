<!-- mustard:generated at:2026-05-13 role:ui -->
# Recipes — mustard-dashboard

Compact playbooks for the most common UI changes. Each lists files touched in order.

## Add a new aggregation hook (per-project fan-out)

1. Add typed fetcher `fetchX(projectPath, ...): Promise<XRow[]>` in `src/lib/dashboard.ts`.
2. Create `src/hooks/useX.ts` mirroring `useActivityFeed.ts`:
   - `useQueries({ queries: projects.map((p) => ({ queryKey: ["x", p.path, ...args], queryFn: () => fetchX(p.path, ...args), staleTime: <ms> })) })`.
   - Flatten results client-side with `projects.forEach((p, i) => queries[i]?.data ?? [])`.
   - Sort by timestamp desc using `tsMs()` helper (copy-paste — it is local convention).
3. Page consumes hook: `const { events, loading } = useX(projects, arg)`.

Ref: `src/hooks/useAggregate.ts`, `src/hooks/useActivityFeed.ts`.

## Add a new Tauri command wrapper

1. Implement the Rust command (`src-tauri/src/...`) and register in `tauri.conf.json` / `lib.rs`.
2. Add `export async function fetchY(path: string): Promise<YRow[]> { return invoke("y_command", { path }); }` to `src/lib/dashboard.ts`.
3. Export the row type as a sibling `interface`.
4. Consume from a hook or a one-shot `useQuery`.

## Add a new route

1. Create `src/pages/Foo.tsx` (export named function).
2. Register `<Route path="/foo" element={<Foo />} />` in `src/App.tsx`.
3. Add sidebar link in `src/components/layout/Sidebar.tsx`.
4. Add LABELS entry in `src/components/layout/Topbar.tsx`.

Memory anchor: `routing_implicit_boundary`.

## Add a shadcn primitive

```bash
pnpm dlx shadcn add <name>
```
Do NOT pass `--style` or `--base-color` (deprecated in current shadcn version).
Output lands in `src/components/ui/<name>.tsx`. Import via `@/components/ui/<name>`.

## Add a zustand slice

1. Edit `src/lib/store.ts`. Add field + setter on the `State` interface and initial value.
2. Consume via `useStore((s) => s.field)` — single-field slice per call.

## Add a static catalog row

For `Commands` / `Env` catalogs:
- `src/data/commands-catalog.ts` — append entry.
- `src/data/env-catalog.ts` — append entry.
No backend wiring needed.

<!-- mustard:generated at:2026-05-13 role:ui -->
# Guards — mustard-dashboard

DO/DON'T rules. Code examples live in `patterns.md`.

## Data fetching

- DO use `useQueries` (TanStack Query v5) for any per-project fan-out. Key by `project.path`.
- DO put `invoke()` calls in `src/lib/dashboard.ts` or `src/api/*.ts`, never inline in a component.
- DO type the return of every `fetchX` wrapper with an exported `interface`.
- DON'T call Tauri `invoke()` directly from a component or page.
- DON'T forget `staleTime` — without it React Query refetches on every focus.
- DON'T use `refetchInterval` >5s when the watcher already pushes updates; pick one mechanism.

## React rendering

- DO null-guard React Query `data` (`data?.field`, never `data!.field`) on first render.
- DO reset internal state in `useEffect` when the query key changes.
- DO select zustand fields via slices: `useStore((s) => s.field)`.
- DON'T destructure the entire zustand store — it re-renders on every change.
- DON'T re-introduce the `inline` prop on react-markdown v10 `code` overrides.

## Routing

- DO use `HashRouter` (Tauri-safe). Never `BrowserRouter`.
- DO update Sidebar links AND Topbar LABELS map when adding a route.
- DON'T add a route without registering it in the sidebar — it becomes orphaned.

## Tauri integration

- DO pass absolute paths (`project.path`) into Rust commands. Don't rely on cwd.
- DO use `find_mustard_root()` on the Rust side when resolving relative paths.
- DON'T assume `pnpm tauri:dev` cwd is the repo root — it is `src-tauri/`.

## UI components

- DO use `cn()` from `@/lib/utils` to merge classNames.
- DO add new primitives via `npx shadcn add <name>` without `--style`/`--base-color`.
- DO co-locate `cva()` variants with the primitive file.
- DON'T add a new dependency just for a one-off icon — `lucide-react` covers the set.

## Build / type

- DO run `pnpm build` (`tsc -b && vite build`) before declaring a change shippable.
- DO keep ESLint clean — `eslint .` runs across the whole repo.
- DON'T commit `any` or `as unknown as X` without a comment explaining why.

## Watcher / lifecycle

- DO subscribe to `subscribeFsChange()` exactly once at mount (top-level effect).
- DO use a stable `pathsKey` (`paths.sort().join('|')`) as effect dep when restarting the watcher.
- DON'T put `subscribeFsChange()` inside a per-route component — it leaks subscriptions.

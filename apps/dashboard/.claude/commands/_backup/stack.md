<!-- mustard:generated at:2026-05-13 role:ui -->
# Stack — mustard-dashboard

Tauri 2 desktop dashboard. React 19 + TypeScript 5.8 frontend, Rust backend (`src-tauri/`). Multi-project Mustard scaffold inspector.

## Runtime

| Layer | Library | Version | Role |
|-------|---------|---------|------|
| UI runtime | react / react-dom | ^19.1.0 | App rendering |
| Router | react-router | ^7 | HashRouter (Tauri-safe) |
| Server cache | @tanstack/react-query | ^5 | Fetch + cache + refetchInterval |
| Global store | zustand | ^5 | `projectsRoot`, `activeWorkspaceId` |
| Styling | tailwindcss / @tailwindcss/vite | ^4.3.0 | Utility CSS |
| Component primitives | radix-ui | ^1.4.3 | Headless primitives |
| Component meta | shadcn | ^4.7.0 | Component scaffolder (CLI) |
| Variants | class-variance-authority / clsx / tailwind-merge | latest | `cn()` + `cva()` |
| Icons | lucide-react | ^1.14.0 | Icon set |
| Markdown | react-markdown + remark-gfm | ^10 / ^4 | PRD + knowledge rendering |
| Cmd palette | cmdk | ^1.1.1 | Cmd-K UI |
| Toaster | sonner | ^2.0.7 | Notifications |
| i18n | i18next / react-i18next | ^26 / ^17 | EN/PT translation |
| Date | dayjs | ^1.11.20 | Relative time formatting |

## Tauri integration

| Plugin | Purpose |
|--------|---------|
| @tauri-apps/api ^2 | `invoke()` core bridge |
| @tauri-apps/plugin-dialog | Folder picker |
| @tauri-apps/plugin-log | Tauri log proxy |
| @tauri-apps/plugin-opener | Open external URL/path |
| @tauri-apps/plugin-store | Persisted KV (settings) |
| @tauri-apps/plugin-updater | Auto-update channel |
| @tauri-apps/plugin-window-state | Window geometry restore |

## Build / lint / test

| Task | Command | Notes |
|------|---------|-------|
| Dev (web only) | `pnpm dev` | Vite dev server, no Tauri shell |
| Dev (desktop) | `pnpm tauri:dev` | Launches Tauri shell; Rust cwd = `src-tauri/` |
| Build | `pnpm build` | `tsc -b && vite build` — type-check then bundle |
| Build (desktop) | `pnpm tauri:build` | Packaged installer |
| Lint | `pnpm lint` | `eslint .` |
| Test | `pnpm test` | Placeholder (`echo "no tests yet" && exit 0`) — no test runner yet |
| Type-check only | `pnpm tsc --noEmit` | Implicit via `tsc -b` in build |

## Path aliases

`@/*` → `src/*` (configured in `tsconfig` + `vite.config.ts`).

## Source layout

| Path | Contents |
|------|----------|
| `src/api/` | Tauri `invoke()` wrappers (`discovery.ts`, `env.ts`) returning typed promises |
| `src/lib/` | Pure modules — `dashboard.ts` (invoke wrappers), `store.ts` (zustand), `watcher.ts` (fs event subscription), `time.ts`, `format.ts`, `utils.ts` (`cn()`), `query-client.ts`, `prd-template.ts` |
| `src/hooks/` | `use*` React hooks — most wrap `useQueries` over `Project[]` |
| `src/components/` | Feature components (e.g. `LivePipelineCard`, `AggregateOverview`, `CommandPalette`, `KnowledgeCard`, `SpecsList`, `SpecSidePanel`, `Markdown`, `StatusDot`) |
| `src/components/ui/` | shadcn-style primitives (`button.tsx`, `card.tsx`, `dialog.tsx`, etc.) |
| `src/components/layout/` | `AppShell`, `Sidebar`, `Topbar`, `WorkspaceSwitcher` |
| `src/pages/` | Route components — `Home`, `ProjectDetail`, `SpecDetail`, `Commands`, `Knowledge`, `Activity`, `Settings`, `Telemetry`, `Prd`, `Quality` |
| `src/data/` | Static catalogs (`commands-catalog.ts`, `env-catalog.ts`) |
| `src-tauri/` | Rust backend (commands, watcher, etc.) |

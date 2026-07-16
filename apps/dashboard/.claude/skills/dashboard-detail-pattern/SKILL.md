---
name: dashboard-detail-pattern
description: Use when adding or refactoring a route-driven drill-in page (`XxxDetail`) under `src/pages/` that renders one entity's detail from a `react-router` URL param.
tags: [add, refactor]
appliesTo: [detail]
scope: [code-editing]
source: scan
metadata:
  generated_by: scan
  cluster:
    label: detail
---

# detail pattern

## Purpose

Detail pages are the drill-in views of the dashboard: one entity (a project, a session) rendered from a `react-router` URL param. Each page reads its `:id` via `useParams` (decoding with `decodeURIComponent` when the id can carry encoded characters), pulls global selection state from `useStore` in `@/lib/store`, and composes the shared page scaffold from `@/components/page` — everything wrapped in `PageSurface`, headed by an `EditorialBand` whose `eyebrow` is a breadcrumb `Link` back to the list. The page itself stays thin: the heavy body is delegated to feature components (`SpecsList`, `ExecutionTrace`, `LivePipelineCard`) under `src/features/`. Missing data is never an exception path — when the entity or `projectsRoot` is absent, the page early-returns `PageSurface > EmptyState`, matching the guard that Tauri commands are failure-tolerant. Live data comes from TanStack Query (`queryKey` array with the project path as a leaf, `enabled` guard) and is refreshed by watcher-driven invalidation, never by polling.

## Convention

- Folder: `apps/dashboard/src/pages/`
- Extension: `.tsx`
- Naming: `{Entity}Detail.tsx` exporting a named `export function {Entity}Detail()` (no default export)
- Files: 2 route pages define the mold (`ProjectDetail`, `SessionDetail`); 2 further declarations share the suffix but are NOT this shape — `features/specs/SpecDetailDashboard` (per-tab feature container) and `components/layout/SplitDetail` (layout primitive). A new routed detail follows the page shape.

## How to apply

To add a new detail page for entity `Foo`:

1. Create `apps/dashboard/src/pages/FooDetail.tsx` with `export function FooDetail()`.
2. Register the route in `src/App.tsx` next to the existing ones: `<Route path="/foo/:id" element={<FooDetail />} />`.
3. Read the param with `useParams<{ id: string }>()`; call hooks unconditionally (Rules of Hooks), then early-return `PageSurface > EmptyState` when `projectsRoot`/the entity is missing — with a `Link` pointing the user somewhere useful, as `ProjectDetail` does.
4. Head the page with `EditorialBand` (breadcrumb `eyebrow`, entity `title`, status `subtitle`); keep user-visible labels consistent with the sibling pages' existing pt-BR strings, while code and comments stay English.
5. Fetch via TanStack Query wrappers from `@/lib/dashboard` — never `invoke()` directly — with a stable `queryKey` array (`['foo-detail', project?.path, id]`-shaped) and `enabled: !!project`; register the key prefix in `lib/watcher.ts` for live refresh instead of polling.
6. Delegate the body to components under `src/features/` — the page composes; features fetch and render the detail.
7. Tab or wave selection that must survive navigation goes in the URL (`useSearchParams` with `{ replace: true }`), as `ProjectDetail` does for its `tab` param.

## Examples

- Ref: apps/dashboard/src/pages/ProjectDetail.tsx
- Ref: apps/dashboard/src/pages/SessionDetail.tsx

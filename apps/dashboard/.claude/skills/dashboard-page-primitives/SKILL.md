---
name: dashboard-page-primitives
description: "Shared page-level visual primitives barrel in src/components/page/. Use when adding a new dashboard page, adding KPI cards, phase chips, empty states, or data tables. Even if the user just says 'add a page for X', 'show a stat card', or 'display a phase badge'."
source: scan
---
<!-- mustard:generated at:2026-05-19 role:ui -->

## Convention

- All cross-page visual components live in `src/components/page/` and are barrel-exported from `src/components/page/index.ts`.
- Import path is always `@/components/page` — never import individual files directly.
- `KPICard` renders a stat card with a colored top-stripe accent; choose `KPIAccent` semantically: `emerald` good, `amber` caution, `rose` error, `indigo` primary, `violet` QA, `sky` info, `zinc` neutral.
- `PhaseChip` reads phase colors from `phaseTheme()` in `src/lib/phaseTheme.ts` — never hard-code Tailwind phase colors in components.
- `EventChip` reads event colors from `eventTheme()` — same file as `phaseTheme`.
- `EmptyState` has three variants: `info` (neutral), `warning` (amber), `error` (destructive). Accepts a `right` slot for action buttons.
- `DataCard` is a table/list wrapper — do NOT add `padded` unless the content is free-form (for tables, let table cells handle padding).
- Adding a new primitive: create file → add export to `index.ts`. Two steps, no exceptions.
- Component props: always export the `interface` alongside the component (same file).

## Real examples in this codebase

- `src/components/page/KPICard.tsx` — accent stripe + value + label + hint. 3 consumers: Home, Telemetry, Quality.
- `src/components/page/PhaseChip.tsx` — uses `phaseTheme()` tokens as Tailwind classes.
- `src/components/page/EmptyState.tsx` — info/warning/error variants with right-action slot.
- `src/components/page/DataCard.tsx` — table/list wrapper with `padded` prop.
- `src/components/page/index.ts` — the barrel; 9 primitives exported today.

## References

Full verbatim examples: `references/examples.md`

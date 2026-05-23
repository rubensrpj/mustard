# Notes: mustard-dashboard (ui)

> Project-specific notes for mustard-dashboard. Edit freely — this file is never overwritten by /scan.

## Mandatory Patterns

- **Folder-per-component** (Wave 4 of spec `2026-05-23-dashboard-design-system`):
  - Shared primitives live in `src/components/{page,layout,ui}/{Name}/index.tsx`.
  - Domain components live in `src/features/{feature}/{Name}/index.tsx` (8 features: specs, workspace, economy, knowledge, prd, telemetry, amend, trace).
  - Helpers used by 2+ components in a feature go to `features/{feature}/_shared/{helper}.ts` — the underscore prefix keeps them out of the barrel.
  - Tests stay at the feature root: `features/{feature}/__tests__/`.
  - The codemod `scripts/refactor-folder-per-component.mjs` is the single source of truth — it's idempotent. Add a new component anywhere it should live and re-run to refresh barrels.
  - Pages import either granular (`@/features/specs/SpecCard`) or aggregated (`@/features/specs`). Never `@/components/{specs,workspace,…}/…` — those paths no longer exist.

- **Inline-visual gate** (Wave 5/6 will turn this from non-blocking to blocking):
  - `scripts/check-pages-no-inline-visual.mjs` walks `src/pages/**/*.tsx` and fails on inline `style={{ color, background, … }}`, raw Tailwind palette tokens (e.g. `text-red-500`), hex colors, and non-whitelisted className tokens.
  - Pages should compose from `@/components/page` + `@/features/{name}` and stay structural-classes-only.

## Known Pitfalls

- `acorn-jsx` doesn't parse TypeScript syntax. The inline-visual checker uses `@typescript-eslint/typescript-estree` instead (transitively available via eslint).
- `git mv` preserves history for renames; the codemod uses it first and falls back to `fs.rename` only if git is unavailable or the file is untracked.

## Observations

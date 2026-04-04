---
name: templates-sync-detect
description: "Pattern for subproject discovery, role detection via scoring, and SHA-256
  incremental hashing in sync-detect.js. Use when modifying detection logic,
  adding a new role/stack, changing hash computation, adding new scoring signals,
  or the user says 'add stack detection', 'new role', 'detect framework',
  'fix detection', 'change scoring'."
---
<!-- mustard:generated at:2026-03-25T00:00:00.000Z role:general -->

# Sync-Detect Pattern

sync-detect.js discovers subprojects, detects their role via weighted scoring, and computes SHA-256 hashes for incremental scan.

## Pattern

### Discovery Order
1. `git submodule status` — prefer submodule paths
2. Fallback: scan root for dirs containing `CLAUDE.md`

### Role Scoring (weighted signals)

| Weight | Constant | Signal Type |
|--------|----------|-------------|
| 10 | `HIGH` | Config files: `.csproj` Sdk.Web, `next.config.*`, `drizzle.config.*`, `pubspec.yaml` |
| 5 | `MEDIUM` | Package deps: `react`, `express`, `drizzle-orm` in package.json |
| 3 | `LOW` | Directories: `Controllers/`, `app/`+`components/`, `migrations/` |

Roles: `api`, `ui`, `database`, `library`, `mobile`, `general` (fallback).
Role maps to agent: `api→backend`, `ui→frontend`, `database→database`, `library→backend`, `mobile→mobile`.

### Incremental Hashing
- Collect source files (`.cs`, `.ts`, `.tsx`, `.js`, `.jsx`, `.dart`)
- Exclude: `node_modules`, `.next`, `bin`, `obj`, `dist`, `migrations`, `_backup`, dotfiles
- Sort files → hash(path + content) per file → SHA-256 digest
- Module-level hashes for fine-grained incremental (backend: `Modules/v{N}/{Module}/`, frontend: `app/(dashboard)/*/`)

### Output
JSON to stdout: `{ subprojects, agents, detectedAgents, sourceHashes, moduleHashes }`
Writes `.claude/.detect-cache.json` (unless `--no-cache`).

## Example

```js
function detectRole(absPath) {
  const scores = { api: 0, ui: 0, database: 0, library: 0, mobile: 0 };
  if (isCsprojWeb(absPath)) scores.api += 10;
  if (fileExists(absPath, 'next.config.*')) scores.ui += 10;
  // ... more signals ...
  let maxScore = 0, role = 'general';
  for (const [r, s] of Object.entries(scores)) {
    if (s > maxScore) { maxScore = s; role = r; }
  }
  return { role, scores };
}
```
Ref: `scripts/sync-detect.js`

## References

For full code examples with variants:
> Read `references/examples.md`

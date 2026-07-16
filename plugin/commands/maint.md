---
name: maint
description: Use when the user runs /maint or asks about dependencies, validate, sync, or doctor (project hygiene, build/type-check, registry sync, installation health check).
source: manual
---
<!-- mustard:generated -->
# /maint - Maintenance Utilities

## Trigger

`/maint <action>`

| Action | Description |
|--------|-------------|
| `deps` | Install dependencies for all subprojects (or root if single repo) |
| `validate` | Build + type-check across subprojects |
| `sync` | `mustard-rt run scan` — refresh `grain.model.json` |
| `doctor` | Installation health check — wiring, drift, state + OTEL diagnostics |

## deps

```bash
rtk mustard-rt run maint-deps           # install deps in every detected subproject
rtk mustard-rt run maint-deps --dry-run # preview the resolved install commands only
```

Print stdout verbatim. The binary auto-discovers subprojects from `grain.model.json` and picks the install command per project kind (`pnpm install`, `cargo fetch`, `dotnet restore`, …) — never read the Agents table or `{subproject}/CLAUDE.md` by hand. Output is a JSON `{ dry_run, installs[] }` report; on failure (`ok: false`) surface which subproject's install command failed.

## validate

```bash
rtk mustard-rt run maint-validate           # build/type-check every detected subproject
rtk mustard-rt run maint-validate --dry-run # preview the resolved validate commands only
```

Print stdout verbatim. The binary enumerates subprojects from `grain.model.json` and picks the canonical validate command per project kind (`pnpm typecheck`, `cargo check`, `dotnet build`, …) — never read the Agents table or `{subproject}/CLAUDE.md` by hand. Output is a JSON `{ dry_run, overall, validates[] }` report; on `overall: fail` surface which subproject's validate command failed.

## sync

`mustard-rt run scan`. Use after creating new entities, importing code, or major edits.

## doctor

Consolidated check — never blocks, reports only.

```bash
mustard-rt run doctor              # wiring + drift + pipeline-state health
mustard-rt run doctor --residue    # also scan for dead file/script references
mustard-rt run diagnose-otel       # OTEL telemetry pipeline health
```

Print all three as one consolidated report. Categories: `wiring`, `drift`, `state-health`, `residue` (`--residue`) — each OK / WARN / FAIL.

Run periodically, after a plugin update, after telemetry looks wrong, or when hooks appear to skip silently.

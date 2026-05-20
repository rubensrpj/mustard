---
name: mustard-maint
description: "Maintenance utilities: install dependencies, run build/type-check validations, sync entity registry, and run the doctor installation health check. Use when the user asks about deps, validate, sync, or doctor."
source: scan
---
<!-- mustard:generated -->
# /maint - Maintenance Utilities

> Dependencies, build validation, and registry sync.

## Trigger

`/maint <action>`

## Actions

| Action | Description |
|--------|-------------|
| `deps` | Install dependencies for all projects |
| `validate` | Run build and type-check validations |
| `sync` | Update entity-registry.json from code |
| `doctor` | Full installation health check: wiring, drift, state + skill/OTEL diagnostics |

---

## deps

Installs dependencies. Monorepo: runs in all subprojects. Single repo: runs in root.

### Flow

1. Read `.claude/pipeline-config.md` → extract `Agents` table for subproject paths and install commands
2. For each subproject (or root if single repo), run the restore/install command from `## Commands` in `{subproject}/CLAUDE.md`
3. Run all in parallel

**Single repo**: read root `CLAUDE.md` → `## Commands` → run the install/restore command.

---

## validate

Runs build and type-check validations.

### Flow

1. Read `.claude/pipeline-config.md` → extract `Agents` table for validate/build commands
2. For each subproject (or root if single repo), run the build/type-check command from `{subproject}/CLAUDE.md` → `## Commands`
3. Run all in parallel

### Result

- **Success** — project compiles and passes type-check
- **Failure** — lists errors found

**Single repo**: read root `CLAUDE.md` → `## Commands` → run the build command.

---

## sync

Scans the project and updates `.claude/entity-registry.json`.

### Flow

1. `mustard-rt run sync-registry`
2. If script not found, scan manually:
   - Search database schemas (EF Core `DbSet<T>`, Drizzle `pgTable()`, Prisma `model`, etc.)
   - Build entity map with relationships
   - Update `.claude/entity-registry.json`

### When to Use

- After creating new entity
- After importing existing code
- To sync after manual changes

---

## doctor

Consolidated installation health check. Never blocks — reports only.

### Flow

1. `mustard-rt run doctor` — checks hook wiring, install drift, and pipeline state health; pass `--residue` to also scan for dead file/script references
2. `mustard-rt run skills orphans` — lists skills not invoked in N days (env `MUSTARD_SKILL_ORPHAN_DAYS`, default 30)
3. `mustard-rt run diagnose-otel` — end-to-end health check of the OTEL telemetry pipeline (collector process, `/healthz`, data flow)
4. Present all three results as one consolidated report to the user

### Report categories

| Category | OK | WARN | FAIL |
|----------|----|------|------|
| wiring | all hooks/run-cmds resolve | — | broken `mustard-rt on` or `run` reference |
| drift | installed matches templates/ | folders differ | — |
| state-health | registry present, no orphan states | orphan/expired states or missing registry | — |
| residue (`--residue`) | no dead refs | dead `.js` / empty `scripts/` | — |

### When to Use

- Periodic project hygiene
- After `mustard update` to confirm the install is clean
- After telemetry or metrics output looks wrong
- When hooks appear to be silently skipping

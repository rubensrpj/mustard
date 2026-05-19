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
| `audit` | Run diagnostics: orphan-skill audit + OTEL pipeline health |

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

## audit

Runs read-only diagnostics. Never blocks — reports only.

### Flow

1. `bun .claude/scripts/skills.js orphans` — lists skills not invoked in N days (env `MUSTARD_SKILL_ORPHAN_DAYS`, default 30)
2. `bun .claude/scripts/diagnose-otel.js` — end-to-end health check of the OTEL telemetry pipeline (collector process, `/healthz`, data flow)
3. Report both results to the user

### When to Use

- Periodic project hygiene
- After telemetry or metrics output looks wrong

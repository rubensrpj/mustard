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

1. `node .claude/scripts/sync-registry.js`
2. If script not found, scan manually:
   - Search database schemas (EF Core `DbSet<T>`, Drizzle `pgTable()`, Prisma `model`, etc.)
   - Build entity map with relationships
   - Update `.claude/entity-registry.json`

### When to Use

- After creating new entity
- After importing existing code
- To sync after manual changes

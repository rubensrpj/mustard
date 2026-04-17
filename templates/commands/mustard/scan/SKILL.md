# /scan - Agnostic Code Analyzer

> ALWAYS before making any change. Search on the web for the newest documentation and only implement if you are 100% sure it will work.

## Trigger

`/scan` or `/scan <subproject>`

## Execution Model

**CRITICAL — Context Protection:**
- The orchestrator MUST NOT perform analysis directly. ALL analysis MUST be delegated to Task agents.
- Orchestrator's role: discover → incremental check → launch agents → collect results → compile.
- **NO confirmation prompts**: never ask the user for approval. Just do it.
- **NO `run_in_background: true`** for Task agents that write files.

**CRITICAL — Read-Before-Write Protocol:**
Claude Code's `Write` and `Edit` tools fail with `File has not been read yet. Read it first before writing to it.` when targeting an existing file without a prior `Read` call in the same context.

Whenever the orchestrator (or a Task agent) modifies an existing file during `/scan`, it MUST:
1. Call `Read` on the target path first (even if just the first few lines).
2. Only then issue `Write` (full overwrite) or `Edit` (patch).
3. If the path genuinely does not exist, `Write` is safe without `Read` — but verify via `Glob` rather than guessing.

This applies especially to: `.claude/CLAUDE.md` regeneration, root `CLAUDE.md` updates, `.claude/docs/*.md` frontmatter injection, and subproject `CLAUDE.md` section edits.

## Process

### 1. Discover Subprojects & Incremental Detection

**Step A — Read OLD cache FIRST** (before running detect):
```bash
# Read the existing cache to get previous hashes
cat .claude/.detect-cache.json
```
Save the old `sourceHashes` and `moduleHashes` values.

**Step B — Run detect with `--no-cache`** (does NOT overwrite cache):
```bash
node .claude/scripts/sync-detect.js --no-cache
```
Parse JSON output → list of `{ name, path, role, agent, stackSummary, gitDirty?, gitDirtyCount? }` + new `sourceHashes` + `moduleHashes`.
- If `/scan <subproject>` was called, filter to that subproject only.

**Step C — Compare old vs new hashes + git dirty state:**
1. For each subproject, compare NEW `sourceHashes[name]` with OLD cached value
2. **Also check `gitDirty` flag** from detect output — if `gitDirty: true`, the subproject has uncommitted source file changes
3. **Hash match AND NOT gitDirty** → skip agent for this subproject (reuse existing `.claude/commands/` output)
4. **Hash mismatch OR gitDirty** → include in agent launch list (dirty files indicate changes the previous scan may not have captured)
5. If ALL subprojects can be skipped → skip to step 4 (Update CLAUDE.md) + step 5 (Compile) directly
6. **No old cache** → scan ALL subprojects (first run)

**Module-level incremental** (when subproject hash changed):
- Compare `moduleHashes[subproject][module]` with cached values
- Pass changed module names to the agent:
```
INCREMENTAL MODE:
Changed modules: [Contracts, PaymentGateway]
Unchanged modules: [Partners, Banks, Users, ...]
For UNCHANGED modules: reuse cached patterns (DO NOT re-analyze).
For CHANGED modules: full analysis (read code, detect patterns, update guards).
Merge results: combine cached + new into final output files.
```

**Impact estimates:**
| Scenario | Before | After |
|----------|--------|-------|
| Zero changes | ~225s | ~2-5s |
| 1 module changed | ~225s | ~40-60s |
| 1 subproject changed | ~225s | ~90s |
| Full scan (no cache) | ~225s | ~225s |

### 2.5. Cleanup Stale Subprojects

Compare OLD cached `subprojects[].name` list with NEW detected list.
For each name present in OLD but **absent** in NEW:

1. Delete `{name}/` directory if it exists and contains NO non-generated user files (check for `.claude/commands/notes.md` — if it has user content, warn and skip)
2. Remove stale entries from `.claude/.detect-cache.json` (`subprojects`, `sourceHashes`, `moduleHashes`)
3. Remove stale agent files: `.claude/agents/{name}-impl.md` if no remaining subproject uses that name
4. Remove stale skill directories in `.claude/skills/` that reference the removed subproject
5. Remove stale entity-registry entries under `e` that reference the removed subproject
6. Log: `CLEANUP: removed {name} (no longer detected)`

**Safety**: only delete directories that are NOT git submodules (`git submodule status` does not list them) and are NOT tracked by git (`git ls-files {name}` returns empty).

### 2.7. Scan Product Docs

If `.claude/docs/` exists and contains `.md` files:

1. For each `.md` file, read content and analyze:
   - Extract or infer: name (from H1), description (from first paragraph/blockquote), topics (from H2 headings as keywords)
2. **Read the file first** (required by Claude Code's Write/Edit contract), then generate/update YAML frontmatter with `name`, `description`, `topics`, `scanned-at`
   - If file has existing frontmatter WITH `scanned-at` → overwrite (auto-generated)
   - If file has existing frontmatter WITHOUT `scanned-at` → preserve user frontmatter, skip
   - If file has no frontmatter → prepend generated frontmatter
3. This step does NOT require a Task agent — orchestrator can do it inline (small, deterministic work). Always `Read` before `Edit`/`Write`.

### 2.6. Bootstrap (if needed)

**Fast-path**: If root `CLAUDE.md` exists AND `.claude/entity-registry.json` exists → skip to step 3 (Launch Agents).
Bootstrap only runs on first scan or when foundational files are missing.

Otherwise create foundational files:

**`.claude/CLAUDE.md`** — orchestrator entry point (always regenerate). If the file already exists, `Read` it first before calling `Write` — otherwise Claude Code's Write tool will reject the call:
```markdown
<!-- mustard:generated -->
# Orchestrator Rules

## Role
You do NOT implement code — you delegate via Task tool.

## Intent Routing

| Intent | Signals | Action |
|--------|---------|--------|
| Feature | create, add, new entity, new CRUD, implement | Pipeline Feature |
| Enhancement | improve, adjust, change, add field/column, optimize, update | Pipeline Feature |
| Bugfix | error, bug, not working, broken, fix, correct | Pipeline Bugfix |
| Analyze | analyze, audit, evaluate, check, compare, inspect, assess | Delegate via /task |
| Simple | config, docs, small refactor, rename, move | Delegate via Task |

Any change that touches production code (schema, API, UI) → Pipeline Feature.
Read `.claude/pipeline-config.md` for agent dispatch rules.

## Full Reference
Rules, pipeline, naming: `.claude/pipeline-config.md`
```

**Root `CLAUDE.md`** — project map from detected subprojects:
```markdown
# {ProjectName} - Project Context

> Framework rules: See [.claude/CLAUDE.md](./.claude/CLAUDE.md)

## Project Structure

| Subproject | Technology | Port | CLAUDE.md |
|------------|------------|------|-----------|
| {name} | {detected stack} | {port or -} | [{name}](./{name}/CLAUDE.md) |

## Entity Registry

**CRITICAL:** Before searching for ANY entity, read `.claude/entity-registry.json` first.

## Ignore Paths

Never search in:
- `node_modules/`, `.next/`, `bin/`, `obj/`, `dist/`, `migrations/`
```

**`.claude/entity-registry.json`** — generate via registry scanner:
```bash
node .claude/scripts/sync-registry.js --force
```
If `sync-registry.js` fails or is not available, create empty skeleton:
```json
{ "_meta": { "version": "4.0" }, "_patterns": {}, "_enums": {}, "e": {} }
```

**`{subproject}/CLAUDE.md`** — per subproject (skip if exists):
```markdown
# {SubprojectName}

> Parent: [../CLAUDE.md](../CLAUDE.md) | Orchestrator: [../.claude/CLAUDE.md](../.claude/CLAUDE.md)
> Skills: `{name}/.claude/skills/` | Guards: `{name}/CLAUDE.md`

## Stack

{stackSummary from sync-detect.js}

## Commands

{detected build/run/test commands}

## Key Paths

{detected from folder structure}

## Guards

{leave empty — populated after analysis}
```

Ensure each detected subproject has a `CLAUDE.md` file.

### 3. Launch Agents

**CRITICAL: Launch ALL agents in a SINGLE message with parallel tool calls.**
**CRITICAL: NEVER use `run_in_background: true` — agents MUST write files (Write/Edit/Bash are denied in background mode). Always use foreground (default).**

For each subproject to scan, launch one Task agent with `subagent_type: "general-purpose"`:

```
Read .claude/commands/mustard/scan-format.md for analysis and format rules.

**EXECUTION RULE — NO CONFIRMATION PROMPTS**: NEVER ask the user to confirm file writes, overwrites, deletes, or directory creations. The user already invoked /scan — that IS the approval. Proceed autonomously. If an action fails, surface the error in the return format and move on; do NOT stop to ask what to do.

Subproject: {name}
Path: {path}
Role: {role}
Stack: {stackSummary}

Tasks:
1. Read existing knowledge from {path}/.claude/commands/ and {path}/CLAUDE.md
2. Backup generated files to {path}/.claude/commands/_backup/
3. Ensure notes.md exists
4. Analyze source code following scan-format.md rules
5. Write generated files to {path}/.claude/commands/
6. Generate granular skills following scan-format.md §10 (skill-creator methodology)
7. Update {path}/CLAUDE.md with scan references
```

### 4.5. Generate Agents

For each detected subproject, generate `.claude/agents/{subproject.name}-impl.md`:

```yaml
---
name: {subproject.name}-impl
description: {role} implementation for {subproject.name}. Reads {subproject.name}/CLAUDE.md for guards.
model: sonnet
tools: [Read, Write, Edit, Bash, Grep, Glob]
memory: project
---
```

Body (below frontmatter):
```markdown
<!-- mustard:generated -->

# {Role} Implementation Agent

## Mandatory Reads
1. `{subproject.path}/CLAUDE.md` — guards, stack, key paths
2. `{subproject.path}/.claude/commands/guards.md` — DO/DON'T rules
3. `{subproject.path}/.claude/commands/notes.md` — project-specific notes

## Boundary
{boundary from Role Rules table}

## Validation
{validate command from subproject CLAUDE.md → Commands section}

## Return Format
### Files Modified/Created
| File | Action |
|------|--------|

### {role-specific sections from Role Rules}

### Build / Type-check
{output}

### Guards Verified
Total: {n}/{total} | Violations: {v}
```

Also generate `.claude/agents/{subproject.name}-explorer.md` for each subproject:
```yaml
---
name: {subproject.name}-explorer
description: Read-only exploration agent for {subproject.name} codebase analysis and investigation.
model: haiku
tools: [Read, Grep, Glob]
memory: project
---
```

Body (below frontmatter):
```markdown
<!-- mustard:generated at:{ISO} role:{role} -->

# {Subproject} Explorer Agent

> Read-only analysis of {subproject.name} codebase. Patterns, dependencies, architecture, quality evaluation.

## Mandatory Reads
1. `{subproject.path}/CLAUDE.md` — project rules, guards, stack
2. `{subproject.path}/.claude/commands/guards.md` — DO/DON'T rules

## Skill References (load when relevant to task)
- Design/UX analysis: `design-craft` skill
- Architecture analysis: `senior-architect` skill

## Boundary
- **Read-only** — NEVER write, edit, or execute commands
- Scope: `{subproject.path}/` directory only
- Ignore: `bin/`, `obj/`, `node_modules/`, `.next/`, `Migrations/`
- **Budget: ≤20 tool uses total, ≤3 full file reads** — prefer Grep over Read
- Return findings as soon as pattern/root-cause is clear — do NOT exhaustively scan

## Return Format
### Findings
| Severity | File:Line | Detail |
|----------|-----------|--------|
| CRITICAL / WARNING / NOTE | path:line | description |

### Suggested Actions
- Concrete `/task` or pipeline commands to address findings
```

Mark all with `<!-- mustard:generated -->`. Overwrite on next scan.

### 4.6. Generate Granular Skills (skill-creator methodology)

For each detected pattern, generate a **granular skill** following skill-creator methodology.
See `scan-format.md` §10 for decomposition rules, SKILL.md format, and description guidelines.

**Key rules:**
- One conceptual pattern = one skill (not one file = one skill)
- Skill name: `{subproject-short}-{pattern-name}` (e.g., `api-endpoint-wiring`, `app-mvvm-feature`)
- Description must be "pushy" — include casual trigger phrases (see scan-format.md §10)
- Extract real code examples into `references/examples.md`
- Max 500 lines per SKILL.md body (ideally <200)

**Output structure per skill:**
```
.claude/skills/{skill-name}/
├── SKILL.md              → Pattern instruction
└── references/
    └── examples.md        → Real code from codebase
```

Skills are generated ONLY in `{subproject}/.claude/skills/{skill-name}/` (NOT in root `.claude/skills/`).
Mark all with `<!-- mustard:generated -->`. Overwrite on next scan.

### 4.7. Generate Pattern Skills from Registry (OODA: Observe → Act)

After agent-generated skills (4.6), run the registry-based skill generator to create structural pattern skills from `_patterns`:

```bash
node .claude/scripts/sync-registry.js --force
node .claude/scripts/skill-generator.js --force
```

This generates skills that the agents in Step 3 may have missed — particularly:
- `{role}-entity-creation` — entity folder, base class, interfaces, namespace
- `{role}-enum-placement` — enum folder, decorators, NEVER inline in entities
- `{role}-route-conventions` — route naming, auth pattern, CRUD standard
- `{role}-service-pattern` — interface-first, base interface, DI
- `{role}-repository-pattern` — base class, interface, DI
- `{role}-dto-conventions` — folder, naming, validation pattern
- `{role}-module-registration` — DI registration, route wiring

These skills are derived from **detected patterns** (not hardcoded). They complement agent-generated skills by covering structural conventions that agents may not explicitly document.

**Skip conditions:**
- `entity-registry.json` version < 4.0 → skip (registry not populated)
- `skill-generator.js` not present → skip
- Pattern skill already exists and was NOT generated by mustard → skip (user-edited)

### 4. Update CLAUDE.md files

After agents complete:
- **Regenerate `.claude/CLAUDE.md`** from the template in step 2 (always overwrite — it's `mustard:generated`). `Read` it first if it exists, then `Write`.
- Update root `CLAUDE.md`:
  - `Read` the current file before any `Edit` call (avoids `File has not been read yet` errors)
  - `## Project Structure` table if subprojects changed
  - Project-specific commands detected
  - `## Ignore Paths` with detected paths

### 5. Update Cache

Update the detect cache so the NEXT scan can use it for incremental detection:
```bash
node .claude/scripts/sync-detect.js
```
This runs detect WITH cache writing (no `--no-cache` flag), persisting current hashes.

### Phase: Security Scan

Run after code analysis (step 3) or independently via `/scan --security`:

```bash
node .claude/scripts/security-scan.js "$PROJECT_DIR"
# JSON output for programmatic use:
node .claude/scripts/security-scan.js "$PROJECT_DIR" --json
```

Include findings in scan output under a `## Security` section:

| Severity | Finding Type | Action |
|----------|-------------|--------|
| **CRITICAL** | Secrets detected | Flag in verification checklist; do not commit |
| **WARNING** | Env file not in .gitignore | Add to .gitignore before any push |
| **ADVISORY** | Dangerous permission rule in settings.json | Review and tighten |

- Exit code 0 = clean; exit code 1 = findings present
- Secret previews are truncated to 8 chars — never log full values
- Skip if `$PROJECT_DIR` is not set; use `process.cwd()` as fallback

## Verification

1. All skills in `{subproject}/.claude/skills/` have valid SKILL.md
2. Every generated file has `<!-- mustard:generated -->` header
3. Every generated file has a blockquote description after the H1 title
4. Every pattern references a real file
5. Old files backed up in `_backup/`
6. Each subproject's CLAUDE.md has `## Scan References`
7. Root CLAUDE.md has `## Project Structure` with all subprojects
8. `.claude/entity-registry.json` exists and is v4.0
9. Pattern skills generated from registry (entity-creation, enum-placement, route-conventions, etc.)
10. Each generated skill has valid YAML frontmatter (name + description)
10. Each skill's description is "pushy" — includes casual trigger phrases
11. If security scan ran: findings summarized in `## Security` section of output

## Return Format

```json
{
  "scanned": ["{subproject-1}", "{subproject-2}"],
  "generated": { "{subproject-1}": ["stack.md", "modules.md", "guards.md"] },
  "skills_generated": { "{subproject-1}": ["api-endpoint-wiring", "api-service-base", "api-entity-config"] },
  "errors": []
}
```

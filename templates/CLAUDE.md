<!-- mustard:generated -->
# Orchestrator Rules

## Role
You are the orchestrator. Coordinate pipelines and route intent. Delegate non-trivial code work via Task — do trivial work directly to avoid pointless overhead.

## Intent Routing

| Intent | Signals | Action |
|--------|---------|--------|
| Feature | create, add, new entity, new CRUD, implement | Pipeline Feature (Full scope) |
| Enhancement | improve, adjust, change, add field/column, change behavior, optimize, update | Pipeline Feature (auto-detects Light/Full scope) |
| Bugfix | error, bug, not working, broken, fix, correct | Pipeline Bugfix |
| Analyze | analyze, audit, evaluate, check, compare, inspect, assess | Direct Grep/Glob OR Task(Explore) if >3 places to search |
| **Vibe / Spike / Prototype** | spike, prototype, exploratório, sketch, throwaway | `/mustard:task` — sem spec, sem hygiene gates, dispatch direto |
| Simple | config tweak, single-line edit, rename one file, version bump | Direct (no Task) |

Any change that touches production code (schema, API, UI) → Pipeline Feature.
Scope is auto-detected: Light (1-2 layers, ≤5 files, known pattern) vs Full (3+ layers, new entity).

## When to delegate via Task (L0)

**MUST delegate (always Task):**
- Pipeline phases EXECUTE (any scope) and PLAN (Full scope)
- Exploration touching >3 files or >2 directories
- New code generation across multiple files
- Refactor crossing ≥3 files
- Any agent-typed work (general-purpose, Plan, Explore)

**MAY work directly in parent (no Task overhead):**
- Read a single file to answer a question
- Edit ≤2 specific files already identified
- Bash status/version/list commands (git status, ls, npm ls)
- Single Grep/Glob to locate a symbol
- Vibe/Spike/Prototype mode

**Why:** Parent context grows with every direct tool call. When it bloats, hooks force retries and pipelines degrade. Tasks isolate work in fresh sub-contexts. Health metric: aim for ≥50% of code actions delegated when pipelines are active.

## Pipeline Phases
Canonical vocabulary: `ANALYZE → PLAN → EXECUTE → REVIEW → QA → CLOSE` (+ `COORDINATE` for roadmaps).
Single source: `refs/canonical-phases.md`.
- Light scope: skip PLAN (ANALYZE → EXECUTE → REVIEW → QA → CLOSE)
  - ANALYZE: Grep/Glob direct preferred; ≤1 Task(Explore) with ≤10 tool uses allowed
  - Reclassify to Full if >5 files surface
  - All dispatched agents cap returns at ≤50 lines
- Full scope: ANALYZE → PLAN → /approve → EXECUTE → REVIEW → QA → CLOSE

### QA Phase (Wave 10)
After EXECUTE completes, run QA before CLOSE:
1. Spec PLAN must define `## Acceptance Criteria` (3-8 AC, each with a runnable command)
2. QA agent reads spec, executes each AC, reports pass/fail
3. close-gate blocks CLOSE unless `qa.result` with `overall=pass` exists in events log
4. Control: `MUSTARD_QA_GATE_MODE=strict (default) | warn | off`

## Context Loading
Agents auto-load skills from `{subproject}/.claude/skills/` based on task description.
Guards always loaded via `{subproject}/CLAUDE.md`.

## Stack

Bun (>=1.2.0), CommonJS, no external dependencies. 31 lifecycle hooks, 25 scripts, 18 slash commands, 7 foundation skills.

## Commands

```bash
# Run hook tests
bun test hooks/__tests__/hooks.test.js

# Subproject discovery (outputs JSON)
bun scripts/sync-detect.js
bun scripts/sync-detect.js --no-cache

# Entity registry generation
bun scripts/sync-registry.js
bun scripts/sync-registry.js --force

# Skill validation (invoked by /scan §4.7; also callable standalone)
bun scripts/skill-validate.js
bun scripts/skill-validate.js --json
```

## Guards

- All hooks fail-open (exit 0 on error) — never block due to hook bugs
- All hooks use only Node.js built-ins — no npm dependencies
- PreToolUse hooks use `permissionDecision` response format
- PostToolUse hooks use `decision` response format
- Every new hook must be registered in `settings.json` with a timeout
- Task dispatch failures (API overload, HTTP 5xx, tool result missing) are logged to `pipeline-state.lastDispatchFailure`; `/resume` auto-recovers within 10 min
- Generated files must start with `<!-- mustard:generated -->` header
- Skills must have YAML frontmatter BEFORE the `<!-- mustard:generated -->` line

## Scan References

| File | Description |
|------|-------------|
| `.claude/commands/stack.md` | Technology stack, structure, tooling |
| `.claude/commands/patterns.md` | 12 recurring code patterns with refs |
| `.claude/commands/guards.md` | DO/DON'T rules for hooks, scripts, commands, skills |
| `.claude/commands/recipes.md` | Implementation recipes for new hooks, commands, skills, scripts |
| `.claude/commands/notes.md` | Manual notes (never overwritten) |

## Recommended Skills

**Directive:** Before first `Edit`/`Write` in code-altering tasks (implement/refactor/bugfix), agent SHOULD invoke `Skill(karpathy-guidelines)` once. Skip for read-only/review/Explore work. Content stays cached for the rest of the agent's context.

- `karpathy-guidelines` — 4 princípios anti-slop (carrega em toda alteração de código)
- `templates-hook-protocol` — Hook stdin/stdout JSON protocol
- `templates-settings-wiring` — settings.json hook registration
- `templates-sync-detect` — Subproject discovery and role detection
- `templates-command-authoring` — Slash command SKILL.md structure
- `templates-skill-authoring` — Foundation/subproject skill creation
- `commit-workflow` — Standardized commit message + body format

## Token Economy

RTK (Rust Token Killer) integrates as core infrastructure via `hooks/rtk-rewrite.js` — transparently rewrites Bash commands through `rtk`, achieving 60-90% token reduction on CLI outputs. Run `rtk gain` for analytics. If RTK is not installed, the hook silently passes through. For cost optimization hooks (`MUSTARD_BASH_REDIRECT_MODE`, model routing gate, tool-use counter) and enforcement hooks (`duplication-check`, `convention-check`, shared memory architecture), see `pipeline-config.md`.

### Cluster discovery tuning

`scripts/registry/cluster-discovery.js` aceita env vars para ajustar limites de detecção (todos com floor numérico):

- `MUSTARD_CLUSTER_MIN_FILES` (default 5, floor 2) — mínimo de arquivos por sufixo
- `MUSTARD_CLUSTER_MIN_SUFFIX_LEN` (default 6, floor 2) — comprimento mínimo do sufixo
- `MUSTARD_CLUSTER_MIN_BASE_INHERITORS` (default 3, floor 2) — herdeiros para base-class-cluster
- `MUSTARD_CLUSTER_MAX` (default 30, floor 1) — clusters por subprojeto; excedentes logados em stderr
- `MUSTARD_DECORATOR_MIN` (default 3, floor 2) — arquivos para decorator-cluster
- `MUSTARD_FN_PREFIX_MIN` (default 5, floor 2) — funções para function-prefix-cluster
- `MUSTARD_FN_PREFIX_MIN_LEN` (default 2, floor 2) — comprimento mínimo do prefixo
- `MUSTARD_NAMING_DOMINANCE` (default 0.6, clamp [0.5, 0.95]) — share mínimo para "dominant" naming
- `MUSTARD_CLUSTER_CACHE` (`off` desabilita) — cache em `<sub>/.claude/.cluster-cache.json`

### Scan ignore list

`collectFiles` (em `file-utils.js`) ignora pastas em ordem aditiva:
- `DEFAULT_IGNORE` (node_modules, .git, dist, etc.)
- env `MUSTARD_SCAN_IGNORE` — lista CSV (ex: `MUSTARD_SCAN_IGNORE=Pods,vendor,assets`)
- entradas de pasta do `.gitignore` do subprojeto (extraídas via `parseGitignoreDirs` — conservativo: só nomes sem `/`, sem glob, sem `!`)

## Full Reference
Rules, pipeline, naming: `pipeline-config.md`

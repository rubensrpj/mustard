<!-- mustard:generated -->
# Orchestrator Rules

## Role
You are the orchestrator. Coordinate pipelines and route intent. Delegate non-trivial code work via Task — do trivial work directly to avoid pointless overhead.

## Response Style

When talking to the user (chat, AskUserQuestion options, banners, errors), be didactic — expand abbreviations on first use, prefer common words over jargon. Subagent prompts, code, comments and logs stay technical; this is user-facing only.

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

Rust (mustard-cli, mustard-rt). Enforcement runs as the single Rust binary `mustard-rt` (the `apps/rt` crate — one `mustard-rt on <event>` entry per lifecycle event in `settings.json`); 28 scripts, 18 slash commands, 13 foundation skills.

## Memory Layout — Substitution vs Harness Engineering Book

The Harness Engineering book (§5.3) treats `MEMORY.md` as an entry index with a hard cap (200 lines / 25 KB). Mustard substitutes this with structured SQLite tables (`memory_decisions` and `memory_lessons`) ranked by confidence × recency. Same goal (index, not body), different mechanism — structured ranking lets `SessionStart` inject only the top-N relevant entries within a capped budget via `SELECT … ORDER BY confidence*recency LIMIT N`, rather than a fixed line-limit on a plain-text file. The `MEMORY.md` you may see at `~/.claude/projects/<project>/memory/MEMORY.md` is your user-global memory (managed by Claude Code), not the project memory layer.

## Commands

```bash
# Run hook tests
cargo test -p mustard-rt

# Subproject discovery (outputs JSON)
mustard-rt run sync-detect

# Entity registry generation
mustard-rt run sync-registry
mustard-rt run sync-registry --force

# Skill CLI (invoked by /scan §4.7; also callable standalone)
mustard-rt run skills validate
mustard-rt run skills validate --json
```

## Guards

- All hooks fail-open (exit 0 on error) — never block due to hook bugs
- Hooks are modules of the Rust `mustard-rt` binary — no external runtime
- PreToolUse hooks use `permissionDecision` response format
- PostToolUse hooks use `decision` response format
- Every new hook event must be registered in `settings.json` with a timeout
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
- `commit-workflow` — Standardized commit message + body format

**Engineering/productivity skills (verbatim from `github.com/mattpocock/skills` — not Mustard-generated):**

- `grill-me` — Relentless plan/design interview until shared understanding
- `grill-with-docs` — Grill a plan against the domain model; updates `CONTEXT.md`/ADRs inline
- `diagnose` — Disciplined diagnosis loop for hard bugs and perf regressions
- `improve-codebase-architecture` — Find deepening opportunities informed by `CONTEXT.md` + `docs/adr/`

## Token Economy

RTK (Rust Token Killer) integrates as core infrastructure via the `mustard-rt` `bash_guard` module — transparently rewrites Bash commands through `rtk`, achieving 60-90% token reduction on CLI outputs. Run `rtk gain` for analytics. If RTK is not installed, the rewrite silently passes through. For cost-optimization gates (`MUSTARD_BASH_REDIRECT_MODE`, model-routing gate, tool-use counter) and the shared-memory architecture, see `pipeline-config.md`.

### Cluster discovery tuning

A descoberta de clusters (`mustard-rt run sync-registry`, módulo `scan/cluster_discovery.rs`) aceita env vars para ajustar limites de detecção (todos com floor numérico):

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

A coleta de arquivos do scanner (`scan/file_utils.rs`) ignora pastas em ordem aditiva:
- `DEFAULT_IGNORE` (node_modules, .git, dist, etc.)
- env `MUSTARD_SCAN_IGNORE` — lista CSV (ex: `MUSTARD_SCAN_IGNORE=Pods,vendor,assets`)
- entradas de pasta do `.gitignore` do subprojeto (extraídas via `parseGitignoreDirs` — conservativo: só nomes sem `/`, sem glob, sem `!`)

## Spec Layout

Specs live under a **flat** directory: `.claude/spec/{name}/`. There are no `active/`, `completed/`, or `superseded/` bucket subdirectories — status comes from the `### Stage:` + `### Outcome:` headers inside `spec.md`, and archival is semantic-only (the `pipeline.status` event in SQLite, not a filesystem move). Wave plans add a `wave-plan.md` plus `wave-N-{role}/spec.md` subdirs inside the same `{name}/` directory.

## Full Reference
Rules, pipeline, naming: `pipeline-config.md`

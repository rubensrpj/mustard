<!-- mustard:generated -->
# Orchestrator Rules

## Role
You are the orchestrator. Coordinate pipelines and route intent. Delegate non-trivial code work via Task — do trivial work directly to avoid pointless overhead.

## Response Style

When talking to the user (chat, AskUserQuestion options, banners, errors), be didactic — expand abbreviations on first use, prefer common words over jargon. Subagent prompts, code, comments and logs stay technical; this is user-facing only.

## Intent Routing

| Intent | Signals | Action |
|--------|---------|--------|
| Feature | create, add, new entity, implement | Pipeline Feature (Full scope) |
| Enhancement | improve, adjust, change, add field/column, change behavior, optimize, update | Pipeline Feature (auto-detects Light/Full scope) |
| Bugfix | error, bug, not working, broken, fix, correct | Pipeline Bugfix |
| Analyze | analyze, audit, evaluate, check, compare, inspect, assess | Direct Grep/Glob OR Task(Explore) if >3 places to search |
| Vibe / Spike / Prototype | spike, prototype, sketch, throwaway | `/mustard:task` — no spec, no hygiene gates, direct dispatch |
| Simple | config tweak, single-line edit, rename one file, version bump | Direct (no Task) |

Signals are heuristics — the pipeline detects what makes sense for the project that was scanned. Any change that touches production code → Pipeline Feature. Scope is auto-detected: Light (1-2 layers, ≤5 files, known pattern) vs Full (3+ layers, new entity).

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
- Bash status/version/list commands
- Single Grep/Glob to locate a symbol
- Vibe/Spike/Prototype mode

**Why:** Parent context grows with every direct tool call. When it bloats, hooks force retries and pipelines degrade. Tasks isolate work in fresh sub-contexts. Health metric: aim for ≥50% of code actions delegated when pipelines are active.

## Pipeline Phases

Canonical vocabulary: `ANALYZE → PLAN → EXECUTE → REVIEW → QA → CLOSE` (+ `COORDINATE` for roadmaps). Single source of truth: `refs/canonical-phases.md`.

- **Light scope**: skip PLAN (`ANALYZE → EXECUTE → REVIEW → QA → CLOSE`)
  - ANALYZE: Grep/Glob direct preferred; ≤1 Task(Explore) with ≤10 tool uses allowed
  - Reclassify to Full if >5 files surface
  - All dispatched agents cap returns at ≤50 lines
- **Full scope**: `ANALYZE → PLAN → /approve → EXECUTE → REVIEW → QA → CLOSE`

### QA Phase (Wave 10)

After EXECUTE completes, run QA before CLOSE:

1. Spec PLAN must define `## Acceptance Criteria` (3-8 AC, each with a runnable command)
2. QA agent reads spec, executes each AC, reports pass/fail
3. close-gate blocks CLOSE unless `qa.result` with `overall=pass` exists in the events log
4. Control: `MUSTARD_QA_GATE_MODE=strict (default) | warn | off`

## Context Loading

Agents auto-load skills from `{subproject}/.claude/skills/` based on task description. Guards always loaded via `{subproject}/CLAUDE.md`. Skill catalog: `.claude/skills/`. Progressive-disclosure refs live in `.claude/refs/{command}/` and are pulled on demand.

## Spec Layout

Specs live under a **flat** directory: `.claude/spec/{name}/`. There are no `active/`, `completed/`, or `superseded/` bucket subdirectories — lifecycle state (`stage` + `outcome` + `flags`) lives in the `meta.json` sidecar beside each `spec.md` (the single source of truth), and archival is semantic-only (recorded as a `pipeline.status` event, not a filesystem move). The `spec.md` is **pure narrative** — it carries no `### Stage:` / `### Outcome:` / `### Flags:` / `### Phase:` / `### Scope:` / `### Lang:` / `### Checkpoint:` / `### Parent:` / `### Total waves:` header lines; never read or write lifecycle metadata from the markdown. Wave plans add a `wave-plan.md` plus `wave-N-{role}/spec.md` subdirs (each with its own `meta.json`) inside the same `{name}/` directory.

## Full Reference

Rules, pipeline, naming, role rules, hooks: `pipeline-config.md`.

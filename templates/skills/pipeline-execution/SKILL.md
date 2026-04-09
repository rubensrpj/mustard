---
name: pipeline-execution
description: Pipeline phases, dispatch rules, wave system, validate, retry. Load for /feature /resume /approve.
disable-model-invocation: true
---
<!-- mustard:generated -->

# Pipeline Execution Detail

> Phases, role rules, dispatch mechanics, validation, bugfix paths. Loaded on-demand.

## Pipeline Feature

### ANALYZE Phase (collapses old SYNC+UNDERSTAND+SCOPE+EXPLORE)

1. **AUTO-SYNC:** `node .claude/scripts/sync-registry.js`
2. Read `entity-registry.json` → entity found? → infer layers. Not found? → all layers.
3. Extract `_patterns`, `e.{Entity}`, `_enums`.

| Signal | Layers |
|--------|--------|
| New field/column/relation | DB (+ Backend/FE if visible) |
| New endpoint, business logic | Backend (+ FE if visible) |
| New screen/component | Frontend (+ Backend if new endpoint) |
| New CRUD / sub-entity | DB + Backend + Frontend |
| Refactoring, bug fix | Root cause layer(s) |

When in doubt → `AskUserQuestion`: "Which layers?"

**Scope Detection:**

| Signal | → Scope |
|--------|---------|
| 1-2 layers, ≤5 files, known pattern, no new entity | **Light** |
| 3+ layers, 5+ files, new entity/CRUD, new pattern | **Full** |

**Explore (conditional):**
- Entity in registry → SKIP Explore, read 2-3 reference files directly
- Entity NOT in registry → Explore agent ("medium"), then straight to PLAN
- **MAX 5 file reads in ANALYZE** (registry/pipeline-config are free)

### PLAN Phase (collapses old SPEC)

Create `.claude/spec/active/{date}-{name}/spec.md`:
- **Full scope:** Summary, Entity Info, Files, Tasks (by wave), Dependencies. Header: `Scope: full`.
- **Light scope:** Summary (1-2 lines), Checklist (tasks by agent, no waves). Header: `Scope: light`.

Add checkpoint: `Status: draft`, `Phase: PLAN`, `Scope: {light|full}`, `Checkpoint: {now}`.
Create `.claude/.pipeline-states/{spec-name}.json`.

**Light Scope → Inline Path:** When `/feature` detects Light scope and user approves inline, EXECUTE runs in same session. No PLAN phase needed.

### EXECUTE Phase (collapses old IMPLEMENT+VALIDATE+REVIEW)

**1. Read Recipes:** Match spec work to a recipe type. Grep `recipes.md` for title — do NOT read full file.

**2. Skills Auto-Loading:**

Agents auto-load relevant skills from `{subproject}/.claude/skills/` based on task description.
Orchestrator may hint specific skills via `{recommended_skills}` in the agent prompt.

**3. Plan Waves:**
- **Wave 1:** 🟡 Database + 🔵 Backend + 🟣 Libs — independent, dispatched together
- **Wave 2:** 🟢 Frontend + 🟠 Mobile — starts ONLY after ALL Wave 1 complete
- ALL agents in same wave → SINGLE message (multiple `<invoke>` blocks)
- NEVER nest dispatch — nesting breaks parallel execution

**4. Dispatch Agent:**

IF `.claude/agents/{subproject}-impl.md` exists:
  Use `subagent_type: "{subproject}-impl"`. Compact prompt (~30-40 lines):
  - REFERENCE: pattern file §sections + reference module
  - ENTITY: registry info
  - SKILLS: recommended skills for this task
  - EFFICIENCY: absolute paths, max 3 builds, chain commands
  - TASK: checkboxed steps

ELSE (fallback):
  Use `subagent_type: "general-purpose"` with full template (~80 lines).

**5. Validate:**
- Build passes (backend: `dotnet build`, frontend: `pnpm build`, mobile: `fvm flutter analyze`)
- Zero critical guard violations
- All spec `[ ]` → `[x]`
- Any failure → retry (max 2/agent), then STOP + replan

**6. Review (MANDATORY — NEVER skip):**
Dispatch review agent for EACH affected subproject. The review agent reads `{subproject}/CLAUDE.md` + `{subproject}/.claude/commands/guards.md` and runs the full 7-category checklist:
1. **SOLID** — SRP, OCP, LSP, ISP, DIP
2. **Design System** — tokens, typography, spacing, components, icons, theme
3. **Patterns** — project conventions from guards.md
4. **i18n** — all strings localized, all locale files updated
5. **Integration** — types synced, no orphans, no circular deps
6. **Build** — compiles/analyzes clean
7. **Elegance** — simplest solution, no over-engineering

APPROVED (zero CRITICAL) → CLOSE. REJECTED (any CRITICAL) → fix agent dispatched (max 2 fix loops), then re-review.

### CLOSE Phase (collapses old COMPLETE)

1. `node .claude/scripts/sync-registry.js`
2. Update spec: `Status: completed`, `Phase: CLOSE`, all `[ ]` → `[x]`
3. Move spec to `.claude/spec/completed/`
4. **Delete** `.claude/.pipeline-states/{spec-name}.json`
5. Output with agent colors: `═══ PIPELINE COMPLETE — {name} | Agents: {n} ok | Files: {c} created, {m} modified ═══`

### Replan Protocol

When: agent FAILED structurally, retry exhausted, user reports unexpected behavior, review REJECTED with architectural concern.
Steps: update spec → summarize failure → Explore → rewrite tasks → re-approve → resume EXECUTE.

## Role Rules

> See `.claude/pipeline-config.md § Role Rules` for role boundaries and validation rules.

## Pipeline Bugfix

### Fast Path (1-2 files, clear cause)
ANALYZE → FIX → VALIDATE → CLOSE. No spec needed.

### Full Path (3+ files, unclear impact)
ANALYZE → PLAN → APPROVE → FIX → VALIDATE → CLOSE.

### Decision
Explore returns clear root cause in 1-2 files → Fast Path. Otherwise → Full Path.

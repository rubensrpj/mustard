---
name: pipeline-execution
description: Pipeline phases, dispatch rules, wave system, validate, retry. Use when running /feature, /resume, /approve or any pipeline phase requiring dispatch/wave context.
tags: [plan, any]
appliesTo: []
scope: [plan, code-editing]
metadata:
  generated_by: foundation
disable-model-invocation: true
source: manual
---

# Pipeline Execution Detail

> Phases, role rules, dispatch mechanics, validation, bugfix paths. Loaded on-demand.

## Pipeline Feature

### ANALYZE Phase (collapses old SYNC+UNDERSTAND+SCOPE+EXPLORE)

1. **MODEL:** `mustard-rt run scan` (produce/refresh `.claude/grain.model.json`).
2. Research via the scan digest — NEVER read the repo or the model whole. Run `mustard-rt run feature --intent "{request}"`; the insumos carry the matched slices/contracts/hubs + the anchor files. `miss: true` → no repo-vocabulary precedent (re-query with repo terms; treat true net-new as design). Otherwise infer layers from the matched slices.
3. Read ONLY the `anchors` the insumos point to (~12 real files), then ask `mustard-rt run scan spec` per unit.

| Signal                       | Layers                               |
| ---------------------------- | ------------------------------------ |
| New field/column/relation    | DB (+ Backend/FE if visible)         |
| New endpoint, business logic | Backend (+ FE if visible)            |
| New screen/component         | Frontend (+ Backend if new endpoint) |
| New CRUD / sub-entity        | DB + Backend + Frontend              |
| Refactoring, bug fix         | Root cause layer(s)                  |

When in doubt → `AskUserQuestion`: "Which layers?"

**Scope Detection:**

| Signal                                             | → Scope   |
| -------------------------------------------------- | --------- |
| 1-2 layers, ≤5 files, known pattern, no new entity | **Light** |
| 3+ layers, 5+ files, new entity/CRUD, new pattern  | **Full**  |

**Explore (conditional):**

- Entity in registry → SKIP Explore, read 2-3 reference files directly
- Entity NOT in registry → Explore agent ("medium"), then straight to PLAN
- **MAX 5 file reads in ANALYZE** (registry/pipeline-config are free)

### PLAN Phase (collapses old SPEC)

Create `.claude/spec/{date}-{name}/spec.md`:

- **Full scope:** Summary, Entity Info, Files, Tasks (by wave), Dependencies. Header: `Scope: full`.
- **Light scope:** Summary (1-2 lines), Checklist (tasks by agent, no waves). Header: `Scope: light`.

Add checkpoint: `Status: draft`, `Phase: PLAN`, `Scope: {light|full}`, `Checkpoint: {now}`.
Create `.claude/.pipeline-states/{spec-name}.json`.

**Light Scope → Inline Path:** When `/feature` detects Light scope and user approves inline, EXECUTE runs in same session. No PLAN phase needed.

### EXECUTE Phase (collapses old IMPLEMENT+VALIDATE+REVIEW)

**1. Skills Auto-Loading:**

Agents auto-load relevant skills from `{subproject}/.claude/skills/` based on task description.
The subproject's curated Guards are injected inline by the renderer (`## GUARDS`); there is no `{recommended_skills}` hint block (generated skills were removed).

**2. Plan Waves — routed by Rust, the LLM relays:**

Do NOT read `wave-plan.md` or decide the wave order by hand. Run:

```bash
mustard-rt run dispatch-plan --spec {specName}
```

It returns a deterministic JSON array ordered by dependency level. Each item is `{wave, role, subproject, depends_on, level, prompt_cmd, subagent_type}`:

- **`level`** = dispatch round. Items sharing a `level` have no dependency between them → dispatch them together in ONE message (multiple `<invoke>` blocks). A higher `level` starts ONLY after every lower-level wave completes.
- NEVER nest dispatch — nesting breaks parallel execution.
- `resume-bootstrap` decides the **stage**; `dispatch-plan` decides the **wave routing**. The orchestrator is a relay over the array, not a planner.

**3. Dispatch Agent:**

For each item, run its `prompt_cmd` (a ready `mustard-rt run agent-prompt-render` invocation — never hand-assembled) and pass the **stdout** to the Task `prompt` with the item's **`subagent_type`** (the tool picks it per role: read-only roles run tool-restricted — `explore`→`Explore`, `review`/`qa`→`mustard-review`, `guards`→`mustard-guards`, so they physically cannot write; writing roles → `general-purpose`). Never pick the agent by hand. The rendered template carries the role contract + boundary + return cap inline, plus the spec's project section + its anchors.

**4. Validate:**

- Build passes (backend: `dotnet build`, frontend: `pnpm build`, mobile: `fvm flutter analyze`)
- Zero critical guard violations
- Checklist marking is automatic: `checklist-auto-mark.js` hook runs after every Edit/Write and marks the Checklist item that matches the file (incremental, silent). To make a Checklist item auto-markable, give it a file pista — either include the file basename in the item text (e.g. `- [ ] Validate UserService.cs`) or append a target arrow (e.g. `- [ ] Validate input → src/Services/UserService.cs`). Items without any pista won't auto-mark; close-gate will surface them at CLOSE.
- Any failure → retry (max 2/agent), then STOP + replan

**6. Review (MANDATORY — NEVER skip):**
Dispatch review agent for EACH affected subproject. The review agent reads `{subproject}/CLAUDE.md` — the `## Guards` section carries the subproject's DO/DON'T rules — and runs the full 7-category checklist:

1. **SOLID** — SRP, OCP, LSP, ISP, DIP
2. **Design System** — tokens, typography, spacing, components, icons, theme
3. **Patterns** — project conventions from the subproject's `## Guards`
4. **i18n** — all strings localized, all locale files updated
5. **Integration** — types synced, no orphans, no circular deps
6. **Build** — compiles/analyzes clean
7. **Elegance** — simplest solution, no over-engineering

APPROVED (zero CRITICAL) → CLOSE. REJECTED (any CRITICAL) → fix agent dispatched (max 2 fix loops), then re-review.

### CLOSE Phase (collapses old COMPLETE)

1. `mustard-rt run scan` (refresh `grain.model.json` if the codebase changed)
2. Update spec: `Status: completed`, `Phase: CLOSE`. Checklist must already be fully `[x]` from EXECUTE — `close-gate.js` blocks CLOSE if any `[ ]` remains in the Checklist section.
3. Run `mustard-rt run close-orchestrate --spec {name}`. When `overall == pass` it **auto-chains the finalize in-process** (flips the spec to `closed-followup`, emits + verifies `pipeline.complete`); the LLM does not call `complete-spec` itself. When `overall == fail` it is report-only — fix the failing gate and re-run. (Terminal archival of long-stale follow-ups is a separate hygiene sweep: `complete-spec --archive-stale`; no filesystem move — spec dir stays at `.claude/spec/{name}/`.)
4. Output with agent colors: `═══ PIPELINE COMPLETE — {name} | Agents: {n} ok | Files: {c} created, {m} modified ═══`

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

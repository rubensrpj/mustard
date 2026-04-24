# /feature - Feature Pipeline

> ALWAYS before making any change. Search on the web for the newest documentation and only implement if you are 100% sure it will work.

## Trigger

`/feature <feature-name>`

## Description

Starts the pipeline to implement a feature or enhancement. Self-contained: ANALYZE → PLAN phases. Light scope may include EXECUTE inline.

## Action

### Spec Hygiene (automatic, before ANALYZE)

Audit specs in `active/` before starting — steps 1-5: scan, verify completed/cancelled, handle in-progress via AskUserQuestion, no-active path.

→ See `../../../refs/feature/spec-hygiene.md`

### ANALYZE Phase

**Auto-sync (silent):** Run `node .claude/scripts/sync-detect.js`. If output shows any subproject with `hashChanged: true`, then run `node .claude/scripts/sync-registry.js`. Otherwise skip sync-registry entirely.

**Diff Context (automatic):** At the start of ANALYZE, PLAN, and EXECUTE, run `node .claude/scripts/diff-context.js --subproject {subproject_path}`. Save to `.claude/.pipeline-states/{specName}.diff-{subproject}.md`. Prepend the subproject diff to every subagent prompt (`## Current Git State\n{diff}\n\n## Your Task\n...`). Skip header if diff empty/missing. Never dispatch without attempting interpolation.

1. Read `.claude/pipeline-config.md` — agents, wave transitions, model selection
2. Grep `entity-registry.json` for the specific entity name — NEVER read the full JSON
3. Determine layers from signals:

| Signal | Layers |
|--------|--------|
| New field/column/relation | DB (+Backend/FE if visible) |
| New endpoint, business logic | Backend (+FE if visible) |
| New screen/component | Frontend (+Backend if new endpoint) |
| New CRUD / sub-entity | DB + Backend + Frontend |
| Refactoring, bug fix | Root cause layer(s) |

When in doubt → `AskUserQuestion`: "Which layers?"

#### Scope Detection

| Signal | → Scope |
|--------|---------|
| 1-2 layers, ≤5 files, known pattern, no new entity | **Light** |
| Entity in registry + modification (add field/column/endpoint/behavior) + ≤8 files, no new entity/table/enum | **Extended Light** |
| 3+ layers, 5+ files, new entity/CRUD, new pattern | **Full** |

Any **Full** signal → Full. All **Light** or **Extended Light** → skip PLAN. Record scope (`light`, `extended-light`, or `full`) for PLAN phase branching.

**Extended Light** = same flow as Light (skip PLAN, inline EXECUTE):
- Entity MUST exist in `entity-registry.json` (Grep confirms it)
- Operation modifies existing entity (NOT creates new one)
- Up to 8 files, up to 3 layers — pattern is known
- No new database table, no new enum type, no new module
- If ANY condition fails → reclassify as Full
- Reclassify to Full if >8 files surface during ANALYZE

Light/Extended Light scope CAN use Task(Explore) ONCE with ≤10 tool uses. Prefer Grep/Glob direct when targets are known. If >5 files surface during ANALYZE on Light, RECLASSIFY to Extended Light (if entity in registry) or Full.

#### Explore (conditional, budget-capped)

**File budget: MAX 5 reads total in ANALYZE phase (excludes registry/pipeline-config)**

**Path A — SKIP Explore agent** (DEFAULT when entity exists in registry): Entity in registry → skip Explore agent. Read 2-3 reference files directly. Go straight to PLAN.

**Path B — Explore agent ("medium")** (ONLY for genuinely new entities/patterns): Entity NOT in registry AND new CRUD/entity → use Explore agent. **Explorer cap: ≤20 tool uses, ≤3 full file reads.** After Explore returns → go straight to PLAN, ZERO additional reads. NEVER duplicate reads the Explore agent already performed.

**HARD RULE:** If you already understand the change, STOP reading and write the spec. More reads ≠ better spec.

#### Compact Advisory

After ANALYZE, if heavy exploration (>8 file reads, >3 Grep rounds, or multiple Explore agents):
Suggest: _"Analysis complete. Context is heavy — consider `/compact` before proceeding to implementation, then `/resume`."_ Advisory only.

### Decomposition Rule (Wave 7)

When ANALYZE surfaces >5 files, >3 architectural layers, or multiple independent sub-behaviors: STOP, decompose into child specs (2-5 children, each ≤5 files, ≤2 layers). Link via `spec-link.js`. Parent enters `COORDINATE` phase until all children reach CLOSE.

→ See `../../../refs/feature/wave-decomposition.md`

### End of ANALYZE — Validation

Run: `rtk node .claude/scripts/analyze-validation.js --spec .claude/spec/active/{specName}/spec.md`
If output `ok: false`, append each `issues[]` entry to the spec under `## Concerns` (non-blocking). Continue to PLAN regardless.

### PLAN Phase

#### Wave Decomposition Pre-Check (Full scope only)

Check whether the work should be decomposed into waves before writing a single spec. Signals: fileCount, layerCount, newEntityCount, knowledgeMatches. Runs `scope-decompose.js` + `wave-dependency.js`. Produces wave-plan.md + per-wave spec.md if decompose=true.

→ See `../../../refs/feature/wave-decomposition.md`

#### Full Scope

1. Create `.claude/spec/active/{date}-{name}/spec.md` with:
   - Summary, Entity Info, Files, Tasks, Dependencies
   - Tasks organized by `### {Agent} Agent (Wave {N})`
   - 3-8 checkboxed steps per agent, decomposed by operation type (NOT by file)
   - Mark `(parallel-safe)` on frontend tasks with no dependency on new backend endpoints
   - **MANDATORY: `## Acceptance Criteria` section** (Wave 10) — 3-8 binary, executable items: `- [ ] AC-1: {description} — Command: \`{exact command}\``. Each: exit 0 = pass; runnable from project root; focus on observable behavior (build, endpoint, test). Include `Testable, binary (pass/fail) criteria. Each MUST be executable and independent.` header line.
2. Add checkpoint fields: `Status: draft`, `Phase: PLAN`, `Scope: full`, `Checkpoint: {now}`
3. Create `.claude/.pipeline-states/{spec-name}.json`: `specName`, `status: "active"`, `phase: 2`, `phaseName: "PLAN"`, `scope: "full"`
4. Elegance Check: 3+ files or complex logic → "Is there a more elegant approach?"
5. **Present full spec to user:** Read spec file and print ENTIRE contents verbatim in a fenced markdown block. Add 1-line change summary (WHAT + WHY). Then `AskUserQuestion`: **"Approve and implement?"** / **"Adjust (give feedback)"** / **"Save for later (stop)"**.

#### Light Scope

1. Create `.claude/spec/active/{date}-{name}/spec.md` with compact format — headers: `# Enhancement: {name}`, `### Status: draft | Phase: PLAN | Scope: light`, `### Checkpoint: {ISO}`, `## Summary` (1-2 lines), `## Checklist` → `### {Agent} Agent` (steps + build/type-check), `## Files (~{N})` (paths), `## Acceptance Criteria` (1-3 items, `- [ ] AC-1: {description} — Command: \`{exact command}\``). At least AC-1 must verify the feature works.
2. Create `.claude/.pipeline-states/{spec-name}.json`: `specName`, `status: "active"`, `phase: 2`, `scope: "light"`
3. **Present full spec to user:** Print ENTIRE contents verbatim in fenced markdown block. Then `AskUserQuestion`: **"Approve and implement now"** / **"Approve for later"** / **"Adjust"**.

#### Spec Boundaries

Add `## Boundaries` section before writing tasks: list only paths intentionally touched (exact files, directories, globs). Out-of-boundary edits surface `[BOUNDARY WARNING]` — treat as signal, not error to suppress.

### Pre-EXECUTE Existence Gate (Full scope only)

Dispatch 1 Haiku Task(Explore) to verify work is still needed. Pre-check via `rtk git diff --stat` first (skip if <10 insertions/deletions). Decision: all-no → transparent; mixed → mark done tasks [x], re-dispatch for remaining; all-yes → mandatory AskUserQuestion before closing as already-implemented.

→ See `../../../refs/feature/existence-gate.md`

### EXECUTE Phase (Light scope — same session)

When user chooses "Approve and implement now":
1. Update spec: `Status: implementing`, `Phase: EXECUTE`. Every agent prompt MUST include: `Return format cap: ≤50 lines. Apply compact Return Format from .claude/pipeline-config.md strictly.`
2. Update pipeline state: `status: "implementing"`, `phase: 3`
3. Read `.claude/pipeline-config.md` for agent config. Grep `entity-registry.json` for specific entity block only
4. Match recipes by title via Grep on `{subproject}/.claude/commands/recipes.md` — do NOT read full file
4b. **Structured Recipe (if available):** Run `node .claude/scripts/recipe-match.js --entity {entity} --operation {operation} --subproject {subproject_path}`. If non-empty JSON, inject into agent prompt as `{recipe_context}`. Gives agent a 90%-complete skeleton.
5. Identify relevant skills for `{recommended_skills}`: list skill names most relevant to the task
6. Dispatch agents (wave rules: DB+Backend parallel, Frontend after Backend UNLESS `(parallel-safe)`)
7. Wave transitions between waves (from `.claude/pipeline-config.md`)
8. On return: validate (build/type-check), update spec `[ ]` → `[x]` (line-by-line edits, NEVER copy entire spec blocks)
8b. **Agent Memory:** `node .claude/scripts/memory-write.js --json '{"agent_type":"{type}","wave":{N},"pipeline":"{spec-name}","summary":"{what}","details":{...}}'` — one per agent. Skip if single-wave pipeline.

#### Escalation Status Handling

After each agent returns, check for escalation before advancing:

- **Internal error** — re-dispatch sequentially (not parallel). Max 1 Internal retry per agent
- `CONCERN` — record verbatim under `## Concerns`; continue to next step
- `BLOCKED` — stop immediately; AskUserQuestion with exact blocker; do NOT retry or advance
- `PARTIAL` — apply Granular Retry Protocol from last completed step; do NOT restart from step 1
- `DEFERRED` — note in spec with agent justification; ask user if deferred item is load-bearing before closing

If two or more agents in same wave return `CONCERN`, surface all concerns together before starting next wave. See `.claude/pipeline-config.md` Escalation Statuses and Diagnostic Failure Routing.

9. **REVIEW** — dispatch review agent per affected subproject (guards + relevant skills, 7-category checklist). REJECTED → see `resume/SKILL.md § Fix Loop Dispatch Protocol` (max 2 loops). Re-reviews always use `model: "sonnet"`.
10. All passed + APPROVED → CLOSE flow inline (sync registry, move spec, cleanup state)
11. Failed → max 2 retries, then STOP + report

#### Failure Routing

Classify before retrying: (1) **Transient?** → retry once immediately. (2) **Resolvable?** (≤3-line patch, no new reads) → apply patch, retry (counts as retry 1). (3) **Structural?** (spec assumed false) → re-analyze 1-2 files, update spec, re-dispatch — does NOT count against 2-retry cap. Retry cap applies to Transient + Resolvable only.

### QA Phase (Wave 10)

After all EXECUTE tasks complete: (1) set `phaseName: "QA"` in pipeline state. (2) Run `node .claude/scripts/qa-run.js --spec {specName}`. (3) `overall=pass` → CLOSE; `overall=fail` → return failing AC list to implementation agent, re-run; `overall=skip` (no AC) → warn + allow CLOSE. Max 3 QA iterations — then `AskUserQuestion`: "QA has failed 3 times. Choose: (a) Fix manually and retry, (b) Relax the AC, (c) Abort pipeline."

Update `## Acceptance Criteria` checkboxes: `[x]` passed, `[ ]` failed. Visual: `[v] ANALYZE  [v] PLAN  [v] EXECUTE  [>] QA  [ ] CLOSE`

## Visual Output

Progress: `[v] ANALYZE  [>] PLAN  [ ] EXECUTE  [ ] QA  [ ] CLOSE` — add `[LIGHT]` or `[FULL]` scope tag after progress line.

## Spec Layout

Specs may grow beyond a manageable size. Apply the same progressive disclosure pattern used in skills:

- **Default:** single `spec.md` (Light OR small Full ≤200 lines).
- **When spec.md > 200 lines:** extract autonomous sections to `spec-references/{section}.md` in the SAME spec directory; spec.md body keeps `→ See spec-references/{section}.md` pointers.
- **Hard block at 500 lines:** gate `MUSTARD_SPEC_SIZE_MODE=strict` (default `warn`) — at warn, log `[SPEC-SIZE] {name} is {N} lines; consider splitting`; at strict, block PLAN from writing a spec exceeding 500 lines.
- **Wave plans already follow this pattern:** `wave-plan.md` + per-wave `spec.md` directories are the canonical multi-file spec form.
- **Reference:** Anthropic progressive disclosure (skill-creator best practices) — same principle: load detail on demand, keep body scannable.

### Acceptance Criteria — Cross-Shell Pattern

Write AC commands in portable form: prefer `node -e "..."` for multi-step assertions, `bash -c '...'` for shell pipes, or single commands where exit code is the verdict. Avoid raw bash syntax (`for`, `test`, `[...]`) — cmd.exe silently mishandles it on Windows.

→ See `../../../refs/feature/ac-cross-shell.md`

## Rules
- This command is self-contained — reads `.claude/pipeline-config.md` directly
- NEVER implement code in Full scope — only PLAN. EXECUTE via `/approve` + `/resume` (or `/approve --resume` to chain inline)
- NEVER launch Explore agent when entity already exists in registry — read 2-3 files directly
- NEVER read additional files after Explore agent returns — its output is final
- NEVER exceed 5 file reads in ANALYZE phase (registry + pipeline-config are free)
- Light scope + user chose "implement now" → proceed to EXECUTE inline
- ALWAYS read `.claude/pipeline-config.md` for agent/wave/model info
- ALWAYS create pipeline state at PLAN phase
- ALWAYS record `scope` in spec header AND pipeline state
- ALWAYS go straight to PLAN once you understand the change — more reads ≠ better spec
- Light scope inline implement follows same dispatch rules as `/resume` (template, waves, retries)
- Context budget: Grep entity-registry (not full read), Grep recipes (not full read), line-by-line checkbox updates
- Wave decomposition is opt-in via signals (knowledge matches, layer/file/entity counts) — never force waves on small scopes
- If wave decomposition is approved, single-spec Full Scope flow is skipped — waves execute sequentially via `/resume`
ULTRATHINK

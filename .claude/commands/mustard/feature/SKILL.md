# /feature - Feature Pipeline

> ALWAYS before making any change. Search on the web for the newest documentation and only implement if you are 100% sure it will work.

## Trigger

`/feature <feature-name>`

## Description

Starts the pipeline to implement a feature or enhancement. Self-contained: ANALYZE → PLAN phases. Light scope may include EXECUTE inline.

## Action

### Spec Hygiene (automatic, before ANALYZE)

Before starting a new pipeline, audit specs in `active/`:

1. **Scan** all specs in `.claude/spec/active/*/spec.md`
2. **For each spec**, read the full header and checklist to extract `Status:`, `Phase:`, and checkbox completion (`[x]` vs `[ ]`)
3. **Verify completed/cancelled specs before moving:**
   - If `Status: completed` or `Status: cancelled`:
     - **Analyze first**: check that ALL checklist items are `[x]`, no `## Concerns` with unresolved `BLOCKED` items, and build/type-check references are satisfied
     - If analysis confirms done → move from `.claude/spec/active/{name}/` to `.claude/spec/completed/{name}/`, delete `.claude/.pipeline-states/{name}.json` and `.diff.md` if they exist, log: `[HYGIENE] Verified and moved {name} → completed/`
     - If analysis finds incomplete items → update `Status: implementing`, log: `[HYGIENE] {name} marked completed but has {N} unchecked items — reverted to implementing`, then treat as in-progress (step 4)
4. **In-progress specs** (`Status: draft` or `Status: implementing`):
   - Use `AskUserQuestion`: _"Found spec in progress: **{name}** (Status: {status}, Phase: {phase}, {done}/{total} tasks done). Do you want to continue this spec before starting a new one?"_
   - If **yes** → stop, suggest `/resume` to continue the existing spec
   - If **no** → proceed to ANALYZE for the new pipeline (existing spec stays in `active/`)
5. **No active specs** → proceed to ANALYZE normally

This step is silent when there's nothing to audit — no output if `active/` is empty.

### ANALYZE Phase

**Auto-sync (silent):** Run `node .claude/scripts/sync-detect.js`. If output shows any subproject with `hashChanged: true`, then run `node .claude/scripts/sync-registry.js`. Otherwise skip sync-registry entirely.

### Diff Context (automatic)

**Diff snapshot (run once per phase, per subproject):**
Run `node .claude/scripts/diff-context.js --subproject {subproject_path}` at the start of ANALYZE, PLAN, and EXECUTE. Save the output to `.claude/.pipeline-states/{specName}.diff-{subproject}.md` (overwrite each phase). Generate one diff per subproject involved in the pipeline. For orchestrator-level decisions, run without `--subproject` for the global view.

**Inject into every Task dispatch in this pipeline:**
Prepend the subproject-specific diff (NOT the global diff) to EVERY subagent prompt dispatched during the pipeline:

```
## Current Git State
{contents of .claude/.pipeline-states/{specName}.diff.md}

## Your Task
...original prompt...
```

If the diff file is empty or missing, skip the Git State header entirely. Never dispatch an agent without attempting interpolation.

1. Read `.claude/pipeline-config.md` — agents, wave transitions, model selection
2. Read `entity-registry.json` via Grep for the specific entity name (e.g. `"Contract":`) — NEVER read the full JSON. Entity found? infer layers. Not found? all layers.
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

Classify based on ANALYZE output:

| Signal | → Scope |
|--------|---------|
| 1-2 layers, ≤5 files, known pattern, no new entity | **Light** |
| Entity in registry + modification (add field/column/endpoint/behavior) + ≤8 files, no new entity/table/enum | **Extended Light** |
| 3+ layers, 5+ files, new entity/CRUD, new pattern | **Full** |

Any **Full** signal → Full. All **Light** or **Extended Light** → skip PLAN.
Record scope (`light`, `extended-light`, or `full`) for PLAN phase branching.

**Extended Light** = same flow as Light (skip PLAN, inline EXECUTE):
- Entity MUST exist in `entity-registry.json` (Grep confirms it)
- Operation modifies existing entity (NOT creates new one)
- Up to 8 files, up to 3 layers — pattern is known
- No new database table, no new enum type, no new module
- If ANY condition fails → reclassify as Full
- Reclassify to Full if >8 files surface during ANALYZE

- Light/Extended Light scope CAN use Task(Explore) ONCE with ≤10 tool uses. Prefer Grep/Glob direct when targets are known.
- If >5 files surface during ANALYZE on Light, RECLASSIFY to Extended Light (if entity in registry) or Full.

#### Explore (conditional, budget-capped)

**File budget: MAX 5 reads total in ANALYZE phase (excludes registry/pipeline-config)**

**Path A — SKIP Explore agent** (DEFAULT when entity exists in registry):
- Entity in registry → ALWAYS skip Explore agent
- Read 2-3 reference files directly (the files you'll actually modify)
- Go straight to PLAN

**Path B — Explore agent ("medium")** (ONLY for genuinely new entities/patterns):
- Entity NOT in registry AND new CRUD/entity → use Explore agent
- **Explorer cap: ≤20 tool uses, ≤3 full file reads** — prefer Grep over Read
- After Explore returns → go straight to PLAN, ZERO additional reads
- NEVER duplicate reads the Explore agent already performed

**HARD RULE:** If you already understand the change (which files, which pattern), STOP reading and write the spec. More reads ≠ better spec.

#### Compact Advisory
After ANALYZE completes, if the analysis required heavy exploration (>8 file reads, >3 Grep rounds, or multiple Explore agents):
- Suggest to user: _"Analysis complete. Context is heavy — consider `/compact` before we proceed to implementation, then `/resume`."_
- This is advisory only — proceed immediately if user declines or ignores.

### End of ANALYZE — Validation

Run: `rtk node .claude/scripts/analyze-validation.js --spec .claude/spec/active/{specName}/spec.md`
If output `ok: false`, append each `issues[]` entry to the spec under `## Concerns` (non-blocking).
Continue to PLAN regardless.

### PLAN Phase

#### Full Scope

1. Create `.claude/spec/active/{date}-{name}/spec.md` with:
   - Summary, Entity Info, Files, Tasks, Dependencies
   - Tasks organized by `### {Agent} Agent (Wave {N})`
   - 3-8 checkboxed steps per agent, decomposed by operation type (NOT by file)
   - If a frontend task has NO dependency on new backend endpoints or types, mark it as `(parallel-safe)` in the spec header:
     `### Frontend Agent (Wave 1, parallel-safe)`
     This allows the orchestrator to dispatch it alongside backend in Wave 1.
2. Add checkpoint fields: `Status: draft`, `Phase: PLAN`, `Scope: full`, `Checkpoint: {now}`
3. Create `.claude/.pipeline-states/{spec-name}.json`: `specName`, `status: "active"`, `phase: 2`, `phaseName: "PLAN"`, `scope: "full"`
4. Elegance Check: 3+ files or complex logic → "Is there a more elegant approach?"
5. **Present full spec to user before asking for approval:**
   - Read `.claude/spec/active/{date}-{name}/spec.md` (the spec just written) and print its ENTIRE contents verbatim inside a fenced markdown block (```` ```markdown ... ``` ````). Do NOT summarize, truncate, or paraphrase — the user asked to read the complete plan before approving.
   - After the fenced block, add a 1-line change summary (WHAT + WHY) to orient the reader.
   - Then `AskUserQuestion`: **"Approve and implement?"** / **"Adjust (give feedback)"** / **"Save for later (stop)"**.

#### Light Scope

1. Create `.claude/spec/active/{date}-{name}/spec.md` with compact format:
   ```
   # Enhancement: {name}
   ### Status: draft | Phase: PLAN | Scope: light
   ### Checkpoint: {ISO}

   ## Summary
   {1-2 lines: what + why}

   ## Checklist
   ### {Agent} Agent
   - [ ] {step}
   - [ ] Build/type-check

   ## Files (~{N})
   - `path/to/file.ext` (create|modify)
   ```
2. Create `.claude/.pipeline-states/{spec-name}.json`: `specName`, `status: "active"`, `phase: 2`, `scope: "light"`
3. **Present full spec to user before asking for approval:**
   - Read the spec file just written and print its ENTIRE contents verbatim inside a fenced markdown block. Do NOT summarize — Light scope specs are already compact, so the full print is cheap and the user asked to read the complete plan before approving.
   - Then `AskUserQuestion`:
     - **"Approve and implement now"** → Phase 3 inline (same session)
     - **"Approve for later"** → stop, user runs `/approve` + `/resume` (or `/approve --resume` to chain inline)
     - **"Adjust"** → user gives feedback

#### Spec Boundaries

Before writing spec tasks, identify and record which files/directories are in scope. Add a `## Boundaries` section to the spec:

```
## Boundaries
- `path/to/directory/` — directory scope (all files within)
- `path/to/file.ext` — exact file
- `**/*.controller.ts` — glob pattern
```

Rules:
- Only list paths the feature **intentionally** touches
- Be specific: prefer exact files over broad directories when the change is known
- Out-of-boundary edits during EXECUTE will surface a `[BOUNDARY WARNING]` from guard-verify — treat as a signal to re-evaluate, not an error to suppress

### Pre-EXECUTE Existence Gate (Full scope only)

**Skip conditions**: Light scope OR `## Files` section lists more than 8 files (cost-benefit inverts — Haiku 10-tool-use cap will not cover).

Before dispatching implementation agents, run 1 Haiku explorer to verify the work is still needed.

**Pre-check (free, zero LLM tokens)**: Before dispatching Haiku, run:

```bash
rtk git diff --stat HEAD -- <files listed in `## Files` of spec>
```

Skip rules based on pre-check output:
- **Empty output** (no changes) → skip gate entirely, proceed to EXECUTE normally (nothing to verify)
- **<10 total insertions/deletions** → skip gate entirely, proceed to EXECUTE normally (trivial changes, verification not worth the overhead)
- **≥10 insertions/deletions** → proceed with Haiku dispatch below

**Dispatch:**

```javascript
Task({
  subagent_type: "Explore",
  model: "haiku",
  description: "Pre-EXECUTE existence check",
  prompt: `# EXISTENCE CHECK
Read .claude/spec/active/{specName}/spec.md sections: "## Files" and "## Checklist".

For EACH checklist task (task-level, NOT file-level):
  1. Extract 1-3 concrete identifiers from the task text — function names, component names, file path fragments, string literals.
     Example: task "Add LogoutButton component with handleLogout handler" → identifiers: ["LogoutButton", "handleLogout"].
  2. Identify target files for the task from "## Files" (match by extension, name hint, or task context).
  3. Grep each target file for the identifiers.
  4. Verdict for this task:
     - ALL target files contain a MAJORITY of identifiers → all_present=yes
     - SOME do, SOME do not → all_present=partial
     - NONE do → all_present=no

Return a markdown table:
| task | target_files | all_present | evidence |
|------|--------------|-------------|----------|
| <task text> | <comma-sep files> | yes/partial/no | <identifier:line or "none"> |

Return ≤20 lines total. Self-cap: ≤10 tool uses (the tool-use budget is the true limit, not the task count).`
})
```

**Decision after return (orchestrator inspects the returned table):**

- **All tasks `all_present=no`** → Gate is transparent. Proceed to EXECUTE normally.
- **Mixed** (any combination that is NOT all-no AND NOT all-yes — includes all-partial, yes+no, partial+no, yes+partial, yes+partial+no) → Edit the spec: mark `[x]` on tasks where `all_present=yes`. Leave `[ ]` on `partial` and `no` (both require re-dispatch). Re-dispatch EXECUTE only for tasks still `[ ]`. Keep the original scope (Light/Full). Do NOT invent a new "PARTIAL" state.
- **All tasks `all_present=yes`** → **MANDATORY user surface** via `AskUserQuestion`: _"Pre-EXECUTE Existence Gate detected all N tasks already implemented. Evidence: {inline table}. Choose: (a) Close as already-implemented, (b) Force EXECUTE anyway (the gate may be wrong), (c) Abort pipeline."_ Never silently skip EXECUTE.

### EXECUTE Phase (Light scope — same session)

When user chooses "Approve and implement now":
1. Update spec: `Status: implementing`, `Phase: EXECUTE`
   Every agent prompt dispatched in Light scope MUST include:
   `Return format cap: ≤50 lines. Apply compact Return Format from .claude/pipeline-config.md strictly.`
2. Update pipeline state: `status: "implementing"`, `phase: 3`
3. Read `.claude/pipeline-config.md` for agent config. For `entity-registry.json`: Grep for specific entity block only
4. Match recipes by title via Grep on `{subproject}/.claude/commands/recipes.md` — do NOT read full file. Extract recipe number + pattern refs
4b. **Structured Recipe (if available):** Run `node .claude/scripts/recipe-match.js --entity {entity} --operation {operation} --subproject {subproject_path}`. If output is non-empty JSON, inject into agent prompt as `{recipe_context}`:
    ```
    ## RECIPE (follow this pattern — fill in specifics)
    {recipe_output}
    ```
    This gives the agent a 90%-complete skeleton — it fills in concrete values instead of reasoning about architecture. If no recipe matches, `{recipe_context}` is empty (omit section).
5. Identify relevant skills for `{recommended_skills}`: list skill names most relevant to the task (e.g., `api-endpoint-wiring, api-dto-validation`). Agents use these as hints — Claude natively decides which to load based on descriptions
6. Dispatch agents (wave rules: DB+Backend parallel, Frontend after Backend UNLESS spec marks task as `(parallel-safe)` — see `.claude/pipeline-config.md` Parallel Rules). Agent prompt includes `{recommended_skills}` as skill hints — agents read SKILL.md of relevant skills before implementing
7. Wave transitions between waves (from `.claude/pipeline-config.md`)
8. On return: validate (build/type-check), update spec `[ ]` → `[x]` (line-by-line edits, NEVER copy entire spec blocks as old_string)
8b. **Agent Memory:** After agents return and spec is updated, write agent memory: `node .claude/scripts/memory-write.js --json '{"agent_type":"{type}","wave":{N},"pipeline":"{spec-name}","summary":"{what agent did}","details":{...}}'` — one per agent. Skip if single-wave pipeline (no downstream agents to benefit).

#### Escalation Status Handling

After each agent returns, check the return value for an escalation status before advancing:

- **Internal error** (no parseable output, empty return, API error) — re-dispatch **sequentially** (not parallel) with same prompt. Max 1 Internal retry per agent
- `CONCERN` — record verbatim under `## Concerns` in the spec; continue to next step
- `BLOCKED` — stop immediately; use `AskUserQuestion` to report the exact blocker; do NOT retry or advance
- `PARTIAL` — apply Granular Retry Protocol from the last completed step; do NOT restart from step 1
- `DEFERRED` — note in spec with agent justification; ask user if the deferred item is load-bearing before closing

If two or more agents in the same wave return `CONCERN`, surface all concerns together before starting the next wave. See `.claude/pipeline-config.md` Escalation Statuses and Diagnostic Failure Routing for the full status table.

9. **REVIEW** — dispatch review agent for each affected subproject (reads guards + relevant skills, runs 7-category checklist: SOLID, Design System, Patterns, i18n, Integration, Build, Elegance). REJECTED → fix + re-review (max 2 loops).

   Re-reviews always dispatch with `model: "sonnet"` (see `review/SKILL.md § Model Selection`).
10. All passed + APPROVED → CLOSE flow inline (sync registry, move spec, cleanup state)
11. Failed → max 2 retries, then STOP + report

#### Failure Routing

Before retrying, classify the failure with 3 questions:

1. **Transient?** — Would re-running succeed without any change? → Retry once immediately.
2. **Resolvable?** — Is the fix clear and patchable in ≤3 lines without new reads? → Apply patch, retry (counts as retry 1).
3. **Structural?** — Did the spec assume something false about structure or layer? → Re-analyze (read 1-2 key files), update spec, re-dispatch. Does NOT count against the 2-retry cap.

Retry cap applies to Transient + Resolvable only. Structural failures reset the attempt after spec correction.

## Visual Output

Progress: `[v] ANALYZE  [>] PLAN  [ ] EXECUTE  [ ] CLOSE`
Scope tag: `[LIGHT]` or `[FULL]` after progress line.

## Rules
- This command is self-contained — reads `.claude/pipeline-config.md` directly
- NEVER implement code in Full scope — only PLAN. EXECUTE via `/approve` + `/resume` (or `/approve --resume` to skip the session hop)
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

ULTRATHINK

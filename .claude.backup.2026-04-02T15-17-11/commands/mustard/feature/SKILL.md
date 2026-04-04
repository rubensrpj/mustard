# /feature - Feature Pipeline

> ALWAYS before making any change. Search on the web for the newest documentation and only implement if you are 100% sure it will work.

## Trigger

`/feature <feature-name>`

## Description

Starts the pipeline to implement a feature or enhancement. Self-contained: ANALYZE → PLAN phases. Light scope may include EXECUTE inline.

## Action

### ANALYZE Phase

**Auto-sync (silent):** `node .claude/scripts/sync-registry.js`

### Diff Context (automatic)
Run `node .claude/scripts/diff-context.js` to capture the current git state. Include the output in the agent prompt as `{diff_context}` so agents know what has already changed.

1. Read `pipeline-config.md` — agents, wave transitions, model selection
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
| 3+ layers, 5+ files, new entity/CRUD, new pattern | **Full** |

Any **Full** signal → Full. All **Light** → Light.
Record scope for PLAN phase branching.

#### Explore (conditional, budget-capped)

**File budget: MAX 5 reads total in ANALYZE phase (excludes registry/pipeline-config)**

**Path A — SKIP Explore agent** (DEFAULT when entity exists in registry):
- Entity in registry → ALWAYS skip Explore agent
- Read 2-3 reference files directly (the files you'll actually modify)
- Go straight to PLAN

**Path B — Explore agent ("medium")** (ONLY for genuinely new entities/patterns):
- Entity NOT in registry AND new CRUD/entity → use Explore agent
- After Explore returns → go straight to PLAN, ZERO additional reads
- NEVER duplicate reads the Explore agent already performed

**HARD RULE:** If you already understand the change (which files, which pattern), STOP reading and write the spec. More reads ≠ better spec.

#### Compact Advisory
After ANALYZE completes, if the analysis required heavy exploration (>8 file reads, >3 Grep rounds, or multiple Explore agents):
- Suggest to user: _"Analysis complete. Context is heavy — consider `/compact` before we proceed to implementation, then `/resume`."_
- This is advisory only — proceed immediately if user declines or ignores.

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
5. Present to user with change summary (WHAT + WHY) → "Approve and implement?" or "Save for later?"

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
3. Present to user → `AskUserQuestion`:
   - **"Approve and implement now"** → Phase 3 inline (same session)
   - **"Approve for later"** → stop, user runs `/approve` + `/resume`
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

### EXECUTE Phase (Light scope — same session)

When user chooses "Approve and implement now":
1. Update spec: `Status: implementing`, `Phase: EXECUTE`
2. Update pipeline state: `status: "implementing"`, `phase: 3`
3. Read `pipeline-config.md` for agent config. For `entity-registry.json`: Grep for specific entity block only
4. Match recipes by title via Grep on `{subproject}/.claude/commands/recipes.md` — do NOT read full file. Extract recipe number + pattern refs
5. Identify relevant skills for `{recommended_skills}`: list skill names most relevant to the task (e.g., `api-endpoint-wiring, api-dto-validation`). Agents use these as hints — Claude natively decides which to load based on descriptions
6. Dispatch agents (wave rules: DB+Backend parallel, Frontend after Backend UNLESS spec marks task as `(parallel-safe)` — see `pipeline-config.md` Parallel Rules). Agent prompt includes `{recommended_skills}` as skill hints — agents read SKILL.md of relevant skills before implementing
7. Wave transitions between waves (from `pipeline-config.md`)
8. On return: validate (build/type-check), update spec `[ ]` → `[x]` (line-by-line edits, NEVER copy entire spec blocks as old_string)
8b. **Agent Memory:** After agents return and spec is updated, write agent memory: `echo '{"agent_type":"{type}","wave":{N},"pipeline":"{spec-name}","summary":"{what agent did}","details":{...}}' | node .claude/scripts/memory-write.js` — one per agent. Skip if single-wave pipeline (no downstream agents to benefit).

#### Escalation Status Handling

After each agent returns, check the return value for an escalation status before advancing:

- `CONCERN` — record verbatim under `## Concerns` in the spec; continue to next step
- `BLOCKED` — stop immediately; use `AskUserQuestion` to report the exact blocker; do NOT retry or advance
- `PARTIAL` — apply Granular Retry Protocol from the last completed step; do NOT restart from step 1
- `DEFERRED` — note in spec with agent justification; ask user if the deferred item is load-bearing before closing

If two or more agents in the same wave return `CONCERN`, surface all concerns together before starting the next wave. See `pipeline-config.md` Escalation Statuses for the full status table.

9. **REVIEW** — dispatch review agent for each affected subproject (reads guards + relevant skills, runs 7-category checklist: SOLID, Design System, Patterns, i18n, Integration, Build, Elegance). REJECTED → fix + re-review (max 2 loops)
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
- This command is self-contained — reads `pipeline-config.md` directly
- NEVER implement code in Full scope — only PLAN. EXECUTE via `/approve` + `/resume`
- NEVER launch Explore agent when entity already exists in registry — read 2-3 files directly
- NEVER read additional files after Explore agent returns — its output is final
- NEVER exceed 5 file reads in ANALYZE phase (registry + pipeline-config are free)
- Light scope + user chose "implement now" → proceed to EXECUTE inline
- ALWAYS read `pipeline-config.md` for agent/wave/model info
- ALWAYS create pipeline state at PLAN phase
- ALWAYS record `scope` in spec header AND pipeline state
- ALWAYS go straight to PLAN once you understand the change — more reads ≠ better spec
- Light scope inline implement follows same dispatch rules as `/resume` (template, waves, retries)
- Context budget: Grep entity-registry (not full read), Grep recipes (not full read), line-by-line checkbox updates

ULTRATHINK

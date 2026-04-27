# Wave Decomposition Reference

> Detail for `/feature` — Wave Decomposition Pre-Check (Full scope only) and COORDINATE phase.

#### Wave Decomposition Pre-Check (Full scope only)

**Skip for Light/Extended Light** — decomposition only makes sense when scope is genuinely large.

Before writing the single spec in Full scope, check whether the work should be decomposed into waves:

1. **Compute signals from ANALYZE output:**
   - `fileCount` — files that will go into `## Files`
   - `layerCount` — distinct layers (use role detection derived from paths: schema/api/ui/lib). **`layerCount >= 2` is sufficient to trigger decomposition** regardless of fileCount.
   - `newEntityCount` — new entities created by this spec
   - `estimatedTouchPoints` — count of imports/refs from Grep on affected directories (optional)

   Decomposition reasons emitted: `history-match:{id}`, `multi-layer`, `wide-and-new-entities`. Single-layer specs return `decompose: false` with reason `single-layer`.

2. **Read knowledge matches:** Read `.claude/knowledge.json` (if it exists). Extract entries whose `id` starts with `heavy-pipeline` or `high-hook-retry`. Each entry's scope signals represent a historical pipeline that cost a lot.

3. **Run decomposition decision:**
   ```bash
   echo '{"fileCount":{N},"layerCount":{L},"newEntityCount":{E},"knowledgeMatches":[...]}' | node .claude/scripts/scope-decompose.js
   ```
   Output JSON: `{decompose: bool, reason: string, signals: {...}}`

4. **If `decompose: false`** → proceed to `#### Full Scope` below as usual (single spec).

5. **If `decompose: true`** → build wave plan:
   ```bash
   echo '{"files":[...all paths from ANALYZE...],"projectRoot":"."}' | node .claude/scripts/wave-dependency.js
   ```
   Output cases:
   - `{error: "cyclic-dependency", cycle: [...]}` → warn user about cyclic imports (pre-existing architecture issue), fall back to single spec with note in `## Concerns`. Proceed to `#### Full Scope`.
   - `{error: ...}` → fail-open: fall back to single spec.
   - `{waves: [...]}` with only 1 wave → no real DAG depth, fall back to single spec.
   - `{waves: [...]}` with 2+ waves → write **Wave Plan** (step 6).

6. **Write Wave Plan structure:**
   ```
   .claude/spec/active/{date}-{name}/
     ├── wave-plan.md
     ├── wave-1-{role}/spec.md
     ├── wave-2-{role}/spec.md
     └── wave-N-{role}/spec.md
   ```

   `wave-plan.md` contains:
   ```markdown
   # Wave Plan: {name}
   ### Status: draft | Phase: PLAN | Scope: full | Decomposed: yes
   ### Checkpoint: {ISO now}
   ### Reason: {decompose.reason}

   ## Summary
   {1-2 lines: what + why}

   ## Waves
   ### Wave 1 — {roles of wave 1}
   Depends on: none
   Files ({count}): {file1}, {file2}, ...

   ### Wave 2 — {roles of wave 2}
   Depends on: wave 1
   Files ({count}): {file3}, ...

   {... for each wave ...}

   ## Rationale
   {which knowledge entry matched or which threshold triggered; signals from scope-decompose}
   ```

   Each `wave-N-{role}/spec.md` is a **complete atomic spec** scoped to just that wave's files. Use the same template as Full scope single spec (Summary, Entity Info, Files, Tasks, Dependencies, Boundaries). Reference `../wave-plan.md` at the top as context.

7. **Write pipeline state for wave plan:**
   ```json
   {
     "specName": "{date}-{name}",
     "status": "draft",
     "phase": 2,
     "phaseName": "PLAN",
     "scope": "full",
     "isWavePlan": true,
     "currentWave": 1,
     "totalWaves": N,
     "completedWaves": [],
     "failedWaves": []
   }
   ```

8. **Present wave plan to user:**
   - Read `wave-plan.md` and print its ENTIRE contents verbatim inside a fenced markdown block.
   - Also list each wave's spec file paths (one line each) so the user can open individual wave specs if desired.
   - Then `AskUserQuestion`:
     - **"Approve wave plan and implement now"** → goes to EXECUTE wave 1 inline (same rules as Light inline)
     - **"Approve wave plan for later"** → stop, user runs `/approve` + `/resume`
     - **"Edit decomposition (hint PLAN)"** → user provides hint (e.g., "merge waves 2 and 3"), PLAN reexecutes with the hint appended to `estimatedTouchPoints`/manual grouping. Re-decompose once.
     - **"Reject decomposition — use single spec"** → discard wave plan files, set `scopeOverride: "user-rejected-waves"` in pipeline state, proceed to `#### Full Scope` as if `decompose: false`.

9. **If user approves the wave plan**, the single-spec `#### Full Scope` flow below is **skipped** — wave-1 becomes the first thing to execute (via `/approve --resume` or `/resume`).

#### COORDINATE phase (parent specs)

A spec with `children_specs.length > 0` may enter `COORDINATE`. In this phase the orchestrator tracks children progress — it does NOT implement. Update `.claude/.pipeline-states/{epic}.json` to `phase: "COORDINATE"` after linking. When all children = CLOSE, update parent to `phase: "CLOSE"`.

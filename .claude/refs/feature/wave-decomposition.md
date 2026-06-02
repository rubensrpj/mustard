# Wave Decomposition Reference

> Detail for `/feature` — Wave Decomposition Pre-Check (Full scope only) and COORDINATE phase.

#### Wave Decomposition Pre-Check (Full scope only)

**Skip for Light/Extended Light** — decomposition only makes sense when scope is genuinely large.

**Invariant — Full ⇒ ≥1 wave** (parent = orchestrator, wave = subagent). This pre-check decides **1-vs-N** waves, never whether to have a wave at all. `decompose: false` still yields a wave plan with a **single** wave (`totalWaves: 1`); it never collapses to a non-wave spec. `scope-decompose` owns the 1-vs-N signal — do not hard-code thresholds here.

Before building the wave plan in Full scope, check whether the work warrants more than one wave:

1. **Compute signals from ANALYZE output:**
   - `fileCount` — files that will go into `## Files`
   - `layerCount` — distinct layers (use role detection derived from paths: schema/api/ui/lib). **`layerCount >= 2` is sufficient to trigger decomposition** regardless of fileCount.
   - `newEntityCount` — new entities created by this spec
   - `estimatedTouchPoints` — count of imports/refs from Grep on affected directories (optional)

   Decomposition reasons emitted: `history-match:{id}`, `multi-layer`, `wide-and-new-entities`. Single-layer specs return `decompose: false` with reason `single-layer`.

2. **Read knowledge matches:** Query `mustard-rt run event-projections --view knowledge-list` and filter entries whose `id` starts with `heavy-pipeline` or `high-hook-retry`. Each entry's scope signals represent a historical pipeline that cost a lot.

3. **Run decomposition decision:**
   ```bash
   echo '{"fileCount":{N},"layerCount":{L},"newEntityCount":{E},"knowledgeMatches":[...]}' | mustard-rt run scope-decompose
   ```
   Output JSON: `{decompose: bool, reason: string, signals: {...}}`

4. **If `decompose: false`** → build a **single-wave** plan (`totalWaves: 1`) — the parent spec stays the orchestration doc, wave-1 is the executing subagent. NEVER a non-wave Full spec (Full ⇒ ≥1 wave).

5. **If `decompose: true`** → build a **multi-wave** plan:
   ```bash
   echo '{"files":[...all paths from ANALYZE...],"projectRoot":"."}' | mustard-rt run wave-dependency
   ```
   Output cases:
   - `{error: "cyclic-dependency", cycle: [...]}` → warn user about cyclic imports (pre-existing architecture issue), fall back to a single-wave plan with note in `## Concerns`.
   - `{error: ...}` → fail-open: fall back to a single-wave plan.
   - `{waves: [...]}` with only 1 wave → no real DAG depth, single-wave plan.
   - `{waves: [...]}` with 2+ waves → write a multi-wave **Wave Plan** (step 6).

6. **Write Wave Plan structure:**
   ```
   .claude/spec/{date}-{name}/
     ├── wave-plan.md
     ├── wave-1-{role}/spec.md
     ├── wave-2-{role}/spec.md
     └── wave-N-{role}/spec.md
   ```

   `wave-plan.md` is pure narrative — a title followed by `## Summary` (1-2 lines what + why), `## Waves` (per-wave H3 with Depends on + Files list), and `## Rationale` (knowledge entry matched / threshold triggered / signals from scope-decompose). Its lifecycle metadata (`stage: Plan`, `outcome: Active`, `scope: full (wave plan)`, `isWavePlan: true`, `totalWaves`, `checkpoint`) lives in the `meta.json` sidecar, written by `mustard-rt run wave-scaffold` — never as `### Key:` headers in the markdown.

   Each `wave-N-{role}/spec.md` is a **complete atomic spec** scoped to just that wave's files. Use the same template as Full scope single spec (Summary, Entity Info, Files, Tasks, Dependencies, Boundaries). Reference `../wave-plan.md` at the top as context.

7. **Pipeline state lives in the `meta.json` sidecar, not a JSON state file.** `mustard-rt run wave-scaffold` writes the per-wave `meta.json` (`stage: Plan`, `outcome: Active`, `isWavePlan: true`, `totalWaves: N`, `currentWave: 1`); the `completedWaves`/`currentWave` progression is derived from `pipeline.wave.complete` events. Do NOT hand-write a `pipeline-state.json`.

8. **Present wave plan to user:**
   - Read `wave-plan.md` and print its ENTIRE contents verbatim inside a fenced markdown block.
   - Also list each wave's spec file paths (one line each) so the user can open individual wave specs if desired.
   - Then `AskUserQuestion` — Full scope STOPS at PLAN, so the only forward path is approval via `/spec` (`/feature` never executes a Full spec inline):
     - **"Approve wave plan for later"** → stop, user runs `/mustard:spec {letter}` to approve (new session) or `/mustard:spec {letter}r` to approve + resume inline.
     - **"Edit decomposition (hint PLAN)"** → user provides hint (e.g., "merge waves 2 and 3"), PLAN reexecutes with the hint appended to `estimatedTouchPoints`/manual grouping. Re-decompose once.
     - **"Reject decomposition"** → collapse to a **single wave** via `mustard-rt run wave-collapse --spec {specName} --mode full` (the approve-flow's reject path — see `refs/spec/approve-only-flow.md`). NEVER a non-wave Full spec — Full ⇒ ≥1 wave.

9. The wave plan is the spec — there is no separate single-spec Full flow to skip. Approval (via `/mustard:spec`) makes wave-1 the first thing to execute.

#### COORDINATE phase (parent specs)

A spec with `children_specs.length > 0` may enter `COORDINATE`. In this phase the orchestrator tracks children progress — it does NOT implement. Emit `mustard-rt run emit-phase --spec {epic} --to COORDINATE` after linking. When all children = CLOSE, emit `mustard-rt run emit-phase --spec {epic} --to CLOSE`.

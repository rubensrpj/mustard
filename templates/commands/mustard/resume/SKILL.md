# /resume - Resume Pipeline

> ALWAYS before making any change. Search on the web for the newest documentation and only implement if you are 100% sure it will work.

## Trigger

`/resume`

## Description

Resumes an interrupted pipeline. The main context BECOMES the Pipeline Runner — dispatches agents directly via Task tool. NEVER delegates entire pipeline to a single intermediate Task agent.

## Action

### Step 0: Dispatch Failure Pre-Check

Before the normal detect-and-confirm flow, scan the newest pipeline state for a recent dispatch failure flagged by `subagent-tracker` (PostToolUse on Task).

1. Glob `.claude/.pipeline-states/*.json` (exclude `*.metrics.json`) and pick the file with the newest mtime.
2. Read it and inspect the `lastDispatchFailure` field.
3. If present:
   - Compute `ageMs = Date.now() - new Date(lastDispatchFailure.at).getTime()`.
   - **If ageMs <= 10 * 60 * 1000** (≤10 min, fresh):
     1. Inform the user: `Detected failed dispatch ({agentType}) due to {reason} at {at}. Re-dispatching with same prompt.`
     2. Re-invoke the Task tool with:
        - `subagent_type`: `lastDispatchFailure.agentType` (fallback: `general-purpose`)
        - `description`: `lastDispatchFailure.description`
        - `prompt`: `lastDispatchFailure.prompt`
     3. After the re-dispatch returns, clear the flag: remove `lastDispatchFailure` from the state object and rewrite the pipeline-state JSON.
     4. Fall through to Step 1 (normal resume flow continues from the updated state).
   - **If ageMs > 10 * 60 * 1000** (stale): silently remove `lastDispatchFailure` from the state and rewrite the file, then continue to Step 1.
4. If `lastDispatchFailure` is absent, skip Step 0 entirely and proceed to Step 0.5.

### Step 0.5: Resume Mode (continuar vs reanalisar)

Before loading heavy context (sync-registry, diff-context, Explore Gate), ask the user which mode to use. This gates roughly 2-5k tokens per resume.

1. **Skip conditions** — enter `reanalyze` mode automatically without prompting:
   - Step 0 just re-dispatched a failed agent (recovery path → always reanalyze next step)
   - `pipeline-state.lastDispatchFailure` was present and <10min old (already handled in Step 0)
   - Wave plan with `failedWaves.length > 0` (handled in wave failure section below — forces `reanalyze`)

2. **Otherwise, AskUserQuestion:**
   - **"Continuar de onde parou (modo leve)"** → `mode = "continued"`: skip sync-registry (Step 2 #6), skip diff-context (unless wave transition forces), skip Pre-EXECUTE Existence Gate (Step 12b). Trust pipeline-state as source of truth.
   - **"Reanalisar contexto (modo completo)"** → `mode = "reanalyzed"`: run Step 2 fully (default behavior, relê tudo).

3. **Record mode in pipeline state:** write `resumeMode: "continued" | "reanalyzed"` and `resumeModeAt: {ISO now}` so downstream steps know which path they are in.

4. **Stale-context fallback (safety net):** if a dispatched agent in `continued` mode returns an error indicating stale context (e.g., references a missing file, fails boundary check, or returns `BLOCKED` with reason citing out-of-date registry), escalate automatically:
   - Update pipeline state: `resumeMode: "escalated-to-reanalyze"`, append to `resumeEscalations` array with `{at, reason}`
   - Re-run Step 2 in full (sync-registry + diff-context)
   - Re-dispatch the failed agent with fresh context
   - Fail-open: escalation never blocks, just upgrades to the heavier path

### Step 1: Detect & Confirm

1. Glob `.claude/spec/active/*/spec.md` — if 0 specs → inform user and stop
2. If multiple → ask which one; if 1 → use automatically
3. **Read entire spec** (single Read) — extract header (Status/Phase/Checkpoint) + count `[x]` vs `[ ]` + identify agents/waves from headers `### {Agent} Agent (Wave {N})`
4. If `.claude/.pipeline-states/{spec-name}.json` exists → read for current wave + scope + `explorationSummary` + `decisions`
5. **Validate pipeline state integrity:**
   - Missing or unparseable JSON → rebuild from spec (phase from header, tasks from `[x]`/`[ ]` checkboxes, status inferred)
   - Phase mismatch between spec header and JSON → trust spec header (it's the source of truth)
   - Tasks in JSON marked `completed` but spec has `[ ]` → trust spec, reset task to `pending`
   - If rebuilt → warn user: "Pipeline state was recovered from spec"
5. **Present Handoff Summary:**

   Compile from pipeline state + spec + agent memory + git context:

   ```
   === PIPELINE HANDOFF ===

   Pipeline: {spec-name}
   Scope:    {light|full}
   Phase:    {ANALYZE|PLAN|EXECUTE|CLOSE}
   Started:  {timestamp} | Elapsed: {duration}

   ## Completed
   {For each [x] checkbox in spec:}
   - [x] {task description}

   ## Pending
   {For each [ ] checkbox in spec:}
   - [ ] {task description}

   ## Concerns
   {Scan spec for <!-- CONCERN: ... --> comments. Omit section if none.}
   - {concern text}

   ## Context
   - Branch: {from git}
   - Files changed: {run `node .claude/scripts/diff-context.js`}
   - Last agent: {Read `.claude/.agent-memory/_index.json` and pick the last entry's `agent_type`. If the file or `.agent-memory/` directory is missing, print literal `(none)` — do NOT probe with `ls`/`grep`, it surfaces noisy exit codes}
   - Last action: {from the same last entry's `summary` field. If missing, print literal `(no prior memory)`}
   - Decisions: {decisions[] from pipeline state, if any}

   ## Next Action
   → {ONE specific next step}
   ===
   ```

6. Ask: **"Continue from next action, or review spec first?"**

### Step 2: Bootstrap (after confirmation)

6. **AUTO-SYNC:** `node .claude/scripts/sync-registry.js`
   - **Skip if `resumeMode === "continued"`** (Step 0.5): registry is reused from prior session.
   - Always run if `resumeMode === "reanalyzed"` or `"escalated-to-reanalyze"`.

### Diff Context (automatic)
Run `node .claude/scripts/diff-context.js --subproject {subproject_path}` per subproject to capture the current git state scoped to each subproject. Include the subproject-specific output in the agent prompt as `{diff_context}` so agents see only changes relevant to their scope.

**Skip if `resumeMode === "continued"`** unless a wave just completed (wave transitions always refresh diff). The prior diff snapshot is reused from `.claude/.pipeline-states/{specName}.diff-{subproject}.md`.

7. **Read** `.claude/pipeline-config.md`. For `entity-registry.json`: use Grep to extract ONLY the relevant entity block (e.g. `"Contract":`), NEVER read the full JSON
9. **Update spec header:** `Status: implementing`, `Phase: EXECUTE`, `Checkpoint: {ISO now}`
10. **Update/create pipeline state:** `status: "implementing"`, `phaseName: "EXECUTE"`, `specName`
11. **TaskCreate** — 1 per pending agent (skip completed)

### Step 3: Execute — Wave System

**CRITICAL: Main context IS the Pipeline Runner. NEVER delegate to intermediate Task agent.**

12. **Match recipe by name only:** Grep `{subproject}/.claude/commands/recipes.md` for recipe title matching the task type — do NOT read the full recipes file. Extract only: recipe number, pattern refs, reference modules
12b. **Pre-EXECUTE Existence Gate**: Same gate as `feature/SKILL.md § Pre-EXECUTE Existence Gate`. Invoke identically (Full scope only, `## Files` ≤ 8). On retry/resume, the gate naturally handles idempotence: tasks already `[x]` from a prior run are treated as Mixed — the Haiku confirms they stay done and the orchestrator only re-dispatches what remains `[ ]`.

   **Skip entirely if `resumeMode === "continued"`** (Step 0.5). The `continued` mode trusts pipeline-state checkboxes as-is. If the stale-context fallback escalates to `reanalyze`, the gate runs on the re-dispatch.

    **Pre-check (same as `feature/SKILL.md § Pre-EXECUTE Existence Gate`):** Before dispatching Haiku, run `rtk git diff --stat HEAD -- <files listed in spec's ## Files>`. Skip gate entirely if output is empty (no changes) or total insertions/deletions <10. Only proceed with Haiku dispatch if ≥10 lines changed.

12c. **Wave Plan Scope (conditional — only if `pipeline-state.isWavePlan === true`):**

When the pipeline state indicates a wave plan, the orchestrator dispatches only the **current wave**, not the full spec:

1. Read `pipeline-state.currentWave` and `pipeline-state.totalWaves`.
2. The spec to work from for this invocation is `.claude/spec/active/{specName}/wave-{currentWave}-*/spec.md`. Replace any prior reference to `spec.md` at the root of the spec dir with the current wave's spec.
3. **Between waves** (see Step 17 post-dispatch):
   - On wave completion: run `/mustard:git commit` style commit with message `feat(wave-{N}/{role}): {summary}`. If `/mustard:git commit` is not appropriate for the project, fall back to `git add {files} && git commit -m "..."`.
   - Update state: `completedWaves.push(currentWave)`, `currentWave += 1`, `updatedAt`.
   - Force `resumeMode = "reanalyzed"` for the next wave transition so diff-context refreshes with the just-committed changes.
   - If `currentWave > totalWaves` → skip remaining wave dispatch, go to Step 19 REVIEW + Step 20 CLOSE on the overall wave plan.
4. **If a wave fails (REJECTED after 2 fix-loops, or BLOCKED)** — see § Wave Failure Handling below.

13. **Plan waves:** `Depends on: none` → Wave 1; dependencies → later. DB+Backend parallel. Frontend after Backend UNLESS all parallel override conditions met (see `.claude/pipeline-config.md` Parallel Rules). Review agents: ALWAYS dispatch in single parallel message. Skip completed tasks.

**Note on wave plans:** when `isWavePlan === true`, this step plans the agent wave structure **within** the current wave's spec only — agents internal to the current wave-spec may still split across DB/Backend/Frontend sub-waves. The outer wave (1..N) is the cross-spec sequence managed by Step 12c.
14. **Build agent prompts using template** (`.claude/commands/mustard/templates/agent-prompt/SKILL.md`):
    - Read template once, then fill placeholders per agent using `.claude/pipeline-config.md` data:
      - `{subproject}` → from Agents table (Subproject column)
      - `{reference_files}` → 2-3 files from matched recipe
      - `{guards_summary}` → key guards from `{subproject}/CLAUDE.md`
      - `{entity_info}` → `_patterns` type, refs, subs from registry
      - `{role}`, `{boundary}`, `{return_sections}` → from Role Rules table in config
      - `{validate_command}`, `{build_command}` → from Agents table in config
      - `{retry_context}` → empty on first dispatch. On retry, fill per `agent-prompt/SKILL.md § Retry Modes`. Granular retries use Step 4 § Granular Retry Protocol. Fix-loops (after REJECTED review) use Step 19b § Fix Loop Dispatch Protocol.
      - `{task_steps}` → checkboxed steps from spec
      - `{recommended_skills}` → from Skill Recommendations in `.claude/pipeline-config.md`:
        1. Glob `{subproject}/.claude/skills/` for generated pattern skills
        2. Add foundation skills matching the role (ui→design-craft+react-best-practices, mobile→design-craft)
        3. Format as bullet list: `- {skill-name}`

16. **Wave transitions** — between waves, execute transitions from `.claude/pipeline-config.md`:
    - After Wave 1 (api/database/library) completes, before Wave 2 (ui):
      - Execute each command listed in the matching `Wave Transitions` section
    - Wait for transitions to complete before dispatching next wave

17. **Dispatch:** TaskUpdate(in_progress) + pipeline state. ALL agents in same wave → SINGLE message (multiple Task invocations). **Pass `model` from pipeline state** (e.g. `model: "opus"`) in each Task tool call — this overrides the agent YAML default. On return: pipeline state update, spec `[ ]` → `[x]` (use `replace_all` per section header, or line-by-line — NEVER copy entire spec blocks as old_string), TaskUpdate(completed), advance wave.

17b. **Agent Memory:** After each wave completes and spec checkboxes are updated, write agent memories for downstream waves:
    ```bash
    node .claude/scripts/memory-write.js --json '{"agent_type":"{agent_type}","wave":{N},"pipeline":"{spec-name}","summary":"{1-line summary of what agent did}","details":{"files_modified":[...],"decisions":[...]}}'
    ```
    One call per agent in the completed wave. Summary ≤300 chars (key facts: files created, patterns used, endpoints added). Skip if no downstream waves remain.

#### Escalation Status Handling

After each agent returns, check the return value for an escalation status before advancing to the next wave:

- **Internal error** (no parseable output, empty return, API error) — re-dispatch the failed agent(s) **sequentially** (not parallel) with the same prompt. Max 1 Internal retry per agent. If still failing: STOP + report
- `CONCERN` — record verbatim under `## Concerns` in the spec; continue to next wave
- `BLOCKED` — stop immediately; use `AskUserQuestion` to report the exact blocker; do NOT advance
- `PARTIAL` — apply Granular Retry Protocol from the last completed step; do NOT restart from step 1
- `DEFERRED` — note in spec with agent justification; ask user if the deferred item is load-bearing before closing

If two or more agents in the same wave return `CONCERN`, surface all concerns together before dispatching the next wave. See `.claude/pipeline-config.md` Escalation Statuses and Diagnostic Failure Routing for the full status table.

### Step 4: Validate, Review & Complete

18. **VALIDATE** — Parse agent results: Backend→`dotnet build`, Frontend→`pnpm build`, Mobile→`fvm flutter analyze`. All passed → next. Failed → **granular retry** (see below).
19. **REVIEW (MANDATORY — NEVER SKIP):**
    - Dispatch review agent for EACH affected subproject in a SINGLE message (multiple Task invocations) using `subagent_type: "general-purpose"` with review prompt
    - Review agent MUST read `{subproject}/CLAUDE.md` + `{subproject}/.claude/commands/guards.md`
    - Review checklist categories (inline): **SOLID, Design System, Patterns, i18n, Integration, Build, Elegance**
    - Checklist categories: **SOLID, Design System, Patterns, i18n, Integration, Build, Elegance**
    - Each issue classified: CRITICAL (blocks), WARNING (recommended), NOTE (suggestion)
    - APPROVED (zero CRITICAL) → CLOSE
    - REJECTED (any CRITICAL) → see Step 19b § Fix Loop Dispatch Protocol (max 2 loops)
    - **NEVER skip review** — not even for Light scope. Light scope gets same checklist, just fewer files to review

### Step 19b: Fix Loop Dispatch Protocol

When REVIEW returns REJECTED (any CRITICAL):

1. Read `.claude/.agent-memory/_index.json`, find last entry where `agent_type == {review_target_agent_type}` and `pipeline == {spec-name}`. If absent (shouldn't happen but be defensive): fall back to first-dispatch template.
2. Extract:
   - `prior_summary` ← `entry.summary`
   - `files_modified` ← `entry.details.files_modified` (list)
3. Extract review findings VERBATIM:
   - All CRITICAL findings (required)
   - All WARNING findings (optional — include if fix is cheap)
   - Copy the exact text returned by the review agent; do NOT paraphrase
4. Compose `{retry_context}` using Mode=fix-loop format (see `agent-prompt/SKILL.md § Retry Modes`). Set K = current loop number (1 or 2; max 2 fix-loops):
   ```
   ## RETRY CONTEXT
   **Mode:** fix-loop ({K}/2)
   **Prior dispatch:** {prior_summary}
   **Files modified previously:**
   {files_modified}
   **Review findings (verbatim):**
   {findings_verbatim}
   ```
5. Render the **Minimal Retry Template** from `agent-prompt/SKILL.md § Retry Modes` (skips CONTEXT/REFERENCE/ENTITY/SKILLS/WEB VALIDATION/ROLE/RECIPE).
6. Dispatch the same `subagent_type` + `model` as the original impl agent (do NOT change the role or model).
7. On return, re-dispatch REVIEW agent (normal dispatch, not retry — review is read-only).
8. If review still REJECTED after 2 fix-loops: STOP + report exhausted retries.

20. **CLOSE:**
    - **Wave plan gate:** if `pipeline-state.isWavePlan === true`, only CLOSE when `completedWaves.length === totalWaves`. If waves remain (`currentWave <= totalWaves` and wave N-1 just finished), **do not** run CLOSE — instead update state (`currentWave++`, `completedWaves.push`), output `═══ WAVE {N-1} COMPLETE — {role} ═══`, and stop. Next `/mustard:resume` picks up wave N.
    - `node .claude/scripts/sync-registry.js`
    - Spec: `Status: completed`, `Phase: CLOSE`, all `[ ]` → `[x]`. For wave plans: mark `wave-plan.md` status `completed`, and mark each `wave-N-{role}/spec.md` completed too.
    - Move spec to `.claude/spec/completed/` (the entire `{specName}/` directory, including wave subdirs if any)
    - **Delete** `.claude/.pipeline-states/{spec-name}.json`
    - Output with agent colors: `═══ PIPELINE COMPLETE — {name} | Agents: {n} ok | Files: {c} created, {m} modified ═══` (for wave plans: append `| Waves: {totalWaves}`).

### Wave Failure Handling

Applies only when `pipeline-state.isWavePlan === true`.

A wave is considered **failed** when:
- REVIEW returns REJECTED after 2 fix-loops exhausted (see Step 19b), OR
- An implementation agent returns `BLOCKED` and the user cannot resolve inline, OR
- Build/type-check fails repeatedly (max 2 retries) after Granular Retry Protocol is exhausted.

**On wave failure:**

1. Update pipeline state:
   - `failedWaves.push(currentWave)`
   - `status = "failed"`
   - `updatedAt = {ISO now}`
2. Write failure log to `.claude/spec/active/{specName}/wave-{currentWave}-{role}/failure.md`:
   ```markdown
   # Wave {N} Failure — {role}
   ## When: {ISO}
   ## Phase: {EXECUTE | REVIEW | CLOSE}
   ## Reason: {short cause — e.g., "REVIEW REJECTED after 2 fix-loops"}
   ## Findings (verbatim)
   {last review findings OR BLOCKED rationale OR build error}
   ## Files touched
   {list from agent memory}
   ```
3. **Do NOT** attempt further automatic recovery. Wave N-1 commits remain in place — they are real progress.
4. **Prompt the user via AskUserQuestion:**
   - **"Corrigir wave {N} manualmente e retomar"** → user fixes by hand; next `/mustard:resume` clears `failedWaves` entry and restarts wave N from EXECUTE.
   - **"Reescrever wave {N} (re-PLAN dessa onda)"** → delete `wave-{N}-{role}/spec.md`, re-enter PLAN for wave N only (run PLAN sub-flow scoped to wave N's files). User then re-approves via `/mustard:approve` for wave N.
   - **"Abortar pipeline"** → set `status: "aborted"`, move spec to `.claude/spec/aborted/{specName}/` (create dir if needed), keep waves 1..N-1 commits. Inform user: `Pipeline aborted. Waves 1..{N-1} commits preserved. Waves {N}..{totalWaves} discarded.`

**Risco residual documentado:** wave N-1 commits podem estar incompletos semanticamente sem wave N (ex.: schema criado mas API não). O usuário foi avisado disso no `/approve` da wave plan. O log `failure.md` explicita qual superfície ficou exposta.

### Granular Retry Protocol

When an agent fails:

1. **Parse return** to identify last completed step (look for `[x]` markers or explicit "Step N completed" in output)
2. **Determine retry scope:**
   - Build error → retry from build step (don't redo edits)
   - Edit error → retry from that edit step
   - Unknown → retry all remaining unchecked steps
3. **Re-dispatch with retry context** — fill `{retry_context}` using Mode=granular format:
   - Read `.claude/.agent-memory/_index.json`, find last entry where `agent_type == {failed_agent_type}` and `pipeline == {spec-name}`
   - Extract `entry.summary` → `prior_summary`; `entry.details.files_modified` → `files_modified` (list)
   - Fill:
     ```
     ## RETRY CONTEXT
     **Mode:** granular
     **Prior dispatch:** {prior_summary}
     **Files modified previously:**
     {files_modified}
     **Previous error:** {error_message}
     **Resume from step:** {N+1}
     ```
   - Set `{task_steps}` to only the remaining steps ({N+1} onwards)
   - Use the **Minimal Retry Template** from `agent-prompt/SKILL.md § Retry Modes` (skips CONTEXT/REFERENCE/ENTITY/SKILLS/WEB VALIDATION/ROLE/RECIPE blocks)
4. **Spec checkboxes:** steps 1-{N} already `[x]`, remaining continue `[ ]`
5. **Max 2 retries per agent** — exhausted → STOP + report

### Pause Handoff

When a pipeline is paused (user leaves session or requests pause):

1. Update pipeline state JSON (`.claude/.pipeline-states/{spec-name}.json`):
   - Set `pausedAt` to current ISO timestamp
   - Set `pauseReason` to user-provided reason (or "session ended")
   - Set `nextAction` to the specific next step (ONE sentence)
2. Write agent memory for carry-over:
   ```bash
   node .claude/scripts/memory-write.js --json '{"agent_type":"orchestrator","wave":0,"pipeline":"{spec-name}","summary":"Paused at {phase}. Next: {nextAction}"}'
   ```
3. Confirm to user: "Pipeline paused. Next action saved: {nextAction}"

### Next Action Rule

The handoff MUST end with exactly ONE next action:

**Wrong:** "You could dispatch the backend agent, review the spec, or run tests"
**Right:** "→ Dispatch backend agent for task 3 (add /api/users endpoint)"

This eliminates decision fatigue on resume. The user can always override, but the default path is a single clear step.

## INVIOLABLE RULES

- Main context IS the Pipeline Runner — NEVER wrap in a single Task agent
- NEVER implement code directly — ALL via Task agents (1 per subproject per wave)
- Wave dispatch: ALL agents in same wave in a SINGLE message
- Each sub-agent reads its own `{subproject}/CLAUDE.md` + auto-loads relevant skills (orchestrator does NOT read them)
- Update pipeline state + spec checkboxes at each transition
- ALWAYS read `.claude/pipeline-config.md` for agent/wave/model config — NEVER hardcode project-specific values
- ALWAYS use agent-prompt template — NEVER build prompts from scratch
- ALWAYS execute wave transitions between waves

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
4. If `lastDispatchFailure` is absent, skip Step 0 entirely and proceed to Step 1.

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
   - Last agent: {from `.claude/.agent-memory/_index.json` last entry}
   - Last action: {summary from last agent memory entry}
   - Decisions: {decisions[] from pipeline state, if any}

   ## Next Action
   → {ONE specific next step}
   ===
   ```

6. Ask: **"Continue from next action, or review spec first?"**

### Step 2: Bootstrap (after confirmation)

6. **AUTO-SYNC:** `node .claude/scripts/sync-registry.js`

### Diff Context (automatic)
Run `node .claude/scripts/diff-context.js` to capture the current git state. Include the output in the agent prompt as `{diff_context}` so agents know what has already changed.

7. **Read** `pipeline-config.md`. For `entity-registry.json`: use Grep to extract ONLY the relevant entity block (e.g. `"Contract":`), NEVER read the full JSON
9. **Update spec header:** `Status: implementing`, `Phase: EXECUTE`, `Checkpoint: {ISO now}`
10. **Update/create pipeline state:** `status: "implementing"`, `phaseName: "EXECUTE"`, `specName`
11. **TaskCreate** — 1 per pending agent (skip completed)

### Step 3: Execute — Wave System

**CRITICAL: Main context IS the Pipeline Runner. NEVER delegate to intermediate Task agent.**

12. **Match recipe by name only:** Grep `{subproject}/.claude/commands/recipes.md` for recipe title matching the task type — do NOT read the full recipes file. Extract only: recipe number, pattern refs, reference modules
13. **Plan waves:** `Depends on: none` → Wave 1; dependencies → later. DB+Backend parallel. Frontend after Backend UNLESS all parallel override conditions met (see `pipeline-config.md` Parallel Rules). Review agents: ALWAYS dispatch in single parallel message. Skip completed tasks.
14. **Build agent prompts using template** (`.claude/commands/mustard/templates/agent-prompt/SKILL.md`):
    - Read template once, then fill placeholders per agent using `pipeline-config.md` data:
      - `{subproject}` → from Agents table (Subproject column)
      - `{reference_files}` → 2-3 files from matched recipe
      - `{guards_summary}` → key guards from `{subproject}/CLAUDE.md`
      - `{entity_info}` → `_patterns` type, refs, subs from registry
      - `{role}`, `{boundary}`, `{return_sections}` → from Role Rules table in config
      - `{validate_command}`, `{build_command}` → from Agents table in config
      - `{retry_context}` → empty on first dispatch (see Step 4 for retries)
      - `{task_steps}` → checkboxed steps from spec
      - `{recommended_skills}` → from Skill Recommendations in `pipeline-config.md`:
        1. Glob `{subproject}/.claude/skills/` for generated pattern skills
        2. Add foundation skills matching the role (ui→design-craft+react-best-practices, mobile→design-craft)
        3. Format as bullet list: `- {skill-name}`

16. **Wave transitions** — between waves, execute transitions from `pipeline-config.md`:
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

If two or more agents in the same wave return `CONCERN`, surface all concerns together before dispatching the next wave. See `pipeline-config.md` Escalation Statuses and Diagnostic Failure Routing for the full status table.

### Step 4: Validate, Review & Complete

18. **VALIDATE** — Parse agent results: Backend→`dotnet build`, Frontend→`pnpm build`, Mobile→`fvm flutter analyze`. All passed → next. Failed → **granular retry** (see below).
19. **REVIEW (MANDATORY — NEVER SKIP):**
    - Dispatch review agent for EACH affected subproject in a SINGLE message (multiple Task invocations) using `subagent_type: "general-purpose"` with review prompt
    - Review agent MUST read `{subproject}/CLAUDE.md` + `{subproject}/.claude/commands/guards.md`
    - Review checklist categories (inline): **SOLID, Design System, Patterns, i18n, Integration, Build, Elegance**
    - Checklist categories: **SOLID, Design System, Patterns, i18n, Integration, Build, Elegance**
    - Each issue classified: CRITICAL (blocks), WARNING (recommended), NOTE (suggestion)
    - APPROVED (zero CRITICAL) → CLOSE
    - REJECTED (any CRITICAL) → dispatch fix agent with exact issues, then re-review (max 2 fix loops)
    - **NEVER skip review** — not even for Light scope. Light scope gets same checklist, just fewer files to review
20. **CLOSE:**
    - `node .claude/scripts/sync-registry.js`
    - Spec: `Status: completed`, `Phase: CLOSE`, all `[ ]` → `[x]`
    - Move spec to `.claude/spec/completed/`
    - **Delete** `.claude/.pipeline-states/{spec-name}.json`
    - Output with agent colors: `═══ PIPELINE COMPLETE — {name} | Agents: {n} ok | Files: {c} created, {m} modified ═══`

### Granular Retry Protocol

When an agent fails:

1. **Parse return** to identify last completed step (look for `[x]` markers or explicit "Step N completed" in output)
2. **Determine retry scope:**
   - Build error → retry from build step (don't redo edits)
   - Edit error → retry from that edit step
   - Unknown → retry all remaining unchecked steps
3. **Re-dispatch with retry context** — fill `{retry_context}` placeholder:
   ```
   ## RETRY CONTEXT
   Steps 1-{N} completed. Resume from step {N+1}.
   Previous error: {error_message}
   ```
   And set `{task_steps}` to only the remaining steps ({N+1} onwards).
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
- ALWAYS read `pipeline-config.md` for agent/wave/model config — NEVER hardcode project-specific values
- ALWAYS use agent-prompt template — NEVER build prompts from scratch
- ALWAYS execute wave transitions between waves

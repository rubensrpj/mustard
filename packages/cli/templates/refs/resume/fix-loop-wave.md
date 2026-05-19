# Fix Loop, Wave Failure & Retry Reference

> Detail for `/resume` pipeline execution: fix-loop dispatch after review rejection, wave failure handling, granular retry protocol, pause handoff, and next action rule.

## Step 19b: Fix Loop Dispatch Protocol

When REVIEW returns REJECTED (any CRITICAL):

1. **Try harness view first (Wave 3):** `bun .claude/scripts/event-projections.js --view pipeline-state --spec {spec-name}` — if it returns `decisions` or `lessons`, prefer those. Otherwise fall back to reading `.claude/.agent-memory/_index.json`, finding the last entry where `agent_type == {review_target_agent_type}` and `pipeline == {spec-name}`. If absent (shouldn't happen but be defensive): fall back to first-dispatch template.
2. Extract:
   - `prior_summary` ← `entry.summary`
   - `files_modified` ← `entry.details.files_modified` (list)
3. Extract review findings VERBATIM:
   - All CRITICAL findings (required)
   - All WARNING findings (optional — include if fix is cheap)
   - Copy the exact text returned by the review agent; do NOT paraphrase
4. Compose `{retry_context}` using Mode=fix-loop format (see `.claude/refs/agent-prompt/agent-prompt.md § Retry Modes`). Set K = current loop number (1 or 2; max 2 fix-loops):
   ```
   ## RETRY CONTEXT
   **Mode:** fix-loop ({K}/2)
   **Prior dispatch:** {prior_summary}
   **Files modified previously:**
   {files_modified}
   **Review findings (verbatim):**
   {findings_verbatim}
   ```
5. Render the **Minimal Retry Template** from `.claude/refs/agent-prompt/agent-prompt.md § Retry Modes` (skips CONTEXT/REFERENCE/ENTITY/SKILLS/WEB VALIDATION/ROLE/RECIPE).
6. Dispatch the same `subagent_type` + `model` as the original impl agent (do NOT change the role or model).
7. On return, re-dispatch REVIEW agent (normal dispatch, not retry — review is read-only).
8. If review still REJECTED after 2 fix-loops: STOP + report exhausted retries.

## Wave Failure Handling

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

## Granular Retry Protocol

When an agent fails:

1. **Parse return** to identify last completed step (look for `[x]` markers or explicit "Step N completed" in output)
2. **Determine retry scope:**
   - Build error → retry from build step (don't redo edits)
   - Edit error → retry from that edit step
   - Unknown → retry all remaining unchecked steps
3. **Re-dispatch with retry context** — fill `{retry_context}` using Mode=granular format:
   - **Try harness view first:** `bun .claude/scripts/event-projections.js --view pipeline-state --spec {spec-name}` — use decisions/lessons if available. Fallback: Read `.claude/.agent-memory/_index.json`, find last entry where `agent_type == {failed_agent_type}` and `pipeline == {spec-name}`
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
   - Use the **Minimal Retry Template** from `.claude/refs/agent-prompt/agent-prompt.md § Retry Modes` (skips CONTEXT/REFERENCE/ENTITY/SKILLS/WEB VALIDATION/ROLE/RECIPE blocks)
4. **Spec checkboxes:** steps 1-{N} already `[x]`, remaining continue `[ ]`
5. **Max 2 retries per agent** — exhausted → STOP + report

## Pause Handoff

When a pipeline is paused (user leaves session or requests pause):

1. Update pipeline state JSON (`.claude/.pipeline-states/{spec-name}.json`):
   - Set `pausedAt` to current ISO timestamp
   - Set `pauseReason` to user-provided reason (or "session ended")
   - Set `nextAction` to the specific next step (ONE sentence)
2. Write agent memory for carry-over:
   ```bash
   bun .claude/scripts/memory.js agent --json '{"agent_type":"orchestrator","wave":0,"pipeline":"{spec-name}","summary":"Paused at {phase}. Next: {nextAction}"}'
   ```
3. Confirm to user: "Pipeline paused. Next action saved: {nextAction}"

## Next Action Rule

The handoff MUST end with exactly ONE next action:

**Wrong:** "You could dispatch the backend agent, review the spec, or run tests"
**Right:** "→ Dispatch backend agent for task 3 (add /api/users endpoint)"

This eliminates decision fatigue on resume. The user can always override, but the default path is a single clear step.

# /resume - Resume Pipeline

> ALWAYS before making any change. Search on the web for the newest documentation and only implement if you are 100% sure it will work.

## Trigger

`/resume`

## Description

Resumes an interrupted pipeline. The main context BECOMES the Pipeline Runner — dispatches agents directly via Task tool. NEVER delegates entire pipeline to a single intermediate Task agent.

## Action

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
5. **Present summary and ASK before continuing:**

   ```
   Pipeline: {spec-name}
   Scope:    {light|full}
   Status:   {status} | Phase: {phase}
   Progress: {completed}/{total} tasks
   Next:     {next agent/wave}
   Decisions: {decisions[] if present}

   Continue?
   ```

### Step 2: Bootstrap (after confirmation)

6. **AUTO-SYNC:** `node .claude/scripts/sync-registry.js`
7. **Read** `pipeline-config.md`. For `entity-registry.json`: use Grep to extract ONLY the relevant entity block (e.g. `"Contract":`), NEVER read the full JSON
9. **Update spec header:** `Status: implementing`, `Phase: EXECUTE`, `Checkpoint: {ISO now}`
10. **Update/create pipeline state:** `status: "implementing"`, `phaseName: "EXECUTE"`, `specName`
11. **TaskCreate** — 1 per pending agent (skip completed)

### Step 3: Execute — Wave System

**CRITICAL: Main context IS the Pipeline Runner. NEVER delegate to intermediate Task agent.**

12. **Match recipe by name only:** Grep `{subproject}/.claude/commands/recipes.md` for recipe title matching the task type — do NOT read the full recipes file. Extract only: recipe number, pattern refs, reference modules
13. **Plan waves:** `Depends on: none` → Wave 1; dependencies → later. DB+Backend parallel. Frontend ALWAYS after Backend. Skip completed tasks.
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

### Step 4: Validate, Review & Complete

18. **VALIDATE** — Parse agent results: Backend→`dotnet build`, Frontend→`pnpm build`, Mobile→`fvm flutter analyze`. All passed → next. Failed → **granular retry** (see below).
19. **REVIEW (MANDATORY — NEVER SKIP):**
    - Dispatch review agent for EACH affected subproject using `subagent_type: "general-purpose"` with review prompt
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

## INVIOLABLE RULES

- Main context IS the Pipeline Runner — NEVER wrap in a single Task agent
- NEVER implement code directly — ALL via Task agents (1 per subproject per wave)
- Wave dispatch: ALL agents in same wave in a SINGLE message
- Each sub-agent reads its own `{subproject}/CLAUDE.md` + auto-loads relevant skills (orchestrator does NOT read them)
- Update pipeline state + spec checkboxes at each transition
- ALWAYS read `pipeline-config.md` for agent/wave/model config — NEVER hardcode project-specific values
- ALWAYS use agent-prompt template — NEVER build prompts from scratch
- ALWAYS execute wave transitions between waves

ULTRATHINK

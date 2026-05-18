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

1. **Skip conditions — auto `reanalyze` (no prompt):**
   - Step 0 just re-dispatched a failed agent (recovery path → always reanalyze next step)
   - `pipeline-state.lastDispatchFailure` was present and <10min old (already handled in Step 0)
   - Wave plan with `failedWaves.length > 0` (handled in wave failure section below — forces `reanalyze`)

1b. **Auto-continue conditions — auto `continued` (no prompt):**
    - `pipeline-state.updatedAt` within last 10 min AND `status === 'in_progress'` AND no `lastDispatchFailure`. The user just paused or re-opened the session; trust state as source of truth.
    - Override env: `MUSTARD_RESUME_MODE=continued|reanalyzed|ask` lets a user force a mode for the whole session. `ask` restores the legacy prompt.

2. **Otherwise (state >10min old OR forced via env=ask):** AskUserQuestion:
   - **"Continuar de onde parou (modo leve)"** → `mode = "continued"`: skip sync-registry (Step 2 #6), skip diff-context (unless wave transition forces), skip Pre-EXECUTE Existence Gate (Step 12b). Trust pipeline-state as source of truth.
   - **"Reanalisar contexto (modo completo)"** → `mode = "reanalyzed"`: run Step 2 fully (default behavior, relê tudo).

3. **Record mode in pipeline state:** write `resumeMode: "continued" | "reanalyzed"` and `resumeModeAt: {ISO now}` so downstream steps know which path they are in.

4. **Stale-context fallback (safety net):** if a dispatched agent in `continued` mode returns an error indicating stale context (e.g., references a missing file, fails boundary check, or returns `BLOCKED` with reason citing out-of-date registry), escalate automatically:
   - Update pipeline state: `resumeMode: "escalated-to-reanalyze"`, append to `resumeEscalations` array with `{at, reason}`
   - Re-run Step 2 in full (sync-registry + diff-context)
   - Re-dispatch the failed agent with fresh context
   - Fail-open: escalation never blocks, just upgrades to the heavier path

### Step 1: Detect & Confirm

1. **Detect active specs** — Glob BOTH root markers (a wave plan has no `spec.md` at its root, only `wave-plan.md`):
   - `.claude/spec/active/*/spec.md` (single specs)
   - `.claude/spec/active/*/wave-plan.md` (wave plans)

   Each match identifies one active spec by its parent dir `{specName}`. Union the results. If 0 matches → inform user and stop. Do NOT replace with `active/**/spec.md` — that would also pick up per-wave specs and double-count wave plans.
2. If multiple → ask which one; if 1 → use automatically.
3. **Resolve operational spec file** (the file the rest of resume operates on):
   - If matched root file is `spec.md` → operational spec = that file (single-spec mode).
   - If matched root file is `wave-plan.md` → wave-plan mode:
     a. Read `.claude/.pipeline-states/{specName}.json`. If present with `isWavePlan: true` + `currentWave: N`, use that state — skip to (c).
     b. **State missing → reconstruct inline** (no `/mustard:approve` roundtrip; the user just wants to continue):
        1. Run `bun .claude/scripts/wave-tree.js --spec-dir .claude/spec/active/{specName} --format json` and parse `waves[]` (each has `{label, folder, status}`).
        2. **Truly fresh plan** — every wave has `status === "queued"` (never executed) → stop and instruct: `Wave plan isn't approved yet. Run /mustard:approve {specName} first.`
        3. **Plan already in progress** — at least one wave has `status !== "queued"` (proves it was approved & started, the state file was just lost):
           - Build pipeline-state:
             - `specName`, `isWavePlan: true`, `status: "implementing"`, `phaseName: "EXECUTE"`
             - `totalWaves: waves.length`
             - `completedWaves: <wave numbers where status === "completed">` (1-indexed, parsed from `folder` like `wave-3-backend`)
             - `currentWave: <smallest wave number where status !== "completed">`
             - `reconstructedFromWavePlan: true`, `reconstructedAt: <ISO now>`
           - Write to `.claude/.pipeline-states/{specName}.json`.
           - Inform user inline: `Reconstruí pipeline-state do wave-plan.md (W{completed} done, W{currentWave} next).`
     c. With state (loaded or reconstructed), operational spec = result of Glob `.claude/spec/active/{specName}/wave-{currentWave}-*/spec.md` (one match expected).
   - **3d. Stub Expansion (wave-plan mode only).** By design `/feature` expands wave-1 fully and leaves waves N≥2 as skeletons (Status: queued, Title + 1-line summary). When resume picks up wave N≥2, the stub must be expanded inline — no `/mustard:approve` roundtrip:
     1. Read first 30 lines of the operational spec. Treat as **stub** if `### Status: queued` AND neither `## Files` nor `## Tasks` heading is present.
     2. If not a stub → continue to step 4.
     3. If stub → expand inline via `Task(Plan)` (single dispatch, `model: "opus"`):
        - Prompt inputs: this wave's row in `wave-plan.md` (role, file list, deps, Rationale), the most recent completed wave's spec (entity/pattern continuity), `entity-registry.json` Grepped for entities mentioned in the file list.
        - Required return: full expanded spec content for this wave matching the Full-scope template (Status: draft, Summary, Entity Info, Files, Tasks per agent, Dependencies, Boundaries, Acceptance Criteria, Checklist). Nothing else.
        - On return: **Write** the content to the operational spec file (replace the skeleton), then update its header to `Status: implementing`, `Phase: EXECUTE`, `Checkpoint: {ISO now}`.
        - Inform user inline: `Expandi wave-{N} stub via Plan agent. Avançando para EXECUTE.`
     3b. **Wave size audit (advisory).** Right after the expanded spec is written, run `bun .claude/scripts/wave-size-check.js --spec-dir .claude/spec/active/{specName}`. If the result is `action: "audited"` and the entry for the current wave (`wave === currentWave`) has `oversized: true`, print the advisory line `⚠ Wave {N} ({folder}) — {fileCount} arquivos, {layerCount} camada(s) — considere dividir ({reason})`, noting the freshly-expanded wave came out large. This is **advisory** — do NOT block, do NOT re-plan automatically; continue into EXECUTE normally. Rationale: waves N≥2 only become a full spec here at resume, so `/approve`'s size audit never sees their real size — this is the checkpoint that catches large late waves.
     4. Proceed to step 4 with the now-expanded spec.
4. **Read entire operational spec** (single Read) — extract header (Status/Phase/Checkpoint) + count `[x]` vs `[ ]` + identify agents/waves from headers `### {Agent} Agent (Wave {N})`

   4a. **Phase marker on ANALYZE resume:** if the extracted `Phase` is `ANALYZE`, run `bun .claude/scripts/emit-phase.js --spec {specName} --to ANALYZE`. A pipeline interrupted mid-ANALYZE has no pipeline-state file yet, so `pipeline-phase.js` never recorded it — emit the marker here. Idempotent (no duplicate if already emitted) and fail-open. Skip for any other phase (those already have a pipeline-state file the hook tracks).
5. If `.claude/.pipeline-states/{specName}.json` exists (single-spec mode; wave-plan mode already loaded it in step 3) → read for current wave + scope + `explorationSummary` + `decisions`. Optionally enrich with harness view (fail-open). Validate integrity (trust spec header on mismatch).
6. **Present Handoff Summary** — compiled from pipeline state + spec + agent memory + git context.

→ See `../../../refs/resume/handoff-summary.md` for exact format and integrity validation rules.

   6a. **Wave Tree:** Run `bun .claude/scripts/wave-tree.js --spec-dir .claude/spec/active/{specName}` and print inline. Fail-open.

7. **Auto-continue default.** Inform user inline: `"Continuando da próxima ação. Se quiser revisar a spec antes, interrompa e diga 'review'."` Then proceed directly to Step 2 (Bootstrap). Only ask when ALL apply: scope=full AND currentWave==1 AND no completedWaves yet (fresh start, expensive to redo). Override env: `MUSTARD_RESUME_CONFIRM=always|never|fresh-only(default)`.

### Step 2: Bootstrap (after confirmation)

6. **AUTO-SYNC:** `bun .claude/scripts/sync-registry.js`
   - **Skip if `resumeMode === "continued"`** (Step 0.5): registry is reused from prior session.
   - Always run if `resumeMode === "reanalyzed"` or `"escalated-to-reanalyze"`.

### Diff Context (automatic)
Run `bun .claude/scripts/diff-context.js --subproject {subproject_path}` per subproject to capture the current git state scoped to each subproject. Include the subproject-specific output in the agent prompt as `{diff_context}` so agents see only changes relevant to their scope.

**Skip if `resumeMode === "continued"`** unless a wave just completed (wave transitions always refresh diff). The prior diff snapshot is reused from `.claude/.pipeline-states/{specName}.diff-{subproject}.md`.

7. **Read** `.claude/pipeline-config.md`. For `entity-registry.json`: use Grep to extract ONLY the relevant entity block (e.g. `"Contract":`), NEVER read the full JSON
9. **Update spec header:** `Status: implementing`, `Phase: EXECUTE`, `Checkpoint: {ISO now}`
10. **Update/create pipeline state:** `status: "implementing"`, `phaseName: "EXECUTE"`, `specName`
11. **TaskCreate** — 1 per pending agent (skip completed)

### Step 3: Execute — Wave System

**CRITICAL: Main context IS the Pipeline Runner. NEVER delegate to intermediate Task agent.**

11b. **Pre-EXECUTE Rewave Check** (skip if `pipeline-state.isWavePlan === true`): Run `bun .claude/scripts/exec-rewave-check.js --spec .claude/spec/active/{specName}/spec.md`. Parse JSON output. If `action: "decomposed"`, the spec was split into N waves — update `pipeline-state.isWavePlan: true, currentWave: 1` and proceed using wave-1's spec (`wave-1-{role}/spec.md`). If `action: "keep-single"` or `"skip"`, continue with the original spec. Silent — no AskUserQuestion.

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
   - After updating state, run `bun .claude/scripts/wave-tree.js --spec-dir .claude/spec/active/{specName}` to show progress.
   - Force `resumeMode = "reanalyzed"` for the next wave transition so diff-context refreshes with the just-committed changes.
   - **Cache this wave's diff:** right after the wave-{N-1} commit, run `git diff HEAD~1 HEAD > .claude/.pipeline-states/{spec-name}.wave-{N-1}.diff.md` so the next wave can be sliced against it.
   - **Wave Slice Injection (see § Wave Slice Injection below):** before dispatching wave N (N≥2):
     1. Run `bun .claude/scripts/spec-extract.js --spec {spec_path} --wave {N}` to get the wave slice; save as `{spec_slice}`.
     2. Prepend the cached previous wave diff from `.claude/.pipeline-states/{spec-name}.wave-{N-1}.diff.md`. Inject `{spec_slice}` + the diff (NOT the full spec, NOT the full touched files) into the agent prompt.
   - If `currentWave > totalWaves` → skip remaining wave dispatch, go to Step 19 REVIEW + Step 20 CLOSE on the overall wave plan.
4. **If a wave fails (REJECTED after 2 fix-loops, or BLOCKED)** — see § Wave Failure Handling below.

#### Wave Slice Injection

Entre waves, o orquestrador NÃO reinjeta a spec inteira nem os arquivos de código completos nos prompts dos agentes — `spec-extract.js` recorta só a seção da wave (ou, em wave-plan, a sub-spec inteira) e o `git diff` cacheado substitui os arquivos full.

A subtraction `wave-slice` é emitida automaticamente pelo hook `subagent-tracker.js` em todo despacho de Task na fase EXECUTE — o orquestrador não faz nada. Fail-open: se a omissão não aconteceu (seção da wave não medível), nenhum evento é registrado.

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
        1. **Prepend `karpathy-guidelines`** for code-editing agents (impl/backend/frontend/database/bugfix). **Skip** for read-only Explore and Review agents.
        2. Glob `{subproject}/.claude/skills/` for generated pattern skills
        3. Add foundation skills matching the role (ui→design-craft+react-best-practices, mobile→design-craft)
        4. Format as bullet list: `- {skill-name}`

16. **Wave transitions** — between waves, execute transitions from `.claude/pipeline-config.md`:
    - After Wave 1 (api/database/library) completes, before Wave 2 (ui):
      - Execute each command listed in the matching `Wave Transitions` section
    - Wait for transitions to complete before dispatching next wave

17. **Dispatch:** TaskUpdate(in_progress) + pipeline state. ALL agents in same wave → SINGLE message (multiple Task invocations). **Pass `model` from pipeline state** (e.g. `model: "opus"`) in each Task tool call — this overrides the agent YAML default. On return: pipeline state update, TaskUpdate(completed), advance wave. The `checklist-auto-mark.js` hook marks Checklist items silently as files are edited. close-gate denies CLOSE if any `[ ]` remains.

17b. **Agent Memory:** After each wave completes, run `memory.js agent` once per agent (summary ≤300 chars, include `files_modified` + `decisions`). Skip if no downstream waves remain.

#### Escalation Status Handling

After each agent returns, check the return value for an escalation status before advancing to the next wave:

- **Internal error** (no parseable output, empty return, API error) — re-dispatch the failed agent(s) **sequentially** (not parallel) with the same prompt. Max 1 Internal retry per agent. If still failing: STOP + report
- `CONCERN` — record verbatim under `## Concerns` in the spec; continue to next wave
- `BLOCKED` — stop immediately; use `AskUserQuestion` to report the exact blocker; do NOT advance
- `PARTIAL` — apply Granular Retry Protocol from the last completed step; do NOT restart from step 1
- `DEFERRED` — note in spec with agent justification; ask user if the deferred item is load-bearing before closing

If two or more agents in the same wave return `CONCERN`, surface all concerns together before dispatching the next wave. See `.claude/pipeline-config.md` Escalation Statuses and Diagnostic Failure Routing for the full status table.

### Step 4: Validate, Review, QA & Complete

18. **VALIDATE** — Parse agent results: Backend→`dotnet build`, Frontend→`pnpm build`, Mobile→`fvm flutter analyze`. All passed → next. Failed → **granular retry** (see below).
19. **REVIEW (MANDATORY — NEVER SKIP):**
    - Dispatch review agent for EACH affected subproject in a SINGLE message (multiple Task invocations) using `subagent_type: "general-purpose"` with review prompt
    - Review agent MUST read `{subproject}/CLAUDE.md` + `{subproject}/.claude/commands/guards.md`
    - Checklist categories: **SOLID, Design System, Patterns, i18n, Integration, Build, Elegance**
    - Each issue classified: CRITICAL (blocks), WARNING (recommended), NOTE (suggestion)
    - APPROVED (zero CRITICAL) → CLOSE
    - REJECTED (any CRITICAL) → see Step 19b § Fix Loop Dispatch Protocol (max 2 loops)
    - **NEVER skip review** — not even for Light scope. Light scope gets same checklist, just fewer files to review

### Step 19b: Fix Loop Dispatch Protocol

Extract CRITICAL findings verbatim from review return (or harness view). Build retry context using Mode=fix-loop (K=1 or 2). Dispatch same subagent_type + model. Re-dispatch review after. Max 2 fix-loops → STOP.

→ See `../../../refs/resume/fix-loop-wave.md`

### Step 19c: QA Phase (Wave 10) — MANDATORY

After REVIEW returns APPROVED, run QA before CLOSE. NEVER go REVIEW→CLOSE directly — `close-gate.js` denies CLOSE without a passing `qa.result` event.

1. Update pipeline state via Write/Edit: `phaseName: "QA"`.
2. Run `bun .claude/scripts/qa-run.js --spec {specName}`. For wave plans, `{specName}` is the wave-plan directory name.
3. Branch on `overall`:
   - **pass** — update `## Acceptance Criteria` checkboxes in the spec (`[x]` for each passed AC), then update pipeline state via Write/Edit with `phaseName: "CLOSE"` (this Write triggers `close-gate.js`, which verifies the `qa.result` event before allowing CLOSE) → proceed to Step 20.
   - **fail** — extract the failing AC list and re-dispatch via the Step 19b Fix Loop Dispatch Protocol, then re-run this step. Maximum 3 QA iterations.
   - **skip** (no Acceptance Criteria section) — inform inline `QA pulado — spec sem Acceptance Criteria`, then proceed to Step 20.
4. After 3 failed QA iterations → `AskUserQuestion`: "(a) corrigir manualmente e repetir, (b) relaxar o AC na spec, (c) abortar pipeline."
5. Visual: `[v] EXECUTE  [v] REVIEW  [>] QA  [ ] CLOSE`.

20. **CLOSE:**
    - **Wave plan gate:** if `pipeline-state.isWavePlan === true`, only CLOSE when `completedWaves.length === totalWaves`. If waves remain (`currentWave <= totalWaves` and wave N-1 just finished), **do not** run CLOSE — instead update state (`currentWave++`, `completedWaves.push`), output `═══ WAVE {N-1} COMPLETE — {role} ═══`, and stop. Next `/mustard:resume` picks up wave N.
    - `bun .claude/scripts/sync-registry.js`
    - Spec: `Status: completed`, `Phase: CLOSE`, all `[ ]` → `[x]`. For wave plans: mark `wave-plan.md` status `completed`, and mark each `wave-N-{role}/spec.md` completed too.
    - Move spec to `.claude/spec/completed/` (the entire `{specName}/` directory, including wave subdirs if any)
    - **Delete** `.claude/.pipeline-states/{spec-name}.json`
    - **Pipeline Summary (BEFORE banner):** run `bun .claude/scripts/pipeline-summary.js --spec-dir .claude/spec/active/{specName}` and print the markdown inline. For wave plans, the same spec-dir applies (the script reads the root `spec.md`; for wave-plan-final closes, use the wave-plan dir). Fail-open: on non-zero exit or missing script, log a warning and continue with the banner — do NOT abort CLOSE. Apply to both single-spec and wave-plan-final paths.
    - **Wave Tree (before banner):** `bun .claude/scripts/wave-tree.js --spec-dir .claude/spec/active/{specName}` (or `completed/` if already moved). Fail-open.
    - Output with agent colors: `═══ PIPELINE COMPLETE — {name} | Agents: {n} ok | Files: {c} created, {m} modified ═══` (for wave plans: append `| Waves: {totalWaves}`).

### Wave Failure Handling

Only when `isWavePlan === true`. Triggers: REJECTED after 2 fix-loops, BLOCKED by user, or repeated build failures. Updates `failedWaves`, writes `failure.md` log, then asks user: fix manually / re-PLAN wave / abort. Preserves prior wave commits.

→ See `../../../refs/resume/fix-loop-wave.md`

### Granular Retry Protocol

Parse last completed step → retry only from that step forward. Build Mode=granular retry context (harness view or agent memory fallback). Use Minimal Retry Template. Max 2 retries per agent.

→ See `../../../refs/resume/fix-loop-wave.md`

### Pause Handoff & Next Action Rule

On pause: write `pausedAt`/`pauseReason`/`nextAction` to pipeline state, then `memory.js agent`. Handoff MUST end with exactly ONE next action (no lists of options).

→ See `../../../refs/resume/fix-loop-wave.md`

## INVIOLABLE RULES

- Main context IS the Pipeline Runner — NEVER wrap in a single Task agent
- NEVER implement code directly — ALL via Task agents (1 per subproject per wave)
- Wave dispatch: ALL agents in same wave in a SINGLE message
- Each sub-agent reads its own `{subproject}/CLAUDE.md` + auto-loads relevant skills (orchestrator does NOT read them)
- Update pipeline state + spec checkboxes at each transition
- ALWAYS read `.claude/pipeline-config.md` for agent/wave/model config — NEVER hardcode project-specific values
- ALWAYS use agent-prompt template — NEVER build prompts from scratch
- ALWAYS execute wave transitions between waves
- ALWAYS run QA (Step 19c) after REVIEW and before CLOSE — NEVER go REVIEW→CLOSE directly

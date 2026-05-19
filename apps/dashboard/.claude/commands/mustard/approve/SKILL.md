# /approve - Approve Spec

## Trigger

`/approve [--resume]`

## Description

Approves the active spec and prepares the implementation phase.

- **Default** (`/approve`): prepares pipeline state and STOPS, instructing the user to run `/resume` in a new session with clean context. Recommended for Full-scope specs with 5+ files.
- **With `--resume` flag** (`/approve --resume`): after preparation, immediately hands off to the `/resume` flow in the same session (skips `/resume` Step 0 and Step 1 — no dispatch-failure check, no handoff summary, no re-confirmation). Use when the spec was just approved and you want to skip the session-restart hop. Tradeoff: the EXECUTE phase inherits the ANALYZE+PLAN context instead of starting clean — fine for small/medium specs, less efficient for large ones.

## Prerequisites

- Active spec in `.claude/spec/active/`
- Spec presented to user and awaiting approval

## Action

1. **Step 0: AUTO-SYNC (MANDATORY)** — Run via Bash tool BEFORE any other action:
   - `node .claude/scripts/sync-registry.js`
   - Do NOT proceed to step 2 without running this command
2. **Read** `.claude/pipeline-config.md` — agents, model selection
3. Locate active spec in `.claude/spec/active/`

### Step 3b: Wave Plan Detection

Check if the located spec is a wave plan: look for `.claude/spec/active/{specName}/wave-plan.md`.

**If `wave-plan.md` exists:**

1. Read `.claude/.pipeline-states/{specName}.json` — expect `isWavePlan: true`, `totalWaves: N`, `currentWave: 1`, `completedWaves: []`.
2. Read `wave-plan.md` and print its ENTIRE contents verbatim inside a fenced markdown block (```` ```markdown ... ``` ````). List each wave spec file path below the block (one line each).
3. `AskUserQuestion`:
   - **"Approve wave plan — start with wave 1"** → proceed to step 4 (update header + state for wave 1 dispatch)
   - **"Reject decomposition — use single spec"** → merge all wave specs back into a single spec at `.claude/spec/active/{specName}/spec.md` (concatenate `## Files`, `## Tasks`, `## Boundaries` from each wave), delete `wave-plan.md` and `wave-N-*/` subdirectories, set `scopeOverride: "user-rejected-waves"` and `isWavePlan: false` in pipeline state, proceed to step 4 on the single spec
   - **"Stop — re-plan with guidance"** → stop. Instruct user: `Delete .claude/spec/active/{specName}/ and re-run /feature {name} with explicit guidance (e.g., "keep wave 2 and wave 3 together").`
4. If user approved wave plan, for step 4 and onward, operate on the **wave 1 spec** (`.claude/spec/active/{specName}/wave-1-{role}/spec.md`) — update its header, not the wave-plan.md header.

**If `wave-plan.md` does NOT exist:** proceed as a single spec (original behavior below).

4. **Spec Checkpoint — update spec header:**
   - `### Status: approved`
   - `### Phase: PLAN`
   - `### Checkpoint: {ISO timestamp now}`
5. **Pipeline State — create or update `.claude/.pipeline-states/{spec-name}.json`:**
   - Extract `spec-name` from the spec directory (e.g. basename of path → `2026-02-26-linked-services-card`)
   - **If wave plan (from Step 3b):** state already exists. Update fields: `status: "approved"`, `currentWave: 1`, `updatedAt`. Parse tasks from **wave-1** spec only (not all waves). Preserve `isWavePlan`, `totalWaves`, `completedWaves`, `failedWaves`.
   - **If single spec:** Parse Tasks from spec to extract tasks per agent (DB, Backend, Frontend, etc.). Create `.claude/.pipeline-states/` directory if it doesn't exist. Write state file with `specName`, `status: "approved"`, `phaseName: "PLAN"`, `tasks` with names and agents, `model`, `updatedAt`.
5b. **Memory Persist — record architectural decisions:**
   - For each significant decision in the spec (technology choices, design patterns, trade-offs):
     ```bash
     echo '{"type":"decision","content":"<decision description>","source":"<spec-name>","context":"approved at PLAN phase"}' | node .claude/scripts/memory-persist.js
     ```
   - Focus on: why a pattern was chosen over alternatives, constraints that shaped the design
   - Skip trivial or obvious decisions (max 3 entries)
6. **Model selection** — read `Model Selection` from `.claude/pipeline-config.md` and record `"model"` field in state:
   - Count total estimated files in spec
   - Apply rule: ≤5 files/known patterns → `"model": "sonnet"`, 5+ files/new patterns → `"model": "opus"`
7. **Task Tracking — create TaskCreate for each agent:**
   - 1 TaskCreate per agent identified in spec
   - Subject: `"{Layer}: {brief description}"`
   - activeForm: `"Running {Layer} agent"`
8. **Output — visual feedback:**
   - Output progress line: `[v] ANALYZE  [v] PLAN  [>] EXECUTE  [ ] CLOSE`
9. **Branch on `--resume` flag:**

   **Without `--resume` (default) — STOP and instruct user to start a new session:**
   - Do NOT execute implementation in this session (context already consumed by /feature + /approve)
   - Final output:

     ```
     Spec approved and pipeline prepared.
     Open a new session and run /resume to start implementation with clean context.
     ```

   - **CRITICAL**: Do NOT dispatch Task agent, do NOT implement code — just STOP

   **With `--resume` — hand off to `/resume` flow in the same session:**
   - Inform user: `Spec approved. Resuming inline (--resume). Dispatching EXECUTE directly.`
   - Jump to `/resume` **Step 2: Bootstrap** (`.claude/commands/mustard/resume/SKILL.md`)
   - **SKIP** `/resume` Step 0 (Dispatch Failure Pre-Check — not applicable, state was just created above) and Step 1 (Detect & Confirm — the spec is already known, user just approved it)
   - From Step 2 onwards, follow the full `/resume` flow: AUTO-SYNC → Diff Context → Wave System → VALIDATE → REVIEW → CLOSE
   - Apply all INVIOLABLE RULES from `/resume` (main context IS the Pipeline Runner, wave dispatch in single message, etc.)

## Alternative Flow

If the spec is not satisfactory:
- Provide textual feedback for adjustments
- Use /complete to cancel

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
4. **Spec Checkpoint — update spec header:**
   - `### Status: approved`
   - `### Phase: PLAN`
   - `### Checkpoint: {ISO timestamp now}`
5. **Pipeline State — create `.claude/.pipeline-states/{spec-name}.json`:**
   - Extract `spec-name` from the spec directory (e.g. basename of path → `2026-02-26-linked-services-card`)
   - Parse Tasks from spec to extract tasks per agent (DB, Backend, Frontend, etc.)
   - Create `.claude/.pipeline-states/` directory if it doesn't exist
   - Write state file with `specName`, `status: "approved"`, `phaseName: "PLAN"`, `tasks` with names and agents, `model`, `updatedAt`
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

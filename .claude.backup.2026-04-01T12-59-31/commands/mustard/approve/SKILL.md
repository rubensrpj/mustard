# /approve - Approve Spec

## Trigger

`/approve`

## Description

Approves the active spec and prepares the implementation phase. Does NOT execute implementation — only prepares state and instructs user to run `/resume` in a new session.

## Prerequisites

- Active spec in `.claude/spec/active/`
- Spec presented to user and awaiting approval

## Action

1. **Step 0: AUTO-SYNC (MANDATORY)** — Run via Bash tool BEFORE any other action:
   - `node .claude/scripts/sync-registry.js`
   - Do NOT proceed to step 2 without running this command
2. **Read** `pipeline-config.md` — agents, model selection
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
6. **Model selection** — read `Model Selection` from `pipeline-config.md` and record `"model"` field in state:
   - Count total estimated files in spec
   - Apply rule: ≤5 files/known patterns → `"model": "sonnet"`, 5+ files/new patterns → `"model": "opus"`
7. **Task Tracking — create TaskCreate for each agent:**
   - 1 TaskCreate per agent identified in spec
   - Subject: `"{Layer}: {brief description}"`
   - activeForm: `"Running {Layer} agent"`
8. **Output — visual feedback:**
   - Output progress line: `[v] ANALYZE  [v] PLAN  [>] EXECUTE  [ ] CLOSE`
9. **STOP and instruct user to start a new session:**
   - Do NOT execute implementation in this session (context already consumed by /feature + /approve)
   - Final output:

   ```
   Spec approved and pipeline prepared.
   Open a new session and run /resume to start implementation with clean context.
   ```

   - **CRITICAL**: Do NOT dispatch Task agent, do NOT implement code — just STOP

## Alternative Flow

If the spec is not satisfactory:
- Provide textual feedback for adjustments
- Use /complete to cancel

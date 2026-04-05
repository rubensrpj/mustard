# /bugfix - Bug Fix Pipeline

> ALWAYS before making any change. Search on the web for the newest documentation and only implement if you are 100% sure it will work.

## Trigger

`/bugfix <error-description>`

## Description

Autonomous pipeline to diagnose and fix bugs. Zero context-switch — never ask the user what can be discovered autonomously.

## Procedure

### ANALYZE (diagnose + assess)

1. **AUTO-SYNC:** Run `node .claude/scripts/sync-detect.js`. If output shows any subproject with `hashChanged: true`, then run `node .claude/scripts/sync-registry.js`. Otherwise skip sync-registry entirely.

### Diff Context (automatic)

**Diff snapshot (run once per phase):**
Run `node .claude/scripts/diff-context.js` at the start of ANALYZE and EXECUTE. Save the output to `.claude/.pipeline-states/{specName}.diff.md` (overwrite each phase).

**Inject into every Task dispatch in this pipeline:**
Prepend the following to EVERY subagent prompt dispatched during the pipeline:

```
## Current Git State
{contents of .claude/.pipeline-states/{specName}.diff.md}

## Your Task
...original prompt...
```

If the diff file is empty or missing, skip the Git State header entirely. Never dispatch an agent without attempting interpolation.

2. **DIAGNOSE:** Dispatch Explore agent (**≤20 tool uses, ≤3 full file reads**):
   - Scoped Grep searches with specific path + pattern for the error/symptom
   - Trace callers/callees via Grep in relevant directories (prefer Grep over Read)
   - Return as soon as root cause is clear — don't exhaustively scan
   - Return: root cause file(s), line(s), explanation
3. **ASSESS — Decision point:**
   - Explore returns clear root cause in 1-2 files → **Fast Path** (skip PLAN)
   - 3+ files, unclear impact, cross-layer → **Full Path** (brief spec via PLAN)

**Fast Path:** Go directly to EXECUTE.
**Full Path:** Write brief spec in `.claude/spec/active/{date}-{name}/spec.md` → present to user → `/approve` → EXECUTE.

- Fast Path CAN use Task(Explore) ONCE with ≤10 tool uses. Prefer Grep/Glob direct when the root cause location is known.
- If >5 files surface during DIAGNOSE, RECLASSIFY to Full Path and write a spec before proceeding.

#### Spec Boundaries

When writing a Full Path spec (or noting files for Fast Path), record which files are in scope under a `## Boundaries` section:

```
## Boundaries
- `path/to/directory/` — directory scope (all files within)
- `path/to/file.ext` — exact file
- `**/*.controller.ts` — glob pattern
```

Rules:
- List only files the fix **intentionally** touches (root cause + direct dependants)
- For Fast Path: boundaries are implicit from the ANALYZE output — no spec section required
- Out-of-boundary edits during EXECUTE will surface a `[BOUNDARY WARNING]` from guard-verify — re-evaluate scope before proceeding

### EXECUTE (fix + validate)

Every agent prompt dispatched in Fast Path MUST include:
`Return format cap: ≤50 lines. Apply compact Return Format from pipeline-config.md strictly.`

Dispatch bugfix agent with:
- Root cause from ANALYZE
- `{subproject}/CLAUDE.md` + `{subproject}/.claude/commands/guards.md` for context
- Specific files to modify
- Expected behavior after fix

**Validate:**
- Build check: `dotnet build` / `pnpm typecheck` (as applicable)
- Verify fix resolves the reported issue
- No regression in adjacent code
- If build fails: diagnose + fix (max 3 iterations)

#### Escalation Status Handling

After the bugfix agent returns, check for an escalation status before closing:

- `CONCERN` — record verbatim in the bugfix report under `## Concerns`; continue to CLOSE
- `BLOCKED` — stop immediately; use `AskUserQuestion` to report the exact blocker; do NOT close
- `PARTIAL` — agent fixed some but not all reported issues; resume from the last incomplete fix step (max 2 retries)
- `DEFERRED` — agent intentionally left a related issue unfixed with justification; confirm with user before closing

See `pipeline-config.md` Escalation Statuses for the full status table.

#### Retry Compact Advisory
If an agent fails and requires >2 retry attempts during EXECUTE:
- Suggest to user: _"Multiple retries detected — stale context may be contributing. Consider `/compact` to clear context, then `/resume` to continue the pipeline."_
- This is advisory only — continue fixing if user declines.

#### Failure Routing (Bugfix)

Before retrying a failed fix attempt, classify the failure:

1. **Transient?** — Would re-running succeed without any change? (flaky test, cache, env) → Retry once immediately.
2. **Resolvable?** — Is the fix clear and patchable in ≤3 lines without new reads? → Apply patch, retry (counts as retry 1).
3. **Structural?** — Did the original ANALYZE misidentify the root cause? → Re-analyze: dispatch a focused Explore on the actual failure point, update root cause, re-dispatch bugfix agent. Does NOT count against the 2-retry cap.

Max 2 retries for Transient + Resolvable. Structural failures trigger a targeted re-ANALYZE, not a blind retry.

### CLOSE

- `node .claude/scripts/sync-registry.js` (if entities changed)
- Output bugfix report (diagnosis, fix, validation)

## Zero Context-Switch Protocol

- NEVER ask "can you show the error?" — find it via logs/Grep
- NEVER ask "which file?" — trace from the error
- NEVER ask "how to fix?" — propose + implement
- CI test fails: read → fix → re-run — without reporting and waiting
- MANDATORY: Follow Visual Output, Pipeline State, Task Tracking rules at each phase

ULTRATHINK

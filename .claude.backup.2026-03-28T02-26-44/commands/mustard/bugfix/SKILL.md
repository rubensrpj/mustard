# /bugfix - Bug Fix Pipeline

> ALWAYS before making any change. Search on the web for the newest documentation and only implement if you are 100% sure it will work.

## Trigger

`/bugfix <error-description>`

## Description

Autonomous pipeline to diagnose and fix bugs. Zero context-switch — never ask the user what can be discovered autonomously.

## Procedure

### ANALYZE (diagnose + assess)

1. **AUTO-SYNC:** `node .claude/scripts/sync-registry.js`
2. **DIAGNOSE:** Dispatch Explore agent:
   - Scoped Grep searches with specific path + pattern for the error/symptom
   - Trace callers/callees via Grep in relevant directories
   - Return: root cause file(s), line(s), explanation
3. **ASSESS — Decision point:**
   - Explore returns clear root cause in 1-2 files → **Fast Path** (skip PLAN)
   - 3+ files, unclear impact, cross-layer → **Full Path** (brief spec via PLAN)

**Fast Path:** Go directly to EXECUTE.
**Full Path:** Write brief spec in `.claude/spec/active/{date}-{name}/spec.md` → present to user → `/approve` → EXECUTE.

### EXECUTE (fix + validate)

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

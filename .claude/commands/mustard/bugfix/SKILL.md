# /bugfix - Bug Fix Pipeline

> ALWAYS before making any change. Search on the web for the newest documentation and only implement if you are 100% sure it will work.

## Trigger

`/bugfix <error-description>`

## Description

Autonomous pipeline to diagnose and fix bugs. Zero context-switch — never ask the user what can be discovered autonomously.

## Procedure

### Spec Hygiene (automatic, before ANALYZE)

Before starting a new pipeline, audit specs in `active/`:

1. **Scan** all specs in `.claude/spec/active/*/spec.md`
2. **For each spec**, read the full header and checklist to extract `Status:`, `Phase:`, and checkbox completion (`[x]` vs `[ ]`)
3. **Verify completed/cancelled specs before moving:**
   - If `Status: completed` or `Status: cancelled`:
     - **Analyze first**: check that ALL checklist items are `[x]`, no `## Concerns` with unresolved `BLOCKED` items, and build/type-check references are satisfied
     - If analysis confirms done → move from `.claude/spec/active/{name}/` to `.claude/spec/completed/{name}/`, delete `.claude/.pipeline-states/{name}.json` and `.diff.md` if they exist, log: `[HYGIENE] Verified and moved {name} → completed/`
     - If analysis finds incomplete items → update `Status: implementing`, log: `[HYGIENE] {name} marked completed but has {N} unchecked items — reverted to implementing`, then treat as in-progress (step 4)
4. **In-progress specs** (`Status: draft` or `Status: implementing`):
   - Use `AskUserQuestion`: _"Found spec in progress: **{name}** (Status: {status}, Phase: {phase}, {done}/{total} tasks done). Do you want to continue this spec before starting a new one?"_
   - If **yes** → stop, suggest `/resume` to continue the existing spec
   - If **no** → proceed to ANALYZE for the new pipeline (existing spec stays in `active/`)
5. **No active specs** → proceed to ANALYZE normally

This step is silent when there's nothing to audit — no output if `active/` is empty.

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

2b. **Cache root-cause for retry reuse:**

After DIAGNOSE returns, compute a cache signature so fix-loop retries can skip re-DIAGNOSE when the affected surface hasn't changed:

```javascript
// in-memory during bugfix session (also persisted to pipeline-state for Full Path)
const affectedFiles = [...root-cause file(s) from Explore return, sorted];
const bugDescription = {user's error description, canonical — trimmed and lowercased};
const rootCauseHash = sha256(bugDescription + '|' + affectedFiles.join(','));
const rootCauseSummary = {1-line root cause from Explore, ≤500 chars};
const affectedFilesHash = sha256(concatenated contents of affectedFiles right now);
```

Write to pipeline-state if Full Path (`.claude/.pipeline-states/{specName}.json`):
```json
{
  "rootCauseHash": "sha256...",
  "rootCauseSummary": "...",
  "affectedFilesHash": "sha256...",
  "affectedFiles": ["path/a.ts", "path/b.ts"],
  "cachedAt": "{ISO}"
}
```

For Fast Path (no spec yet), keep the cache in-memory only — it lives for the duration of the bugfix session, which is sufficient for the retry loop.
3. **ASSESS — Decision point:**
   - Explore returns clear root cause in 1-2 files → **Fast Path** (skip PLAN)
   - 3+ files, unclear impact, cross-layer → **Full Path** (brief spec via PLAN)

**Fast Path:** Go directly to EXECUTE. No spec, no approval gate (Zero Context-Switch Protocol). If you want to review the fix plan before EXECUTE, force Full Path by listing >5 files in the ANALYZE return.
**Full Path:** Write brief spec in `.claude/spec/active/{date}-{name}/spec.md`. The spec MUST include (Wave 10):
   ```markdown
   ## Acceptance Criteria

   - [ ] AC-1: Bug is no longer reproducible — Command: `{command that previously triggered the bug}`
   - [ ] AC-2: {additional verification if applicable} — Command: `{cmd}`
   ```
   Minimum 1 AC: the reproduction command for the bug (exits non-zero before fix, exits 0 after fix).
   Then **present the full spec to the user before stopping**:
   - Read the spec file just written and print its ENTIRE contents verbatim inside a fenced markdown block (```` ```markdown ... ``` ````). Do NOT summarize — the user asked to read the complete plan before approving.
   - After the fenced block, instruct: _"Run `/approve` (or `/approve --resume` to chain inline) to proceed to EXECUTE."_

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
`Return format cap: ≤50 lines. Apply compact Return Format from .claude/pipeline-config.md strictly.`

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

See `.claude/pipeline-config.md` Escalation Statuses for the full status table.

#### Retry Compact Advisory
If an agent fails and requires >2 retry attempts during EXECUTE:
- Suggest to user: _"Multiple retries detected — stale context may be contributing. Consider `/compact` to clear context, then `/resume` to continue the pipeline."_
- This is advisory only — continue fixing if user declines.

#### Failure Routing (Bugfix)

Before retrying a failed fix attempt, classify the failure:

1. **Transient?** — Would re-running succeed without any change? (flaky test, cache, env) → Retry once immediately.
2. **Resolvable?** — Is the fix clear and patchable in ≤3 lines without new reads? → Apply patch, retry (counts as retry 1).
3. **Structural?** — Did the original ANALYZE misidentify the root cause? → **Before re-Exploring, consult the root-cause cache from Step 2b:**
   - Recompute `affectedFilesHash` for the cached `affectedFiles`.
   - **Cache hit (hash matches) AND failure signal does NOT suggest a different cause** (no keyword in the failure pointing to files outside `affectedFiles`, no REVIEW rationale explicitly naming a different root) → skip re-Explore, inject `rootCauseSummary` verbatim into the retry prompt. Log: `root-cause cached (retry {N}/2), skipping diagnose`.
   - **Cache miss (files changed) OR failure rationale points elsewhere** → invalidate cache, run targeted Explore on the actual failure point, update root cause (including new cache entry via Step 2b), re-dispatch bugfix agent.
   - Re-ANALYZE (with or without cache) does NOT count against the 2-retry cap.

Max 2 retries for Transient + Resolvable. Structural failures trigger a targeted re-ANALYZE (cache-gated), not a blind retry.

**Cache invalidation signals:**
- Affected files changed on disk → hash mismatch invalidates
- Review/build failure rationale mentions files outside `affectedFiles` → invalidate
- User explicitly overrides (rare) → invalidate
- After 2 retries exhausted, the cache is naturally flushed when the pipeline aborts or advances

### QA Phase (Wave 10)

After EXECUTE (fix + validate) completes:

1. Update pipeline state: `phaseName: "QA"`
2. Run: `node .claude/scripts/qa-run.js --spec {specName}` (Full Path only)
   - For Fast Path: manually verify the bug reproduction command exits 0, emit result to harness
3. If `overall=pass`: proceed to CLOSE
4. If `overall=fail`: the bug reproduction AC still fails — return to EXECUTE for targeted fix, max 3 QA iterations
5. Maximum 3 QA iterations — after that, escalate to user

### CLOSE

- `node .claude/scripts/sync-registry.js` (if entities changed)
- Output bugfix report (diagnosis, fix, validation, QA result)

## Zero Context-Switch Protocol

- NEVER ask "can you show the error?" — find it via logs/Grep
- NEVER ask "which file?" — trace from the error
- NEVER ask "how to fix?" — propose + implement
- CI test fails: read → fix → re-run — without reporting and waiting
- MANDATORY: Follow Visual Output, Pipeline State, Task Tracking rules at each phase
ULTRATHINK

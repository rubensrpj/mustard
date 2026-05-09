# /complete - Finalize Pipeline

## Trigger

`/complete`

## Description

Finalizes the current pipeline, either completing or canceling.

## Verification Gate (MANDATORY)

1. **Review completed**: Check pipeline state — review agent MUST have run and returned APPROVED. If not → dispatch review first (see resume.md step 19)
2. **Build passes**: run build command for each affected subproject (from .claude/pipeline-config.md)
3. **Changes match spec**: each `[x]` corresponds to a real file
4. **Zero CRITICAL issues**: review report shows zero CRITICAL violations (SOLID, design system, patterns, i18n, integration)
5. **No regressions**: existing features still work

#### Verification Gate (automated)

Before finalizing, run build/test verification:

```bash
node .claude/scripts/verify-pipeline.js "$PROJECT_DIR"
```

- **Exit 0** (all passed): proceed to completion
- **Exit 1** (failures): report failed builds/tests to user, do NOT proceed
  - Show: which subproject failed, which command, error excerpt
  - Ask user: fix and retry, or force-complete anyway
- **Script missing/errors**: warn but continue (fail-open)

If ANY gate fails: do NOT mark complete → report what failed + suggest fix. If review wasn't run → run it now before completing.

Re-reviews always dispatch with `model: "sonnet"` (see `review/SKILL.md § Model Selection`).

#### Surface Accumulated Concerns

Before finalizing, scan the active spec for any `## Concerns` section written during EXECUTE:

- If concerns exist: list them in the completion output under `## Concerns Surfaced`
- If any concern was classified `BLOCKED` or was never resolved: do NOT complete — report to user first
- If all concerns are `CONCERN` or `DEFERRED` (non-blocking): note them and proceed
- This step is a read-only scan — do NOT alter or dismiss concerns during CLOSE

See `.claude/pipeline-config.md` Escalation Statuses for concern classification rules.

## Action

1. Locate active spec in `.claude/spec/active/`
2. If none exists → inform user and stop
3. **Spec Checkpoint — update spec header:**
   - `### Status: completed`
   - `### Phase: CLOSE`
   - `### Checkpoint: {ISO timestamp now}`
   - **Verify Checklist consistency** — count `- [ ]` lines in `## Checklist`. If any remain, ABORT and report the unmarked items to the user (each item should already have been marked by the executor agent during EXECUTE via `mark-checklist-item.js`). Do NOT batch-mark on behalf of the agents. `close-gate.js` enforces this same rule with `MUSTARD_CHECKLIST_GATE_MODE=strict`.
4. **Entity Registry — update if needed:**
   - `node .claude/scripts/sync-registry.js`
   - **Schema-aware refresh (conditional):** If the spec's `## Files` section touched any file matching `*.schema.ts`, `*.entity.ts`, `*.prisma`, `*DbContext*.cs`, or `schema.rs`, run `rtk node .claude/scripts/sync-registry.js` to refresh `entity-registry.json`. If sync-registry fails (non-zero exit or script missing), log a warning and continue with close — this step is non-blocking.
5. **Move spec** from `.claude/spec/active/` to `.claude/spec/completed/`
6. **Pipeline State — cleanup:**
   - Extract `spec-name` from the spec directory (e.g. `2026-02-26-linked-services-card`)
   - **Delete** `.claude/.pipeline-states/{spec-name}.json` (removes from statusline)
6b. **Knowledge Capture:**
   - Review patterns discovered during this pipeline
   - For each significant pattern/convention/entity discovered:
     ```bash
     echo '{"type":"pattern","name":"...","description":"...","source":"{spec-name}"}' | node .claude/scripts/knowledge-update.js
     ```
   - Focus on: naming conventions used, architectural decisions, integration patterns
   - Skip trivial or already-known patterns

6b2. **Lessons Persist — record lessons learned:**
   - Review what went well or poorly during this pipeline
   - For each lesson worth remembering across sessions:
     ```bash
     echo '{"type":"lesson","content":"<lesson description>","source":"<spec-name>","context":"learned during EXECUTE/CLOSE"}' | node .claude/scripts/memory-persist.js
     ```
   - Focus on: integration gotchas, naming issues discovered, performance pitfalls
   - Skip trivial or already-captured lessons (max 3 entries)

6c. **Token Economy — RTK report (if available):**
   - Run `rtk gain --all --format json` via Bash
   - If RTK available: extract `saved_tokens` and `savings_pct`
   - Include in output block below
6d. **Metrics Archive:**
   - Read metrics from `.claude/.pipeline-states/{spec-name}.json`
   - If metrics exist, ensure `.claude/metrics/` directory exists
   - Save to `.claude/metrics/{spec-name}.json`:
     ```json
     {
       "name": "{spec-name}",
       "completedAt": "{ISO timestamp}",
       "durationMs": "{calculated from startedAt to now}",
       "apiCalls": "{from metrics}",
       "retries": "{from metrics}",
       "pass1": "{true if metrics.retries === 0, otherwise false}",
       "toolBreakdown": "{from metrics}",
       "agentAttempts": "{from metrics, if present}",
       "gate_saves": "{from metrics, if present}",
       "wave_reentry": "{from metrics, if present}",
       "skillHits": "{from metrics, if present}",
       "rtkSavings": { "saved": N, "pct": N }
     }
     ```
   - Set `"pass1": true` if `metrics.retries === 0`, otherwise `"pass1": false`
   - Omit `agentAttempts`, `gate_saves`, `wave_reentry`, `skillHits` if not present in state metrics
   - If no metrics in state file, skip silently
7. **Output — visual feedback:**

   ```
   ================================================================
     PIPELINE COMPLETE — {spec-name}
     Agents: {n} ok | Files: {created} created, {modified} modified
     [v] Registry updated | [v] Spec moved to completed/
     Token Economy: {saved}k saved ({pct}% reduction) — RTK
   ================================================================
   ```

   If RTK is not installed or the gain command fails, omit the Token Economy line.

## Cancellation Flow

If the user wants to cancel (not complete):
- Update spec: `### Status: cancelled`
- Move to `completed/` anyway (for history)
- Delete `.claude/.pipeline-states/{spec-name}.json`
- Output: "Pipeline cancelled. Spec archived in completed/."

## Results Documentation

On completion, the output must include:
- Summary of changes (what and why)
- Files created/modified

### Wave 8 — Epic Auto-Fold

After marking a spec CLOSE, check if the parent epic is now complete:
```bash
node .claude/scripts/epic-fold.js --detect
```
If output lists epics ready to fold:
```bash
node .claude/scripts/epic-fold.js --epic <name>
```
This consolidates learning into knowledge.json and marks granular events compactable.

## When to Use

- After successful implementation and review
- To cancel an ongoing pipeline
- To force close if something went wrong

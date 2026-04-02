# /complete - Finalize Pipeline

## Trigger

`/complete`

## Description

Finalizes the current pipeline, either completing or canceling.

## Verification Gate (MANDATORY)

1. **Review completed**: Check pipeline state — review agent MUST have run and returned APPROVED. If not → dispatch review first (see resume.md step 19)
2. **Build passes**: run build command for each affected subproject (from pipeline-config.md)
3. **Changes match spec**: each `[x]` corresponds to a real file
4. **Zero CRITICAL issues**: review report shows zero CRITICAL violations (SOLID, design system, patterns, i18n, integration)
5. **No regressions**: existing features still work

If ANY gate fails: do NOT mark complete → report what failed + suggest fix. If review wasn't run → run it now before completing.

## Action

1. Locate active spec in `.claude/spec/active/`
2. If none exists → inform user and stop
3. **Spec Checkpoint — update spec header:**
   - `### Status: completed`
   - `### Phase: CLOSE`
   - `### Checkpoint: {ISO timestamp now}`
   - Mark all remaining `[ ]` as `[x]`
4. **Entity Registry — update if needed:**
   - `node .claude/scripts/sync-registry.js`
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
       "toolBreakdown": "{from metrics}",
       "rtkSavings": { "saved": N, "pct": N }
     }
     ```
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

## When to Use

- After successful implementation and review
- To cancel an ongoing pipeline
- To force close if something went wrong

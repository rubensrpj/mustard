# Spec Hygiene Reference

> Detail for `/feature` — automatic spec audit before ANALYZE.

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

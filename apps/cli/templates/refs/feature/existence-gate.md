# Existence Gate Reference

> Detail for `/feature` — Pre-EXECUTE Existence Gate (Full scope only).

### Pre-EXECUTE Existence Gate (Full scope only)

**Skip conditions**: Light scope OR `## Files` section lists more than 8 files (cost-benefit inverts — the explorer's self-cap of ≤10 tool uses will not cover).

Before dispatching implementation agents, run 1 explorer to verify the work is still needed.

**Pre-check (free, zero LLM tokens)**: Before dispatching the explorer, run:

```bash
rtk git diff --stat HEAD -- <files listed in `## Files` of spec>
```

Skip rules based on pre-check output:
- **Empty output** (no changes) → skip gate entirely, proceed to EXECUTE normally (nothing to verify)
- **<10 total insertions/deletions** → skip gate entirely, proceed to EXECUTE normally (trivial changes, verification not worth the overhead)
- **≥10 insertions/deletions** → proceed with the explorer dispatch below

**Dispatch:**

```javascript
Task({
  subagent_type: "Explore",
  description: "Pre-EXECUTE existence check",
  prompt: `# EXISTENCE CHECK
Read .claude/spec/{specName}/spec.md sections: "## Files" and "## Checklist".

For EACH checklist task (task-level, NOT file-level):
  1. Extract 1-3 concrete identifiers from the task text — function names, component names, file path fragments, string literals.
     Example: task "Add LogoutButton component with handleLogout handler" → identifiers: ["LogoutButton", "handleLogout"].
  2. Identify target files for the task from "## Files" (match by extension, name hint, or task context).
  3. Grep each target file for the identifiers.
  4. Verdict for this task:
     - ALL target files contain a MAJORITY of identifiers → all_present=yes
     - SOME do, SOME do not → all_present=partial
     - NONE do → all_present=no

Return a markdown table:
| task | target_files | all_present | evidence |
|------|--------------|-------------|----------|
| <task text> | <comma-sep files> | yes/partial/no | <identifier:line or "none"> |

Return ≤20 lines total. Self-cap: ≤10 tool uses (the tool-use budget is the true limit, not the task count).`
})
```

**Decision after return (orchestrator inspects the returned table):**

- **All tasks `all_present=no`** → Gate is transparent. Proceed to EXECUTE normally.
- **Mixed** (any combination that is NOT all-no AND NOT all-yes — includes all-partial, yes+no, partial+no, yes+partial, yes+partial+no) → Edit the spec: mark `[x]` on tasks where `all_present=yes`. Leave `[ ]` on `partial` and `no` (both require re-dispatch). Re-dispatch EXECUTE only for tasks still `[ ]`. Keep the original scope (Light/Full). Do NOT invent a new "PARTIAL" state.
- **All tasks `all_present=yes`** → **MANDATORY user surface** via `AskUserQuestion`: _"Pre-EXECUTE Existence Gate detected all N tasks already implemented. Evidence: {inline table}. Choose: (a) Close as already-implemented, (b) Force EXECUTE anyway (the gate may be wrong), (c) Abort pipeline."_ Never silently skip EXECUTE.

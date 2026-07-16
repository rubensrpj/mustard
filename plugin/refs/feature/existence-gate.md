# Existence Gate

> Detail for `/feature` — Pre-EXECUTE existence check (Full scope only).

Skip when: Light scope, OR `## Files` lists more than 8 files (the explorer's ≤10 tool-use self-cap will not cover — cost-benefit inverts).

Before dispatching implementation agents, verify the work is still needed.

Pre-check (free, zero LLM tokens):
```bash
rtk git diff --stat HEAD -- <files listed in `## Files`>
```
- Empty output (no changes) → skip the gate, EXECUTE normally.
- < 10 total insertions/deletions → skip the gate, EXECUTE normally (trivial change, not worth the overhead).
- ≥ 10 insertions/deletions → dispatch the explorer below.

Dispatch one `Task(subagent_type: "Explore")` with this prompt:
```
# EXISTENCE CHECK
Read .claude/spec/{specName}/spec.md sections "## Files" and "## Checklist".
For EACH checklist task (task-level, NOT file-level):
  1. Extract 1-3 concrete identifiers from the task text — function/component names, path fragments, string literals. e.g. "Add LogoutButton with handleLogout" -> ["LogoutButton","handleLogout"].
  2. Identify the task's target files from "## Files" (extension, name hint, or context).
  3. Grep each target file for the identifiers.
  4. Verdict: ALL targets contain a MAJORITY of identifiers -> yes; SOME do -> partial; NONE -> no.
Return a markdown table: | task | target_files | all_present (yes/partial/no) | evidence (identifier:line or "none") |
Return <=20 lines. Self-cap: <=10 tool uses (the true limit, not the task count).
```

Decision on the returned table:
- All tasks `no` → gate is transparent; EXECUTE normally.
- Mixed (anything not all-no and not all-yes) → mark `[x]` on `yes` tasks; leave `[ ]` on `partial` and `no` (both re-dispatch); re-dispatch EXECUTE only for still-`[ ]` tasks. Keep the original scope. Do NOT invent a "PARTIAL" state.
- All tasks `yes` → MANDATORY user surface via `AskUserQuestion`: "All N tasks already implemented. Evidence: {inline table}. (a) Close as already-implemented, (b) Force EXECUTE anyway (the gate may be wrong), (c) Abort." Never silently skip EXECUTE.

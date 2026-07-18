# Existence Gate

> Detail for `/feature` — Pre-EXECUTE existence check (Full scope only).

Skip when: Light scope, OR `## Files` lists more than 8 files (the explorer's tool-use budget — 15, warn 12 — will not cover; cost-benefit inverts).

Before dispatching implementation agents, verify the work is still needed.

Pre-check (free, zero LLM tokens):
```bash
rtk git diff --stat HEAD -- <files listed in `## Files`>
```
- Empty output (no changes) → skip the gate, EXECUTE normally.
- < 10 total insertions/deletions → skip the gate, EXECUTE normally (trivial change, not worth the overhead).
- ≥ 10 insertions/deletions → dispatch the explorer below.

Render the explorer prompt via the binary — NEVER hand-assemble it (`${CLAUDE_PLUGIN_ROOT}/refs/agent-prompt/agent-prompt.md` owns the contract):
```bash
mustard-rt run agent-prompt-render --spec {specName} --role explore \
  --task-text "EXISTENCE CHECK — read .claude/spec/{specName}/spec.md, sections Files and Checklist. For EACH checklist task (task-level, NOT file-level): (1) extract 1-3 concrete identifiers from the task text (function/component names, path fragments, string literals; e.g. Add LogoutButton with handleLogout -> LogoutButton, handleLogout); (2) identify the task target files from the Files section; (3) grep each target file for the identifiers; (4) verdict: ALL targets contain a MAJORITY of identifiers -> yes, SOME -> partial, NONE -> no. Return ONLY a markdown table with columns task, target_files, all_present (yes/partial/no), evidence (identifier:line or none)." \
  --mode first --emit ref
```
Pass the stdout **verbatim** as the `Task(subagent_type: "Explore")` prompt — with `--emit ref` it is a 2-line `MUSTARD-PROMPT-REF` stub the PreToolUse hook expands at dispatch (the explorer's return cap + the 15/12 tool-use limit ride in the rendered role block, not the prompt body).

Decision on the returned table:
- All tasks `no` → gate is transparent; EXECUTE normally.
- Mixed (anything not all-no and not all-yes) → mark `[x]` on `yes` tasks; leave `[ ]` on `partial` and `no` (both re-dispatch); re-dispatch EXECUTE only for still-`[ ]` tasks. Keep the original scope. Do NOT invent a "PARTIAL" state.
- All tasks `yes` → MANDATORY user surface via `AskUserQuestion`: "All N tasks already implemented. Evidence: {inline table}. (a) Close as already-implemented, (b) Force EXECUTE anyway (the gate may be wrong), (c) Abort." Never silently skip EXECUTE.

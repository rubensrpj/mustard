# Pipeline Handoff Summary Reference

> Detail for `/resume` Step 1: the exact format and data sources for the Handoff Summary presented to the user before continuing a pipeline.

## Handoff Summary Format

Compile from pipeline state + spec + agent memory + git context:

```
=== PIPELINE HANDOFF ===

Pipeline: {spec-name}
Scope:    {light|full}
Phase:    {ANALYZE|PLAN|EXECUTE|CLOSE}
Started:  {timestamp} | Elapsed: {duration}

## Completed
{For each [x] checkbox in spec:}
- [x] {task description}

## Pending
{For each [ ] checkbox in spec:}
- [ ] {task description}

## Concerns
{Scan spec for <!-- CONCERN: ... --> comments. Omit section if none.}
- {concern text}

## Context
- Branch: {from git}
- Files changed: {run `node .claude/scripts/diff-context.js`}
- Last agent: {Read `.claude/.agent-memory/_index.json` and pick the last entry's `agent_type`. If the file or `.agent-memory/` directory is missing, print literal `(none)` — do NOT probe with `ls`/`grep`, it surfaces noisy exit codes}
- Last action: {from the same last entry's `summary` field. If missing, print literal `(no prior memory)`}
- Decisions: {decisions[] from pipeline state, if any}

## Next Action
→ {ONE specific next step}
===
```

## Pipeline State Integrity Validation

- Missing or unparseable JSON → rebuild from spec (phase from header, tasks from `[x]`/`[ ]` checkboxes, status inferred)
- Phase mismatch between spec header and JSON → trust spec header (it's the source of truth)
- Tasks in JSON marked `completed` but spec has `[ ]` → trust spec, reset task to `pending`
- If rebuilt → warn user: "Pipeline state was recovered from spec"

## Harness View Enrichment (Wave 3 — fail-open)

```bash
node .claude/scripts/harness-views.js --view pipeline-state --spec {spec-name}
```

If the command succeeds, merge its `phase`, `decisions`, and `lessons` into the Handoff Summary. If it fails or is absent, proceed with `.pipeline-states/{spec-name}.json` alone — never block on this.

---
name: mustard:qa
description: Run QA phase — execute Acceptance Criteria from spec. Use after EXECUTE completes to validate all AC pass before CLOSE. Triggers automatically in pipeline but can be run manually.
---
<!-- mustard:generated -->
# /qa - QA Phase

## Trigger

`/mustard:qa [--spec <name>]`

## Description

Executes the QA phase: reads Acceptance Criteria from the active spec, runs each AC command, and reports pass/fail. Blocks CLOSE if any AC fails.

This is Wave 10 of the Mustard pipeline — the formal Dev/QA contract.

## Action

### Step 1 — Identify spec

If `--spec <name>` provided: use that spec name.
Otherwise: Glob `.claude/spec/active/*/spec.md` and pick the most recently modified.

### Step 2 — Validate spec has AC

Check that spec contains `## Acceptance Criteria` section with ≥1 item in format:
```
- [ ] AC-N: description — Command: `cmd`
```

If section missing: inform user:
> "Spec has no Acceptance Criteria section. Add the section before running QA. See Wave 10 spec template."
Stop here.

### Step 3 — Run QA

```bash
mustard-rt run qa-run --spec {specName}
```

If `mustard-rt` not found: dispatch Task(general-purpose) with QA agent context loaded from `.claude/context/qa/qa.core.md`.

### Step 4 — Update pipeline state

```json
{
  "phaseName": "QA",
  "qa": {
    "iteration": 1,
    "lastRun": "{ISO now}",
    "overall": "pass|fail|skip"
  }
}
```

### Step 5 — Branch on result

**Overall = pass:**
- Output QA report
- Update pipeline state: `phaseName: "CLOSE"`
- Output: "QA passed. All criteria met. Run `/mustard:close` or proceed to CLOSE."

**Overall = fail:**
- Output QA report with failing criteria
- Output: "QA failed. Fix the following before re-running /mustard:qa:"
  - List each FAIL criterion with its command
- Increment `qa.iteration` in pipeline state
- If `qa.iteration >= 3`: STOP and `AskUserQuestion`: "QA has failed 3 times. Manual intervention required. Review the failing criteria and decide: (a) Fix and retry, (b) Relax the AC in the spec, (c) Abort pipeline."

**Overall = skip (no AC section):**
- Warn user: "No Acceptance Criteria in spec — QA skipped. Consider adding AC before CLOSE."
- Pipeline may proceed (QA is advisory when no AC exists).

### Step 6 — CLOSE check

Before proceeding to CLOSE (either here or in `/mustard:close`), close-gate will verify `qa.result` event with `overall=pass` exists in harness log.

## Return Format

```
[QA] spec: {spec-name}

- AC-1: ✅ PASS — exit 0 (2.3s)
- AC-2: ❌ FAIL — exit 1 (0.8s) — stderr: {excerpt}

Overall: FAIL (1 of 2 failed)

→ Next: fix AC-2, then run /mustard:qa again
```

## Rules

- NEVER run QA before EXECUTE phase completes
- NEVER modify code during QA — QA is read-only execution
- Maximum 3 QA iterations per pipeline
- close-gate blocks CLOSE without qa.result=pass in events log
- `MUSTARD_QA_GATE_MODE=warn` — allows CLOSE with stderr warning even if QA absent
- `MUSTARD_QA_GATE_MODE=off` — skips QA check entirely in close-gate

---
name: mustard-qa
description: Use when the user runs /qa or asks to run QA, validate AC, or check acceptance criteria. Executes the QA phase (Wave 10) — runs each AC and reports pass/fail. Blocks CLOSE on failure.
source: manual
---
<!-- mustard:generated -->
# /qa - QA Phase

## Trigger

`/mustard:qa [--spec <name>]`

## Action

### 1. Identify + validate

`--spec` provided → use it. Else: `rtk mustard-rt run active-specs --format json` first entry. Spec needs `## Acceptance Criteria` with ≥1 `- [ ] AC-N: ... Command: \`cmd\``. Missing → *"Spec has no Acceptance Criteria."* stop.

### 2. Run

```bash
mustard-rt run emit-pipeline --kind pipeline.stage --spec {spec} --payload "{\"stage\":\"QaReview\"}"
mustard-rt run qa-run --spec {spec}
```

`qa-run` emits `qa.result`. If `mustard-rt` unavailable, dispatch Task(general-purpose) with `.claude/context/qa/qa.core.md`.

### 3. Branch

- **`pass`**: emit `pipeline.stage: Close`. *"QA passed."*
- **`fail`**: list failing AC. After 3 failures → AskUserQuestion: (a) fix+retry, (b) relax AC, (c) abort.
- **`skip`**: warn *"No AC — QA skipped."*

### 4. Tactical-fix discovery (post-pass, advisory)

Scan for `## Tactical Fix Candidates` / `## Candidatos a Tactical Fix`. Print *"Tactical fix candidate: <desc>\nRun: /mustard:tactical-fix <parent> \"<desc>\""*. Doesn't block CLOSE.

### 5. CLOSE gate

`close-gate` requires `qa.result.overall=pass`. Env: `MUSTARD_QA_GATE_MODE=strict|warn|off`.

## INVIOLABLE RULES

- NEVER run QA before EXECUTE completes; NEVER modify code during QA (read-only). Max 3 QA iterations.

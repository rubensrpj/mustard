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

`--spec` provided → use it. Else: `rtk mustard-rt run active-specs --format json` first entry. Spec needs an `## Acceptance Criteria` / `## Critérios de Aceitação` section with ≥1 `AC-N` that carries a `Command:`. Don't gate on the exact shape — `qa-run` parses BOTH the drafter multi-line form (`- **AC-1** — desc.` with `Command: \`cmd\`` on the next indented line, no checkbox) AND the historical one-line form (`- [ ] AC-N: desc — Command: \`cmd\``); a section with no `Command:` at all → `qa-run` returns `overall: skip`. No section → *"Spec has no Acceptance Criteria."* stop.

### 2. Run

```bash
mustard-rt run emit-pipeline --kind pipeline.stage --spec {spec} --payload "{\"stage\":\"QaReview\"}"
mustard-rt run qa-run --spec {spec}
```

`qa-run` emits `qa.result`. If `mustard-rt` unavailable, dispatch Task(general-purpose) with `.claude/context/qa/qa.core.md`.

### 3. Branch

- **`pass`**: emit `pipeline.stage: Close`. *"QA passed."*
- **`fail`**: list failing AC. After 3 failures → AskUserQuestion: (a) fix+retry, (b) relax AC, (c) abort.
- **`skip`**: not a failure — CLOSE treats skip as pass. Happens when there's no AC at all, OR when every AC timed out (per-AC limit 120s — e.g. `cargo test && cargo clippy --all-targets` may skip on a slow workspace; raise the timeout or split the AC). Warn *"QA skipped — {no AC | all AC timed out}; CLOSE not blocked."*

### 4. Tactical-fix discovery (post-pass, semi-automatic — detect + propose)

Scan for `## Tactical Fix Candidates` / `## Candidatos a Tactical Fix`. Print *"Tactical fix candidate: <desc>\nRun: /mustard:tactical-fix <parent> \"<desc>\""*. Doesn't block CLOSE.

**Structured payload contract (F4-c).** Include a `tactical_fix_candidates` array in the `qa.result` payload so `mustard-rt run tactical-fix-detect --spec <spec>` proposes each fix deterministically. Each entry: `{ "description": "required one-liner", "scope": "optional area", "severity": "critical|major|minor" }`. `tactical-fix-detect` emits one idempotent `tactical_fix.proposed` event per new candidate and **never** creates a sub-spec — creation stays a one-confirmation step (decision 6 — "não auto-aprovar").

### 5. CLOSE gate

`close-gate` requires `qa.result.overall=pass`. Env: `MUSTARD_QA_GATE_MODE=strict|warn|off`.

## INVIOLABLE RULES

- NEVER run QA before EXECUTE completes; NEVER modify code during QA (read-only). Max 3 QA iterations.

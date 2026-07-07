---
name: mustard-qa
description: Use when the user runs /qa or asks to run QA, validate AC, or check acceptance criteria. Executes the QA phase (Wave 10) — runs each AC and reports pass/fail. Blocks CLOSE on failure.
source: manual
---
<!-- mustard:generated -->
# /qa - QA Phase

**Iron law: an AC not executed is an AC failed.**

## Rationalizations that don't fly

| Excuse | Answer |
|--------|--------|
| "the build passed, that's basically QA" | build is a separate close-gate; QA is running each AC's `Command:` and reading its exit code |
| "I read the diff — it obviously satisfies the AC" | a pass is an OBSERVED exit code, never an inference; `qa-run` executes, you relay |
| "this AC is slow, I'll assert it mentally" | an AC without execution has no pass — raise the timeout or split the AC, don't skip it |
| "the spec changed only slightly after the pass" | any `spec.md`/`wave-plan.md` edit after a pass marks QA STALE; the close-gate blocks until re-run |
| "I'll quickly fix the failing code while QA runs" | QA is read-only — fixing mid-QA invalidates the result; fail, fix, re-run |

**Red flags** — catch yourself thinking any of these and return to the flow: *"I'm reporting pass from reading the code."* · *"Skipping the flaky AC just this once."* · *"Three failed iterations and I'm still patching instead of asking."* · *"I'll edit the AC to match what the code does."*

## Trigger

`/mustard:qa [--spec <name>]`

## Action

### 1. Identify + validate

`--spec` provided → use it. Else: `rtk mustard-rt run active-specs --format json` first entry. Spec needs an `## Acceptance Criteria` / `## Critérios de Aceitação` section with ≥1 `AC-N` that carries a `Command:`. Don't gate on the exact shape — `qa-run` parses BOTH the drafter multi-line form (`- **AC-1** — desc.` with `Command: \`cmd\`` on the next indented line, no checkbox) AND the historical one-line form (`- [ ] AC-N: desc — Command: \`cmd\``); a section with no `Command:` at all → `qa-run` returns `overall: skip`. No section → *"Spec has no Acceptance Criteria."* stop.

**Operative AC file (post-decompose).** When a spec was re-waved at EXECUTE entry, the monolithic `spec.md` is renamed to `spec.original.md` and the global ACs are carried into `wave-plan.md`. So the operative AC file is `spec.md` when present, else `wave-plan.md` — `qa-run` resolves this for you. Any **manual** AC edit/re-scope must target that same operative file; never edit `spec.md` blindly, since after a decompose it no longer exists.

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

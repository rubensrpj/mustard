---
description: Use when the user runs /qa or asks to run QA, validate AC, or check acceptance criteria. Executes the QA phase (Wave 10) — runs each AC and reports pass/fail. Blocks CLOSE on failure.
source: manual
---
<!-- mustard:generated -->
# /qa — QA Phase

**Iron law: an AC not executed is an AC failed.** A pass is an OBSERVED exit code, never an inference; `qa-run` executes, you relay. QA is **read-only** — fixing code mid-QA invalidates the result. Max 3 iterations.

`/mustard:qa [--spec <name>]`

## 1. Identify + validate

`--spec` given → use it. Else `rtk mustard-rt run active-specs --format json` first entry. The spec needs an `## Acceptance Criteria` / `## Critérios de Aceitação` section with ≥1 `AC-N` carrying a `Command:`. `qa-run` parses BOTH the drafter multi-line form (`- **AC-1** — desc.` + `Command: \`cmd\`` on the next indented line) AND the historical one-line form (`- [ ] AC-N: desc — Command: \`cmd\``). No `Command:` at all → `qa-run` returns `overall: skip`. No section → *"Spec has no Acceptance Criteria."* stop.

**Operative AC file:** `spec.md` when present, else `wave-plan.md` (after a decompose the monolithic `spec.md` becomes `spec.original.md` and the ACs move into `wave-plan.md`). `qa-run` resolves this; any manual AC edit must target the same operative file.

## 2. Run

```bash
mustard-rt run emit-pipeline --kind pipeline.stage --spec {spec} --payload "{\"stage\":\"QaReview\"}"
mustard-rt run qa-run --spec {spec}
```

`qa-run` emits `qa.result`. If `mustard-rt` is unavailable, dispatch `Task(general-purpose)` with `${CLAUDE_PLUGIN_ROOT}/context/qa/qa.core.md`.

## 3. Branch

- **`pass`** → emit `pipeline.stage: Close`. *"QA passed."*
- **`fail`** → list failing AC. After 3 failures → `AskUserQuestion`: (a) fix+retry, (b) relax AC, (c) abort.
- **`skip`** → two shapes, told apart by `criteria` in the result. **No AC at all** (`criteria` empty) → advisory; warn *"QA skipped — spec carries no AC; CLOSE not blocked."* **ACs exist but every one skipped** (per-AC timeout 120s / spawn failure) → NOT a pass: the strict close gate blocks CLOSE. Fix the AC commands (raise the timeout or split the AC) and re-run — or confirm the skip with the user (`AskUserQuestion`) before closing under `MUSTARD_QA_GATE_MODE=warn`.

## 4. Tactical-fix discovery (post-pass — detect + propose, never auto-create)

Scan for `## Tactical Fix Candidates` / `## Candidatos a Tactical Fix`; per entry print *"Tactical fix candidate: <desc>\nRun: /mustard:tactical-fix <parent> \"<desc>\""*. Doesn't block CLOSE. Include a `tactical_fix_candidates` array in the `qa.result` payload (each `{description (required), scope?, severity?}`) so `mustard-rt run tactical-fix-detect --spec <spec>` proposes each deterministically — one idempotent `tactical_fix.proposed` event per candidate; it never creates a sub-spec (creation stays a one-confirmation step).

## 5. CLOSE gate

`close-gate` requires `qa.result.overall=pass`. Env: `MUSTARD_QA_GATE_MODE=strict|warn|off`. Any `spec.md`/`wave-plan.md` edit after a pass marks QA STALE — the gate blocks until re-run.

## Inviolable

- NEVER run QA before EXECUTE completes; NEVER modify code during QA (read-only).

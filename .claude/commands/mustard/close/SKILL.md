---
name: mustard-close
description: Use when the user runs /close or asks to finalize, complete, or cancel the active pipeline. Verifies build/review/QA, archives the spec, and emits the completion banner.
source: manual
---
<!-- mustard:generated -->
# /close - Finalize Pipeline

## Trigger

`/close`

## Verification Gate + auto-finalize (MANDATORY, deterministic)

The CLOSE gates run via **`mustard-rt run close-orchestrate --spec {spec}`** (one machine-readable JSON report). It runs each gate — (1) **Build + tests** `verify-pipeline`; (2) **QA** `qa-run` (fail → block; skip → pass); (3) **review-spans** (any red span verdict → block); (4) **Docs audit** `docs-stale-check` (`--skip-docs` for non-architectural specs); (5) **pipeline-summary** (advisory) — and derives `overall`.

**The finalize is automatic. The orchestrator does not decide whether to call `complete-spec`.** When `overall == "pass"`, `close-orchestrate` itself chains the close **in-process** (calls the `complete-spec` finalize directly, no extra command): the spec flips straight to `completed` and `pipeline.complete` is emitted, then the event is auto-verified. The report carries `"chained": true` and `"verified": true/false`. When `overall == "fail"` it is **report-only** (`"chained": false`, no finalize) — fix the failing gate(s) and re-run; never hand-call `complete-spec` to bypass a red gate. The `emit-pipeline` QA-gate (refuses `pipeline.complete` without `qa.result=pass`) remains the strict safety net.

Concerns/Checklist still block: unresolved `BLOCKED` → block; `CONCERN`/`DEFERRED` → surface + proceed; any `- [ ]` left in the Checklist → ABORT + report unmarked items (these are inputs to the gates above).

## Action

1. Locate spec at `.claude/spec/{name}/`. Lifecycle state from the `meta.json` sidecar + the event projection (`spec.md` is pure narrative).
2. Lifecycle state (`stage: Close`, `outcome: Completed`, `phase: CLOSE`, `checkpoint: {ISO now}`) is stamped into the `meta.json` sidecar **by the `close-orchestrate` chain itself** when `overall == pass` (its in-process `complete-spec` finalize patches the sidecar). Do NOT hand-stamp it — never hand-edit `spec.md`, and never emit `pipeline.stage`/`pipeline.outcome` yourself (a hand-emit after a *failing* orchestrate would falsely mark the spec Completed).
3. `mustard-rt run scan` if `## Files` touched the codebase materially (refresh `grain.model.json`).
4. Run the gate + auto-finalize (one command — relay its JSON; do **not** call `complete-spec` and do **not** hand-emit the Close/Completed stage+outcome — the chain owns both):

```bash
mustard-rt run close-orchestrate --spec {spec}
# overall == pass → already chained: spec is completed, meta.json stamped Close/Completed/CLOSE, pipeline.complete emitted + verified.
# overall == fail → report-only; fix the failing gate and re-run (nothing was stamped).
```

   A close lands straight on `completed` — there is no follow-up grace window. Any follow-up work goes into a separate linked sub-spec (`/mustard:tactical-fix`), not a flag on this spec.

5. Knowledge: one `mustard-rt run memory knowledge` per significant pattern; one `mustard-rt run memory decision` per lesson (max 3 each, skip trivial).
6. Metrics archive: read pipeline-state projection → save to `.claude/metrics/{spec}.json` (omit missing fields).
7. Print: `pipeline-summary` → `wave-tree` → banner `PIPELINE COMPLETE — {spec}` with agents/files/registry + optional `rtk gain` token line. All fail-open.
8. Epic auto-fold (Wave 8): `epic-fold --detect` (reads the NDJSON event stream — `spec.link` + `pipeline.phase`, not the legacy `.pipeline-states` sidecar) → if non-empty, `epic-fold --epic <name>` per entry.

## Lexicon feedback — feed the self-learning dictionary (every close)

Before finalizing, fold what THIS spec taught the cross-language dictionary so the next query lands deterministically (feature + bugfix; fail-open — skip on any error). Full contract: `../../../refs/lexicon-feedback.md`.

## Cancellation

Stage `Close`, Outcome `Cancelled`. Emit `pipeline.stage: Close` + `pipeline.outcome: Cancelled`. No filesystem move.

## INVIOLABLE RULES

- NEVER bypass the verification gate, and NEVER hand-call `complete-spec` to finalize — the finalize is chained automatically by `close-orchestrate` only when every gate passes. Calling `complete-spec` to force a close past a red gate is forbidden (the `emit-pipeline` QA-gate would reject it anyway).
- NEVER move the spec directory — archival is event-only.
- NEVER batch-mark Checklist items on behalf of agents.

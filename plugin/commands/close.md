---
name: close
description: Use when the user runs /close or asks to finalize, complete, or cancel the active pipeline. Verifies build/review/QA, archives the spec, and emits the completion banner.
source: manual
---
<!-- mustard:generated -->
# /close - Finalize Pipeline

**Iron law: NO close without `qa.result=pass` — the close-gate refuses.**

## Rationalizations that don't fly

| Excuse | Answer |
|--------|--------|
| "QA is red but the work is clearly done" | fix the failing gate and re-run `close-orchestrate`; never hand-call `complete-spec` past a red gate |
| "I'll tick the remaining `- [ ]` boxes myself" | never batch-mark checklist items on behalf of agents; unchecked items ABORT the close |
| "the gate is a formality — `complete-spec` does the same" | the finalize is chained by `close-orchestrate` ONLY on `overall=pass`; the `emit-pipeline` QA-gate rejects a bypass anyway |
| "I'll move the spec dir to a completed folder" | archival is event-only — the directory never moves |
| "the leftover work can ride inside this closed spec" | follow-up work goes into a separate linked sub-spec (`/mustard:tactical-fix`), never a flag on a closed spec |

**Red flags** — catch yourself thinking any of these and stop: *"Let me set the gate env to `off` to get past this."* · *"Hand-emitting `pipeline.stage: Close` to unblock."* · *"Stamping `meta.json` myself so the dashboard looks right."*

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

5. Knowledge: emit decision/lesson EVENTS (max 3 each, skip trivial; durable prose belongs to native auto-memory):
   - per non-obvious decision: `mustard-rt run emit-event --event decision --spec {spec} --payload "title=<what was decided>" --payload "rationale=<why>"`
   - per lesson: `mustard-rt run emit-event --event lesson --spec {spec} --payload "takeaway=<the lesson>" --payload "trigger=<what surfaced it>"`
6. Metrics archive: read pipeline-state projection → save to `.claude/.metrics/{spec}.json` (omit missing fields).
7. Print: `pipeline-summary` → `wave-tree` → banner `PIPELINE COMPLETE — {spec}` with agents/files/registry + optional `rtk gain` token line. All fail-open.
8. Epic auto-fold (Wave 8): handled in-process by `close-orchestrate` — it detects epics whose children are all closed (NDJSON event stream — `spec.link` + `pipeline.phase`) and folds each one. Nothing to run by hand.

## Lexicon feedback

Nothing to run at close: a confirmed vocabulary bridge is persisted the moment it is confirmed, at ANALYZE time — `mustard-rt run equivalence-learn --term <missed> --tokens <code-terms>` (see `/feature` §1: the digest-contract row and the absence radar). There is no close-time suggestion pass.

## Cancellation

Stage `Close`, Outcome `Cancelled`. Emit `pipeline.stage: Close` + `pipeline.outcome: Cancelled`. No filesystem move.

## INVIOLABLE RULES

- NEVER bypass the verification gate, and NEVER hand-call `complete-spec` to finalize — the finalize is chained automatically by `close-orchestrate` only when every gate passes. Calling `complete-spec` to force a close past a red gate is forbidden (the `emit-pipeline` QA-gate would reject it anyway).
- NEVER move the spec directory — archival is event-only.
- NEVER batch-mark Checklist items on behalf of agents.

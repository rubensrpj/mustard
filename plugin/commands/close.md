---
description: Use when the user runs /close or asks to finalize, complete, or cancel the active pipeline. Verifies build/review/QA, archives the spec, and emits the completion banner.
disable-model-invocation: true
source: manual
---
<!-- mustard:generated -->
# /close — Finalize Pipeline

**Iron law: NO close without `qa.result=pass` — the close-gate refuses.** `/close`.

## Verification gate + auto-finalize (deterministic)

One command runs every gate and, on pass, finalizes in-process:

```bash
mustard-rt run close-orchestrate --spec {spec}
```

Gates: (1) **build + tests** `verify-pipeline`; (2) **QA** `qa-run` (fail → block; skip with no AC → advisory; skip with ACs recorded → block in strict — a skip is not a verification); (3) **review-spans** (any red span → block); (4) **docs audit** `docs-stale-check` (`--skip-docs` for non-architectural specs); (5) **pipeline-summary** (advisory). It derives `overall`.

**The finalize is automatic — you never decide whether to call `complete-spec`.** On `overall == "pass"`, `close-orchestrate` chains the finalize in-process: the spec flips to `completed`, `pipeline.complete` is emitted and auto-verified, and `meta.json` is stamped `Close/Completed/CLOSE` (report carries `"chained": true`, `"verified": true|false`). On `overall == "fail"` it is report-only (`"chained": false`) — fix the failing gate and re-run; NEVER hand-call `complete-spec` to bypass a red gate (the `emit-pipeline` QA-gate rejects it anyway).

Blockers: unresolved `BLOCKED` → block; `CONCERN`/`DEFERRED` → surface + proceed; any unchecked `- [ ]` in the Checklist → ABORT + report the unmarked items (a `/close` precondition, not a gate).

## Action

1. Locate the spec at `.claude/spec/{name}/`; lifecycle from the `meta.json` sidecar + event projection. NEVER hand-edit `spec.md` and never emit `pipeline.stage`/`pipeline.outcome` yourself — the chain owns both (a hand-emit after a *failing* orchestrate would falsely mark the spec Completed).
2. `mustard-rt run scan` if `## Files` touched the codebase materially.
3. Run `close-orchestrate` (above) and relay its JSON. A close lands straight on `completed` — no grace window; follow-up work goes into a linked sub-spec (`/mustard:tactical-fix`), never a flag on this spec.
4. Knowledge (max 3 each, skip trivial; durable prose belongs to native auto-memory):
   - decision: `mustard-rt run emit-event --event decision --spec {spec} --payload "title=…" --payload "rationale=…"`
   - lesson: `mustard-rt run emit-event --event lesson --spec {spec} --payload "takeaway=…" --payload "trigger=…"`
   - capability: `mustard-rt run capability create --slug {slug} --title "…"` when the spec shipped a durable user-facing capability (then link `[[cap.{slug}]]` in the spec)
5. Metrics: read the pipeline-state projection → `.claude/.metrics/{spec}.json` (omit missing fields).
6. Print `pipeline-summary` → `wave-tree` → banner `PIPELINE COMPLETE — {spec}` (agents/files/registry + optional `rtk gain` line). All fail-open.
7. Epic auto-fold is handled in-process by `close-orchestrate` (children all closed → folded) — nothing to run by hand.

## Cancellation

Stage `Close`, Outcome `Cancelled`: emit `pipeline.stage: Close` + `pipeline.outcome: Cancelled`. No filesystem move.

## Inviolable

- NEVER bypass the gate or hand-call `complete-spec` — the finalize is chained automatically only when every gate passes.
- NEVER move the spec directory — archival is event-only.
- NEVER batch-mark Checklist items on behalf of agents.

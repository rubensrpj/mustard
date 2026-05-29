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

**The finalize is automatic. The orchestrator does not decide whether to call `complete-spec`.** When `overall == "pass"`, `close-orchestrate` itself chains the close **in-process** (calls the `complete-spec` finalize directly, no extra command): the spec flips to `closed-followup` and `pipeline.complete` is emitted, then the event is auto-verified. The report carries `"chained": true` and `"verified": true/false`. When `overall == "fail"` it is **report-only** (`"chained": false`, no finalize) — fix the failing gate(s) and re-run; never hand-call `complete-spec` to bypass a red gate. The `emit-pipeline` QA-gate (refuses `pipeline.complete` without `qa.result=pass`) remains the strict safety net.

Concerns/Checklist still block: unresolved `BLOCKED` → block; `CONCERN`/`DEFERRED` → surface + proceed; any `- [ ]` left in the Checklist → ABORT + report unmarked items (these are inputs to the gates above).

## Action

1. Locate spec at `.claude/spec/{name}/`. Lifecycle state from the `meta.json` sidecar + the event projection (`spec.md` is pure narrative).
2. Lifecycle state (`stage: Close`, `outcome: Completed`, `checkpoint: {ISO now}`) is written to the `meta.json` sidecar by the close pipeline events below — `mustard-rt` patches the sidecar; never hand-edit `spec.md`.
3. `mustard-rt run sync-registry` if `## Files` touched schemas.
4. Run the gate + auto-finalize (one command — relay its JSON; do **not** call `complete-spec` yourself):

```bash
mustard-rt run close-orchestrate --spec {spec}
# overall == pass → already chained: spec is closed-followup, pipeline.complete emitted + verified.
# overall == fail → report-only; fix the failing gate and re-run.
```

   Then stamp Stage/Outcome (these are header/flag emits, not the finalize):

```bash
mustard-rt run emit-pipeline --kind pipeline.stage --spec {spec} --payload "{\"stage\":\"Close\"}"
mustard-rt run emit-pipeline --kind pipeline.outcome --spec {spec} --payload "{\"outcome\":\"Completed\"}"
mustard-rt run emit-pipeline --kind pipeline.flag.set --spec {spec} --payload "{\"flag\":\"followup_open\"}"
```

5. Knowledge: one `mustard-rt run memory knowledge` per significant pattern; one `mustard-rt run memory decision` per lesson (max 3 each, skip trivial).
6. Metrics archive: read pipeline-state projection → save to `.claude/metrics/{spec}.json` (omit missing fields).
7. Print: `pipeline-summary` → `wave-tree` → banner `PIPELINE COMPLETE — {spec}` with agents/files/registry + optional `rtk gain` token line. All fail-open.
8. Epic auto-fold (Wave 8): `epic-fold --detect` (reads the NDJSON event stream — `spec.link` + `pipeline.phase`, not the legacy `.pipeline-states` sidecar) → if non-empty, `epic-fold --epic <name>` per entry.

## Cancellation

Stage `Close`, Outcome `Cancelled`. Emit `pipeline.stage: Close` + `pipeline.outcome: Cancelled`. No filesystem move.

## INVIOLABLE RULES

- NEVER bypass the verification gate, and NEVER hand-call `complete-spec` to finalize — the finalize is chained automatically by `close-orchestrate` only when every gate passes. Calling `complete-spec` to force a close past a red gate is forbidden (the `emit-pipeline` QA-gate would reject it anyway).
- NEVER move the spec directory — archival is event-only.
- NEVER batch-mark Checklist items on behalf of agents.
- Re-reviews always dispatch with `model: "sonnet"`.

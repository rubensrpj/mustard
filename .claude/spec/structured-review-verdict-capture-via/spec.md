---
id: spec.structured-review-verdict-capture-via
---

# structured review verdict capture via SubagentStop hook

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Context

structured review verdict capture via SubagentStop hook.

Anchors (from scan):
- apps/rt/src/commands/review/review_spans.rs (review, verdict, render)
- packages/core/src/domain/model/contract.rs (verdict, subagentstop, contract)
- apps/scan/src/mine.rs (findings, role, contract)
- apps/cli/src/commands/install_grammars.rs (findings, render)
- apps/dashboard/src-tauri/src/telemetry.rs (review, findings, role)
- apps/dashboard/src/components/page/WaveRowLabel/index.tsx (role)
- apps/mcp/src/lib.rs (findings)
- apps/rt/src/commands/agent/agent_prompt_render.rs (findings, role, render, contract)
- apps/rt/src/commands/review/review_result.rs (review, verdict)
- apps/rt/src/commands/review/review_prefetch.rs (review, render)
- apps/rt/src/commands/wave/exec_rewave_check.rs (findings, role, render)
- apps/rt/src/commands/pipeline/wave_advance.rs (review, role, render)

Why now: the review phase already produces a verdict, but the orchestrator reads the
reviewer's PROSE by hand to decide `approved`/`rejected` and the critical-finding count,
then calls `review-result`. That manual reading sits on the critical gate path and invites
the shortcut the resume-loop reference warns against ("never write approved just to
unblock"). The `<MEMORY>` block already proves the fix: a returning subagent emits a tagged
block and a SubagentStop hook (`capture_memory_decision`, `subagent_inject.rs`) harvests it
deterministically. This spec applies that exact pattern to the review verdict, so the
machine — not a human reading prose — records the gate's input.

## Users/Stakeholders

- The orchestrator: loses an error-prone manual interpretation step on the review gate.
- The review→QA gate and the maintainer: a verdict emitted by code is auditable and cannot
  be quietly fudged to unblock a close.
- Downstream work: the bounded remediation loop (roadmap Pilar 1b) and the SDD scoreboard
  (roadmap Pilar 2) both consume a structured `review.result`.

## Success Metric

After a review subagent returns a `<VERDICT>` block, a `review.result` event is emitted
whose `verdict` and `criticalCount` are parsed from that block with ZERO orchestrator
interpretation. When the block is absent or malformed, the existing manual `review-result`
path still works (fail-open) and nothing is emitted by the hook.

## Non-Goals

- The bounded remediation loop / re-dispatch on rejection (Pilar 1b) — this spec only makes
  the verdict machine-readable.
- Any change to the QA gate, `close_gates`, or the Green/Amber/Red regression judge.
- Removing the manual `review-result` CLI — it stays as the fallback path.
- Changing `wave_advance`'s `review_round` filter (that is Pilar 1b).

## Acceptance Criteria

- **AC-1** — when a review subagent's final output contains a `<VERDICT>{"verdict":"rejected","critical":N,"findings":[…]}</VERDICT>` block, then the SubagentStop hook parses it and emits a `review.result` event whose `verdict` and `criticalCount` equal the block's values, with no orchestrator call to `review-result`
  Command: `cargo test -p mustard-rt capture_review_verdict_emits_review_result`
  Expect: `1 passed; 0 failed`
- **AC-2** — when the review output has no `<VERDICT>` block or its JSON is malformed, then the hook is a silent no-op (fail-open) and the manual `review-result` path stays the source of the verdict
  Command: `cargo test -p mustard-rt verdict_block_absent_is_noop`
  Expect: `1 passed; 0 failed`
- **AC-3** — the project build and tests pass green
  Command: `cargo build --workspace`

<!-- PLAN -->

## Files

- `apps/rt/src/hooks/task/subagent_inject.rs` — add `extract_verdict_block` + `capture_review_verdict[_with_session]`, mirroring the `<MEMORY>` twins (`extract_memory_block` / `capture_memory_decision`, ~lines 331-390); wire into the SubagentStop side-effect; gate on the review role.
- `apps/rt/src/commands/review/review_result.rs` — reuse `record_review` to emit the `review.result` event + `review` metric from the parsed verdict; the manual CLI path stays intact.
- `plugin/agents/mustard-review.md` — add the `<VERDICT>{verdict,critical,findings}</VERDICT>` emission contract to the reviewer's output.
- `apps/rt/src/commands/agent/render/role.rs` — add the same `<VERDICT>` instruction to the rendered review role block, so a rendered review agent matches the plugin agent.

## Tasks

- T1 — In `subagent_inject.rs`, add `extract_verdict_block(text) -> Option<ReviewVerdict>` and `capture_review_verdict` / `_with_session` twins mirroring the `<MEMORY>` capture; on the review role, emit `review.result` via `review_result::record_review`; fail-open (no block / wrong role / parse error → no-op). Wire it next to `capture_memory_decision` and `span_level_eval_and_append` in the SubagentStop handler.
- T2 — Extend the review contract in `mustard-review.md` and the rendered review block in `role.rs` to emit the `<VERDICT>` block, documenting the JSON schema and that `critical` counts blocking (Guard / mold / correctness) findings only.
- T3 — Add tests mirroring the existing `capture_memory_decision` tests: AC-1 (block parsed → event emitted with matching fields) and AC-2 (absent / malformed → no-op).

## Boundaries

- Scope is the review role's verdict only; do not touch impl / plan / explore capture.
- Do not remove or alter the manual `review-result` CLI (it is the fallback).
- Do not implement re-dispatch, looping, or gate changes (Pilar 1b).
- Preserve fail-open: any parse or IO error is a silent no-op and never blocks the SubagentStop flow.

## Checklist

- [x] T1 — capture_review_verdict + extract_verdict_block → apps/rt/src/hooks/task/subagent_inject.rs
- [x] T2 — VERDICT emission contract → plugin/agents/mustard-review.md
- [x] T3 — AC-1/AC-2 capture tests → apps/rt/src/hooks/task/subagent_inject.rs
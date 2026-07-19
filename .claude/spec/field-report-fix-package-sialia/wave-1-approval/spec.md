---
id: wave.field-report-fix-package-sialia.1-approval
---

# wave-1-approval

## Summary

approve-spec evaluates every Full-gate requirement and reports ALL missing ones in one refusal

## Network

- Parent: [[spec.field-report-fix-package-sialia]]

## Tasks

- [ ] Refactor the gate section of run() in approve_spec.rs: evaluate the clarify marker (F6, <spec>/.clarified) and the approval marker (T5, <spec>/.approved-by-user) TOGETHER instead of exiting on the first miss
- [ ] When one or both are missing in strict mode, print ONE refusal that lists each missing marker and its minting path (clarify: `mustard-rt run grill-capture --finalize --spec <spec>`; approval: user accepts via ExitPlanMode or answers the approval AskUserQuestion) — keep exit code 1 and the strict|warn|off semantics intact
- [ ] Keep warn mode printing the same aggregated text as a warning without blocking; off mode unchanged
- [ ] Unit tests: combined_refusal_lists_all_missing_gates (both markers absent -> both named in one message), clarify-only-missing and approval-only-missing still refuse with the single relevant requirement, both-present approves
- [ ] Harden the observer-side spec resolution (active_spec in approval_marker_observer.rs, shared with plan_approval_observer): when neither the session binding nor the legacy pipeline-states hint resolves, fall back to the UNIQUE spec whose meta.json is scope=full + stage=Plan + not yet approved — exactly the fact-1 state window; zero or MULTIPLE candidates keep returning None (fail-closed). Field evidence 2026-07-18: the emitter-side bind raced to a dead session, both approval observers went blind, and a real user approval minted nothing
- [ ] Unit tests: unique_pending_full_plan_resolves_without_binding (no bind, one full/Plan/unapproved spec -> marker minted), two_pending_full_plans_stay_none (ambiguity -> nothing)

## Files

- `apps/rt/src/commands/spec/approve_spec.rs`
- `apps/rt/src/hooks/observe/approval_marker_observer.rs`

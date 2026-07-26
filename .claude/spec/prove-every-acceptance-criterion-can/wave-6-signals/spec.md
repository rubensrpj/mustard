---
id: wave.prove-every-acceptance-criterion-can.6-signals
---

# wave-6-signals

## Summary

Three reports stop overstating what they know: a hollow clarification marker becomes visible before the approval gesture, an approved-but-unstarted plan says what to do next, and the dependency pre-gate names what it checked.

## Network

- Parent: [[spec.prove-every-acceptance-criterion-can]]

## Tasks

- [ ] A clarification marker that records nothing is detected in exactly ONE place today — `approve_spec.rs:246`, via `MarkerProvenance::records_substance`. That is the approval gesture: the worst possible moment, because the operator has just asked for the implementation and gets a refusal instead. Surface the same fact earlier, in `active-specs` (the listing) and in `resume-bootstrap` (the resume path), reusing that same predicate — never a second definition of hollow. Advisory there, not blocking: the refusal stays where it is; only the discovery moves earlier.
- [ ] In `post_execute_gate.rs`, close the inference the reader is left with. The gate only acts when the stage is at or after Execute (line 163), so a Full spec that is APPROVED and still sitting in Plan comes back with `stage: "Plan"`, `approvedByUser: true` and NO `nextAction` — and the caller must know, from a reference document, that this combination means 'do not re-present, do not re-approve, just start'. That is a deterministic decision delegated to a model, which contradicts this harness's own principle. Give that state an explicit `nextAction`, in the same vocabulary as the existing `await-approval` and `await-plan-materialize` tokens, and name the published command it implies.
- [ ] In `dependency_precheck.rs`, make the answer say what it verified. The report (line 1156) carries `missing`, `mode`, `ok`, `promise_violations`, `spec`, `subproject` and the tactical-fix hints — nothing states the SCOPE of the check. `ok: true` therefore reads as 'safe to dispatch', while what was actually established is narrower: the symbols exist. Field evidence: a wave dispatched on a green pre-gate returned blocked on two criteria, because what was missing was a capability, not a symbol. Add a field naming the checks performed. Do NOT attempt capability detection — honest labelling is the whole fix here.
- [ ] One test per correction, each asserting the new signal AND that the old behaviour it replaces is gone.

## Files

- `apps/rt/src/commands/spec/active_specs.rs`
- `apps/rt/src/commands/pipeline/resume_bootstrap/mod.rs`
- `apps/rt/src/commands/pipeline/resume_bootstrap/post_execute_gate.rs`
- `apps/rt/src/commands/review/dependency_precheck.rs`

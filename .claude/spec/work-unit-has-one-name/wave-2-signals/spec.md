---
id: wave.work-unit-has-one-name.2-signals
---

# wave-2-signals

## Summary

Two signals stop reporting a state nobody reached: the picker table stops calling a scaffolded plan `running`, and a precheck that declined to judge stops looking like one that passed.

## Network

- Parent: [[spec.work-unit-has-one-name]]

## Tasks

- [ ] `active_specs::derive_status` builds the `W{N} em exec` column from `find_first_active_wave` — the first wave directory whose meta says Outcome=Active. A wave directory is born Active at scaffold time, so a plan nobody dispatched reads as running. Measured live: the table showed `0/2` and `W1 em exec` for a spec `resume-bootstrap` reported as neverDispatched:true.
- [ ] Derive that column from whether anything was actually DISPATCHED, using the same signal `resume-bootstrap` trusts. Read how `never_dispatched` is computed there and reuse it rather than inventing a second reading — two answers to `has this started?` is the shape of the defect, not the fix. When nothing was dispatched the column must NOT say `em exec`; pick wording that asks the reader to START, not to resume.
- [ ] Keep the legend in sync. `active_specs` renders a legend line explaining the codes, and it is asserted to match `derive_status` — a new wording that leaves the legend behind is a worse defect than the one being fixed, because the legend is what the reader trusts.
- [ ] `dependency_precheck` ships `ok: true` for BOTH `checked and found nothing wrong` and `did not look` (see its own doc at line 129). Add a `verdict` field carrying `pass` or `declined` so a reader can tell them apart without knowing to look for the `skipped` key. Do NOT change what `ok` means — consumers read it today, and the existing `skipped` key must keep working.
- [ ] Carry the new field through any caller that TRIMS the report. The file already warns that `wave-advance` folds the per-wave verdict into its round, and that a dropped skip arrives as a bare `ok:true` — the same hazard applies to the new field.
- [ ] Tests, named exactly as the criteria name them: a_scaffolded_plan_is_not_reported_as_running, a_declined_precheck_is_not_a_pass.

## Files

- `apps/rt/src/commands/spec/active_specs.rs`
- `apps/rt/src/commands/review/dependency_precheck.rs`

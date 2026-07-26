---
id: wave.prove-every-acceptance-criterion-can.8-docs
---

# wave-8-docs

## Summary

Lock the two new commands into the published surface and teach the flow to use them — the proof at planning time, the amendment instead of a hand edit.

## Network

- Parent: [[spec.prove-every-acceptance-criterion-can]]
- Depends on: [[wave.prove-every-acceptance-criterion-can.2-gate]], [[wave.prove-every-acceptance-criterion-can.3-amend]], [[wave.prove-every-acceptance-criterion-can.4-verdict]], [[wave.prove-every-acceptance-criterion-can.7-round]]

## Tasks

- [ ] Add `ac-amend` and `ac-negative-check` to the locked `RUN_SUBCOMMANDS` list in `apps/rt/tests/run_command_surface.rs`, keeping it sorted, and update the declared-variant count in its doc comment.
- [ ] Add a test named `amendment_path_is_published_and_instructed` in that same file: both commands are published by the clap tree, AND the shipped dispatch-loop prose names `mustard-rt run ac-amend` while no longer instructing a criterion to be folded in by hand. It reads `plugin/**` from disk exactly as that file's existing surface checks do.
- [ ] In `plugin/commands/feature.md` step 2.5, name `mustard-rt run ac-negative-check --spec .claude/spec/{slug}/spec.md` as the step that runs right after the structural validation on the LIGHT path, and state plainly what it decides: a criterion that does not fail now does not enter the plan, and approval will refuse it.
- [ ] In `plugin/refs/feature/full-plan.md` step 3 and 4, record that the plan materialisation now runs that proof itself and refuses the plan while any criterion is unproven — so on the Full path the operator never meets the refusal at the approval gesture.
- [ ] In `plugin/refs/spec/resume-loop.md`, at the mid-round change paragraph that today says to fold a behaviour change into the criteria and re-run the verification, name `mustard-rt run ac-amend` as the way to do it and state that the replacement must itself come back red. Keep the sentence about the narrative staying frozen.
- [ ] In `docs/2026-07-25-revisao-portoes-pipeline-ondas.md`, extend section 7 with the state of queue item 2: what shipped, under which spec, and the one limit worth stating — the proof is taken in the working tree, so a criterion amended after its own work already landed cannot be proven there and will be refused.
- [ ] In the same `resume-loop.md`, enunciate the MIXED ROUND, which today falls between two rules that both apply and neither covers: commit once per round after every wave returned, and stop on a blocked wave. Both are true when one wave finished and its sibling came back blocked, and nothing says what to do. State it: commit anyway, because preserving work is not advancing; mark done only the waves that finished; do not advance the round. The diff-scoping half of this ships in wave 7 — name it here so the reader knows why the record stays clean.
- [ ] Run the whole workspace build green as the closing check.

## Files

- `apps/rt/tests/run_command_surface.rs`
- `plugin/commands/feature.md`
- `plugin/refs/feature/full-plan.md`
- `plugin/refs/spec/resume-loop.md`
- `docs/2026-07-25-revisao-portoes-pipeline-ondas.md`

---
id: wave.work-unit-has-one-name.1-identity
---

# wave-1-identity

## Summary

The unit's name is minted ONCE, at the base gate, and the draft consumes it — so the branch, the spec directory, the events and the notebook stop being able to disagree.

## Network

- Parent: [[spec.work-unit-has-one-name]]

## Tasks

- [ ] Find the slug derivation `spec-draft` uses today (it turns `--intent` into the spec directory name). Make it callable from the event family WITHOUT copying it — one derivation, two callers. Copying it would recreate the very defect this wave closes, one release later.
- [ ] `emit-pipeline --kind pipeline.kind` mints the canonical slug from `--intent` using that shared derivation, and REPORTS it in its JSON alongside the branch it already reports. The gate is the right place because it is the first moment both a base and an intent exist, and it already computes `{base}_{slug}`.
- [ ] Decide and document what happens to an explicit `--spec` that disagrees with the minted slug. Do NOT silently prefer one: either the minted name wins and the report says so, or the call is refused naming both. Silence here is how two names were born in the first place. Whichever you choose, the JSON must let a caller SEE that a rename happened.
- [ ] `spec-draft` accepts an explicit slug (a flag beside `--intent`) and uses it verbatim instead of deriving a second name. `--intent` keeps its other job — it is still the spec TITLE. Remember the registration guard: a new flag is not a new subcommand, but the CLI surface test must still pass.
- [ ] Correct the docstring at mode_decision.rs:138. It currently claims the two spellings cannot drift because `compute_work_branch` is shared. The FUNCTION is shared; the ARGUMENT is not — one call site passes the slug the gate invented, the other the slug the draft derived. Say what is actually guaranteed, and name what guarantees it now.
- [ ] Tests, named exactly as the criteria name them: the_base_gate_mints_the_canonical_slug, spec_draft_consumes_the_slug_it_is_given, inside_work_branch_holds_when_the_gate_named_the_unit. The third is the one that matters most — it must set up a unit whose branch was cut from the GATE's name and assert that resume-bootstrap reports inside:true, which is the case that fails today.

## Files

- `apps/rt/src/commands/event/emit_pipeline.rs`
- `apps/rt/src/commands/event/base_gate.rs`
- `apps/rt/src/commands/event/cli.rs`
- `apps/rt/src/commands/spec/spec_draft.rs`
- `apps/rt/src/commands/spec/cli.rs`
- `apps/rt/src/commands/pipeline/resume_bootstrap/mode_decision.rs`

## Reality Obligations

- **RO-1.1** — This unit was itself opened with the two names aligned — but only because the orchestrator hand-picked an `--intent` whose derived slug happened to match the `--spec` it passed at the gate. That is a coincidence engineered by hand, not a property of the system. Before reporting done, state plainly in your report whether your change makes the alignment STRUCTURAL (impossible to get wrong) or merely LIKELY (still dependent on the caller passing matching strings). If it is the latter, say so — that is a smaller fix than the spec claims and the reader must know.

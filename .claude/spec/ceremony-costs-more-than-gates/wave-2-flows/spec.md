---
id: wave.ceremony-costs-more-than-gates.2-flows
---

# wave-2-flows

## Summary

The flows stop instructing the two gestures the engine no longer needs

## Network

- Parent: [[spec.ceremony-costs-more-than-gates]]

## Tasks

- [ ] plugin/commands/spec.md: the typed `r` IS the approval — remove the sentence stating it pre-answers only the continuation and never grants the approval, and state the new contract in its place, including that the letter alone (no `r`) still routes through the normal approval
- [ ] plugin/refs/spec/resume-loop.md §A: when the marker is already minted by the typed form, skip straight to the dispatch instead of asking for a second gesture — the same shortcut §A already has for approvedByUser:true
- [ ] plugin/refs/feature/full-plan.md: steps 2 and 3 become one call (spec-draft --plan), with plan-materialize named as the re-materialisation door rather than the first-materialisation one
- [ ] Create apps/rt/tests/spec_flow_prose.rs — the structural test AC-4 names, in the repo's both-halves style: the new instruction present AND the superseded sentence gone, so the assertion can actually fail against the old files

## Files

- `plugin/commands/spec.md`
- `plugin/refs/spec/resume-loop.md`
- `plugin/refs/feature/full-plan.md`
- `apps/rt/tests/spec_flow_prose.rs`

## Reality Obligations

- **RO-2.1** — Pin the prose test on text that literally exists in the files BEFORE this wave edits them, so the 'gone' half genuinely fails on the old content — and on nothing wave 1 has yet to land, so this wave is green independently of its sibling (the lesson the previous unit recorded)

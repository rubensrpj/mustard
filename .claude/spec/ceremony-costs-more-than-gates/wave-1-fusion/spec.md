---
id: wave.ceremony-costs-more-than-gates.1-fusion
---

# wave-1-fusion

## Summary

spec-draft materialises the whole layout in one call, and the text the user types becomes the approval

## Network

- Parent: [[spec.ceremony-costs-more-than-gates]]

## Tasks

- [ ] Extract the in-process composite plan-materialize already runs (wave-scaffold renderer + analyze-validation + the negative proof + the pipeline.scope/PLAN emits) into a shared entry point both commands call — a wiring change, not a new engine
- [ ] spec-draft gains --plan <file>: after writing spec.md/meta.json it runs that composite, so one call produces spec.md, meta.json, wave-plan.md and every wave dir with the proof taken in the same pass
- [ ] A proof refusal on the fused path leaves NO layout behind — the command must not half-materialise and then exit 2, or a retry meets a directory it did not create
- [ ] plan-materialize keeps its published behaviour exactly: still the re-materialisation door that reconciles a layout onto an edited plan before approval
- [ ] Create apps/rt/src/hooks/observe/picker_approval_observer.rs — a UserPromptSubmit observer that mints <spec>/.approved-by-user with `via` naming the picker when the USER's own prompt is the picker's approve-and-implement form and the active spec is a Full plan still awaiting approval in PLAN. Mirror approval_marker_observer's three-fact structure and reuse marker_body / approval_marker_path so the provenance is recorded the same way
- [ ] Register the observer in hooks/observe/mod.rs
- [ ] Tests, both-halves style: spec_draft_materialises_the_whole_layout_in_one_call, spec_draft_plan_refuses_an_unproven_criterion, and picker_approval (minted from the user's own prompt; NOT minted when the same text is not the user's prompt — the property the marker exists for)

## Files

- `apps/rt/src/commands/spec/spec_draft.rs`
- `apps/rt/src/commands/pipeline/plan_materialize.rs`
- `apps/rt/src/hooks/observe/picker_approval_observer.rs`
- `apps/rt/src/hooks/observe/mod.rs`

## Reality Obligations

- **RO-1.1** — Confirm from the harness's own hook contract which field of the UserPromptSubmit payload carries the user's literal typed text, and that an observer on that event cannot be reached by anything the model writes — the marker's entire value is that the model cannot author the gesture, so an observer keyed on a forgeable field would silently destroy the gate rather than shorten it

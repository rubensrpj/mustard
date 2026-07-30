---
id: wave.close-eleven-harness-defects-found.1-proof
---

# wave-1-proof

## Summary

The negative proof bites: a blocking confirmation, a caller for the removal pass, the Control command, sufficiency instead of coverage, one placeholder predicate instead of four, and a linter that reads the ledger before contradicting it.

## Network

- Parent: [[spec.close-eleven-harness-defects-found]]

## Tasks

- [ ] close_pipeline: the confirmation stops being advice — a criterion still red after its work landed refuses the close. Its own comment currently says the composite does not block on it.
- [ ] close_pipeline: the removal pass gets its caller. Enumerating every reference outside its module leaves only the CLI flag and the scratch-tree builder, so Removal::Survived is a value no pipeline can produce today.
- [ ] ac_negative_check: add the optional Control pass — a command that must come back GREEN against the tree as it is, proving the expression can match something. Absent, WARN naming the id. Refuse a criterion whose Control is not green.
- [ ] qa_run/mod: the single AC parser learns the Control marker beside Command and Expect. Every other reader goes through it — do not add a second parser.
- [ ] spec_draft and approve_spec: the seeded skeleton offers the new key; the approval reads the Control verdict off the ledger instead of re-running anything.
- [ ] ac_negative_check: replace the bare angle-bracket test with a match on the skeleton token the drafter actually emits, and hoist it to ONE predicate — it is currently spelled independently in four places.
- [ ] analyze_validation and complete_spec: consume that single predicate rather than repeating it.
- [ ] wave_scaffold: the orphan-path check blocks instead of warning, reaches the JSON instead of dying on stderr, and expands wildcards against the tree instead of comparing them literally.
- [ ] analyze_validation: wire in the strict path recogniser that already exists in the same file — the permissive one accepts wildcards — and read the proof ledger before calling a criterion tautological. The measurement sits in the same directory, and the linter's own docstring already says only the negative test can settle it.

## Files

- `apps/rt/src/commands/pipeline/close_pipeline.rs`
- `apps/rt/src/commands/review/ac_negative_check.rs`
- `apps/rt/src/commands/review/qa_run/mod.rs`
- `apps/rt/src/commands/spec/spec_draft.rs`
- `apps/rt/src/commands/spec/approve_spec.rs`
- `apps/rt/src/commands/wave/wave_scaffold.rs`
- `apps/rt/src/commands/review/analyze_validation.rs`
- `apps/rt/src/commands/spec/complete_spec.rs`

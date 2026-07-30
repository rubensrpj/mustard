---
id: wave.close-eleven-harness-defects-found.2-reporting
---

# wave-2-reporting

## Summary

Every reader names what it actually measured: the files section reads a table and says so when it cannot, the digest stops publishing machine-written modules, wave-dependency emits real edges with their origin, and the phase transition confirms itself.

## Network

- Parent: [[spec.close-eleven-harness-defects-found]]

## Tasks

- [ ] wave_lib: the files-section parser accepts a markdown table, extracting the column that carries paths, alongside the bullet form it already reads. Note a SECOND, format-agnostic reader of the same section already exists in analyze_validation — converge rather than add a third.
- [ ] scope_decompose: the diagnostic distinguishes an absent section from one that has content but no recognised path, and stops asserting the section is empty when it is full.
- [ ] i18n: that diagnostic moves out of its hardcoded language, so it follows the spec's own.
- [ ] digest: exemplar_files filters on anchor_eligible, not only on test paths. The same function builds that class filter twelve lines below for hubs and touchpoints; seven other sites already use it.
- [ ] feature: a result whose reason is generated-only withholds its planning fields, as weak and none already do. The sibling predicate already lists it, so this is an asymmetry rather than a convention.
- [ ] wave_dependency: read the depends_on the author declared on the passthrough path and emit the real topological edges on the DAG path, instead of the index chain repeated verbatim at three sites. Every edge carries its origin. The regression test that pins the chain is rewritten — it pins the defect.
- [ ] emit_phase: print the deterministic success line — previous phase and new — reusing the shape emit_pipeline already carries, including on the idempotent short-circuit.

## Files

- `apps/rt/src/commands/wave/wave_lib.rs`
- `apps/rt/src/commands/spec/scope_decompose.rs`
- `packages/core/src/platform/i18n.rs`
- `apps/scan/src/digest.rs`
- `apps/rt/src/commands/feature.rs`
- `apps/rt/src/commands/wave/wave_dependency.rs`
- `apps/rt/src/commands/event/emit_phase.rs`

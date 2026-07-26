---
id: wave.prove-every-acceptance-criterion-can.4-verdict
---

# wave-4-verdict

## Summary

A verification run that could not attempt its criteria stops being allowed to declare a pass — reproduced live on this spec, which already carries a passing verdict with no work done.

## Network

- Parent: [[spec.prove-every-acceptance-criterion-can]]
- Depends on: [[wave.prove-every-acceptance-criterion-can.1-rt]]

## Tasks

- [ ] REPRODUCED while planning this spec: `<spec>/qa-report.json` reads `overall: pass` and a real `qa.result` event landed, for a spec where nothing has been implemented. Nine criteria reported `skip` (a run inside the product's own process cannot rebuild the binary those criteria target) and the tenth — the trailing build-green safety net, rewritten to EXCLUDE precisely the crate under change — compiled green. Read `overall_verdict` and `should_emit_qa_event` in `qa_run/mod.rs` first and confirm both halves before changing anything: a verdict where skips ride along, and an emission guard whose 'verified nothing' means EVERY criterion skipped, which one incidental pass defeats.
- [ ] Change what such a run is allowed to CLAIM: a run in which the criteria that actually exercise the feature were never attempted must not read `pass`, and must not record a passing result for the spec. Keep `skip`'s existing meaning (the criterion could not be attempted) and keep the self-invocation handling exactly as it is — which criteria a self-invoked run can attempt is out of scope; only the verdict drawn from the outcome changes.
- [ ] Do not reach for a threshold or a ratio knob. The honest reading is already available in the data: a criterion that verifies the feature was never attempted, so the run verified nothing about it. Express THAT, and let the existing external-run path stay byte-for-byte unchanged — an external run that genuinely skips a criterion keeps its historical behaviour.
- [ ] Test both directions on the reproduced shape: a run of skips plus one incidental pass does NOT read pass and records nothing, while a run whose criteria genuinely ran and passed still reads pass and still records. The second half is what keeps the fix from passing by making the verdict inert.
- [ ] Report the stray artefacts this defect already wrote for THIS spec (`qa-report.json` and the `qa.result` event in `<spec>/.events/`) in the wave return, so the operator decides whether to clear them — do not delete spec state on your own.

## Files

- `apps/rt/src/commands/review/qa_run/mod.rs`

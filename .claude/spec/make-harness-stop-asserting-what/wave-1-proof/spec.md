---
id: wave.make-harness-stop-asserting-what.1-proof
---

# wave-1-proof

## Summary

The criterion proof gains its second half — red before, green after — and a criterion proven inexecutable gets a sanctioned repair path.

## Network

- Parent: [[spec.make-harness-stop-asserting-what]]

## Tasks

- [ ] Add a confirmation side to the negative proof: a criterion cleared by failing before the work must be re-run after that work lands, and a criterion still red there is reported unproven rather than clearing on its earlier failure.
- [ ] Record the confirmation verdict per criterion in ac-proof.json alongside the existing red verdict, keeping NEVER TAKEN distinct from TAKEN AND RED, exactly as the module already keeps NEVER TAKEN distinct from TAKEN AND GREEN.
- [ ] Teach ac-amend the one case it cannot express today: when the criterion being replaced is recorded as inexecutable, accept a substitute that passes. Everything else keeps refusing a substitute that is not red.
- [ ] Make approve-spec read the confirmation column without re-running anything, matching how it already reads the red column.

## Files

- `apps/rt/src/commands/review/ac_negative_check.rs`
- `apps/rt/src/commands/spec/ac_amend.rs`
- `apps/rt/src/commands/spec/approve_spec.rs`

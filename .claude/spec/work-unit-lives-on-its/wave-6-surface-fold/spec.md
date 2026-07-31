---
id: wave.work-unit-lives-on-its.6-surface-fold
---

# wave-6-surface-fold

## Summary

The pipeline steps stop being doors and the installation flags fold into upsert, leaving exactly four doors.

## Network

- Parent: [[spec.work-unit-lives-on-its]]
- Depends on: [[wave.work-unit-lives-on-its.5-surface-prune]]

## Tasks

- [ ] Turn qa, close and review into internal steps of pr rather than doors of their own.
- [ ] Turn scan into an automatic step of the base gate rather than a door.
- [ ] Fold unhook, rehook and the maint doctor into upsert as flags — same subject, the state of the installation.
- [ ] Make the cancel path for an abandoned unit git delete instead of close, which is what close does today through stage Close plus outcome Cancelled.
- [ ] Lock the four-door surface in the two tests that guard it, so a future command cannot re-expose itself silently.

## Files

- `plugin/commands/qa.md`
- `plugin/commands/close.md`
- `plugin/commands/review.md`
- `plugin/commands/scan.md`
- `plugin/commands/unhook.md`
- `plugin/commands/rehook.md`
- `plugin/commands/upsert.md`
- `apps/rt/tests/template_parity.rs`

---
id: wave.work-unit-lives-on-its.5-surface
---

# wave-5-surface

## Summary

The exposed surface drops from fifteen doors to four: git, pr, spec and upsert.

## Network

- Parent: [[spec.work-unit-lives-on-its]]
- Depends on: [[wave.work-unit-lives-on-its.3-pr-door]], [[wave.work-unit-lives-on-its.4-unit-tools]]

## Tasks

- [ ] Remove the mustard, status, stats, knowledge, maint and skills doors — the natural-language routing is injected on every prompt and does not depend on the mustard command.
- [ ] Turn qa, close and review into internal steps of pr rather than doors of their own.
- [ ] Turn scan into an automatic step of the base gate rather than a door.
- [ ] Fold unhook, rehook and the maint doctor into upsert as flags — same subject, the state of the installation.
- [ ] Keep every removed capability reachable from the four surviving doors; nothing that the flow needs may become unreachable.

## Files

- `plugin/commands/mustard.md`
- `plugin/commands/status.md`
- `plugin/commands/stats.md`
- `plugin/commands/knowledge.md`
- `plugin/commands/maint.md`
- `plugin/commands/skills.md`
- `plugin/commands/qa.md`
- `plugin/commands/close.md`
- `plugin/commands/review.md`
- `plugin/commands/scan.md`
- `plugin/commands/unhook.md`
- `plugin/commands/rehook.md`
- `plugin/commands/upsert.md`
- `apps/rt/tests/template_parity.rs`

---
id: wave.worktree-isolation-becomes-usable-it.3-isolate
---

# wave-3-isolate

## Summary

A second unit is isolated instead of taking over the checkout, and the prose teaches the arrangement that now actually works.

## Network

- Parent: [[spec.worktree-isolation-becomes-usable-it]]
- Depends on: [[wave.worktree-isolation-becomes-usable-it.1-carry]]

## Tasks

- [ ] In `work_branch_gate`, before checking the unit's branch out, ask what the checkout is currently on. On an integration base, cut in place exactly as today. On a DIFFERENT unit's branch, do not take the checkout: cut the worktree for this unit and report where it went, so the session that was already working keeps its branch and its uncommitted edits.
- [ ] Being already on THIS unit's branch stays the silent fast path it is today, and a detached or unreadable HEAD keeps today's behaviour — an unmeasured position must not trigger an isolation the operator did not ask for.
- [ ] Retire the standing nudge: the gate stops telling every in-place unit how to move to a worktree. Isolation now happens when it is needed instead of being offered every time — a suggestion that fires unconditionally is the shape this project has twice found teaches operators to stop reading.
- [ ] Update the operator prose to match: a worktree is what a SECOND unit gets, the project declares what it carries, and the collector reaps what is orphaned. Ratchet it in the existing prose-test shape.

## Files

- `apps/rt/src/hooks/write/work_branch_gate.rs`
- `apps/rt/src/commands/event/work_branch.rs`
- `plugin/refs/git/git-flow.md`
- `apps/rt/tests/plugin_prose_matches_shipped_behaviour.rs`

<!-- wikilinks-footer-start -->
- [spec.worktree-isolation-becomes-usable-it](spec.md)
- [wave.worktree-isolation-becomes-usable-it.1-carry](spec.md)
<!-- wikilinks-footer-end -->
---
id: wave.work-unit-lives-on-its.3-pr-door
---

# wave-3-pr-door

## Summary

A new /mustard:pr door carries list, review and merge, absorbing what review, qa and close do today.

## Network

- Parent: [[spec.work-unit-lives-on-its]]

## Tasks

- [ ] Add a pr-list command that only runs from an integration base and returns each open PR with its number, title and whether it is mergeable.
- [ ] Add a pr-review command that reviews one PR against its spec and the project patterns, and records the verdict the merge step reads.
- [ ] Add a pr-merge command that merges, deletes the branch, returns to the base and pulls — reusing git-settle for the pruning rather than reimplementing it.
- [ ] Make the merge step WARN and ask for confirmation when no review verdict is recorded, never refuse — this is the user's explicit choice.
- [ ] Author plugin/commands/pr.md as the door, delegating each action to the commands above.

## Files

- `apps/rt/src/commands/review/mod.rs`
- `apps/rt/src/commands/review/cli.rs`
- `apps/rt/src/commands/git_settle.rs`
- `plugin/commands/pr.md`
- `apps/rt/tests/run_command_surface.rs`

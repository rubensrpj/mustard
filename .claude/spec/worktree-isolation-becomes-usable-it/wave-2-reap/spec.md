---
id: wave.worktree-isolation-becomes-usable-it.2-reap
---

# wave-2-reap

## Summary

The collector stops reporting and starts collecting — keyed on whether the owner still exists, not on how many days have passed.

## Network

- Parent: [[spec.worktree-isolation-becomes-usable-it]]

## Tasks

- [ ] Make the session-start probe ACT where it is already proven safe (worktree_gc.rs:371): today it runs dry-run and prints a warning above a threshold, so nothing is ever collected at any age. Keep both existing guards untouched — a unit's worktree is git-settle's alone, and a worktree holding uncommitted or untracked work is never removed.
- [ ] Read ownership instead of waiting for age: the removal-proof worktree carries its creator's process id in its own name (`mustard-removal-{slug}-{pid}`, work_removed.rs:321). An owner that no longer exists means orphan, now — the 7-day threshold exists only because nothing else ever told the collector.
- [ ] Widen where the collector looks to include the OS temp dir prefix the removal proof uses. It has always been outside `.claude/worktrees/`, the only tree the collector walks, so an interrupted proof leaked a worktree that nothing could ever reap — this session produced one.
- [ ] Keep the age threshold as the fallback for a worktree whose ownership cannot be read: unmeasured ownership must not authorise removal.

## Files

- `apps/rt/src/commands/maint/worktree_gc.rs`
- `apps/rt/src/commands/review/work_removed.rs`

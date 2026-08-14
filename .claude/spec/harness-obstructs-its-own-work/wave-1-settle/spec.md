---
id: wave.harness-obstructs-its-own-work.1-settle
---

# wave-1-settle

## Summary

The exit ritual verifies before it prunes, and git — not a stricter pre-check — decides whether the base can advance.

## Network

- Parent: [[spec.harness-obstructs-its-own-work]]

## Tasks

- [ ] Move the `base_advanced` computation (today at git_settle.rs:702-706) ABOVE the prune block at 617-641 and make the prune conditional on it: when the unit's base did not advance, prune NOTHING — no `worktree remove`, no `branch -D`, no `push origin --delete` — and answer `ok:false` with the existing `base-behind` reason plus a `nextAction` naming the command that clears it.
- [ ] Authorise the prune on `base_advanced` ONLY. Never on the unit's commit being an ancestor of the base: a squash merge rewrites the commit, so that criterion is false forever and would strand every squash-merged unit. Document the reason where the criterion is read, so the next reader cannot re-derive the ancestry idea.
- [ ] Move the `push origin --delete` at 634 INSIDE the `floor_clear` condition that already guards the local delete, so a settle that could not free the floor stops touching the server branch.
- [ ] Delete the pre-check inside `update_bases` (the `status --porcelain` + `blocks_fast_forward` scan at 416-421) and let `git merge --ff-only` decide. Keep the three report shapes: `updated:true` on success; on refusal, name `dirty-tree` when the tree was in fact dirty and `non-ff-or-no-remote` when it was clean — the same diagnosis, produced AFTER the attempt instead of instead of it.
- [ ] Remove `blocks_fast_forward` and its two exemptions along with the pre-check: the `.claude/worktrees/` and moved-gitlink carve-outs exist only to compensate a guard that no longer exists.
- [ ] Keep everything the field report explicitly asked not to change: the `base-behind` reason and the `updated:false` distinction at 730-751, and `is_merged` as the hard 100% gate.

## Files

- `apps/rt/src/commands/git_settle.rs`

## Reality Obligations

- **RO-1.1** — Confirm against the git binary actually installed that `merge --ff-only` (a) refuses, with no side effect on the working tree, when the advance would update a locally modified path, and (b) succeeds when the local dirt is disjoint from the advanced paths. The whole wave rests on git being the finer authority; the repository cannot answer this.

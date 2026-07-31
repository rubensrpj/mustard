---
id: wave.exit-ritual-must-measure-reachability.1-reachability
---

# wave-1-reachability

## Summary

The exit ritual's evidence stops being a name-level record and becomes a per-ref measurement taken now

## Network

- Parent: [[spec.exit-ritual-must-measure-reachability]]

## Tasks

- [ ] branch_state: make merged_refs answer per REFNAME instead of collapsing refs to a branch-name set — a unit whose local ref is contained while a remote ref moved ahead must not read as contained
- [ ] branch_state: the classifier requires EVERY existing ref of the unit to be contained-now or covered-by-PR before any pruning state; add UnitState::MovedAfterMerge (token `moved-after-merge`) for a merged PR whose refs moved, and carry it through verdict() + report_value
- [ ] branch_state: ProviderPrCli asks for `state,headRefOid` in the SAME gh pr list call and returns evidence carrying the merged head; covered-by-PR means the ref is that head (or an ancestor of it) — PrStatus::Merged alone never authorises pruning again
- [ ] git_settle: is_merged delegates to the shared per-ref predicate in branch_state and the hand-written second copy is deleted — one home for the question
- [ ] git_settle: update_bases treats gitlink-only dirt (paths from parse_submodule_paths) as non-blocking for the ff-only advance; measured — git's own --ff-only accepts it and cleans it
- [ ] git_settle: after the fast-forward, run `git submodule update -- <path>` ONLY for submodules in detached HEAD; a submodule sitting on any branch is reported and left untouched (measured: an unconditional update yanks a live work branch into detached HEAD)
- [ ] git_settle: when the unit's base did not advance in a finishing shape (action settled/partial), the top-level report answers ok:false with reason base-behind; action exit-and-rerun keeps ok:true
- [ ] diff_context: read commit ranges with rev-list --pretty=oneline --no-commit-header through the SAME rtk_command (measured: rtk filters `git log` and drops merge commits, but passes rev-list through byte-identical)
- [ ] Tests, both-halves style like the modules already use: moved_after_merge, settle_refuses_when_a_ref_moved_after_merge, gitlink_only_dirt, base_behind_downgrades_ok, diff_context_reads_ranges_via_rev_list (source-level argv pin — CI has no rtk)

## Files

- `apps/rt/src/shared/branch_state.rs`
- `apps/rt/src/commands/git_settle.rs`
- `apps/rt/src/commands/pipeline/diff_context.rs`

## Reality Obligations

- **RO-1.1** — Confirm against GitHub's official gh/REST documentation that headRefOid of a MERGED pull request is frozen at merge time and does not follow later pushes to the branch — measured true on this repo (PR #133 head `dev` reports dd095023 while dev sits at b33d4264), but the field must be read from the official contract, not from one observation

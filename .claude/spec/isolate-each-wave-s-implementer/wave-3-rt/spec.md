---
id: wave.isolate-each-wave-s-implementer.3-rt
---

# wave-3-rt

## Summary

The way OUT: a new wave-reclaim step that folds a finished wave's commit back onto the work-unit branch, and refuses to report the wave complete when it cannot.

## Network

- Parent: [[spec.isolate-each-wave-s-implementer]]
- Depends on: [[wave.isolate-each-wave-s-implementer.1-rt]]

## Tasks

- [ ] Add `apps/rt/src/commands/wave/wave_reclaim.rs`: given a spec and a wave, locate that wave's agent checkout, verify it carries a commit, and fold it onto the work-unit branch in the MAIN checkout. Because wave 1 makes the cut descend from the unit's HEAD, the common case is a fast-forward; two waves of the same round diverge from a shared point, so the second needs a real merge. Reclaim in completion order, one at a time.
- [ ] Fail CLOSED on anything that would strand work — this is a verdict about integrity, like `git-settle`'s merge check. A conflict, a checkout that cannot be found, or a fold that git refuses returns `{ok:false, reason, files:[…]}` naming the conflicting paths. Never swallow, never force, never `-X ours`.
- [ ] Nothing is destroyed on failure: leave the agent checkout exactly as it is so the operator can inspect it. Remove it only after a proven fold, when it demonstrably holds no work the unit lacks — the same 'prove it merged, only then prune' order `git-settle` uses.
- [ ] Register the subcommand in BOTH required places (crate Guard): the variant in `WaveCmd` and the arm in `dispatch()`, both in `apps/rt/src/commands/wave/cli.rs`, plus the module in `wave/mod.rs`. Add it to `apps/rt/tests/run_command_surface.rs`, which locks the published list — forgetting either registration compiles but silently drops the command.
- [ ] Fold it into `wave_done.rs` as the FIRST step, before the `pipeline.wave.complete` emit: a wave whose work has not returned is not complete. On `ok:false` the composite must NOT emit completion — it returns the blocking reason. An in-place run with no agent checkout is a clean no-op (`ok:true, action:"nothing-to-reclaim"`), so the shared-tree pipeline keeps working byte-for-byte while isolation is still off.
- [ ] Test `wave_reclaim_folds_commit_onto_unit_branch`: a unit branch, an agent checkout cut from it with one commit; reclaim; assert the unit branch contains that commit and the checkout was pruned. Test `wave_reclaim_blocks_completion_on_conflict`: both touch the same line; assert `ok:false`, the conflicting path is named, `pipeline.wave.complete` was NOT emitted, and the agent checkout still exists.

## Files

- `apps/rt/src/commands/wave/wave_reclaim.rs (new)`
- `apps/rt/src/commands/wave/cli.rs`
- `apps/rt/src/commands/wave/mod.rs`
- `apps/rt/src/commands/pipeline/wave_done.rs`
- `apps/rt/tests/run_command_surface.rs`

---
id: wave.work-unit-lives-on-its.4-unit-tools
---

# wave-4-unit-tools

## Summary

git delete removes a unit whole, and a per-branch notebook collects what does not belong to the current spec.

## Network

- Parent: [[spec.work-unit-lives-on-its]]
- Depends on: [[wave.work-unit-lives-on-its.3-pr-door]]

## Tasks

- [ ] Add a git delete action that runs only from an integration base, takes a branch name, and removes the local branch, the remote branch and any open PR for it.
- [ ] Make git delete the cancel path for an abandoned unit, which is what close does today through stage Close plus outcome Cancelled.
- [ ] Add a per-branch notebook: one record per work unit holding what surfaced during the work and does not belong to its spec.
- [ ] Give the notebook a porta rule in the flow docs: what belongs to the spec amends the spec, what does not goes to the notebook.
- [ ] After the PR opens, surface the notebook as the next cycle's prompt so the loop closes back to the base gate.

## Files

- `apps/rt/src/commands/git_settle.rs`
- `apps/rt/src/commands/event/work_branch.rs`
- `plugin/commands/git.md`
- `plugin/refs/git/git-flow.md`
- `apps/rt/tests/run_command_surface.rs`

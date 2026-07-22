---
id: wave.qa-gate-and-settle-multirepo.2-git
---

# wave-2-git

## Summary

Make the exit ritual honest in a monorepo with a submodule: resolve bases from the superproject, say what was resolved when refusing, and report one entry per repository instead of a half-'settled'.

## Network

- Parent: [[spec.qa-gate-and-settle-multirepo]]
- Depends on: [[wave.qa-gate-and-settle-multirepo.1-qa]]

## Tasks

- [ ] In `git_settle.rs`, when the resolved main checkout carries no `mustard.json`, resolve the integration bases from the SUPERPROJECT instead of falling back to the built-in `{main, master}`. Use `git rev-parse --show-superproject-working-tree` (verified: it returns the parent root from inside a submodule and empty otherwise) — never a filesystem walk, so gitfiles and worktrees keep resolving the way git itself resolves them.
- [ ] Make both refusals diagnosable. `no-base-prefix` must name the root it resolved and the bases it knows (and, when a superproject exists, that the configuration lives there) — today it names only the branch, which blames the branch name for a problem of location. `not-a-git-repo` must echo the path it tried: the field incident came from a `--root` that did not exist, NOT from the submodule, and the message hid that.
- [ ] Report one entry per repository of the work unit. Enumerate submodules with `git submodule status` from the main checkout, and for each one carrying the same unit branch report its own `settled` flag and reason, plus a global `complete` that is false while any repository is unsettled. Keep every existing top-level field so current readers do not break. `git-settle` still ACTS on the repository it was pointed at — this task makes the report tell the whole truth, it does not add automatic mutation of a submodule.
- [ ] `plugin/commands/git.md`: the `pr close` bullet must state the submodule-first order that the same file declares as an iron rule three lines above, and which its `commit`, `push` and `pr` bullets already follow. `plugin/refs/git/submodule-rules.md`: describe the close ritual per repository. Change no other prescribed step.
- [ ] Add `pr_close_ritual_names_submodules` to `apps/rt/tests/run_command_surface.rs` (which already reads shipped instruction surfaces): assert the `pr close` description names submodules, so the promise and the ritual cannot drift apart again.

## Files

- `apps/rt/src/commands/git_settle.rs`
- `apps/rt/tests/run_command_surface.rs`
- `plugin/commands/git.md`
- `plugin/refs/git/submodule-rules.md`

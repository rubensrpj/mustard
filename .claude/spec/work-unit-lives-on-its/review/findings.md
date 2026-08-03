# Review — subproject `.` (root) — REJECTED, 2 critical

## CRITICAL 1 — the injected orchestrator still teaches the carve-out this wave deleted

`packages/core/templates/mustard/orchestrator.md:30` (and the byte-identical delivered
`.claude/mustard/orchestrator.md:30`) still reads:

> "Spec authoring (PLAN) writes IN-PLACE — the `work_branch_gate` carves out `.claude/spec/`
> (like `.claude/plans/`), so `spec.md` is written on the base branch with NO worktree"

Both clauses are now false: `work_branch_gate.rs` replaced that arm with `Some(_) => {}` and
`spec_authoring_on_protected_base_is_refused_then_cuts_the_branch` asserts the base write is
DENIED. The spec's own `## Files` listed this file. `template_parity` passes only because
*both* copies are equally stale. This is the surface injected on every user prompt.

## CRITICAL 2 — the documented EXECUTE isolation step can no longer succeed

`plugin/refs/git/git-flow.md:40` ("isolate before the first edit, ONE native step:
`EnterWorktree name={base}_{slug}`") and the same sentence at orchestrator.md:30.

`spec_draft.rs` now calls `cut_work_branch(project_root)` -> `cut_pending_work_branch` ->
`checkout_work_branch(...)` on the MAIN CHECKOUT at PLAN. So by EXECUTE the branch is already
checked out there, and `work_unit_open::open_at` takes the attach path at
`apps/rt/src/commands/work_unit_open.rs:299` (`git worktree add <path> <branch>`), which git
refuses.

Measured independently by the orchestrator in a scratch repo:

```
$ git checkout -b dev_myunit && git worktree add ../wt dev_myunit
Preparing worktree (checking out 'dev_myunit')
fatal: 'dev_myunit' is already used by worktree at '.../probe2/repo'
EXIT: 128
```

-> `{"ok":false,"reason":"worktree-add-failed"}` (work_unit_open.rs:314-320). Before this wave
the branch did not exist at PLAN, so `worktree add -b` worked. This is a regression on the
documented happy path, covered by no AC and by no prose change. Either units become
in-place-only (and both docs must say so) or `open_at` must handle the already-checked-out
branch.

## MAJOR — `pr-review`'s brief is inert where `pr.md` tells you to run it

`apps/rt/src/commands/review/pr_door.rs:104` resolves `main_checkout_root(root)`, but this wave
moved `.claude/spec/<slug>/` onto the work branch. Run from the base (where `pr list` refuses
to be anywhere else, and where `pr.md:47-53` places step 2), `spec_md.filter(|p| p.exists())`
is `None`, so `spec_path`, `subproject` and `patterns` all come back null — yet `pr.md:53`
promises they are populated. The spec's own `## Decisions` says a per-unit artifact must
resolve `rev-parse --show-toplevel`; `notebook.rs:45` obeys it, `pr_door.rs` does not.
`recorded_verdict`/`record_review` then write `review.result` into a `.claude/spec/<slug>/`
the base branch does not track.

## MINOR — `RUNTIME_WHITELIST` grew by five instead of shrinking

`apps/rt/tests/template_parity.rs` adds `diagnose-otel`, `maint-deps`, `maint-validate`,
`metrics`, `status` — five `run` subcommands whose only caller was a pruned door.

## Acceptance criteria — all nine independently re-run, all green

AC-1 base_gate 5 passed · AC-2 spec_authoring_on_protected_base 1 · AC-3
resume_inside_own_branch 1 · AC-4 pr_list 2 · AC-5 pr_merge_without_verdict 1 · AC-6
git_delete 3 · AC-7 notebook 3 · AC-8 exposed_doors 1 · AC-9 build clean,
`cargo test --workspace` 4688 passed 0 failed.

# Review — root (.) — APPROVED (0 critical, 3 minor)

Every AC run by the reviewer with real output: AC-1..AC-9, AC-11, AC-13, AC-14 → `1 passed` each;
AC-12 → `git check-ignore -v --no-index .claude/scratch/probe.sh` prints
`.claude/.gitignore:14:scratch/`, rc 0; AC-10 → `cargo build --workspace` 0 errors.
Controls: `git_settle` 56 · `work_branch_gate` 21 · `prose_teaches` 10 · `claude_paths` 16.
`cargo test --workspace`: no FAILED/panicked. `cargo clippy --workspace --all-targets`: 0 errors.

Guards: no new `run` subcommand; no panic/unwrap/expect outside `#[cfg(test)]`; observers and
`main.rs` untouched; new report fields byte-stable; `seed_gitignore` writes through
`crate::io::fs::write_atomic`, never `std::fs`. Mold contract is vacuous — no `*-pattern`
directory exists, so the earlier review's `rt-gate-pattern` compliance claim was unverifiable
(not a defect).

Independent checks that HELD:
- Prune gate really precedes the prune (`base_advanced` at :644-648, prune block at :662-710);
  remote delete inside `floor_clear` at :703-704; `blocks_fast_forward`/`status_path` gone.
- "A repo with no remote can never settle now" — REFUTED: `is_merged` (:371) already measures
  against `origin/<base>`, so no remote never passed the gate to begin with.
- Negative control real: 31 files tracked under `.claude/spec/*/qa/`, and
  `git check-ignore .claude/spec/demo/qa/report.md` → rc 1.
- `mustard_core::CLAUDE_GITIGNORE` is `include_str!` of the shipped template, so the test
  exercises the real file; `.claude/.gitignore` and the template are byte-identical.
- Mutation sensitivity reasoned per test: each new assertion inverts a value the pre-change code
  produced (`remoteDeleted` true→false, `updated` false→true, `Deny`→`Allow`,
  `Preserved`→`Updated`).

## MINOR 1 — `plugin/commands/bugfix.md:29` describes a dead-end that usually is not one

The ordering warning says a write from a bare integration base "is REFUSED and the flow dead-ends
here", omitting the case `feature.md:42` spells out: with a pending marker the auto-branch hook
cuts the branch ON that very write and it lands. Two flows now describe the same mechanism
differently. Errs conservative, but tells the reader of a dead-end the common path does not hit.

## MINOR 2 — `plugin/commands/git.md:47` has no prose for the refusal this unit introduces

It still describes the second settle as "(pull, remove the worktree, delete local + remote
branch)". True of the happy path, but the refusal shape this unit adds — `base-behind` +
`nextAction` + `restoredToUnit`, nothing pruned — has no operator-facing prose and no AC
ratcheting it.

## MINOR 3 — `apps/rt/src/commands/git_settle.rs:414` mislabels a divergence on a dirty tree

A refused advance is labelled `dirty-tree` whenever `status --porcelain` is non-empty, so a
genuine divergence that happens on a coincidentally dirty tree reports `dirty-tree` and points
the operator at cleaning rather than at the divergence. Acknowledged in the adjacent comment as
"the common case". Note AC-5 currently SPECIFIES this behaviour, so changing it needs `ac-amend`.

# Review — apps/rt — APPROVED (0 critical, 1 major, 3 minor)

`cargo test --workspace` → 4788 passed, 0 failed, 6 ignored (exit 0).
`cargo clippy --workspace --all-targets` → 0 errors, 173 pre-existing warnings.
Every AC run with real output: AC-1..AC-9, AC-11, AC-13, AC-14 pass (several report `1 passed`
on each of two targets, since the module compiles into both the lib and bin test binaries);
AC-12 `git check-ignore` exit 0; AC-10 `cargo build --workspace` 0 errors.
Controls: `git_settle` 28×2 · `work_branch_gate` · `prose_teaches` 10 · `claude_paths` 16 ·
`run_command_surface` 8 · `template_parity` 3 — all green.

Guards + molds: no new `run` subcommand (both reverse ratchets green); `rt-gate-pattern`
respected — `WorkBranchGate` stays a stateless unit `Check`, the carve-out is a pure
`fn is_harness_carve_out(&str) -> bool` reached before any IO, and `relative_to_cwd` normalises
to `/` so the prefix matches on Windows; observers and `main.rs` untouched; no `unwrap`/`expect`
outside `#[cfg(test)]`; new report fields deterministic; `merged_refs` deleted, not silenced.

Prior round's findings confirmed closed: `.claude/.gitignore` byte-identical to the template,
`seed_gitignore` merges by line at `project_seed.rs:335`, `restoredToUnit` covered by AC-14,
`spec/*/qa/` out of the template while this repo's 31 `qa/report.md` stay tracked.

## MAJOR — AC-7 verifies less than it claims

The test picks its own 4-item `ARTEFACTS` list. Seeding the shipped template into a fresh repo
and writing the paths the runtime really creates leaves SEVEN untracked:

```
?? .claude/.compact-state/s.json        (session_cleanup_observer.rs:183)
?? .claude/.dispatch/x.md               (wave_done.rs:913)
?? .claude/knowledge/k.md               (epic_fold.rs:167)
?? .claude/spec/demo/.dispatch/w1.md
?? .claude/spec/demo/.memory-approved   (context_inject.rs:281)
?? .claude/spec/demo/economy-baselines.json
?? .claude/.dashboard.pid
```

All seven are already covered by this repository's own root `.gitignore` (lines 71, 99-100, 115,
106, 127, 83) and absent from `packages/core/templates/.gitignore` — the SAME omission class as
the previous round's CRITICAL, different entries. Not blocking (the spec's Definitions narrow
"harness artefact" to three sidecars, and wave 1 removed the dirty pre-check outright), but the
criterion's wording and the test's name are broader than what ships.

## MINOR — AC-2's proof is one-directional

`git_settle.rs:1783` proves ancestry is not SUFFICIENT; it never exercises a non-ancestor
(squash) unit, which no fixture can reach without a provider stub. The claim still holds
structurally — there is no ancestry test anywhere in the prune path — and the test doc says so.

## MINOR — `restoredToUnit` and `nextAction` reach no operator prose

`git.md:47` only says "print each JSON verbatim". Two new mechanisms with no documented reader,
in the same unit whose sibling ratchet file exists to catch exactly that. (Raised independently
by the root reviewer.)

## MINOR — `restoredToUnit:false` is ambiguous

At `git_settle.rs:719`: when the in-place `checkout <base>` was itself refused, the operator IS
on the unit branch but the field reads false.

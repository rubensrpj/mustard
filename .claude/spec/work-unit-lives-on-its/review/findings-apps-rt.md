# Review — subproject `apps/rt` — REJECTED, 1 critical

## CRITICAL — the shipped router still teaches the behaviour this spec deleted

`packages/core/templates/mustard/orchestrator.md:30` and the delivered copy
`.claude/mustard/orchestrator.md:30` both still read:

> `Spec authoring (PLAN) writes IN-PLACE — the work_branch_gate carves out .claude/spec/
> (like .claude/plans/), so spec.md is written on the base branch with NO worktree`

AC-2 removed exactly that carve-out (`work_branch_gate.rs:296-305`), and the spec's `## Files`
names BOTH copies. The paragraph directly above (line 28) was rewritten for the base gate and
line 40 for the four doors — line 30 was left asserting the opposite of what wave 2 shipped.
This file is injected on every user prompt (`mustard.json#inject`), so the router is told every
turn that specs live on the base while `spec_draft::cut_work_branch` now cuts the branch first
and the gate denies a bare-base spec write. This is the exact defect
`apps/rt/tests/plugin_prose_matches_shipped_behaviour.rs` exists to catch, and nothing guards
this sentence.

## Non-blocking

- **minor** `apps/rt/src/commands/review/pr_door.rs:366` — `spec_path` puts an ABSOLUTE machine
  path on an `ok:true` report (its own test asserts with `ends_with`, which is the tell).
  `notebook.rs:138-142` in this same wave strips the prefix "so the report carries no machine
  path", and `git_settle.rs:538-543` states the norm (absolute paths on refusals only). The two
  new modules disagree with each other.
- **minor** `pr_door.rs:379` — `MergeConsent` omits `Clone`; `rt-verdict-pattern` specifies
  `#[derive(Debug, Clone, PartialEq, Eq)]` for the payload-carrying flavour. `merge_core` also
  lacks `#[must_use]` while its sibling builders carry it.
- **minor** `notebook.rs:100` uses `git_settle::unit_slug` (splits on the first `_`, any prefix)
  while `pr_door.rs:125` uses `base_of_branch` (declared bases only) — `--unit feature_x`
  silently opens `.claude/spec/x/notebook.md`. Two spellings of "which unit is this".
- **note** `base_gate.rs:152` runs `git fetch origin` on every `pipeline.kind` emit, and
  `refresh_census_if_stale` can trigger a full workspace walk in the same hot path.

## Effectiveness proven end-to-end

In a synthetic `dev`/`main` repo the built binary on `dev_some-unit` answered
`BLOCKED: [Base Gate] the checkout 'dev_some-unit' is not an integration base ... git checkout
dev` with `EXIT=2`; on `dev` it emitted the single line
`{"ok":true,"kind":"pipeline.kind","spec":"demo","branch":"dev_demo"}` with the census-refresh
diagnostic on STDERR — stdout stays byte-stable per the crate Guard.

Agnosticism change request addressed and proven: every base set comes from `git.flow`;
`base_gate.rs:289` and `mode_decision.rs:236` assert against a `develop`/`master` project.

## Acceptance criteria — all nine re-run, all green

AC-1 5 passed · AC-2 1 · AC-3 1 unit + 1 prose ratchet · AC-4 2 · AC-5 1 · AC-6 3 · AC-7 3 ·
AC-8 1 · AC-9 build 0 errors. `cargo test --workspace` 4688 passed 0 failed;
`cargo clippy --workspace --all-targets` 0 errors.

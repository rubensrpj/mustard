---
id: spec.every-git-test-rebuilds-its
---

# every git test rebuilds its repository from scratch with eighteen git calls, when it could clone a template built once

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Context

This is the last repair standing after two cheaper ones were measured and spent. `git init --template=` saved 15% of an init — 1,1s across a module. Declaring the CI's test parallelism saved 54s per Windows round, a sixth of what its local curve promised. Neither touched the cause, because the cause is not that any single `git` call is slow: a bare `git --version` costs 0,037s. The cause is HOW MANY of them there are.

A `git_settle` test makes eighteen before it asserts anything. It builds a bare origin, a checkout, two configs, a branch, a commit, a remote, a push, two worktrees with their own commits, a merge, another push and a reset — and only then tests the thing it came to test. That scenery costs 1,18s of the test's 1,74s. Two thirds of what those 28 tests spend is spent building a world to throw away.

The scenery is IDENTICAL every time. So it can be built once per test process and copied, which was measured at 0,28s per test — **4,2× faster**.

Copying has one trap, and it was found by checking rather than by timing: git records the remote URL and the worktree registration as ABSOLUTE paths. A naive copy produces clones that still resolve to the template's origin and the template's worktree, so tests in parallel would write over one another. Worse, `git worktree repair` does not notice, because while the template still exists nothing looks broken. The repair is to copy the whole tree — origin included — and rewire both paths explicitly, verified afterwards: two git calls against eighteen.

After this: the four heaviest test modules clone their scenery instead of rebuilding it, and each clone is provably independent of the template and of every other clone.

## Users/Stakeholders

Whoever runs the suite — locally on every change, and in CI on every push. The four modules in scope (`git_settle`, `work_branch_gate`, `work_branch`, `worktree_gc`) are 155 tests, 7,5% of the suite, and 94,8s of its 214,7s.

## Success Metric

The suite runs materially faster with the same number of tests passing, and each cloned fixture is independent — its remote and its worktrees resolve inside its own directory, never into the template. Independence is asserted by a test, not assumed: a clone that silently shares the template's origin would trade wall clock for intermittent failures, which is a worse defect than the one being fixed.

## Non-Goals

- The other 1910 tests. They do not spawn git and have nothing to gain; touching them is risk without benefit.
- Changing what any test ASSERTS. This unit changes how the scenery is built and nothing about what is checked on top of it — a fixture change that alters a verdict is a bug in the change, not a finding.
- `cargo nextest`, more CI runners, or splitting the suite. Different levers, none of them measured here.
- The release profile, the CI matrix, and the incremental policy — all settled by the two units before this one.

## Acceptance Criteria

- **AC-1** — when a test asks for the fixture, then the repository it receives was CLONED from the shared template rather than rebuilt, which the clone carries proof of
  Command: `cargo test -p mustard-rt --lib a_fixture_is_cloned_from_the_shared_template`
  Expect: `[1-9][0-9]* passed`
- **AC-2** — when two fixtures are handed out, then each one's remote and worktrees resolve INSIDE its own directory, never into the template or into the other clone
  Command: `cargo test -p mustard-rt --lib a_cloned_fixture_is_independent_of_its_template`
  Expect: `[1-9][0-9]* passed`
- **AC-3** — the project build passes green
  Command: `cargo build --workspace`

## Checklist

- [ ] T1 — add the template-clone helper: build the scenery once per process, copy the tree per test, rewire the remote URL and repair each worktree registration.
- [ ] T2 — add `a_cloned_fixture_is_independent_of_its_template`, asserting that two clones resolve their remote and worktrees inside themselves.
- [ ] T3 — move `git_settle`'s fixture onto the helper and measure the module before and after.
- [x] T4 — extend to `work_branch_gate`, `work_branch` and `worktree_gc` only if T3's number justifies it; record the measurement either way. **NOT extended — the number does not justify it. See below.**

## Outcome — the gain is real and a fifth of what was predicted, because the ruler was wrong

| | before | after |
|---|---|---|
| `git_settle` module | 48,7s | **43,13s** (−11%) |
| whole `mustard-rt` suite | 214,7s | **209,2s** (−2,6%) |
| tests passing | 2065 | 2067 (the two new ones) |

**Predicted 4,2×; delivered 11% of a module.** Two hypotheses were tested for why, and the second one holds:

1. *Copying stops scaling when 22 threads do it at once.* **Refuted** — 20 parallel copies are 2,3× faster in total than 20 sequential ones (0,058s each against 0,131s). Disk contention is not the ceiling.

2. *The benchmark measured the wrong thing.* **Confirmed by arithmetic**: 5,6s saved across 27 tests is 0,2s per test, not the 0,9s predicted — so building the fixture IN RUST never cost 1,18s, it cost about 0,26s. The benchmark invoked `git` **through PowerShell**, which adds its own per-invocation cost; the test helper uses `Command::new` directly. It measured 18 git spawns PLUS 18 PowerShell spawns and charged all of it to git, inflating the build side roughly 4,5×.

This is a different class of error from the refutations before it. The idea was right and the gain is real — the RULER was wrong. **Do not benchmark in PowerShell what runs in Rust**: the measuring instrument entered the measurement.

**Not extended to `fixture_with_submodule`.** At a true build cost of ~0,26s, its 7 tests would yield about 2s — against touching the most delicate fixture in the file, the one that wires real git submodules. The arithmetic does not pay for the risk.

**What is worth more than the 5,6s**: `a_cloned_fixture_is_independent_of_its_template`. It fixes in writing that a cloned fixture may never share a remote or a worktree with another — the trap the first attempt fell into, which surfaced only because the clone was INSPECTED rather than merely timed. Wall clock traded for intermittent failures would have been a worse suite than the slow one.

## Definitions

- **fixture** — the repository a test builds before it can test anything: a bare origin, a working checkout with commits, a remote wired between them and — in git_settle's case — worktrees. It is scenery, not subject matter, and every test currently builds its own from nothing
- **template clone** — the alternative measured here: build the scenery ONCE per test process, then copy the directory tree per test and rewire the two paths that copying breaks (the remote URL and the worktree registration). Two git calls instead of eighteen

## Decisions

- the whole tree is copied — origin included — not just the working repository
  Reason: copying only the checkout leaves its remote pointing at the ORIGINAL bare repo, so every test would push into one shared origin. Measured and observed: the first attempt produced copies whose remote AND worktree still resolved to the template. Tests running in parallel would write over each other, which trades wall clock for intermittent failures — the worst kind of defect
- the copy rewires the remote URL and repairs the worktree registration explicitly
  Reason: git stores ABSOLUTE paths for both. `git worktree repair` alone does not fix them while the template still exists, because git sees nothing broken. Verified after rewiring: the copy's remote resolves to its own origin.git and its worktree to its own directory

## Evidence

- the git_settle fixture makes eighteen git invocations before a test asserts anything — two inits, two configs, a checkout, add/commit, remote add, push, two worktree adds with their own add/commit, a merge, a push and a reset
  Evidence: `apps/rt/src/commands/git_settle.rs:1067`
- building that fixture costs 1,18s; copying the tree and rewiring the two absolute paths costs 0,28s — 4,2× faster, measured five times each on this machine
  Evidence: `apps/rt/src/commands/git_settle.rs`
- the 28 tests of git_settle take 48,7s, or 1,74s each, and the fixture is 1,18s of that — scenery is roughly two thirds of what those tests spend
  Evidence: `apps/rt/src/commands/git_settle.rs`
- a naive copy leaves the clone sharing the template's origin AND its worktree: both are recorded as absolute paths, and `git worktree repair` does not correct them while the template directory still exists, because nothing is broken from git's point of view
  Evidence: `apps/rt/src/commands/git_settle.rs`
- process creation is the cost being avoided: a bare `git --version` costs 0,037s on this machine, and 155 tests across four modules spend 94,8s of the suite's 214,7s making dozens of such calls each
  Evidence: `apps/rt/src/commands/git_settle.rs`
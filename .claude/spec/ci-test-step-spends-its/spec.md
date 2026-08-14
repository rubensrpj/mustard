---
id: spec.ci-test-step-spends-its
---

# the CI test step spends its time waiting on spawned git processes, so the runner runs fewer tests at once than it could

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Context

The unit before this one (`build-test-cycle-is-too`) left the CI's Test step as the last standing cost and, crucially, said WHY: with a warm cache the step is 428s on Windows against 111s on Linux, so it is the tests RUNNING, not compiling or linking. Measured further here: 155 tests — 7,5% of the suite — account for 94,8s of 214,7s locally, and `git_settle` alone (28 tests) takes 48,7s, or 1,74s per test. A test at 1,74s is not computing; it is waiting on `git`.

Two repairs were measured and dropped before this one. `git init --template=` (skipping the thirteen sample hooks) saves 15% of an init — 1,1s across the whole module, real and irrelevant. And rewriting the 155 tests to call `git` less often is the honest fix, but it is a refactor of a seventh of the suite for a benefit nobody has measured yet.

What makes this unit one line instead: work that WAITS scales with threads past the core count, and the suite already proves it. On this 22-core machine, forcing the runner's own shape — 4 test threads — costs 65,4s for `git_settle`, while 8 threads costs 38,4s. Beyond 8 it degrades (12 → 46,0s, 16 → 44,2s). A GitHub Windows runner has 4 cores, so `cargo test` defaults to 4 threads and leaves the processor idle while `git` answers.

After this: the CI names its test parallelism instead of inheriting a number derived from core count, which is the wrong basis for a suite that spends its time waiting.

## Users/Stakeholders

Whoever waits on a merge. The Windows leg is both the slowest runner and the platform where spawning a process is most expensive, so it is where the idle time concentrates.

## Success Metric

The CI declares its test thread count explicitly, with the measurement that chose it recorded beside it. Whether the runner's own numbers confirm the local ratio is settled by the pull request's own checks — this unit's change IS the experiment, and a null result is reported as one rather than quietly kept.

## Non-Goals

- Rewriting the 155 git-heavy tests. Named and measured here; spending that refactor needs its own evidence that the cheap knob was not enough.
- `git init --template=`. Measured at 15% of an init, 1,1s over the module. Skipped on the number, not on taste.
- Changing the local developer default. This machine has 22 cores and its default is already near optimal (44,7s against 39,5s at 16 threads); the runner is the one inheriting a bad number.
- `cargo nextest` or any new tool in the CI. A different runner is a bigger change than a flag, and no measurement here calls for it yet.

## Acceptance Criteria

- **AC-1** — when the CI test step runs, then it declares its test thread count explicitly instead of inheriting one derived from the runner's core count
  Command: `git grep -c "test-threads" -- .github/workflows/ci.yml`
  Expect: `:[1-9][0-9]*$`
- **AC-2** — when the workflow is read, then the chosen number carries the measurement that chose it, so the next reader can refute it instead of guessing
  Command: `git grep -ci "38,4s\|65,4s" -- .github/workflows/ci.yml`
  Expect: `:[1-9][0-9]*$`
- **AC-3** — the project build passes green
  Command: `cargo build --workspace`

## Checklist

- [x] T1 — declare the test thread count in `.github/workflows/ci.yml`, with the measured curve (4 → 65,4s, 8 → 38,4s, 12 → 46,0s) in the comment beside it.
- [ ] T2 — read the PR's own Windows check against the 15m30s baseline of PR #156 and record the real gain — including a null result.
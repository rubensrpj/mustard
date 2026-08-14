---
id: wave.build-test-cycle-is-too.plan
---

# Wave Plan

## Wave Table

| Wave | Spec | Role | Depends on | Summary |
|------|------|------|------------|---------|
| 1 | [[wave.build-test-cycle-is-too.1-measure]] | measure | — | Measure where the cycle's time actually goes — locally on Windows and in CI — and write the numbers into the tree, so every later change is decided by a datum instead of a plausible story. |
| 2 | [[wave.build-test-cycle-is-too.2-ci]] | ci | [[wave.build-test-cycle-is-too.1-measure]] | Stop CI from compiling the whole workspace three times per operating system, and make its incremental policy explicit instead of inherited. |
| 3 | [[wave.build-test-cycle-is-too.3-profile]] | profile | [[wave.build-test-cycle-is-too.1-measure]] | Bound the incremental cache by a policy declared in the repository, so 52 GB cannot accumulate again while everyone waits for someone to remember a command. |

## Acceptance Criteria
- AC-1 — when the measurement wave finishes, then the repository carries a versioned record naming each phase of the cycle with its measured duration. Command: `git ls-files --error-unmatch docs/2026-08-14-build-cycle-measurements.md`
- AC-2 — when that record is read, then it states the Windows and Linux costs of the same work. Command: `git grep -ci "windows" -- docs/2026-08-14-build-cycle-measurements.md`  Expect: `^[1-9][0-9]*$`
- AC-3 — when CI runs, then it no longer compiles the workspace three separate times per operating system. Command: `git grep -c "run: cargo" -- .github/workflows/ci.yml`  Expect: `:[0-2]$`
- AC-4 — when CI runs, then it declares its incremental-compilation policy explicitly. Command: `git grep -q "CARGO_INCREMENTAL" -- .github/workflows/ci.yml`
- AC-5 — when a developer builds locally, then the incremental cache is bounded by a declared policy in the repository. Command: `git grep -qE "^ *incremental *=" -- Cargo.toml`
- AC-6 — the project build passes green. Command: `cargo build --workspace`

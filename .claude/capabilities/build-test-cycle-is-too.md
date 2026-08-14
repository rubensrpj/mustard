---
id: cap.build-test-cycle-is-too
status: active
---

# build test cycle is too

### Requirement: The system SHALL satisfy the acceptance criteria of spec build-test-cycle-is-too.

#### Scenario: AC-1
- when: the measurement wave finishes
- then: the repository carries a versioned record naming each phase of the cycle with its measured duration
- command: `git ls-files --error-unmatch docs/2026-08-14-build-cycle-measurements.md`

#### Scenario: AC-2
- when: that record is read
- then: it states the Windows and Linux costs of the same work, so the platform gap is a measured number and not a suspicion
- command: `git grep -ci "windows" -- docs/2026-08-14-build-cycle-measurements.md`

#### Scenario: AC-3
- when: CI runs
- then: it no longer compiles the workspace three separate times per operating system
- command: `git grep -c "run: cargo" -- .github/workflows/ci.yml`

#### Scenario: AC-4
- when: CI runs
- then: it declares its incremental-compilation policy explicitly instead of inheriting a default that only costs write time on a throwaway runner
- command: `git grep -q "CARGO_INCREMENTAL" -- .github/workflows/ci.yml`

#### Scenario: AC-5
- when: a developer builds locally
- then: the incremental cache is bounded by a declared policy in the repository rather than by someone remembering to delete it
- command: `git grep -qE "^ *incremental *=" -- Cargo.toml`

#### Scenario: AC-6
- when: 
- then: the project build passes green
- command: `cargo build --workspace`

## Covers

## Specs
- [[spec.build-test-cycle-is-too]]

## Related


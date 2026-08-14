---
id: spec.build-test-cycle-is-too
---

# the build and test cycle is too slow and the target directory grows without bound: measure where the time actually goes in the local loop and in CI, then fix what the measurement names

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Context

Waiting is now the dominant cost of working on this project, and nobody knows where the waiting actually goes.

Three numbers say it. The same continuous-integration (CI) job that takes 5m46s on Linux takes 19m18s on Windows — the same commit, the same steps, a cache present on all three runners. The `target/` directory on the operator's machine reached 85 GB, of which 52 GB were a compilation cache that nothing ever prunes. And a release build of a single binary took 6m38s today, which is what stands between merging a change to the harness and actually running it.

What makes this a unit rather than a tweak is that every plausible repair is UNMEASURED. Turning the incremental cache off might help or hurt. Dropping the dependency optimisation might shorten the cold build or lengthen every test run. Merging the 59 test binaries into fewer would cut link time and cost isolation. Each is a coin flip dressed as an opinion, and a performance change shipped on an opinion is the kind that makes things slower while everyone believes it helped.

So the unit measures first and changes second — not out of caution, but because there is no other way to know. After it: the operator can say where the cycle's time goes, with numbers; the two costs that are wasteful under ANY measurement are gone; and the cache stops growing without anyone having to remember a command.

## Users/Stakeholders

Whoever waits on this project — which today is one operator, on Windows, on every single change. The Windows leg matters most because it is both the slowest CI runner and the machine the work happens on, so the same platform penalty is paid twice: once per local edit, once per merge.

## Success Metric

The cycle's cost is KNOWN before it is changed: a versioned record names each phase of the local loop and of CI with its measured duration, on Windows and on Linux, so the platform gap is a number rather than a suspicion. On top of that, two wastes proven independent of any measurement are removed — CI stops compiling the workspace three times per operating system, and the incremental cache stops growing without a bound — and the record states the before and after of each.

## Non-Goals

- Rewriting the test suite. The 59 test binaries are a real cost, but merging them trades isolation for link time and that trade needs the measurement this unit produces before anyone makes it. This unit measures that cost and names it; it does not spend it.
- Changing the release profile's `lto` / `codegen-units`. They are correct for a shipped binary and the slow release build is the price of that correctness. If the measurement says the local install path should use a different profile, that is a follow-up with its own evidence.
- Removing the three-operating-system CI matrix. The cross-platform picture is the point of it; making it cheaper is in scope, making it narrower is not.
- Tuning the machine (antivirus exclusions, disk, toolchain). Real effects, but they belong to the operator's environment and not to this repository — the unit may NAME them in the record, and changes nothing outside the tree.

## Acceptance Criteria

- **AC-1** — when the measurement wave finishes, then the repository carries a versioned record naming each phase of the cycle with its measured duration
  Command: `git ls-files --error-unmatch docs/2026-08-14-build-cycle-measurements.md`
- **AC-2** — when that record is read, then it states the Windows and Linux costs of the same work, so the platform gap is a measured number and not a suspicion
  Command: `git grep -ci "windows" -- docs/2026-08-14-build-cycle-measurements.md`
  Expect: `^[1-9][0-9]*$`
- **AC-3** — when CI runs, then it no longer compiles the workspace three separate times per operating system
  Command: `git grep -c "run: cargo" -- .github/workflows/ci.yml`
  Expect: `:[0-2]$`
- **AC-4** — when CI runs, then it declares its incremental-compilation policy explicitly instead of inheriting a default that only costs write time on a throwaway runner
  Command: `git grep -q "CARGO_INCREMENTAL" -- .github/workflows/ci.yml`
- **AC-5** — when a developer builds locally, then the incremental cache is bounded by a declared policy in the repository rather than by someone remembering to delete it
  Command: `git grep -qE "^ *incremental *=" -- Cargo.toml`
- **AC-6** — the project build passes green
  Command: `cargo build --workspace`

<!-- PLAN -->

## Files

| File | What changes | Wave |
|---|---|---|
| `docs/2026-08-14-build-cycle-measurements.md` (new) | The measured record. Each phase of the local loop (cold build, warm build, test compile, test run, clippy) and of CI, timed on Windows, with the Linux CI figures alongside for the platform gap. Names what `cargo build --timings` attributes the time to, and states the cost of the 59 test binaries separately from the cost of compiling the crates | 1 |
| `.github/workflows/ci.yml` | The three cargo invocations collapse so the workspace is not compiled from scratch three times per runner, and the incremental policy is declared rather than inherited | 2 |
| `Cargo.toml` | The dev profile declares its incremental policy, so the cache is bounded by the repository instead of by memory. Whether the existing `[profile.dev.package."*"] opt-level = 1` survives is decided by wave 1's numbers, not here | 3 |
| `apps/rt/src/commands/review/analyze_validation.rs` | Declared cascade, found while writing THIS spec's own criteria: an `Expect:` regex anchored at `^` against a per-file counting command (`grep -c` prints `file:count`) can never match its own output, so the criterion is red before the work and red after it. The negative proof clears it — the red is real, only its cause is the regex — and `ac-amend` then refuses the repair once the wave has delivered, because the corrected regex passes. The defect therefore has no door left after the spec freezes, so a new `expect-anchored-against-prefixed-output` WARN catches it at drafting time, which is the only cheap moment | 1 (cascade) |

## Boundaries

IN: measuring the local loop and CI and recording the result in the tree; the CI workflow's compilation steps and its incremental policy; the dev profile's incremental policy; naming — without spending — the cost of the 59 test binaries and of the platform gap.

OUT: merging or restructuring the test files; the release profile's `lto` / `codegen-units`; the three-OS matrix itself; anything outside this repository (antivirus, disk, toolchain), which the record may name but the unit never changes; the `apps/dashboard/src-tauri` and `apps/translate` crates, which the CI workflow deliberately excludes and this unit does not bring in.

## Definitions

- **cold build** — a compilation that starts from an empty or invalidated target/ — what a CI runner does on a cache miss, and what this machine now does after the 52 GB incremental cache was deleted
- **incremental cache** — target/debug/incremental — the intermediate state rustc keeps between compilations so it can recompile only what changed. Nothing prunes it: it grows monotonically for the life of the checkout, and past a certain size the cost of reading and writing it exceeds what it saves
- **test binary** — each .rs file under a crate's tests/ directory becomes its OWN executable, compiled and linked in full. The cost of the test step scales with the NUMBER of these files, not with the number of assertions inside them
- **the cycle** — what the operator waits on between asking for a change and learning whether it worked: locally the build plus the test run, and in CI the three cargo invocations the workflow performs on each of three operating systems

## Decisions

- the unit MEASURES before it changes any build setting
  Reason: this project's standing law is that tuning is a DATUM, never a guess. Every candidate here — turning incremental off, cutting the dependency opt-level, merging test binaries — is plausible and none is proven; shipping one on plausibility is how a performance change makes things slower and nobody notices
- the unit covers BOTH the local loop and the CI, not one of them
  Reason: both were measured and both block the work: the local loop is what the operator waits on per change, and the CI's 19-minute Windows leg is what stands between a green branch and a merge. A fix aimed only at the local profile would leave the merge gate exactly as slow
- deleting the incremental cache by hand is treated as relief, not as the repair
  Reason: the 52,28 GB were reclaimed today by hand, and nothing in the repository prevents them from accumulating again. A repair that depends on someone remembering to run a command is not a repair

## Evidence

- the CI compiles the workspace about three times per operating system: `cargo build`, then `cargo test` (which additionally compiles every integration-test binary), then `cargo clippy` — and clippy cannot reuse the earlier artefacts because it drives the compilation through clippy-driver rather than rustc, so it recompiles the whole graph
  Evidence: `.github/workflows/ci.yml:48`
- the workspace has 59 integration-test files, each of which becomes its own linked executable: 32 in apps/rt, 21 in apps/scan, 5 in packages/core and 1 in apps/cli — the link step is paid 59 times, and linking is the slowest phase on Windows
  Evidence: `.github/workflows/ci.yml:56`
- the dev profile optimises every transitive dependency via [profile.dev.package."*"] opt-level = 1, which is paid in full on every cold build; its stated intent is to cut the cold-build tail, and whether it does so on this workspace has never been measured
  Evidence: `Cargo.toml`
- the same CI job takes 19m18s on windows-latest against 5m46s on ubuntu-latest and 6m29s on macos-latest — the same commit, the same steps, a cache action present on all three, so the difference is the platform and not the workload
  Evidence: `.github/workflows/ci.yml:26`
- target/ measured 85,16 GB on this machine, of which 52,28 GB were target/debug/incremental and 24,99 GB target/debug/deps; the whole checkout was 102,75 GB
  Evidence: `Cargo.toml`
- the release profile sets lto = "thin" and codegen-units = 1, which is correct for a shipped binary but makes any release build markedly slower — and installing the harness locally goes through a release build, measured today at 6m38s for mustard-rt alone
  Evidence: `Cargo.toml`
---
id: wave.build-test-cycle-is-too.1-measure
---

# wave-1-measure

## Summary

Measure where the cycle's time actually goes — locally on Windows and in CI — and write the numbers into the tree, so every later change is decided by a datum instead of a plausible story.

## Network

- Parent: [[spec.build-test-cycle-is-too]]

## Tasks

- [ ] Time the local loop on this machine with `cargo build --timings`, which writes an HTML report attributing the wall clock to individual crates: a cold build (target/ removed), a warm no-op build, a one-line-change rebuild, `cargo test --no-run` (compile only) and `cargo test` (compile plus run), and `cargo clippy`. Record each number.
- [ ] Separate the two costs the test step bundles: how much of `cargo test --no-run` is compiling the crates versus linking the 59 integration-test binaries. Time `cargo test --no-run -p mustard-core` (5 test files) against `-p mustard-rt` (32) and derive the per-binary link cost.
- [ ] Measure the incremental cache both ways: run the one-line-change rebuild with CARGO_INCREMENTAL=1 and again with CARGO_INCREMENTAL=0, and record the wall clock AND the bytes target/debug/incremental gains in each case. This is the datum that decides AC-5's policy.
- [ ] Pull the per-step durations of the three CI legs of PR #155 (run 31796118965) with `gh run view --log` or the jobs API, so the Windows/Linux gap is attributed to steps rather than to the job total.
- [ ] Write docs/2026-08-14-build-cycle-measurements.md: a table per phase with its measured duration on Windows locally and in CI, the Linux CI figures beside them, the per-binary link cost, and the incremental on/off comparison. State plainly which of the candidate repairs the numbers support and which they do not — including any that turn out NOT to be worth doing.

## Files

- `docs/2026-08-14-build-cycle-measurements.md`

## Reality Obligations

- **RO-1.1** — Every duration in the record must be a number you TIMED on this machine or read from the GitHub Actions run. Do not estimate, do not carry a figure over from documentation, and do not reuse the numbers quoted in this spec's Evidence section as if you had measured them — they came from a different context and two of them are already stale.
- **RO-1.2** — Report the measurements that REFUTE a candidate repair as prominently as the ones that support it. A wave that only finds confirmation has not measured, it has argued.

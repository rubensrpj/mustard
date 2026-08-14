---
id: wave.build-test-cycle-is-too.2-ci
---

# wave-2-ci

## Summary

Stop CI from compiling the whole workspace three times per operating system, and make its incremental policy explicit instead of inherited.

## Network

- Parent: [[spec.build-test-cycle-is-too]]
- Depends on: [[wave.build-test-cycle-is-too.1-measure]]

## Tasks

- [ ] Read wave 1's record first: it says how much of each CI leg is build, test-compile and clippy. Let it decide the shape — the goal is that the workspace is not compiled from scratch three times, not any particular rewrite.
- [ ] Collapse the three cargo invocations. `cargo clippy --all-targets` compiles the test targets under clippy-driver as well, so the natural shape is one clippy pass covering everything plus one test run, instead of build, then test, then clippy. Verify against the measurement that the collapsed shape is actually cheaper — if the record says otherwise, follow the record and say so in the wave's report.
- [ ] Declare CARGO_INCREMENTAL explicitly in the workflow. A CI runner is thrown away after every job, so incremental state is written and never read back — it is pure cost there, independent of what the local measurement says.
- [ ] Keep the three-OS matrix, the `--locked` flag and the crate scoping (`-p mustard-core -p mustard-cli -p mustard-rt -p scan`) exactly as they are; the deliberate exclusion of the Tauri dashboard stays. Preserve the step that puts target/debug on PATH for core's spawn-based tests — dropping it turns those tests red on a bare runner only.

## Files

- `.github/workflows/ci.yml`

## Reality Obligations

- **RO-2.1** — Confirm against cargo's own current documentation that `clippy --all-targets` covers the test targets, rather than assuming it — if it does not, the collapse is wrong and the wave must say so instead of shipping it.

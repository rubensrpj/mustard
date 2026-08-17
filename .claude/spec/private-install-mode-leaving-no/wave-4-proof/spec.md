---
id: wave.private-install-mode-leaving-no.4-proof
---

# wave-4-proof

## Summary

Ask real git the whole question: a host repo that already versions its own CLAUDE.md must stay byte-identical and report nothing after a private install, a scan and a spec.

## Network

- Parent: [[spec.private-install-mode-leaving-no]]
- Depends on: [[wave.private-install-mode-leaving-no.1-core]], [[wave.private-install-mode-leaving-no.2-rt]], [[wave.private-install-mode-leaving-no.3-cli]]

## Tasks

- [ ] Write `packages/core/tests/private_install_leaves_no_trace.rs::ac8_host_repo_stays_clean_and_untouched`, modelled on `packages/core/tests/seeded_ignore.rs` — which proves coverage by asking real git rather than by reading a template, and derives its path list from the writers in the code instead of the test author's imagination. Do the same here: derive the footprint from wave 1's single declaration, never a list retyped in this file.
- [ ] The fixture is a real repository that ALREADY tracks its own `CLAUDE.md` at a subproject path and has it committed — this is the case a clone-local exclude rule cannot cover, and the reason this unit exists at all.
- [ ] After a private install, write every path the harness produces while working: the four seeds, a subproject scan output, a spec directory with `spec.md` and `qa/report.md`. Then assert `git status --porcelain --untracked-files=all` is EMPTY. Use `--untracked-files=all`: the default collapses a wholly untracked directory into one line and would let an unignored artefact hide behind its parent's name.
- [ ] Assert the host's committed `CLAUDE.md` is byte-identical to what it was before the scan — content and line terminators alike, so a CRLF host file is not silently normalised.
- [ ] Negative control, without which the test proves the wrong thing: run the SHARED install over the same fixture and require git to SEE the footprint. A test that reports clean because nothing was written at all would pass the positive half while the feature was entirely broken.
- [ ] Then run `cargo build --workspace` and confirm the whole workspace still builds (AC-9). Never run `cargo fmt` — this repository is not rustfmt-clean and the CI never formats.

## Files

- `packages/core/tests/private_install_leaves_no_trace.rs`

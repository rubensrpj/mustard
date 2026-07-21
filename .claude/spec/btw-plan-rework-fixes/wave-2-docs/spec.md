---
id: wave.btw-plan-rework-fixes.2-docs
---

# wave-2-docs

## Summary

Instruction surfaces: stop telling readers to run the absorbed command, and lock that with a guard test derived from the published CLI surface.

## Network

- Parent: [[spec.btw-plan-rework-fixes]]
- Depends on: [[wave.btw-plan-rework-fixes.1-rt]]

## Tasks

- [ ] `apps/dashboard/src/components/DoctorBadge/index.tsx`: the broken-wave-link hint must point at `mustard-rt run plan-materialize` (noting it consumes the plan via `--plan`), never at the absorbed `wave-scaffold`.
- [ ] `plugin/refs/feature/full-plan.md` and `plugin/refs/feature/spec-language.md`: keep every existing rule intact, but describe `wave-scaffold` as the scaffold renderer INSIDE `plan-materialize` rather than as an invocable command. Do not restate protocol that lives elsewhere and do not change any prescribed sequence.
- [ ] Add `every_documented_run_command_exists` to `apps/rt/tests/run_command_surface.rs`: scan the shipped instruction surfaces (`plugin/**/*.md` and `apps/dashboard/src`) for the literal `mustard-rt run <token>` and assert every token appears in `RUN_SUBCOMMANDS`. Skip placeholder tokens (those starting with `<`, `{`, `$` or a backtick). Resolve paths from `CARGO_MANIFEST_DIR` so the test is cwd-independent, and skip cleanly when a surface directory is absent.

## Files

- `apps/dashboard/src/components/DoctorBadge/index.tsx`
- `plugin/refs/feature/full-plan.md`
- `plugin/refs/feature/spec-language.md`
- `apps/rt/tests/run_command_surface.rs`

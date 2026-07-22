---
id: wave.btw-plan-rework-fixes.1-rt
---

# wave-1-rt

## Summary

Harness engine: reconcile-vs-freeze scaffold writes, spec-dir flag aliases plus normalisation, schema-teaching plan errors, and the resume-gate token rename.

## Network

- Parent: [[spec.btw-plan-rework-fixes]]

## Tasks

- [ ] In `wave_scaffold::scaffold`, decide the write mode from the canonical approval marker `<spec_dir>/.approved-by-user` (reuse `shared::context::APPROVED_BY_USER_MARKER`; do NOT add a flag or env knob). Unapproved: every scaffold artefact (`wave-plan.md`, each `wave-N-{role}/spec.md`, each wave `meta.json`) becomes a pure function of the plan — rewrite whenever the rendered content differs from disk, and record the relative path under `refreshed`. Approved: keep today's skip-if-present semantics byte-for-byte, and emit ONE stderr WARN naming the change-request route when a file WOULD have changed.
- [ ] Unapproved only: delete `wave-N-*` directories present on disk but absent from the plan, recording each under `removed`. Never touch the root `spec.md`, `meta.json`, `.events/`, `qa/` or `review/`.
- [ ] Extend `ScaffoldOutcome::Created` with `refreshed` and `removed`, and surface both arrays in the `plan-materialize` report. Both keys are ALWAYS present (empty when nothing changed) and sorted, so stdout stays deterministic and byte-stable per the rt Guards.
- [ ] Teach both `ScaffoldOutcome::Unreadable` messages (read failure and JSON parse failure) the minimal one-wave plan example plus a pointer to the `/feature` reference `full-plan.md § Plan JSON schema`. stderr only — stdout must not change.
- [ ] Add hidden `--spec` and `--from-spec` aliases to the four `--spec-dir` arguments (`plan-materialize`, `pipeline-summary` in `commands/pipeline/cli.rs`; `wave-tree`, `wave-size-check` in `commands/wave/cli.rs`), mirroring the existing alias pattern on the spec-path commands. `--spec-dir` stays canonical.
- [ ] Add a shared spec-dir normalisation helper next to the existing spec-path helpers in `shared/context.rs` and call it from the four commands. Precedence: an existing directory resolves unchanged (today's behaviour); a path ending in a file (`.../spec.md`) resolves to its parent; a bare slug with an existing `.claude/spec/{slug}` resolves to that directory.
- [ ] Rename the post-Execute gate token `await-wave-scaffold` to `await-plan-materialize` and update its blocked message to name `plan-materialize`. Update the existing test that asserts the old token.
- [ ] Update doc comments that call `wave-scaffold` a subcommand (its own module header, the `standalone subcommand` phrase in `plan_materialize.rs`) to describe it as the scaffold renderer inside `plan-materialize`.
- [ ] Tests: reconcile before approval (rewrites what differs, reports `refreshed`); frozen after approval (byte-identical files plus the WARN); stale wave directory removal; unchanged-plan re-run stays fully idempotent (extend the existing composite test); spec-dir normalisation precedence; the renamed gate token; the schema-teaching parse error; and alias acceptance for the four commands in `tests/run_command_surface.rs`.

## Files

- `apps/rt/src/commands/wave/wave_scaffold.rs`
- `apps/rt/src/commands/pipeline/plan_materialize.rs`
- `apps/rt/src/commands/pipeline/cli.rs`
- `apps/rt/src/commands/wave/cli.rs`
- `apps/rt/src/shared/context.rs`
- `apps/rt/src/commands/pipeline/resume_bootstrap/post_execute_gate.rs`
- `apps/rt/tests/run_command_surface.rs`

---
id: spec.btw-plan-rework-fixes
---

# btw plan rework fixes: reconcile the wave scaffold before approval, unify spec-dir flags, purge the absorbed wave-scaffold command, surface the plan JSON schema

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Context

A field report (`/btw`) from the sialia project measured one PLAN cycle of Spec-Driven Development: about 15 tool calls where 6 would have sufficed. Two thirds of the waste was the agent's own sequencing (running a validator before the artifact it validates was complete). The remaining third was interface friction, and every item of it was verified `file:line` against the CURRENT tree — the field session ran the installed release (v0.1.19 was cut the same day), so these are live defects, not residue from an older build.

Four defects survive verification:

1. **Re-running `plan-materialize` cannot repair a scaffold.** The writer is skip-if-present (`write_if_absent`), so after fixing a weak acceptance criterion in `plan.json` the stale wave files stay on disk. The field agent had to delete wave directories by hand, was denied by the guard, worked around it through PowerShell, and re-materialised — three calls for a one-line fix.
2. **The `--spec-dir` command family rejects the sibling spellings.** An earlier fix made `--spec` / `--from-spec` interchangeable across the five spec-path commands; the four `--spec-dir` commands were left out, so the habit the interface teaches is punished by a hard clap error and a burned retry.
3. **Live instructions still name `wave-scaffold`, a command absorbed into `plan-materialize`.** The resume gate answers `await-wave-scaffold`, the dashboard tells the reader to run it, and the plugin refs name it as invocable. An obedient agent calls a command the binary does not publish.
4. **The plan JSON schema is invisible at the point of use.** `--help` says only "Path to the plan JSON file" and a parse failure says only "plan JSON parse error"; the schema lives in a reference file the field agent only opened after failing.

## Users/Stakeholders

Any agent or human driving a full-scope pipeline — every PLAN cycle pays these four costs today. Secondarily the maintainers, who currently ship instructions pointing at a command that no longer exists.

## Success Metric

Repairing a weak acceptance criterion costs two actions (edit `plan.json`, re-run `plan-materialize`) instead of five, with no manual deletion and no guard workaround; and no shipped instruction names a command absent from the published CLI surface.

## Non-Goals

- Turning the tautological-AC linter into a hard gate. In the field the warning was read and acted on; what was expensive was the repair path, which point 1 fixes. The unconditional AC-coverage gate already blocks what must be blocked.
- Any new knob (`--force`, mode flag, environment variable). Point 1 decides from the approval marker that already exists.
- Any new prompt-side guard telling the agent to read a reference first. A house law says a mustard defect is fixed in the tool, not in the spec; and the field session proved a freshly-read rule does not become behaviour.
- Adding a "use the composite instead" hint to the primitives' output. The composite is already prescribed in the installed flow; without evidence of why it was ignored, that change would be a guess.

## Acceptance Criteria

- **AC-1** — when `plan-materialize` re-runs on a spec that carries NO `.approved-by-user` marker and whose `plan.json` now renders different content, then the affected wave files are rewritten from the plan and reported under `refreshed`
  Command: `cargo test -p mustard-rt reconciles_scaffold_before_approval`
  Expect: `1 passed`
- **AC-2** — when the spec already carries `.approved-by-user`, then existing scaffold files are left byte-identical and a frozen-plan warning naming the change-request route is written to stderr
  Command: `cargo test -p mustard-rt approved_plan_scaffold_is_frozen`
  Expect: `1 passed`
- **AC-2b** — when an approved spec is re-materialised from a plan that adds a wave, then the approved `totalWaves` in the root sidecar does not move and the divergence is announced instead of applied
  Command: `cargo test -p mustard-rt approved_plan_keeps_its_wave_count`
  Expect: `1 passed`
- **AC-2c** — when a spec has left PLAN, or `wave-collapse` recorded `scopeOverride: "user-rejected-waves"`, then the scaffold falls back to skip-if-present (no rewrite, no prune) even with no approval marker
  Command: `cargo test -p mustard-rt write_mode_freezes_outside_the_plan_authoring_window`
  Expect: `1 passed`
- **AC-3** — when a wave present on disk is dropped from `plan.json` before approval, then its directory is deleted and listed under `removed`
  Command: `cargo test -p mustard-rt removes_wave_dropped_from_plan`
  Expect: `1 passed`
- **AC-4** — when `plan-materialize` re-runs with an UNCHANGED plan, then nothing is created, refreshed or removed and the PLAN phase stays emitted exactly once
  Command: `cargo test -p mustard-rt composite_plan_materialize_scaffolds_validates_and_emits`
  Expect: `1 passed`
- **AC-5** — when `plan-materialize`, `wave-tree`, `wave-size-check` or `pipeline-summary` is invoked with `--spec` or `--from-spec` instead of `--spec-dir`, then the invocation parses instead of failing
  Command: `cargo test -p mustard-rt --test run_command_surface spec_dir_flag_aliases_are_interchangeable`
  Expect: `1 passed`
- **AC-6** — when a spec-dir argument arrives as a path to `spec.md` or as a bare slug, then it resolves to the spec directory; an existing directory path resolves unchanged
  Command: `cargo test -p mustard-rt normalise_spec_dir`
  Expect: `1 passed`
- **AC-7** — when the post-Execute gate blocks a Full spec that has zero waves, then the next action it names is `plan-materialize`
  Command: `cargo test -p mustard-rt blocked_full_spec_awaits_plan_materialize`
  Expect: `1 passed`
- **AC-8** — when the plan file cannot be read or parsed, then the stderr message carries a minimal valid plan example and points at the schema reference
  Command: `cargo test -p mustard-rt unreadable_plan_message_teaches_schema`
  Expect: `1 passed`
- **AC-9** — when the shipped instruction surfaces (plugin references, dashboard hints) are scanned for `mustard-rt run <name>`, then every name found exists in the published CLI surface
  Command: `cargo test -p mustard-rt --test run_command_surface every_documented_run_command_exists`
  Expect: `1 passed`
- **AC-9b** — when the guard's tokenizer meets each invocation spelling (bare, `.exe`, `$RtExe`) it reports the name, and when it meets a placeholder it reports nothing
  Command: `cargo test -p mustard-rt documented_run_tokens_catches_every_spelling`
  Expect: `1 passed`
- **AC-10** — the harness test suite stays green
  Command: `cargo test -p mustard-rt`
  Expect: `test result: ok`

<!-- PLAN -->

## Files

- `apps/rt/src/commands/wave/wave_scaffold.rs` (modify) — reconcile-vs-freeze write mode, stale-wave removal, schema-teaching parse errors, doc comment
- `apps/rt/src/commands/pipeline/plan_materialize.rs` (modify) — `refreshed`/`removed` in the report, doc comment
- `apps/rt/src/commands/pipeline/cli.rs` (modify) — `--spec-dir` aliases + `--plan` long help with the schema example
- `apps/rt/src/commands/wave/cli.rs` (modify) — `--spec-dir` aliases
- `apps/rt/src/shared/context.rs` (modify) — shared spec-dir normalisation helper
- `apps/rt/src/commands/pipeline/resume_bootstrap/post_execute_gate.rs` (modify) — `await-plan-materialize` token and message
- `apps/rt/tests/run_command_surface.rs` (modify) — alias coverage for the four commands; the documented-command guard
- `apps/rt/src/commands/wave/wave_tree.rs` (modify) — call site of the normalisation helper
- `apps/rt/src/commands/wave/wave_size_check.rs` (modify) — call site of the normalisation helper
- `apps/rt/src/commands/pipeline/pipeline_summary.rs` (modify) — call site of the normalisation helper
- `apps/dashboard/src/components/DoctorBadge/index.tsx` (modify) — hint points at `plan-materialize`
- `plugin/refs/feature/full-plan.md` (modify) — name the renderer, not a command
- `plugin/refs/feature/spec-language.md` (modify) — same

## Boundaries

IN: the four verified defects above, their tests, and the instruction surfaces that name the absorbed command.
OUT: the AC linter's severity; new flags, modes or environment knobs; prompt-side guards; any change to `spec-draft`, to the root `spec.md`, or to the approval and coverage gates themselves.

Corrected during REVIEW (this spec first claimed the `wave-collapse` edge stayed unchanged — that was wrong, and reconciling made it destructive rather than merely resurrecting directories). The reconcile window is therefore the PLAN AUTHORING window, and all three facts must hold: no approval marker, `stage` still `Plan`, and no `scopeOverride: "user-rejected-waves"`. Anything else falls back to skip-if-present. The root sidecar's wave count is the one exception: it is structural and non-destructive, so it is frozen by the approval marker alone — freezing it more widely would resurrect the known stale-`totalWaves` defect that mis-renders the dashboard.
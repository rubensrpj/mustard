---
id: cap.btw-plan-rework-fixes
status: active
---

# btw plan rework fixes

### Requirement: The system SHALL satisfy the acceptance criteria of spec btw-plan-rework-fixes.

#### Scenario: AC-1
- when: `plan-materialize` re-runs on a spec that carries NO `.approved-by-user` marker and whose `plan.json` now renders different content
- then: the affected wave files are rewritten from the plan and reported under `refreshed`
- command: `cargo test -p mustard-rt reconciles_scaffold_before_approval`

#### Scenario: AC-2
- when: the spec already carries `.approved-by-user`
- then: existing scaffold files are left byte-identical and a frozen-plan warning naming the change-request route is written to stderr
- command: `cargo test -p mustard-rt approved_plan_scaffold_is_frozen`

#### Scenario: AC-2B
- when: an approved spec is re-materialised from a plan that adds a wave
- then: the approved `totalWaves` in the root sidecar does not move and the divergence is announced instead of applied
- command: `cargo test -p mustard-rt approved_plan_keeps_its_wave_count`

#### Scenario: AC-2C
- when: a spec has left PLAN, or `wave-collapse` recorded `scopeOverride: "user-rejected-waves"`
- then: the scaffold falls back to skip-if-present (no rewrite, no prune) even with no approval marker
- command: `cargo test -p mustard-rt write_mode_freezes_outside_the_plan_authoring_window`

#### Scenario: AC-3
- when: a wave present on disk is dropped from `plan.json` before approval
- then: its directory is deleted and listed under `removed`
- command: `cargo test -p mustard-rt removes_wave_dropped_from_plan`

#### Scenario: AC-4
- when: `plan-materialize` re-runs with an UNCHANGED plan
- then: nothing is created, refreshed or removed and the PLAN phase stays emitted exactly once
- command: `cargo test -p mustard-rt composite_plan_materialize_scaffolds_validates_and_emits`

#### Scenario: AC-5
- when: `plan-materialize`, `wave-tree`, `wave-size-check` or `pipeline-summary` is invoked with `--spec` or `--from-spec` instead of `--spec-dir`
- then: the invocation parses instead of failing
- command: `cargo test -p mustard-rt --test run_command_surface spec_dir_flag_aliases_are_interchangeable`

#### Scenario: AC-6
- when: a spec-dir argument arrives as a path to `spec.md` or as a bare slug
- then: it resolves to the spec directory; an existing directory path resolves unchanged
- command: `cargo test -p mustard-rt normalise_spec_dir`

#### Scenario: AC-7
- when: the post-Execute gate blocks a Full spec that has zero waves
- then: the next action it names is `plan-materialize`
- command: `cargo test -p mustard-rt blocked_full_spec_awaits_plan_materialize`

#### Scenario: AC-8
- when: the plan file cannot be read or parsed
- then: the stderr message carries a minimal valid plan example and points at the schema reference
- command: `cargo test -p mustard-rt unreadable_plan_message_teaches_schema`

#### Scenario: AC-9B
- when: the guard's tokenizer meets each invocation spelling (bare, `.exe`, `$RtExe`) it reports the name, and when it meets a placeholder it reports nothing
- then: 
- command: `cargo test -p mustard-rt documented_run_tokens_catches_every_spelling`

#### Scenario: AC-10
- when: 
- then: the harness test suite stays green
- command: `cargo test -p mustard-rt`

## Covers

## Specs
- [[spec.btw-plan-rework-fixes]]

## Related


---
id: cap.close-eleven-harness-defects-found
status: active
---

# close eleven harness defects found

### Requirement: The system SHALL satisfy the acceptance criteria of spec close-eleven-harness-defects-found.

#### Scenario: AC-1
- when: a close runs and a criterion is still red after its work landed
- then: the close reports it verbatim - taken, named, with what its column says - and is not withheld on it: QA gates the same commands moments earlier in this composite, so only the removal pass blocks
- command: `cargo test -p mustard-rt close_reports_a_still_red_criterion_without_withholding`

#### Scenario: AC-2
- when: a close completes
- then: the removal pass has been taken, so a criterion that
- command: `cargo test -p mustard-rt close_takes_the_removal_pass`

#### Scenario: AC-3
- when: a criterion carries a Control command that is not green against the tree as it is,
- then: 
- command: `cargo test -p mustard-rt control_command_must_be_green_today`

#### Scenario: AC-4
- when: a wave claims a criterion whose command inspects a path that wave does not
- then: 
- command: `cargo test -p mustard-rt wave_claiming_a_criterion_must_contain_its_paths`

#### Scenario: AC-5
- when: a command contains an angle bracket that is not the skeleton token the drafter
- then: 
- command: `cargo test -p mustard-rt placeholder_matches_the_skeleton_token_not_any_angle_bracket`

#### Scenario: AC-6
- when: the proof ledger already records a criterion red
- then: the tautology linter stays
- command: `cargo test -p mustard-rt weak_ac_defers_to_the_recorded_proof`

#### Scenario: AC-7
- when: the files section is written as a markdown table
- then: its paths are read; and when
- command: `cargo test -p mustard-rt files_section_reads_a_table_and_names_an_unreadable_one`

#### Scenario: AC-8
- when: a slice's exemplar files include a module the census classified as machine-written,
- then: 
- command: `cargo test --workspace exemplar_files_exclude_machine_written_modules`

#### Scenario: AC-9
- when: a plan declares a wave's dependencies
- then: the dependency command emits those
- command: `cargo test -p mustard-rt wave_dependency_honours_the_declared_edges`

#### Scenario: AC-10
- when: a phase transition is recorded
- then: the command prints the previous and the new
- command: `cargo test -p mustard-rt emit_phase_confirms_the_transition`

#### Scenario: AC-11
- when: a pipeline event binds a session to a spec
- then: the binding lands under the
- command: `cargo test -p mustard-rt session_binding_reaches_the_reading_session`

#### Scenario: AC-12
- when: the boundary gate checks an edit against a wave's file list
- then: the warning
- command: `cargo test -p mustard-rt boundary_warning_names_the_boundary_it_checked`

#### Scenario: AC-13
- when: the work branch cannot be created and the run continues on the previous branch,
- then: 
- command: `cargo test -p mustard-rt work_branch_record_reconciles_with_the_real_branch`

#### Scenario: AC-14
- when: 
- then: the project build and tests pass green
- command: `cargo build --workspace`

## Covers

## Specs
- [[spec.close-eleven-harness-defects-found]]

## Related


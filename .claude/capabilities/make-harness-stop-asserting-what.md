---
id: cap.make-harness-stop-asserting-what
status: active
---

# make harness stop asserting what

### Requirement: The system SHALL satisfy the acceptance criteria of spec make-harness-stop-asserting-what.

#### Scenario: AC-1
- when: a spec is closed after its work has landed
- then: the pipeline itself TAKES the confirmation pass over the criteria proven red at plan time and records the verdict, instead of clearing on the red proof alone
- command: `cargo test -p mustard-rt close_pipeline_takes_the_confirmation_pass`

#### Scenario: AC-2
- when: the criterion being replaced is recorded as inexecutable, then
- then: 
- command: `cargo test -p mustard-rt ac_amend_accepts_inexecutable_predecessor`

#### Scenario: AC-3
- when: active-spec discovery runs on a branch that does not carry the
- then: 
- command: `cargo test -p mustard-rt active_specs_lists_in_flight_from_other_branches`

#### Scenario: AC-4
- when: a plan declares reality obligations for a wave
- then: those
- command: `cargo test -p mustard-rt plan_reality_obligations_reach_wave_prompt`

#### Scenario: AC-5
- when: a wave closes without reporting the reality obligations it was
- then: 
- command: `cargo test -p mustard-rt wave_done_flags_unreported_reality_obligation`

#### Scenario: AC-6
- when: a spec has wave directories but no dispatch event
- then: resume
- command: `cargo test -p mustard-rt wave_progress_distinguishes_never_dispatched`

#### Scenario: AC-7
- when: the dependency precheck declines to judge an unsupported stack,
- then: 
- command: `cargo test -p mustard-rt dependency_precheck_skip_is_surfaced`

#### Scenario: AC-8
- when: a checklist item is dropped on purpose with a stated reason,
- then: 
- command: `cargo test -p mustard-rt checklist_records_dropped_with_reason`

#### Scenario: AC-10
- when: the CLOSE prose is read
- then: it teaches the confirmation pass the pipeline now takes, instead of describing only the red half and leaving the second one undiscovered
- command: `cargo test -p mustard-rt close_prose_teaches_the_confirmation_pass`

#### Scenario: AC-11
- when: the picker prose spells the Siglas out literally
- then: it carries the `Onde` legend for the column the table now prints
- command: `cargo test -p mustard-rt picker_prose_teaches_the_onde_column`

#### Scenario: AC-12
- when: the resume prose tells the orchestrator which wave-progress fields to read
- then: it names `neverDispatched` beside `currentWave`
- command: `cargo test -p mustard-rt resume_prose_teaches_never_dispatched`

#### Scenario: AC-13
- when: a criterion whose statement spans several lines is amended
- then: the whole statement block is rewritten, instead of leaving the superseded continuation lines orphaned under the new statement
- command: `cargo test -p mustard-rt ac_amend_rewrites_the_whole_statement_block`

#### Scenario: AC-9
- when: 
- then: the project build passes green
- command: `cargo build --workspace`

## Covers

## Specs
- [[spec.make-harness-stop-asserting-what]]

## Related


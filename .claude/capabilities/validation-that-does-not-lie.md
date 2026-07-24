---
id: cap.validation-that-does-not-lie
status: active
---

# validation that does not lie

### Requirement: The system SHALL satisfy the acceptance criteria of spec validation-that-does-not-lie.

#### Scenario: AC-1
- when: the check runs from a directory other than the project root
- then: a file that exists is still found
- command: `cargo test -p mustard-rt validation_resolves_from_any_working_directory -- --exact --nocapture`

#### Scenario: AC-2
- when: a plan names a path whose folder segments carry the punctuation a routing convention requires
- then: that path is validated rather than skipped
- command: `cargo test -p mustard-rt validation_sees_paths_with_punctuated_segments -- --exact --nocapture`

#### Scenario: AC-3
- when: a plan mentions a term that merely looks like a path
- then: it is not reported as a missing file
- command: `cargo test -p mustard-rt validation_does_not_flag_prose_as_a_file -- --exact --nocapture`

#### Scenario: AC-4
- when: a subproject still carries an uncurated rules scaffold
- then: the health check reports it by name
- command: `cargo test -p mustard-rt doctor_reports_uncurated_rule_scaffolds -- --exact --nocapture`

#### Scenario: AC-5
- when: an uncurated scaffold reaches a dispatched agent as its rules
- then: the dispatch says so instead of passing it off as guidance
- command: `cargo test -p mustard-rt dispatch_warns_on_uncurated_rules -- --exact --nocapture`

#### Scenario: AC-6
- when: 
- then: the workspace builds and its suite stays green
- command: `cargo build --workspace && cargo test --workspace`

## Covers

## Specs
- [[spec.validation-that-does-not-lie]]

## Related


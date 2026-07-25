---
id: cap.harness-safety-instruments
status: active
---

# harness safety instruments

### Requirement: The system SHALL satisfy the acceptance criteria of spec harness-safety-instruments.

#### Scenario: AC-1
- when: the kill-switch runs
- then: the harness stops firing and the safety rules stay in place
- command: `cargo test -p mustard-rt unhook_disables_hooks_without_dropping_permissions -- --exact --nocapture`

#### Scenario: AC-2
- when: a hook event is added or removed from what ships
- then: the health check follows it without anyone editing a second list
- command: `cargo test -p mustard-rt known_events_match_shipped_hooks -- --exact --nocapture`

#### Scenario: AC-3
- when: the reporting command is asked about a project that has history
- then: it reports that history instead of zero
- command: `cargo test -p mustard-rt metrics_collect_reports_specs_from_events -- --exact --nocapture`

#### Scenario: AC-4
- when: the shipped settings template is inspected
- then: it grants no permission the platform refuses to honour
- command: `rg --files-without-match "Edit\(\*?\*?/?\.claude" packages/core/templates/settings.json`

#### Scenario: AC-5
- when: 
- then: the workspace builds and its suite stays green
- command: `cargo build --workspace && cargo test --workspace`

#### Scenario: AC-6
- when: a counter cannot be derived from any readable source
- then: the report says so instead of publishing a number that means nothing
- command: `cargo test -p mustard-rt metrics_counters_declare_unknown_when_underived -- --exact --nocapture`

#### Scenario: AC-7
- when: a spec is drafted into a directory that already holds its event log
- then: the draft proceeds without an overwrite flag
- command: `cargo test -p mustard-rt spec_draft_accepts_an_events_only_directory -- --exact --nocapture`

#### Scenario: AC-8
- when: the drafted skeleton is validated
- then: its context section carries no file path and no bullet list
- command: `cargo test -p mustard-rt drafted_context_is_prose_only -- --exact --nocapture`

#### Scenario: AC-9
- when: the approval gate declines to record an answer
- then: it states which condition failed
- command: `cargo test -p mustard-rt approval_refusal_names_the_unmet_condition -- --exact --nocapture`

#### Scenario: AC-10
- when: the glossary scores an intent
- then: a single word stem is counted once, not once per inflection
- command: `cargo test -p mustard-rt glossary_terms_collapse_inflections -- --exact --nocapture`

## Covers

## Specs
- [[spec.harness-safety-instruments]]

## Related


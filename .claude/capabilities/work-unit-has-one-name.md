---
id: cap.work-unit-has-one-name
status: active
---

# work unit has one name

### Requirement: The system SHALL satisfy the acceptance criteria of spec work-unit-has-one-name.

#### Scenario: AC-1
- when: the base gate opens a unit
- then: it mints the canonical slug itself and reports it, so the name the branch carries is the name the spec will carry
- command: `cargo test -p mustard-rt the_base_gate_mints_the_canonical_slug 2>&1 | grep -E "[1-9][0-9]* passed"`

#### Scenario: AC-2
- when: `spec-draft` is given an explicit slug
- then: it uses that one instead of deriving a second name from its own intent
- command: `cargo test -p mustard-rt spec_draft_consumes_the_slug_it_is_given 2>&1 | grep -E "[1-9][0-9]* passed"`

#### Scenario: AC-3
- when: the checkout IS the unit's branch and the slug was decided at the gate
- then: `insideWorkBranch` reports true, so the no-ceremony resume actually fires
- command: `cargo test -p mustard-rt inside_work_branch_holds_when_the_gate_named_the_unit 2>&1 | grep -E "[1-9][0-9]* passed"`

#### Scenario: AC-4
- when: a wave plan was scaffolded but nothing was dispatched
- then: the picker table does NOT read `em exec`, because that word asks the reader to resume work that never started
- command: `cargo test -p mustard-rt a_scaffolded_plan_is_not_reported_as_running 2>&1 | grep -E "[1-9][0-9]* passed"`

#### Scenario: AC-5
- when: the dependency precheck declines to judge
- then: its report says so in its own verdict field, instead of only in the presence of a second key
- command: `cargo test -p mustard-rt a_declined_precheck_is_not_a_pass 2>&1 | grep -E "[1-9][0-9]* passed"`

#### Scenario: AC-6
- when: the picker table is read
- then: it no longer claims a bare letter mints the approval marker, and it names the full form that does
- command: `! grep -q 'the text you typed mints' plugin/commands/spec.md && grep -q 'typed in full' plugin/commands/spec.md`

#### Scenario: AC-7
- when: the Full path is followed from `feature.md`
- then: the text sends the reader to the full-plan machinery BEFORE the census-dependent step, so the first `plan-prepare` is not guaranteed to abstain
- command: `cargo test -p mustard-rt the_full_path_reaches_full_plan_before_the_census_step 2>&1 | grep -E "[1-9][0-9]* passed"`

#### Scenario: AC-9
- when: the picker table prints a status
- then: the page's own `Siglas` legend names it — the scaffolded-but-never-dispatched one included, and says what it asks the reader to do — and names nothing the table cannot print, so the key is never shorter NOR longer than the behaviour
- command: `cargo test -p mustard-rt the_picker_legend_names_the_not_yet_started_status 2>&1 | grep -E "[1-9][0-9]* passed"`

#### Scenario: AC-10
- when: the flow calls `spec-draft`
- then: the call the operator reads carries the name the gate minted, so the page never teaches that the draft may invent a second one
- command: `cargo test -p mustard-rt the_draft_call_carries_the_name_the_gate_minted 2>&1 | grep -E "[1-9][0-9]* passed"`

#### Scenario: AC-11
- when: the widest status the table can print lands in a row
- then: the `Onde` and `Resumo` columns still start where the header puts them, so the new status does not mis-render the table it was added to
- command: `cargo test -p mustard-rt the_status_column_never_shifts_the_columns_to_its_right 2>&1 | grep -E "[1-9][0-9]* passed"`

#### Scenario: AC-8
- when: 
- then: the project build passes green
- command: `cargo build --workspace`

## Covers

## Specs
- [[spec.work-unit-has-one-name]]

## Related


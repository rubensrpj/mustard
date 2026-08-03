---
id: cap.work-unit-lives-on-its
status: active
---

# work unit lives on its

### Requirement: The system SHALL satisfy the acceptance criteria of spec work-unit-lives-on-its.

#### Scenario: AC-1
- when: a pipeline is opened from a checkout that is not a `git.flow` base
- then: the base gate refuses and names the base to switch to.
- command: `cargo test -p mustard-rt base_gate 2>&1 | grep -E "[1-9][0-9]* passed"`

#### Scenario: AC-2
- when: a `spec.md` write is attempted on a protected base
- then: the work-branch gate refuses it instead of carving it out.
- command: `cargo test -p mustard-rt spec_authoring_on_protected_base 2>&1 | grep -E "[1-9][0-9]* passed"`

#### Scenario: AC-3
- when: a spec is resumed from inside its own `{base}_{slug}` branch
- then: it dispatches with no confirmation prompt and no table.
- command: `cargo test -p mustard-rt resume_inside_own_branch 2>&1 | grep -E "[1-9][0-9]* passed"`

#### Scenario: AC-4
- when: `pr list` runs from a work branch instead of an integration base
- then: it refuses and names the base.
- command: `cargo test -p mustard-rt pr_list 2>&1 | grep -E "[1-9][0-9]* passed"`

#### Scenario: AC-5
- when: a merge is requested with no recorded review verdict
- then: the command warns and asks rather than refusing or merging silently.
- command: `cargo test -p mustard-rt pr_merge_without_verdict 2>&1 | grep -E "[1-9][0-9]* passed"`

#### Scenario: AC-6
- when: `git delete` is invoked from a work branch instead of a base
- then: it refuses without touching anything.
- command: `cargo test -p mustard-rt git_delete 2>&1 | grep -E "[1-9][0-9]* passed"`

#### Scenario: AC-7
- when: an out-of-scope item is recorded during a work unit
- then: it lands in that unit's notebook and is readable back by unit.
- command: `cargo test -p mustard-rt notebook 2>&1 | grep -E "[1-9][0-9]* passed"`

#### Scenario: AC-8
- when: the exposed command surface is enumerated
- then: exactly four user-invocable doors remain: git, pr, spec and upsert.
- command: `cargo test -p mustard-rt exposed_doors 2>&1 | grep -E "[1-9][0-9]* passed"`

#### Scenario: AC-9
- when: 
- then: the project build and tests pass green
- command: `cargo build --workspace`

## Covers

## Specs
- [[spec.work-unit-lives-on-its]]

## Related


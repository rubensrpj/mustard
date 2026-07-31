---
id: cap.ceremony-costs-more-than-gates
status: active
---

# ceremony costs more than gates

### Requirement: The system SHALL satisfy the acceptance criteria of spec ceremony-costs-more-than-gates.

#### Scenario: AC-1
- when: `spec-draft` is given `--plan <file>`
- then: `spec.md`, `meta.json`, `wave-plan.md` and every wave directory are produced by that ONE call, with the negative proof taken in the same pass.
- command: `cargo test -p mustard-rt spec_draft_materialises_the_whole_layout_in_one_call 2>&1 | grep -E "test result: ok\. [1-9][0-9]* passed"`

#### Scenario: AC-2
- when: the plan handed to `spec-draft --plan` carries a criterion that already passes against the current tree
- then: the call REFUSES and writes no layout — the negative proof keeps its blocking power on the fused path, exactly as it has on `plan-materialize`.
- command: `cargo test -p mustard-rt spec_draft_plan_refuses_an_unproven_criterion 2>&1 | grep -E "test result: ok\. [1-9][0-9]* passed"`

#### Scenario: AC-3
- when: the USER's own prompt is the picker's approve-and-implement form
- then: `<spec>/.approved-by-user` is minted with `via` naming the picker; and when the identical text is not the user's prompt, nothing is minted — both halves asserted, so the test can fail.
- command: `cargo test -p mustard-rt picker_approval 2>&1 | grep -E "test result: ok\. [1-9][0-9]* passed"`

#### Scenario: AC-4
- when: the flows are read by a test, the picker states that the typed `r` IS the approval and the materialisation is one call — asserted structurally, both halves (the new instruction present AND the superseded "r never approves" sentence gone).
- then: 
- command: `cargo test -p mustard-rt --test spec_flow_prose 2>&1 | grep -E "test result: ok\. [1-9][0-9]* passed"`

#### Scenario: AC-5
- when: 
- then: the project build and tests pass green
- command: `cargo build --workspace`

## Covers

## Specs
- [[spec.ceremony-costs-more-than-gates]]

## Related


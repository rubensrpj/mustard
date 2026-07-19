---
id: cap.field-report-fix-package-sialia
status: active
---

# field report fix package sialia

### Requirement: The system SHALL satisfy the acceptance criteria of spec field-report-fix-package-sialia.

#### Scenario: AC-1
- when: `approve-spec` runs in strict mode on a Full spec that is missing BOTH the `.clarified` and the `.approved-by-user` markers
- then: a SINGLE refusal message names both missing requirements and how each is minted (no more one-gate-at-a-time refusals); and when no session binding resolves, the approval observers fall back to the UNIQUE full/Plan/unapproved spec (fail-closed on zero or many)
- command: `cargo test -p mustard-rt -- combined_refusal unique_pending`

#### Scenario: AC-2
- when: `dispatch-plan` or `wave-advance` emits an item for a plugin-owned role (review/qa/guards/patterns)
- then: its `subagent_type` carries the plugin namespace (`mustard:mustard-review`), while builtin types (Explore, Plan, general-purpose) stay bare; the read-only denylist in `subagent_inject` matches both spellings
- command: `cargo test -p mustard-rt namespac`

#### Scenario: AC-3
- when: an Acceptance Criteria item declares an `Expect:` regex and its command exits 0 WITHOUT the output matching
- then: `qa-run` marks that AC `fail` (vacuous green is dead); with a match it passes; without an `Expect:` line legacy exit-code semantics hold
- command: `cargo test -p mustard-rt expect_regex`

#### Scenario: AC-4
- when: the parent spec declares an AC id that no wave `satisfies` (nor covers via its `acceptance` lines)
- then: `wave-scaffold` reports the gap and, in strict mode, refuses the scaffold (env `MUSTARD_TRACE_GATE_MODE=strict|warn|off`)
- command: `cargo test -p mustard-rt parent_spec_ac`

#### Scenario: AC-5
- when: the census marks a subproject as its own git root (a `.git` directory OR file at its dir)
- then: the dispatched item carries the flag and the rendered implementer prompt states the boundary (separate commit history; never bump the superproject gitlink pointer), and `work_branch_gate` resolves the work-branch base from the nested repo's own default branch
- command: `cargo test --workspace own_git_root`

#### Scenario: AC-6
- when: 
- then: the whole workspace builds and tests green with the new gates active
- command: `cargo test --workspace --quiet`

#### Scenario: AC-7
- when: the plugin prose tables that map roles to agent types are read
- then: they carry the namespaced spelling
- command: `rg -c "mustard:mustard-review" plugin/pipeline-config.md`

## Covers

## Specs
- [[spec.field-report-fix-package-sialia]]

## Related


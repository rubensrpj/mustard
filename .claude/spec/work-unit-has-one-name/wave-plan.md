---
id: wave.work-unit-has-one-name.plan
---

# Wave Plan

## Wave Table

| Wave | Spec | Role | Depends on | Summary |
|------|------|------|------------|---------|
| 1 | [[wave.work-unit-has-one-name.1-identity]] | identity | — | The unit's name is minted ONCE, at the base gate, and the draft consumes it — so the branch, the spec directory, the events and the notebook stop being able to disagree. |
| 2 | [[wave.work-unit-has-one-name.2-signals]] | signals | — | Two signals stop reporting a state nobody reached: the picker table stops calling a scaffolded plan `running`, and a precheck that declined to judge stops looking like one that passed. |
| 3 | [[wave.work-unit-has-one-name.3-prose]] | prose | [[wave.work-unit-has-one-name.1-identity]], [[wave.work-unit-has-one-name.2-signals]] | The flow documents stop promising two things they cannot deliver: an approval from a bare letter, and a Full path whose first census read can only abstain. |

## Acceptance Criteria
- AC-1 — the gate mints the canonical slug and reports it. Command: `cargo test -p mustard-rt the_base_gate_mints_the_canonical_slug 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-2 — the draft consumes a given slug instead of deriving a second. Command: `cargo test -p mustard-rt spec_draft_consumes_the_slug_it_is_given 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-3 — insideWorkBranch holds for a gate-named unit. Command: `cargo test -p mustard-rt inside_work_branch_holds_when_the_gate_named_the_unit 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-4 — a scaffolded, never-dispatched plan is not reported as running. Command: `cargo test -p mustard-rt a_scaffolded_plan_is_not_reported_as_running 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-5 — a declined precheck carries its own verdict. Command: `cargo test -p mustard-rt a_declined_precheck_is_not_a_pass 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-6 — the picker names the full form and stops promising the bare letter mints. Command: `! grep -q 'the text you typed mints' plugin/commands/spec.md && grep -q 'typed in full' plugin/commands/spec.md`
- AC-7 — the Full path reaches the full-plan machinery before the census step. Command: `cargo test -p mustard-rt the_full_path_reaches_full_plan_before_the_census_step 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-9 — the picker's own `Siglas` legend names every status the table can print, and none it cannot. Command: `cargo test -p mustard-rt the_picker_legend_names_the_not_yet_started_status 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-10 — the flow's `spec-draft` call carries the name the gate minted. Command: `cargo test -p mustard-rt the_draft_call_carries_the_name_the_gate_minted 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-11 — the widest status the table can print does not shift the columns to its right. Command: `cargo test -p mustard-rt the_status_column_never_shifts_the_columns_to_its_right 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-8 — the project build and tests pass green. Command: `cargo build --workspace && cargo test --workspace`

<!-- wikilinks-footer-start -->
- [wave.work-unit-has-one-name.1-identity](spec.md)
- [wave.work-unit-has-one-name.2-signals](spec.md)
- [wave.work-unit-has-one-name.3-prose](spec.md)
<!-- wikilinks-footer-end -->
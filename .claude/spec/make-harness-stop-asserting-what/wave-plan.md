---
id: wave.make-harness-stop-asserting-what.plan
---

# Wave Plan

## Wave Table

| Wave | Spec | Role | Depends on | Summary |
|------|------|------|------------|---------|
| 1 | [[wave.make-harness-stop-asserting-what.1-proof]] | proof | — | The criterion proof gains its second half — red before, green after — and a criterion proven inexecutable gets a sanctioned repair path. |
| 2 | [[wave.make-harness-stop-asserting-what.2-discovery]] | discovery | — | Active-spec discovery stops reporting absence it did not verify: a spec on an unmerged work branch is listed as in-flight, with the branch that holds it. |
| 3 | [[wave.make-harness-stop-asserting-what.3-plan]] | plan | [[wave.make-harness-stop-asserting-what.1-proof]] | A plan can oblige a wave to verify something outside the repository, and the closing of that wave reports whether the duty was met. |
| 4 | [[wave.make-harness-stop-asserting-what.4-bootstrap]] | bootstrap | [[wave.make-harness-stop-asserting-what.1-proof]] | Resume bootstrap and the dependency precheck stop implying what they did not check — one about progress, the other about a stack it cannot parse. |
| 5 | [[wave.make-harness-stop-asserting-what.5-checklist]] | checklist | [[wave.make-harness-stop-asserting-what.1-proof]] | Work dropped on purpose is recorded as a decision, so it stops reading as work someone forgot. |

## Acceptance Criteria
- AC-1 — when a spec is closed after its work has landed, then the pipeline itself TAKES the confirmation pass over the criteria proven red at plan time and records the verdict, instead of clearing on the red proof alone Command: `cargo test -p mustard-rt close_pipeline_takes_the_confirmation_pass` Expect: `ok\. [1-9][0-9]* passed`
- AC-2 — when the criterion being replaced is recorded as inexecutable, then ac-amend accepts a substitute that passes, instead of refusing everything that is not red. Command: `cargo test -p mustard-rt ac_amend_accepts_inexecutable_predecessor` Expect: `ok\. [1-9][0-9]* passed`
- AC-3 — when active-spec discovery runs on a branch that does not carry the spec directory, then a spec living on an unmerged work branch is listed as in-flight with the branch that holds it. Command: `cargo test -p mustard-rt active_specs_lists_in_flight_from_other_branches` Expect: `ok\. [1-9][0-9]* passed`
- AC-4 — when a plan declares reality obligations for a wave, then those duties reach the dispatched agent's prompt as their own section. Command: `cargo test -p mustard-rt plan_reality_obligations_reach_wave_prompt` Expect: `ok\. [1-9][0-9]* passed`
- AC-5 — when a wave closes without reporting the reality obligations it was given, then wave-done reports the unmet duty by name. Command: `cargo test -p mustard-rt wave_done_flags_unreported_reality_obligation` Expect: `ok\. [1-9][0-9]* passed`
- AC-6 — when a spec has wave directories but no dispatch event, then resume bootstrap reports the plan as never dispatched instead of as wave 1. Command: `cargo test -p mustard-rt wave_progress_distinguishes_never_dispatched` Expect: `ok\. [1-9][0-9]* passed`
- AC-7 — when the dependency precheck declines to judge an unsupported stack, then the caller surfaces the skip instead of reading the empty result as a clean pass. Command: `cargo test -p mustard-rt dependency_precheck_skip_is_surfaced` Expect: `ok\. [1-9][0-9]* passed`
- AC-8 — when a checklist item is dropped on purpose with a stated reason, then it is recorded as a decision and stays distinct from an unchecked item. Command: `cargo test -p mustard-rt checklist_records_dropped_with_reason` Expect: `ok\. [1-9][0-9]* passed`
- AC-10 — when the CLOSE prose is read, then it teaches the confirmation pass the pipeline now takes, instead of describing only the red half and leaving the second one undiscovered. Command: `cargo test -p mustard-rt close_prose_teaches_the_confirmation_pass` Expect: `ok\. [1-9][0-9]* passed`
- AC-11 — when the picker prose spells the Siglas out literally, then it carries the `Onde` legend for the column the table now prints. Command: `cargo test -p mustard-rt picker_prose_teaches_the_onde_column` Expect: `ok\. [1-9][0-9]* passed`
- AC-12 — when the resume prose tells the orchestrator which wave-progress fields to read, then it names `neverDispatched` beside `currentWave`. Command: `cargo test -p mustard-rt resume_prose_teaches_never_dispatched` Expect: `ok\. [1-9][0-9]* passed`
- AC-13 — when a criterion whose statement spans several lines is amended, then the whole statement block is rewritten, instead of leaving the superseded continuation lines orphaned under the new statement. Command: `cargo test -p mustard-rt ac_amend_rewrites_the_whole_statement_block` Expect: `ok\. [1-9][0-9]* passed`
- AC-9 — the project build passes green. Command: `cargo build --workspace`

<!-- wikilinks-footer-start -->
- [wave.make-harness-stop-asserting-what.1-proof](spec.md)
- [wave.make-harness-stop-asserting-what.2-discovery](spec.md)
- [wave.make-harness-stop-asserting-what.3-plan](spec.md)
- [wave.make-harness-stop-asserting-what.4-bootstrap](spec.md)
- [wave.make-harness-stop-asserting-what.5-checklist](spec.md)
<!-- wikilinks-footer-end -->
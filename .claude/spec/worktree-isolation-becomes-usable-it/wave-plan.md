---
id: wave.worktree-isolation-becomes-usable-it.plan
---

# Wave Plan

## Wave Table

| Wave | Spec | Role | Depends on | Summary |
|------|------|------|------------|---------|
| 1 | [[wave.worktree-isolation-becomes-usable-it.1-carry]] | carry | — | A worktree receives the environment the project declares: small files copied, heavy directories linked, and whatever could not travel is named. |
| 2 | [[wave.worktree-isolation-becomes-usable-it.2-reap]] | reap | — | The collector stops reporting and starts collecting — keyed on whether the owner still exists, not on how many days have passed. |
| 3 | [[wave.worktree-isolation-becomes-usable-it.3-isolate]] | isolate | [[wave.worktree-isolation-becomes-usable-it.1-carry]] | A second unit is isolated instead of taking over the checkout, and the prose teaches the arrangement that now actually works. |

## Acceptance Criteria
- AC-1 — when a project declares a `carry` path and a worktree is cut, then that path exists inside the worktree as a real copy. Command: `cargo test -p mustard-rt a_declared_carry_path_lands_in_a_fresh_worktree`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt work_unit_open`
- AC-2 — when a project declares a `link` path, then the worktree reaches the main checkout's copy instead of duplicating it. Command: `cargo test -p mustard-rt a_declared_link_path_reaches_the_main_checkout`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt work_unit_open`
- AC-3 — when a declared path cannot travel, then the creation still succeeds and the report names that path. Command: `cargo test -p mustard-rt what_did_not_travel_is_named_and_never_aborts`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt work_unit_open`
- AC-4 — when a worktree's owning process no longer exists and it holds no work, then the session-start collector removes it without waiting for any age threshold. Command: `cargo test -p mustard-rt an_orphan_worktree_is_collected_without_waiting_for_age`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt worktree_gc`
- AC-5 — when a worktree holds uncommitted or untracked work, then the acting collector still refuses to remove it, whatever its owner or age. Command: `cargo test -p mustard-rt the_acting_collector_still_refuses_a_worktree_holding_work`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt worktree_gc`
- AC-6 — when a removal-proof worktree is left behind by an interrupted run, then it is within the collector's reach and is collected. Command: `cargo test -p mustard-rt an_abandoned_removal_worktree_is_within_reach_and_collected`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt worktree_gc`
- AC-7 — when the checkout already holds a different unit's branch, then the new unit is cut into its own worktree and the checkout is left untouched. Command: `cargo test -p mustard-rt a_second_unit_is_isolated_instead_of_taking_the_checkout`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt work_branch_gate`
- AC-8 — when a second unit is isolated, then the first unit's uncommitted work stays on the first unit's branch. Command: `cargo test -p mustard-rt the_first_units_uncommitted_work_stays_where_it_was`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt work_branch_gate`
- AC-9 — when the operator prose describes isolation, then it teaches the declared environment and the second-unit rule. Command: `cargo test -p mustard-rt worktree_prose_teaches_the_declared_environment`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt prose_teaches`
- AC-10 — the workspace still builds. Command: `cargo build --workspace`

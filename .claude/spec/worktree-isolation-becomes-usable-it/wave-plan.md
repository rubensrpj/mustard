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
- AC-1 — when a worktree is cut, then it receives only what git and its submodules bring, and the harness adds nothing of its own Command: `cargo test -p mustard-rt a_fresh_worktree_receives_only_git_and_submodules`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt work_unit_open`
- AC-2 — when a worktree is cut, then it holds no link of any kind reaching back into the main checkout Command: `cargo test -p mustard-rt a_fresh_worktree_holds_no_link_into_the_main_checkout`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt work_unit_open`
- AC-3 — when the harness refuses to start a second unit, then the refusal names the paths that hold uncommitted work Command: `cargo test -p mustard-rt the_refusal_names_the_paths_holding_uncommitted_work`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt work_unit_open`
- AC-4 — when a worktree's owning process no longer exists and it holds no work, then the session-start collector removes it without waiting for any age threshold. Command: `cargo test -p mustard-rt an_orphan_worktree_is_collected_without_waiting_for_age`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt worktree_gc`
- AC-5 — when a worktree holds uncommitted or untracked work, then the acting collector still refuses to remove it, whatever its owner or age. Command: `cargo test -p mustard-rt the_acting_collector_still_refuses_a_worktree_holding_work`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt worktree_gc`
- AC-6 — when a removal-proof worktree is left behind by an interrupted run, then it is within the collector's reach and is collected. Command: `cargo test -p mustard-rt an_abandoned_removal_worktree_is_within_reach_and_collected`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt worktree_gc`
- AC-7 — when the checkout already holds a different unit branch carrying uncommitted work, then the new unit is refused and the checkout is left untouched Command: `cargo test -p mustard-rt a_second_unit_is_refused_instead_of_taking_the_checkout`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt work_branch_gate`
- AC-8 — when a second unit is isolated, then the first unit's uncommitted work stays on the first unit's branch. Command: `cargo test -p mustard-rt the_first_units_uncommitted_work_stays_where_it_was`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt work_branch_gate`
- AC-9 — when the operator prose describes what happens with a second unit, then it teaches the refusal and the orphan collector, and mentions no environment declaration Command: `cargo test -p mustard-rt worktree_prose_teaches_the_refusal_and_the_reaper`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt prose_teaches`
- **AC-11** — when spec-draft cuts a unit branch and the checkout already holds another unit uncommitted work, then the cut itself refuses instead of checking out
  Command: `cargo test -p mustard-rt the_branch_cut_itself_refuses_a_busy_checkout`
  Expect: `[1-9][0-9]* passed`
- **AC-12** — when the collector cannot positively establish that a candidate holds no work, then it refuses to remove it, and a candidate directory holding files under .claude is seen as holding work
  Command: `cargo test -p mustard-rt the_collector_refuses_what_it_could_not_prove_empty`
  Expect: `[1-9][0-9]* passed`
- AC-10 — the workspace still builds. Command: `cargo build --workspace`

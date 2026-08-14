---
id: cap.worktree-isolation-becomes-usable-it
status: active
---

# worktree isolation becomes usable it

### Requirement: The system SHALL satisfy the acceptance criteria of spec worktree-isolation-becomes-usable-it.

#### Scenario: AC-1
- when: a worktree is cut
- then: it receives only what git and its submodules bring, and the harness adds nothing of its own
- command: `cargo test -p mustard-rt a_fresh_worktree_receives_only_git_and_submodules`

#### Scenario: AC-2
- when: a worktree is cut
- then: it holds no link of any kind reaching back into the main checkout
- command: `cargo test -p mustard-rt a_fresh_worktree_holds_no_link_into_the_main_checkout`

#### Scenario: AC-3
- when: the harness refuses to start a second unit
- then: the refusal names the paths that hold uncommitted work
- command: `cargo test -p mustard-rt the_refusal_names_the_paths_holding_uncommitted_work`

#### Scenario: AC-4
- when: a worktree's owning process no longer exists and it holds no work
- then: the session-start collector removes it without waiting for any age threshold.
- command: `cargo test -p mustard-rt an_orphan_worktree_is_collected_without_waiting_for_age`

#### Scenario: AC-5
- when: a worktree holds uncommitted or untracked work
- then: the acting collector still refuses to remove it, whatever its owner or age.
- command: `cargo test -p mustard-rt the_acting_collector_still_refuses_a_worktree_holding_work`

#### Scenario: AC-6
- when: a removal-proof worktree is left behind by an interrupted run
- then: it is within the collector's reach and is collected.
- command: `cargo test -p mustard-rt an_abandoned_removal_worktree_is_within_reach_and_collected`

#### Scenario: AC-7
- when: the checkout already holds a different unit branch carrying uncommitted work
- then: the new unit is refused and the checkout is left untouched
- command: `cargo test -p mustard-rt a_second_unit_is_refused_instead_of_taking_the_checkout`

#### Scenario: AC-8
- when: a second unit is isolated
- then: the first unit's uncommitted work stays on the first unit's branch.
- command: `cargo test -p mustard-rt the_first_units_uncommitted_work_stays_where_it_was`

#### Scenario: AC-9
- when: the operator prose describes what happens with a second unit
- then: it teaches the refusal and the orphan collector, and mentions no environment declaration
- command: `cargo test -p mustard-rt worktree_prose_teaches_the_refusal_and_the_reaper`

#### Scenario: AC-11
- when: spec-draft cuts a unit branch and the checkout already holds another unit uncommitted work
- then: the cut itself refuses instead of checking out
- command: `cargo test -p mustard-rt the_branch_cut_itself_refuses_a_busy_checkout`

#### Scenario: AC-12
- when: the collector cannot positively establish that a candidate holds no work
- then: it refuses to remove it, and a candidate directory holding files under .claude is seen as holding work
- command: `cargo test -p mustard-rt the_collector_refuses_what_it_could_not_prove_empty`

#### Scenario: AC-10
- when: 
- then: the workspace still builds.
- command: `cargo build --workspace`

## Covers

## Specs
- [[spec.worktree-isolation-becomes-usable-it]]

## Related


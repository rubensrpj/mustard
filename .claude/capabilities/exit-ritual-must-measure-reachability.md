---
id: cap.exit-ritual-must-measure-reachability
status: active
---

# exit ritual must measure reachability

### Requirement: The system SHALL satisfy the acceptance criteria of spec exit-ritual-must-measure-reachability.

#### Scenario: AC-1
- when: a merged PR's branch has moved (any ref: local or remote), the classifier answers the new state `moved-after-merge` and never a pruning state; per-ref evidence replaces the name-level collapse.
- then: 
- command: `cargo test -p mustard-rt moved_after_merge 2>&1 | grep -E "test result: ok\. [1-9][0-9]* passed"`

#### Scenario: AC-2
- when: any ref of the unit moved after the merge, `git-settle` refuses (`not-merged`) and touches nothing; its gate delegates to the shared per-ref predicate (the hand-written copy in `is_merged` is gone).
- then: 
- command: `cargo test -p mustard-rt settle_refuses_when_a_ref_moved_after_merge 2>&1 | grep -E "test result: ok\. [1-9][0-9]* passed"`

#### Scenario: AC-3
- when: the only dirt is a moved gitlink, the base advance proceeds (measured: git's own `--ff-only` accepts it); after the fast-forward, `git submodule update` aligns ONLY detached submodules — a submodule sitting on any branch is reported and left untouched.
- then: 
- command: `cargo test -p mustard-rt gitlink_only_dirt 2>&1 | grep -E "test result: ok\. [1-9][0-9]* passed"`

#### Scenario: AC-4
- when: the unit's own base did not advance in a finishing shape (`settled`/`partial`), the report answers `ok: false` with reason `base-behind`; `exit-and-rerun` keeps `ok: true`.
- then: 
- command: `cargo test -p mustard-rt base_behind_downgrades_ok 2>&1 | grep -E "test result: ok\. [1-9][0-9]* passed"`

#### Scenario: AC-5
- when: 
- then: the diff context reads commit ranges via `rev-list` (which rtk passes through byte-identical), never via `git log` (which rtk filters). Reproduction: non-zero today because the `log --oneline` argv is present; zero after the swap.
- command: `cargo test -p mustard-rt diff_context_reads_ranges_via_rev_list 2>&1 | grep -E "test result: ok\. [1-9][0-9]* passed"`

#### Scenario: AC-6
- when: the `/git` prose is read by a test, the gitlink stage is conditioned on reachability against the submodule's base, the pending state is named, the bump step exists, and the parent PR opens as draft with a "Blocked by" line while a submodule PR is open — asserted structurally (both halves: the new instruction present AND the unconditional MANDATORY stage gone), never by a bare word search.
- then: 
- command: `cargo test -p mustard-rt --test git_prose_rules git_prose_conditions_gitlink_on_reachability 2>&1 | grep -E "test result: ok\. [1-9][0-9]* passed"`

#### Scenario: AC-7
- when: the same test reads the iron rules, the destructive-decision rule is there: a decision that authorises deletion reads `rev-list`, never `git log`, and states the reason (the wrapper filters `log` and passes `rev-list` through).
- then: 
- command: `cargo test -p mustard-rt --test git_prose_rules git_prose_routes_destructive_decisions_through_rev_list 2>&1 | grep -E "test result: ok\. [1-9][0-9]* passed"`

#### Scenario: AC-8
- when: 
- then: the rtk issue report exists with the two-line reproduction, and the whole workspace stays green.
- command: `grep -q "git log --oneline -1" .claude/spec/exit-ritual-must-measure-reachability/rtk-issue-report.md && cargo test --workspace 2>&1 | tail -20 | grep -E "test result: ok"`

## Covers

## Specs
- [[spec.exit-ritual-must-measure-reachability]]

## Related


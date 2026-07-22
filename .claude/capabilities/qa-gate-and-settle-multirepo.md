---
id: cap.qa-gate-and-settle-multirepo
status: active
---

# qa gate and settle multirepo

### Requirement: The system SHALL satisfy the acceptance criteria of spec qa-gate-and-settle-multirepo.

#### Scenario: AC-1
- when: an acceptance criterion prints far more than the operating system's pipe buffer
- then: it completes and is judged by its real exit code instead of dying at the time limit
- command: `cargo test -p mustard-rt ac_command_past_the_pipe_buffer_is_judged_by_exit_code`

#### Scenario: AC-2
- when: a criterion exceeds its time limit
- then: it is reported as `timeout`, never as a benign `skip`
- command: `cargo test -p mustard-rt ac_command_killed_by_deadline_reports_timeout_not_skip`

#### Scenario: AC-2B
- when: any criterion timed out
- then: the overall verdict is never `pass`, and a real `fail` still outranks it
- command: `cargo test -p mustard-rt overall_verdict_timeout_is_never_pass_and_never_beats_fail`

#### Scenario: AC-3
- when: a self-invoked run skips every criterion because it cannot rebuild the running binary
- then: it emits NO `qa.result` event, so an external passing verdict stays the last word
- command: `cargo test -p mustard-rt self_invoked_all_skipped_run_writes_no_qa_result`

#### Scenario: AC-3B
- when: a self-invoked run finds no runnable criterion at all (no parseable acceptance criteria)
- then: it also records no verdict — the same hole, reached by the other door
- command: `cargo test -p mustard-rt self_invoked_empty_ac_set_writes_no_qa_result`

#### Scenario: AC-4
- when: `git-settle` runs from inside a submodule
- then: it resolves the integration bases from the superproject's `mustard.json` and recognises a `dev_` work branch
- command: `cargo test -p mustard-rt settle_resolves_bases_from_superproject`

#### Scenario: AC-5
- when: a branch prefix names no known base
- then: the refusal names the root it resolved and the bases it knows, instead of only the branch
- command: `cargo test -p mustard-rt no_base_prefix_names_root_and_known_bases`

#### Scenario: AC-6
- when: the unit spans a parent and a submodule
- then: the report carries one entry per repository with its own settled flag plus a global `complete`, and `complete` is false while any repository is unsettled
- command: `cargo test -p mustard-rt settle_reports_every_repo_of_the_unit`

#### Scenario: AC-7
- when: the `pr close` ritual is read
- then: it states the submodule-first order its own iron rule promises
- command: `cargo test -p mustard-rt pr_close_ritual_names_submodules`

#### Scenario: AC-8
- when: 
- then: the harness test suite stays green, and it runs to completion INSIDE the QA gate (the chatty-output proof, end to end)
- command: `cargo test -p mustard-rt`

## Covers

## Specs
- [[spec.qa-gate-and-settle-multirepo]]

## Related


---
id: cap.harness-obstructs-its-own-work
status: active
---

# harness obstructs its own work

### Requirement: The system SHALL satisfy the acceptance criteria of spec harness-obstructs-its-own-work.

#### Scenario: AC-1
- when: the unit's base did NOT advance
- then: the settle pass prunes nothing and answers `ok:false`, leaving the local branch and the remote branch alive.
- command: `cargo test -p mustard-rt prune_waits_for_the_base_to_advance`

#### Scenario: AC-2
- when: the base advanced
- then: the prune is authorised regardless of whether the unit's own commit is reachable from the base, so a squash-merged unit still settles.
- command: `cargo test -p mustard-rt prune_authorisation_reads_the_base_advance_not_unit_ancestry`

#### Scenario: AC-3
- when: the in-place exit could not free the floor
- then: the remote branch survives together with the local one.
- command: `cargo test -p mustard-rt a_blocked_exit_leaves_the_remote_branch_alone`

#### Scenario: AC-4
- when: the working tree is dirty ONLY in paths the advance does not touch
- then: the base still fast-forwards and the report says `updated:true`.
- command: `cargo test -p mustard-rt a_dirty_tree_the_advance_does_not_touch_still_fast_forwards`

#### Scenario: AC-5
- when: git refuses the fast-forward
- then: the report separates a genuine divergence from a merely dirty tree, so the operator is pointed at the real obstacle
- command: `cargo test -p mustard-rt a_refused_advance_separates_divergence_from_dirt`

#### Scenario: AC-6
- when: a write on a bare integration base targets `.claude/scratch/`
- then: the gate allows it, cuts no branch, and keeps the pending marker for the first in-repo edit.
- command: `cargo test -p mustard-rt scratch_evidence_is_writable_on_a_protected_base`

#### Scenario: AC-7
- when: a fresh project is seeded with the shipped ignore template and every path the runtime actually writes is created
- then: git reports nothing dirty — the paths come from the writers in the code, not from a list the test chose for itself
- command: `cargo test -p mustard-core the_seeded_ignore_hides_every_path_the_runtime_writes`

#### Scenario: AC-8
- when: the bugfix flow reaches its spec step
- then: its prose instructs assembling the material file and passing `--material` to `spec-draft`.
- command: `cargo test -p mustard-rt bugfix_prose_teaches_the_material_channel`

#### Scenario: AC-9
- when: the hygiene ref describes step 3
- then: it conditions the question on overlap with the active spec instead of asking unconditionally.
- command: `cargo test -p mustard-rt hygiene_prose_teaches_the_collision_condition`

#### Scenario: AC-11
- when: the unit's base already HOLDS origin's tip (it is ahead of origin, so the fetch refuses)
- then: the prune is authorised — the gate reads the fact, not the fetch exit status
- command: `cargo test -p mustard-rt a_base_ahead_of_origin_authorises_the_prune`

#### Scenario: AC-12
- when: the write gate allows a scratch path in this repository
- then: git also ignores it, so an add -A cannot sweep throwaway evidence into the unit
- command: `git check-ignore --no-index .claude/scratch/probe.sh`

#### Scenario: AC-13
- when: the harness seeds its ignore file over one that already exists
- then: the lines missing from it are appended instead of the whole file being skipped, so an already-initialised project receives new entries
- command: `cargo test -p mustard-core seeding_over_an_existing_ignore_adds_the_missing_lines`

#### Scenario: AC-14
- when: the prune is refused on an in-place unit
- then: the checkout is restored to the unit branch and the report says so, so the refusal leaves the operator where the work is
- command: `cargo test -p mustard-rt a_refused_prune_restores_the_unit_branch`

#### Scenario: AC-10
- when: 
- then: the workspace still builds.
- command: `cargo build --workspace`

## Covers

## Specs
- [[spec.harness-obstructs-its-own-work]]

## Related


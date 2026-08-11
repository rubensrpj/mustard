---
id: wave.harness-obstructs-its-own-work.plan
---

# Wave Plan

## Wave Table

| Wave | Spec | Role | Depends on | Summary |
|------|------|------|------------|---------|
| 1 | [[wave.harness-obstructs-its-own-work.1-settle]] | settle | — | The exit ritual verifies before it prunes, and git — not a stricter pre-check — decides whether the base can advance. |
| 2 | [[wave.harness-obstructs-its-own-work.2-gate]] | gate | — | Diagnosis may produce runnable evidence without leaving the protected base, and the harness stops dirtying its own tree. |
| 3 | [[wave.harness-obstructs-its-own-work.3-prose]] | prose | [[wave.harness-obstructs-its-own-work.2-gate]] | The diagnosis rides into the spec instead of being retyped, and the hygiene question fires on a real collision instead of always. |

## Acceptance Criteria
- AC-1 — when the unit's base did NOT advance, then the settle pass prunes nothing and answers `ok:false`, leaving the local branch and the remote branch alive. Command: `cargo test -p mustard-rt prune_waits_for_the_base_to_advance`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt git_settle`
- AC-2 — when the base advanced, then the prune is authorised regardless of whether the unit's own commit is reachable from the base, so a squash-merged unit still settles. Command: `cargo test -p mustard-rt prune_authorisation_reads_the_base_advance_not_unit_ancestry`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt git_settle`
- AC-3 — when the in-place exit could not free the floor, then the remote branch survives together with the local one. Command: `cargo test -p mustard-rt a_blocked_exit_leaves_the_remote_branch_alone`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt git_settle`
- AC-4 — when the working tree is dirty ONLY in paths the advance does not touch, then the base still fast-forwards and the report says `updated:true`. Command: `cargo test -p mustard-rt a_dirty_tree_the_advance_does_not_touch_still_fast_forwards`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt git_settle`
- AC-5 — when git refuses the fast-forward, then the report names `dirty-tree` if the tree was dirty and `non-ff-or-no-remote` if it was clean. Command: `cargo test -p mustard-rt a_refused_advance_names_dirt_only_when_the_tree_was_dirty`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt git_settle`
- AC-6 — when a write on a bare integration base targets `.claude/scratch/`, then the gate allows it, cuts no branch, and keeps the pending marker for the first in-repo edit. Command: `cargo test -p mustard-rt scratch_evidence_is_writable_on_a_protected_base`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt work_branch_gate`
- AC-7 — when the seeded ignore file is in place and the harness writes its artefacts, then `git status --porcelain` reports nothing. Command: `cargo test -p mustard-core the_seeded_ignore_hides_every_artefact_the_harness_writes`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-core claude_paths`
- AC-8 — when the bugfix flow reaches its spec step, then its prose instructs assembling the material file and passing `--material` to `spec-draft`. Command: `cargo test -p mustard-rt bugfix_prose_teaches_the_material_channel`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt prose_teaches`
- AC-9 — when the hygiene ref describes step 3, then it conditions the question on overlap with the active spec instead of asking unconditionally. Command: `cargo test -p mustard-rt hygiene_prose_teaches_the_collision_condition`  Expect: `[1-9][0-9]* passed`  Control: `cargo test -p mustard-rt prose_teaches`
- **AC-11** — when the unit's base already HOLDS origin's tip (it is ahead of origin, so the fetch refuses), then the prune is authorised — the gate reads the fact, not the fetch exit status
  Command: `cargo test -p mustard-rt a_base_ahead_of_origin_authorises_the_prune`
  Expect: `[1-9][0-9]* passed`
- **AC-12** — when the write gate allows a scratch path in this repository, then git also ignores it, so an add -A cannot sweep throwaway evidence into the unit
  Command: `git check-ignore --no-index .claude/scratch/probe.sh`
- **AC-13** — when the harness seeds its ignore file over one that already exists, then the lines missing from it are appended instead of the whole file being skipped, so an already-initialised project receives new entries
  Command: `cargo test -p mustard-core seeding_over_an_existing_ignore_adds_the_missing_lines`
  Expect: `[1-9][0-9]* passed`
- **AC-14** — when the prune is refused on an in-place unit, then the checkout is restored to the unit branch and the report says so, so the refusal leaves the operator where the work is
  Command: `cargo test -p mustard-rt a_refused_prune_restores_the_unit_branch`
  Expect: `[1-9][0-9]* passed`
- AC-10 — the workspace still builds. Command: `cargo build --workspace`

---
id: wave.work-unit-lives-on-its.2-spec-home
---

# wave-2-spec-home

## Summary

The branch is cut at approval and the spec, its waves and the whole ceremony are materialized inside it; resuming from within that branch costs no ceremony.

## Network

- Parent: [[spec.work-unit-lives-on-its]]
- Depends on: [[wave.work-unit-lives-on-its.1-gate]]

## Tasks

- [ ] Remove the .claude/spec/ carve-out from the work-branch gate so authoring a spec on a protected base is refused like any other write.
- [ ] Reorder the pipeline so the branch is cut when the analysis is approved, before spec-draft runs — the draft, the wave layout and the proof all land inside the branch.
- [ ] Keep the approval gates meaningful in their new position: ac-negative-check and the clarify marker run inside the branch before wave 1, where the code still does not exist.
- [ ] Make /mustard:spec detect that the current branch is {base}_{slug} for the spec it was given and dispatch straight into the wave loop — no table, no header, no 'implement now' question.

## Files

- `apps/rt/src/hooks/write/work_branch_gate.rs`
- `apps/rt/src/commands/spec/spec_draft.rs`
- `apps/rt/src/commands/pipeline/resume_bootstrap/mode_decision.rs`
- `plugin/commands/spec.md`
- `plugin/refs/spec/resume-loop.md`

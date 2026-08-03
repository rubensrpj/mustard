---
id: wave.work-unit-lives-on-its.1-gate
---

# wave-1-gate

## Summary

A base gate refuses to start an analysis off an integration base or on a base behind its remote, and refreshes the census there.

## Network

- Parent: [[spec.work-unit-lives-on-its]]

## Tasks

- [ ] Add a base gate that runs before ANALYZE: it reads the git.flow keys of mustard.json, and refuses when the current checkout is not one of those bases.
- [ ] Extend the same gate to refuse when the base is behind its remote, naming the pull command in the refusal message.
- [ ] Trigger the census refresh from the gate when the model is stale AND the tree is clean — the only moment scan can run, since everything it writes is versioned.
- [ ] Wire the gate into the orchestrator so every pipeline-opening path crosses it; a read-only request never reaches it.

## Files

- `apps/rt/src/hooks/write/work_branch_gate.rs`
- `apps/rt/src/commands/event/emit_pipeline.rs`
- `packages/core/src/domain/config.rs`
- `.claude/mustard/orchestrator.md`
- `apps/cli/templates/.claude/mustard/orchestrator.md`

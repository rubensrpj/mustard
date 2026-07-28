---
id: wave.make-harness-stop-asserting-what.3-plan
---

# wave-3-plan

## Summary

A plan can oblige a wave to verify something outside the repository, and the closing of that wave reports whether the duty was met.

## Network

- Parent: [[spec.make-harness-stop-asserting-what]]
- Depends on: [[wave.make-harness-stop-asserting-what.1-proof]]

## Tasks

- [ ] Give the plan JSON a per-wave place to declare reality obligations — duties to check the world rather than the text of the code: read an official document, call a live endpoint, read a stored row.
- [ ] Carry those duties into the wave scaffold and through the rendered agent prompt as their own section, so the dispatched agent reads them as instructions rather than as prose someone happened to write.
- [ ] Have wave-done report each declared duty the returning wave did not account for, by name.
- [ ] Document the mechanism where the flow already documents the research step, so the next planner finds it without being told.

## Files

- `plugin/refs/feature/full-plan.md`
- `apps/rt/src/commands/pipeline/plan_materialize.rs`
- `apps/rt/src/commands/agent/agent_prompt_render.rs`
- `apps/rt/src/commands/pipeline/wave_done.rs`

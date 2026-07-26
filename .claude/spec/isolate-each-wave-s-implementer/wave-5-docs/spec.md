---
id: wave.isolate-each-wave-s-implementer.5-docs
---

# wave-5-docs

## Summary

Correct the role-to-subagent map wherever it is written down, and pin it to the code so prose can never contradict behaviour again.

## Network

- Parent: [[spec.isolate-each-wave-s-implementer]]
- Depends on: [[wave.isolate-each-wave-s-implementer.4-rt]]

## Tasks

- [ ] `plugin/refs/agent-prompt/agent-prompt.md` holds the canonical map. Update the writing-role row and the sentence above it ('writing roles rely on the per-role contract + the scope_guard hook'), stating the consequence a reader needs: a writing role now runs in its own checkout, so its commit boundary is that checkout and the wave's work returns via reclaim.
- [ ] `plugin/commands/feature.md` repeats the map inline in its Inviolable list (`writing roles→general-purpose`). Update that copy to match.
- [ ] Add `agent_prompt_ref_matches_subagent_map` to `apps/rt/tests/plugin_agents.rs`: read the committed ref and assert it names the same subagent type `recommended_subagent_type` returns for a writing role. Same drift-guard shape as `plugin_namespace_matches_manifest_name` — prose that contradicts the code is exactly the defect it catches.
- [ ] Run the workspace build to confirm nothing else referenced the old mapping.

## Files

- `plugin/refs/agent-prompt/agent-prompt.md`
- `plugin/commands/feature.md`
- `apps/rt/tests/plugin_agents.rs`

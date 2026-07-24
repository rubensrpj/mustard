---
id: wave.structured-review-verdict-capture-via.1-contract
---

# wave-1-contract

## Summary

Emit the structured <VERDICT> block from the review agent

## Network

- Parent: [[spec.structured-review-verdict-capture-via]]

## Tasks

- [ ] Add the <VERDICT>{"verdict":...,"critical":N,"findings":[...]} emission contract to plugin/agents/mustard-review.md — document the JSON schema and that `critical` counts blocking Guard/mold/correctness findings only
- [ ] Mirror the same <VERDICT> instruction in the rendered review role block in apps/rt/src/commands/agent/render/role.rs so a rendered review agent matches the plugin agent

## Files

- `plugin/agents/mustard-review.md`
- `apps/rt/src/commands/agent/render/role.rs`

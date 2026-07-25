---
id: wave.isolate-each-wave-s-implementer.4-rt
---

# wave-4-rt

## Summary

The switch: point every writing role at the isolated implementer — last, because both the way in and the way out must already work.

## Network

- Parent: [[spec.isolate-each-wave-s-implementer]]
- Depends on: [[wave.isolate-each-wave-s-implementer.2-plugin]], [[wave.isolate-each-wave-s-implementer.3-rt]]

## Tasks

- [ ] In `recommended_subagent_type`, the catch-all arm returns `general-purpose` — the arm every writing role (impl, backend, frontend, core, …) falls through to. Route it through the existing `qualify_plugin_agent` to the plugin implementer, so the returned type is namespaced exactly like the read-only plugin agents. Without the namespace Claude Code cannot resolve it and silently falls back to `general-purpose`.
- [ ] Leave the built-in read-only roles untouched and bare: `explore` stays `Explore`, `plan` stays `Plan`. Only the writing fall-through moves.
- [ ] Update the doc comment: it currently states the old rationale ('writing roles stay general-purpose: they need Edit/Write and rely on the per-role contract + scope_guard'). The contract and `scope_guard` still apply — what changed is that the agent now also carries isolation, which no prompt can carry.
- [ ] Update the two existing tests that pin the old mapping (`recommended_subagent_type_locks_read_only_roles`, `recommended_subagent_type_namespaces_plugin_agents_only`): they are the drift guard, so restate the new contract rather than delete them.
- [ ] Test `recommended_subagent_type_routes_writing_roles_to_impl`: assert `impl`, `backend` and an unknown role all resolve to the plugin-qualified implementer, and that built-in types still carry no namespace separator.

## Files

- `apps/rt/src/commands/agent/render/role.rs`

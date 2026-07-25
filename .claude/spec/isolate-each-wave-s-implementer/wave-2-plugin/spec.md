---
id: wave.isolate-each-wave-s-implementer.2-plugin
---

# wave-2-plugin

## Summary

Create the implementer subagent the plugin never had — deliberately thin, carrying only what a prompt cannot carry: the worktree isolation.

## Network

- Parent: [[spec.isolate-each-wave-s-implementer]]

## Tasks

- [ ] Create `plugin/agents/mustard-impl.md` mirroring its siblings' shape (`mustard-review.md` is the closest exemplar). Unlike them it is a WRITING agent, so it keeps the edit tools rather than restricting them.
- [ ] Declare `isolation: worktree` in the frontmatter. That single field is the whole reason this file exists: the per-wave contract already reaches the agent through the rendered prompt (`{role_block}`), which is computed per subproject and per wave. A static body here would duplicate it and be poorer.
- [ ] Keep the body MINIMAL for that reason — only what the rendered prompt cannot state: your checkout is yours alone, so `add -A` inside it is this wave's boundary; never create a branch (the harness owns naming); a git command aimed outside the checkout fails by design, so do not try to reach the main tree.
- [ ] Do not add a `skills:` preload. The ref explains why: the native preload is static and injects skill BODIES, which would break the per-subproject shelf and the PREFIX-STABLE byte-identical prompt head the prompt cache depends on.
- [ ] Keep the description keyword-led (descriptions are shortened when many are discovered): lead with words a dispatch would contain — implement, wave, subproject.
- [ ] Test `impl_agent_declares_worktree_isolation` in a new `apps/rt/tests/plugin_agents.rs`: read the committed file, assert the frontmatter carries `isolation: worktree` and a `name` matching the file stem. Follow the fail-open convention of `plugin_namespace_matches_manifest_name` — print a skip note when the workspace root cannot be resolved.

## Files

- `plugin/agents/mustard-impl.md (new)`
- `apps/rt/tests/plugin_agents.rs (new)`

---
id: wave.field-report-fix-package-sialia.2-agents
---

# wave-2-agents

## Summary

plugin-owned subagent_type strings gain the mustard: namespace at the single resolver; readonly denylist normalizes the prefix

## Network

- Parent: [[spec.field-report-fix-package-sialia]]

## Tasks

- [ ] In role.rs introduce a single source for the plugin namespace (const PLUGIN_NAMESPACE: &str = "mustard") and a helper that qualifies plugin-owned agent names (mustard-review, mustard-guards, mustard-patterns) as `mustard:mustard-*`; builtin types (Explore, Plan, general-purpose) stay bare — recommended_subagent_type returns the qualified form for review/qa/guards/patterns
- [ ] Add a consistency test that reads plugin/.claude-plugin/plugin.json from the repo and asserts PLUGIN_NAMESPACE equals its name field (drift guard)
- [ ] In subagent_inject.rs role_is_readonly: normalize the incoming subagent_type by stripping any `<ns>:` prefix before matching, and ADD the missing mustard-patterns to the read-only set
- [ ] Update tests and doc comments in dispatch_plan.rs and wave_advance.rs that assert the bare `mustard-review` string so they expect `mustard:mustard-review`; the call sites themselves keep delegating to recommended_subagent_type
- [ ] FINDING #7 (live regression, same file): wave_number_from_link in dispatch_plan.rs only resolves `[[N]]`, `[[wave-N-role]]` (hyphen) and bare roles, but wave-scaffold writes the DOTTED wikilink `[[wave.<slug>.<N>-<role>]]` (e.g. `[[wave.field-report-fix-package-sialia.2-agents]]`). `strip_prefix("wave-")` fails on `wave.`, the edge drops, the whole DAG flattens to level 0 and every wave dispatches as one parallel round (proven live 2026-07-18 on this pipeline). Extend wave_number_from_link to also parse the dotted form: after the existing branches, split the inner token on '.' and, when the LAST segment matches `<digits>-<role>` (or the segment right after a `wave` head is numeric), take those leading digits as the wave number. Keep the hyphen and bare-role branches intact (back-compat).
- [ ] Unit tests named with `namespac` covering: resolver emits namespaced plugin agents and bare builtins; role_is_readonly accepts both `mustard-review` and `mustard:mustard-review`; mustard-patterns is read-only
- [ ] Unit test named `dotted_wikilink_resolves_wave` (dependency parse): `wave_number_from_link("wave.some-slug.2-agents", ..)` -> Some(2); a full parse_wave_table over a `| Wave | Spec | Role | Depends on | Summary |` fixture whose Depends-on cell holds the dotted link yields the correct non-flat levels (a dependent wave gets level >= 1)

## Files

- `apps/rt/src/commands/agent/render/role.rs`
- `apps/rt/src/hooks/task/subagent_inject.rs`
- `apps/rt/src/commands/pipeline/dispatch_plan.rs`
- `apps/rt/src/commands/pipeline/wave_advance.rs`

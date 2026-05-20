---
name: rt-hook-module-pattern
description: "Pattern for adding an enforcement module to mustard-rt hooks. Use when adding a new gate, adding a new observer, implementing Check or Observer trait, registering a hook module, writing enforcement logic. Even if the user just says 'add a hook' or 'block a tool call'."
source: scan
---
<!-- mustard:generated at:2026-05-19T00:00:00.000Z role:general -->

## Convention

- One unit struct per module (e.g. `BashGuard`, `BudgetGuard`).
- Implement `Check` when the module makes a blocking/advisory verdict (`Verdict`).
- Implement `Observer` when the module emits telemetry — returns `()`, never affects outcome.
- Both traits may be implemented on the same struct (dual modules: `BashGuard`, `PostEdit`).
- Module is registered in `src/registry.rs::Registry::new()` with `id`, `applies_to`, `check`, `observer`.
- `applies_to` is a `&'static [(Trigger, ToolMatch)]` slice.
- `ToolMatch::Named("Bash")` — single tool; `ToolMatch::Any` — all tools (wildcard).
- Fail-open contract: `evaluate` returns `Ok(Verdict::Allow)` on unexpected input — never `Err` on non-fatal paths.
- Enforcement `Mode` is applied by the dispatcher, not the module; modules never read `MUSTARD_*_MODE` except for module-local overrides (e.g. `MUSTARD_COMMIT_GATE_MODE` in `bash_guard`).
- `unsafe` is forbidden (`#![forbid(unsafe_code)]`); `unwrap_used` is `deny` in non-test code.

## Real examples in this codebase

- `apps/rt/src/hooks/bash_guard.rs` — dual Check+Observer, four gate chain
- `apps/rt/src/hooks/budget.rs` — Check-only, Task/Agent prompt-size gate
- `apps/rt/src/hooks/model_routing.rs` — Check-only, model-selection gate
- `apps/rt/src/hooks/tracker.rs` — three Observer structs + two Check structs in one file
- `apps/rt/src/hooks/post_edit.rs` — dual Check+Observer, PostToolUse Write/Edit
- `apps/rt/src/hooks/session_start.rs` — Check-only, SessionStart bootstrap/inject
- `apps/rt/src/hooks/path_guard.rs` — Check-only, Read/Write/Edit sensitive-file gate
- `apps/rt/src/hooks/close_gate.rs` — Check-only, Write/Edit pipeline-CLOSE sensor
- `apps/rt/src/hooks/enforce_registry.rs` — Check-only, Skill pre-pipeline gate
- `apps/rt/src/registry.rs` — full module registration table

## References

See `apps/rt/.claude/skills/rt-hook-module-pattern/references/examples.md` for verbatim code extracts.

---
name: project-w5-test-leak-root-cause
description: cargo test -p mustard-rt was leaking apps/rt/.claude/ via hook tests passing ctx.project_dir="." or empty input.cwd
metadata:
  type: project
---

W5 of `2026-05-26-w2-residuals-50-unlisted-apps-rt`: AC-W5.2 required 10× `cargo test -p mustard-rt` to NOT materialise `apps/rt/.claude/`. Pre-fix leak categories: `spec/`, `.pipeline-states/`, `.agent-state/`, `.harness/`, `.metrics/`.

**Why:** several hook tests construct `HookInput { cwd: None }` + `Ctx { project_dir: ".".to_string() }`. The `"."` placeholder propagated into:

- `tracker::project_dir()` → `MainContextCounter::write_state` → `apps/rt/.claude/.agent-state/main-context.counter.json`
- `budget::metric_cwd()` → `emit_metric` → `apps/rt/.claude/.metrics/budget-check.jsonl`
- `skills_audit::evaluate` → `emit_metric` → `apps/rt/.claude/.metrics/recommended-skills.jsonl`
- `tactical_fix_create::create()` calls `rtk_command("mustard-rt", &["run","spec-link",...])` without `.current_dir()` or `MUSTARD_WORKSPACE_ROOT` env → spawned binary re-walks workspace from `apps/rt/` → emits to `apps/rt/.claude/.pipeline-states/` + `.harness/`.

**How to apply:** When adding a new hook that writes side-effect telemetry, gate the emission on a *non-placeholder* project root resolution. Treat `cwd == "."` as "no cwd supplied" (skip) — never as a valid project. When spawning a child `mustard-rt run ...` from inside another binary, always set `.current_dir(cwd).env("MUSTARD_WORKSPACE_ROOT", cwd)` so the child resolves the same workspace as the parent.

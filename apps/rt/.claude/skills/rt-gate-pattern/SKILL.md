---
name: rt-gate-pattern
description: Use when adding or refactoring a PreToolUse gate struct that evaluates a hook invocation and answers with an Allow/Warn/Deny verdict.
tags: [add, refactor]
appliesTo: [gate]
scope: [code-editing]
source: scan
metadata:
  generated_by: scan
  cluster:
    label: gate
---

# gate pattern

## Purpose

The `*Gate` structs are decision hooks: unit structs implementing the `Check` contract (`mustard_core::domain::model::contract`) whose `evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error>` inspects a `PreToolUse` invocation (Write/Edit, Skill) and returns `Allow`, `Warn { message }` or `Deny { reason }`. Unlike observers, a gate DOES return a verdict — but blocking is expressed only through the `Verdict`, never a panic or non-zero exit; the dispatcher already converts an `Err` into `Allow`. Each gate resolves a three-state mode (`Off`/`Warn`/`Strict`) from a `MUSTARD_*_MODE` env var cascading to a `mustard.json` `gates.*` override; the family default is `warn`, with `close_gate` the deliberate `strict` exception. Every internal failure path — unreadable state, unresolved spec, missing dir — degrades to Allow via `ok()`, `let-else` and `Option` chains, so a broken gate can never wedge the session. A gate only exists at runtime once registered in `src/registry.rs`.

## Convention

- Folder: `apps/rt/src/hooks/write/`
- Suffix: `Gate` (struct), file `{name}_gate.rs`
- Extension: `.rs`
- Declares: structs (unit struct + `impl Check`)
- Count: 9

## How to apply

To add a new `gate`:

- Create `apps/rt/src/hooks/write/{name}_gate.rs` with a `//!` module doc stating scope ("ONE behavior"), the trigger/tool it applies to, the mode cascade, and the fail-open invariant.
- Declare a unit struct `pub struct {Name}Gate;` and `impl Check` for it. First thing in `evaluate`: guard the trigger and tool (`ctx.trigger != Some(Trigger::PreToolUse)` or a foreign tool name → `Ok(Verdict::Allow)`); the gate's real logic lives in a private `fn` returning `Option<Verdict>` where `None` means pass-through.
- Resolve the mode from `MUSTARD_{NAME}_MODE` (env var wins when non-empty, then the `mustard.json` `gates.*` override via `crate::shared::context::project_config_cached`, then the default) into a private `Off`/`Warn`/`Strict` enum. Choose the default deliberately and document why.
- Never `unwrap`/`expect` outside tests (crate-wide `deny`); every error path degrades to Allow. Build user-facing messages with `crate::util::format_gate_message` (or a `[bracketed-tag]` prefix) that names the fix and the escape hatch (`MUSTARD_*_MODE=warn`).
- Register in TWO places: `pub mod {name}_gate;` in `apps/rt/src/hooks/write/mod.rs`, AND a `Module { id, applies_to: &[(Trigger::PreToolUse, ToolMatch::Named("Write")), …], check: Some(Box::new({Name}Gate)), observer: None }` entry in `apps/rt/src/registry.rs` — without the registry entry the gate compiles but never runs.
- Do not put side-effects/telemetry-only behavior here — that is an observer (`hooks/observe`), which returns `()`.
- Tests in-file under `#[cfg(test)] mod tests`: build `HookInput`/`Ctx` fixtures (defaults + `json!` tool_input), seed `.claude/` state in a `tempfile::tempdir`, and assert the verdict for allow, warn, deny and every fail-open path.

## Examples

- Ref: apps/rt/src/hooks/write/boundary_gate.rs
- Ref: apps/rt/src/hooks/write/active_spec_limit_gate.rs
- Ref: apps/rt/src/hooks/write/close_gate.rs

<!-- mustard:generated at:2026-05-19T00:00:00.000Z role:general -->
# Examples: rt-hook-module-pattern

## Minimal Check module (path_guard shape)

```rust
// src/hooks/my_gate.rs
use mustard_core::error::Error;
use mustard_core::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};

pub struct MyGate;

impl Check for MyGate {
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::PreToolUse) {
            return Ok(Verdict::Allow);
        }
        if input.tool_name.as_deref() != Some("Bash") {
            return Ok(Verdict::Allow);
        }
        // decision logic here
        Ok(Verdict::Allow)
    }
}
```

## Registry entry (src/registry.rs)

```rust
Module {
    id: "my_gate",
    applies_to: &[(Trigger::PreToolUse, ToolMatch::Named("Bash"))],
    check: Some(Box::new(MyGate)),
    observer: None,
},
```

## Dual Check+Observer (bash_guard shape)

```rust
impl Check for BashGuard {
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::PreToolUse) { return Ok(Verdict::Allow); }
        if input.tool_name.as_deref() != Some("Bash") { return Ok(Verdict::Allow); }
        let Some(cmd) = Self::command_of(input) else { return Ok(Verdict::Allow); };
        if let Some(v) = bash_safety(&cmd) { return Ok(v); }
        if let Some(v) = bash_native_redirect(&cmd) { return Ok(v); }
        Ok(Verdict::Allow)
    }
}

impl Observer for BashGuard {
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        if ctx.trigger != Some(Trigger::PostToolUse) { return; }
        // telemetry — never affects outcome, never panics
        let _ = emit_pr_event(/* ... */);
    }
}
```

## Enforcement mode in dispatcher (dispatch.rs)

```rust
fn apply_mode(verdict: Verdict, mode: Mode) -> Verdict {
    match (mode, verdict) {
        (Mode::Warn, Verdict::Deny { reason }) => Verdict::Warn { message: reason },
        (_, verdict) => verdict,
    }
}
```

Source: `apps/rt/src/hooks/bash_guard.rs`, `apps/rt/src/registry.rs`, `apps/rt/src/dispatch.rs`

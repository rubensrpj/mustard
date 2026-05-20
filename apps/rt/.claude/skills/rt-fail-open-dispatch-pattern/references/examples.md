<!-- mustard:generated at:2026-05-19T00:00:00.000Z role:general -->
# Examples: rt-fail-open-dispatch-pattern

## Fail-open module runner (dispatch.rs)

```rust
fn run_module(module: &Module, input: &HookInput, ctx: &Ctx, outcome: &mut Outcome) {
    if let Some(observer) = &module.observer {
        observer.observe(input, ctx); // fire-and-forget
    }
    let Some(check) = &module.check else { return; };
    let mode = registry::mode_for(module.id);
    if mode == Mode::Off { return; }
    // Err from evaluate → Allow (fail-open lives here, not in the module)
    let verdict = check.evaluate(input, ctx).unwrap_or(Verdict::Allow);
    outcome.fold(apply_mode(verdict, mode));
}
```

## Mode downgrade (dispatch.rs)

```rust
fn apply_mode(verdict: Verdict, mode: Mode) -> Verdict {
    match (mode, verdict) {
        (Mode::Warn, Verdict::Deny { reason }) => Verdict::Warn { message: reason },
        (_, verdict) => verdict,
    }
}
```

## Wire protocol output (main.rs hook_specific_output shape)

```rust
// Deny
{ "hookSpecificOutput": {
    "hookEventName": "PreToolUse",
    "permissionDecision": "deny",
    "permissionDecisionReason": "..."
}}

// Allow with advisory
{ "hookSpecificOutput": {
    "hookEventName": "PreToolUse",
    "permissionDecision": "allow",
    "additionalContext": "..."
}}

// Silent allow — no stdout at all
```

## stdin parse (main.rs)

```rust
fn read_stdin_input() -> HookInput {
    let mut buf = String::new();
    if std::io::stdin().read_to_string(&mut buf).is_err() {
        return HookInput::default(); // fail-open
    }
    serde_json::from_str(&buf).unwrap_or_default() // fail-open
}
```

## Unknown event / id — fail open

```rust
pub fn run_event(trigger: Option<Trigger>, input: &HookInput) -> Outcome {
    let Some(trigger) = trigger else { return Outcome::allow(); };
    // ...
}

pub fn run_check(id: &str, input: &HookInput) -> Outcome {
    let Some(module) = registry.by_id(id) else { return Outcome::allow(); };
    // ...
}
```

Source: `apps/rt/src/dispatch.rs`, `apps/rt/src/main.rs`, `apps/rt/src/protocol.rs`

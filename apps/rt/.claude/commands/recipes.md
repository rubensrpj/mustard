<!-- mustard:generated at:2026-05-19T00:00:00.000Z role:general -->
# Recipes: mustard-rt

Skeletons for the three most common extension tasks.

## Recipe A — Add a new enforcement module

1. Create `src/hooks/my_gate.rs`.
2. Implement `Check` (and/or `Observer`) for a unit struct.
3. Declare `pub mod my_gate;` in `src/hooks/mod.rs`.
4. Add one `Module { ... }` entry in `Registry::new()` inside `src/registry.rs`.

Skeleton (Check only):

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
        // ... decision logic ...
        Ok(Verdict::Allow)
    }
}
```

Registry entry:

```rust
Module {
    id: "my_gate",
    applies_to: &[(Trigger::PreToolUse, ToolMatch::Named("Bash"))],
    check: Some(Box::new(MyGate)),
    observer: None,
},
```

Ref: `src/registry.rs`, `src/hooks/bash_guard.rs` (full example)

## Recipe B — Add a new run subcommand

1. Create `src/run/my_script.rs` with `pub fn run(arg: &str) { ... }`.
2. Declare `mod my_script;` in `src/run/mod.rs`.
3. Add a variant to `RunCmd` in `src/run/mod.rs`.
4. Add the dispatch arm in `run::dispatch`.

```rust
// RunCmd variant
MyScript {
    #[arg(long)]
    target: String,
},

// dispatch arm
RunCmd::MyScript { target } => my_script::run(&target),
```

Ref: `src/run/mod.rs`, `src/run/emit_event.rs` (simple example)

## Recipe C — Emit a harness event from a run subcommand

```rust
use crate::run::env::{project_dir, session_id};
use crate::util::now_iso8601;
use mustard_core::io::event_store::EventSink;
use mustard_core::io::sqlite_store::SqliteEventStore;
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use serde_json::json;

let event = HarnessEvent {
    v: SCHEMA_VERSION,
    ts: now_iso8601(),
    session_id: session_id().unwrap_or_else(|| "unknown".into()),
    wave: 0,
    actor: Actor { kind: ActorKind::Hook, id: Some("my-script".into()), actor_type: None },
    event: "my.event".into(),
    payload: json!({ "key": "value" }),
    spec: None,
};
let _ = SqliteEventStore::for_project(&project_dir())
    .and_then(|store| store.append(&event));
```

Ref: `src/run/emit_event.rs`, `src/hooks/bash_guard.rs::emit_commit_gate_event`

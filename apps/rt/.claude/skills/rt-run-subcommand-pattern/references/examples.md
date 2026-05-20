<!-- mustard:generated at:2026-05-19T00:00:00.000Z role:general -->
# Examples: rt-run-subcommand-pattern

## Minimal run subcommand (emit_event shape)

```rust
// src/run/my_script.rs
use crate::run::env::{project_dir, session_id};
use crate::util::now_iso8601;
use mustard_core::io::event_store::EventSink;
use mustard_core::io::sqlite_store::SqliteEventStore;
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use serde_json::json;

pub fn run(event_name: Option<&str>, payload_args: &[String]) {
    let Some(event) = event_name.filter(|e| !e.is_empty()) else {
        eprintln!("Usage: my-script --event <name>");
        return;
    };
    let harness_event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id().unwrap_or_else(|| "unknown".into()),
        wave: 0,
        actor: Actor { kind: ActorKind::Hook, id: Some("my-script".into()), actor_type: None },
        event: event.to_string(),
        payload: json!({}),
        spec: None,
    };
    let _ = SqliteEventStore::for_project(&project_dir())
        .and_then(|store| store.append(&harness_event));
    println!("{{\"ok\":true}}");
}
```

## RunCmd variant + dispatch (src/run/mod.rs shape)

```rust
// In RunCmd enum:
MyScript {
    #[arg(long)]
    event: Option<String>,
    #[arg(long = "payload")]
    payload: Vec<String>,
},

// In dispatch():
RunCmd::MyScript { event, payload } => my_script::run(event.as_deref(), &payload),
```

## JSON output with serde (sync_detect shape)

```rust
#[derive(Debug, serde::Serialize)]
struct MyOutput {
    #[serde(rename = "subprojects")]
    items: Vec<Item>,
    #[serde(rename = "hashChanged")]
    hash_changed: bool,
}

let output = MyOutput { items: vec![], hash_changed: false };
println!("{}", serde_json::to_string_pretty(&output).unwrap_or_default());
```

## Windows/POSIX subprocess dispatch

```rust
let mut cmd = if cfg!(windows) {
    let mut c = std::process::Command::new("cmd");
    c.args(["/C", shell_command]);
    c
} else {
    let mut c = std::process::Command::new("sh");
    c.args(["-c", shell_command]);
    c
};
```

Source: `apps/rt/src/run/emit_event.rs`, `apps/rt/src/run/mod.rs`, `apps/rt/src/run/sync_detect.rs`, `apps/rt/src/hooks/bash_guard.rs`

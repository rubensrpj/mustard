<!-- mustard:generated at:2026-05-19T00:00:00.000Z role:general -->
# Patterns: mustard-rt

## Pattern 1 — Fail-open dispatcher

Central fail-open lives in `dispatch.rs::run_module`, never inside a module. A `Check` returning `Err` degrades to `Allow`. The process always exits `0`.

| Step | Location |
|---|---|
| stdin parse failure → `HookInput::default()` | `main.rs::read_stdin_input` |
| unknown event → `Outcome::allow()` | `dispatch.rs::run_event` |
| `Check::evaluate` → `Err` → `Allow` | `dispatch.rs::run_module` |
| stdout write failure → ignored | `main.rs::emit_outcome` |

Ref: `src/dispatch.rs`, `src/main.rs`

## Pattern 2 — Module registration (Open/Closed)

Adding a gate means adding one `Module { id, applies_to, check, observer }` entry in `registry.rs::Registry::new()`. The dispatcher reads the registry; it never changes.

```
Module {
    id: "my_gate",
    applies_to: &[(Trigger::PreToolUse, ToolMatch::Named("Bash"))],
    check: Some(Box::new(MyGate)),
    observer: None,
}
```

Ref: `src/registry.rs`

## Pattern 3 — Check + Observer on one struct

A module can implement both `Check` (gate, returns `Verdict`) and `Observer` (telemetry, returns `()`). Example: `BashGuard` is PreToolUse(Bash) gate **and** PostToolUse(Bash) DORA emitter.

Ref: `src/hooks/bash_guard.rs`

## Pattern 4 — Enforcement mode (`Mode::Off/Warn/Strict`)

`mode_for(id)` defaults to `Mode::Strict`. `apply_mode` downgrades `Deny → Warn` when mode is `Warn`. Modules never read their own mode — the dispatcher applies it.

Exception: `review-gate` inside `bash_guard` reads `MUSTARD_COMMIT_GATE_MODE` independently (default `warn`, not `strict`).

Ref: `src/dispatch.rs::apply_mode`, `src/hooks/bash_guard.rs::commit_gate_mode`

## Pattern 5 — Run-face subcommand pattern

Each `run` subcommand is a module under `src/run/`. It exports a `pub fn run(...)` that takes typed arguments (never reads stdin), prints its own output, and returns `()`.

```rust
// src/run/emit_event.rs
pub fn run(event: Option<&str>, payload: &[String], spec: Option<&str>, wave: u32) { ... }
```

Ref: `src/run/mod.rs` (dispatch table), `src/run/emit_event.rs` (example)

## Pattern 6 — HarnessEvent emission (best-effort telemetry)

All telemetry uses `SqliteEventStore::for_project(project_dir).and_then(|store| store.append(&event))` with the result discarded via `let _ = ...`. Telemetry is never load-bearing.

```rust
let _ = SqliteEventStore::for_project(project_dir)
    .and_then(|store| store.append(&event));
```

Ref: `src/hooks/bash_guard.rs::emit_commit_gate_event`, `src/run/emit_event.rs`

## Pattern 7 — Subproject SHA-256 change detection

`sync-detect` computes a SHA-256 over source files + manifest files per subproject. The hash is compared against a cached value; `hashChanged: true` triggers recompilation. The JS script's module-hash map is always emitted as `{}` (fine-grained later).

Ref: `src/run/sync_detect.rs`, `src/util/sha256.rs`

## Pattern 8 — Gate message format

All human-readable denial reasons use `format_gate_message(gate, what, why, exit)` → `[Gate] what. why. Saída: exit.`

Ref: `src/util/mod.rs::format_gate_message`

## Pattern 9 — Windows / POSIX shell dispatch

Subprocess calls use `cmd /C` on Windows, `sh -c` elsewhere via `cfg!(windows)`. Applies to build commands in `review-gate` and AC commands in `qa-run`.

Ref: `src/hooks/bash_guard.rs::run_build`

## Pattern 10 — OTEL / SQLite store (Wave 6)

`otel-collector` opens `.claude/.harness/mustard.db` via `rusqlite` (bundled). `tiny_http` handles the OTLP/JSON receive loop synchronously (no tokio). The MCP face gets its own `current_thread` tokio runtime so the enforcement path stays synchronous.

Ref: `src/run/otel/`, `src/mcp/mod.rs`

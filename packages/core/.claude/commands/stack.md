<!-- mustard:generated at:2026-05-19T00:00:00Z role:general -->
# Stack — mustard-core

## Crate identity

| Field | Value |
|---|---|
| Crate name | `mustard-core` |
| Rust edition | 2024 |
| Min Rust | 1.85 |
| Publish | false (internal) |

## Dependencies

| Crate | Purpose |
|---|---|
| `serde` + `serde_json` | (De)serialization of all model types and metric lines |
| `thiserror` | Typed error enum via `#[derive(thiserror::Error)]` |
| `rusqlite` | SQLite WAL store; `bundled` feature ships SQLite itself |

## Dev dependencies

| Crate | Purpose |
|---|---|
| `insta` | Snapshot assertions in integration/parity tests |
| `tempfile` | Temporary directories for IO-layer round-trip tests |

## Module map

| Module | Kind | Description |
|---|---|---|
| `model::contract` | Pure data | Hook contract: `HookInput`, `Verdict`, `Outcome`, `Trigger`, `Check`/`Observer` traits |
| `model::event` | Pure data | `HarnessEvent` / `HookEvent` — one row of the event store |
| `model::pipeline` | Pure data | `PipelineState`, `Phase`, `Scope`, `Task` |
| `error` | Cross-cutting | Typed `Error` enum + `fail_open` / `fail_open_with` helpers |
| `config` | Cross-cutting | `EnforcementConfig`, `Mode` — per-check enforcement modes |
| `env` | Cross-cutting | Port of `hook-env.js`: `should_run`, `acquire_guard`, `check_depth`, `guarded_run`, `Env` trait |
| `metrics` | Cross-cutting | Port of `metrics-emit.js`: `MetricLine`, `emit_metric` |
| `knowledge` | Cross-cutting | Port of `knowledge-extract.js`: friction extraction, `ContextSelector` trait |
| `io::event_store` | IO (trait) | `EventSink` trait |
| `io::sqlite_store` | IO (impl) | `SqliteEventStore` — WAL-mode SQLite, implements `EventSink` |
| `io::pipeline_repo` | IO (trait+impl) | `PipelineRepo` trait + `FsPipelineRepo` |
| `io::fs` | IO (primitives) | `write_atomic`, `append_line`, `read_to_string`, `exists` |

## Build commands

```bash
cargo build -p mustard-core
cargo test  -p mustard-core
cargo clippy -p mustard-core
```

Ref: `packages/core/Cargo.toml`

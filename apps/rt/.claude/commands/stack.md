<!-- mustard:generated at:2026-05-19T00:00:00.000Z role:general -->
# Stack: mustard-rt

## Language & edition

| Field | Value |
|---|---|
| Language | Rust (edition 2024, rust-version 1.85) |
| Crate name | `mustard-rt` |
| Binary | `mustard-rt` (single self-contained binary) |

## Dependencies

| Crate | Purpose |
|---|---|
| `mustard-core` | Shared contracts — `Check`, `Observer`, `Verdict`, `Outcome`, `HookInput`, `HarnessEvent`, `SqliteEventStore` |
| `serde` + `serde_json` | JSON (de)serialisation — harness protocol + run-face output |
| `clap` | CLI argument parsing for the four binary faces |
| `tiny_http` | Minimal blocking HTTP server (OTEL collector; no tokio on this path) |
| `rusqlite` (bundled, v0.31) | SQLite embedded in the binary — harness event bus + OTEL store |
| `rmcp` | Rust MCP SDK — JSON-RPC stdio transport for the `mcp` face |
| `tokio` (current_thread) | Async runtime required by `rmcp`; scoped to `mcp` face only |
| `tempfile` (dev) | Throwaway git repos in `bash_guard` integration tests |

## Workspace lints

`pedantic = warn`, `unwrap_used = deny`, `unsafe_code = forbid`. Tests carve out `unwrap`/`expect` with `#[cfg_attr(test, allow(...))]`.

## Build & test commands

```bash
cargo build -p mustard-rt
cargo test  -p mustard-rt
cargo run   -p mustard-rt -- run sync-detect
cargo run   -p mustard-rt -- run scan-orchestrate
cargo run   -p mustard-rt -- run sync-registry
```

Ref: `apps/rt/Cargo.toml`

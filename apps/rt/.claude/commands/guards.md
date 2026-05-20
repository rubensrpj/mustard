<!-- mustard:generated at:2026-05-19T00:00:00.000Z role:general -->
# Guards: mustard-rt

## Fail-open

- DO always return `Ok(Verdict::Allow)` on unexpected input — never `Err` on non-fatal paths.
- DO own the fail-open in `dispatch.rs`, not inside modules.
- DON'T exit with a non-zero code from any hook path — `deny` is signalled in the JSON body.
- DON'T panic in non-test code — `unwrap_used` and `expect_used` are `deny` workspace-wide.

## Module registration

- DO add a module by appending to `Registry::new()` in `registry.rs`.
- DON'T modify the dispatcher (`dispatch.rs`) when adding a new module — it is Open/Closed.
- DO use `ToolMatch::Named` for tool-specific gates; `ToolMatch::Any` only for wildcard lifecycle hooks.
- DON'T register the same concern in two separate modules — consolidate into one family.

## Telemetry (Observer)

- DO discard the result of `store.append(...)` with `let _ = ...`.
- DON'T block or return a verdict from an `Observer` — it returns `()`.
- DON'T let a telemetry failure propagate to the caller — swallow all errors inside `observe`.

## Run-face subcommands

- DO have each `run` subcommand export exactly `pub fn run(...)` and return `()`.
- DON'T read stdin in a `run` subcommand — use `clap` args only.
- DO emit byte-stable JSON to stdout — the pipeline parser is the downstream consumer.
- DON'T break the `sync-detect` / `sync-registry` JSON schema — field order and names must match the JS originals.

## Subprocess calls

- DO dispatch build/AC commands via `cmd /C` on Windows and `sh -c` on POSIX.
- DON'T assume a shell binary is present — spawn failures are `env_error` and fail-open.
- DO apply a timeout to any spawned build command (5 min for `review-gate`).

## SQLite / OTEL

- DO pin `rusqlite` to `0.31` — the dashboard's `src-tauri` uses the same version to share `libsqlite3-sys`.
- DON'T add a second async runtime to the binary — only the `mcp` face gets tokio, and only a `current_thread` one.
- DO use `include_str!` for assets the binary must read (e.g. scan agent-prompt template) — no hard file dependencies at runtime.

## Code style

- DO write all code, comments, and doc-comments in English.
- DON'T translate existing legacy code unless it is the focus of the edit (surgical edits only).
- DO use `#[must_use]` on pure functions returning values.
- DON'T use `unsafe` — `#![forbid(unsafe_code)]` is enforced.

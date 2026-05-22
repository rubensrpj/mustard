---
name: core-fail-open-error
description: "Fail-open error handling pattern for hook-safe Rust code. Use when writing a function that touches the filesystem or environment from a hook context, adding a new error variant to the crate error enum, deciding whether to propagate or swallow an error, or porting a JS try/catch swallow block. Even if the user just says 'hooks must not crash' or 'fail silently on missing file'."
source: scan
---
<!-- mustard:generated at:2026-05-19T00:00:00Z role:general -->

## Convention

- Every fallible library function returns `crate::error::Result<T>` — never panics.
- `Error::NotFound(String)` is separate from `Error::Io` — callers can treat a missing file as "empty" without swallowing real I/O failures.
- `fail_open(result, fallback)` / `fail_open_with(result, || fallback)` are the helpers for call sites where an error should degrade silently.
- Functions that are inherently fail-silent (e.g. telemetry) return `bool` or `()` and collapse `Result` internally.
- The `#[non_exhaustive]` attribute on `Error` lets later waves add variants without breaking downstream `match` arms (consumers keep a wildcard arm).
- Builder constructors on `Error` (`Error::config(msg)`, `Error::env(msg)`, etc.) accept any `Into<String>` — no `.to_string()` at call sites.

## Real examples in this codebase

- Error enum and helpers: `packages/core/src/error.rs`
- Fail-silent emit: `packages/core/src/metrics.rs` — `emit_metric` returns `bool`
- Config fail-open: `packages/core/src/config.rs` — `EnforcementConfig::resolve` swallows a bad file
- NotFound vs Io: `packages/core/src/fs/real.rs` — `map_io` maps `NotFound` separately
- Pipeline repo fail-open helper: `packages/core/src/io/pipeline_repo.rs` — `read_optional`

## References

See `packages/core/.claude/skills/core-fail-open-error/references/examples.md` for verbatim code excerpts.

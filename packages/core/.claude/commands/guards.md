<!-- mustard:generated at:2026-05-19T00:00:00Z role:general -->
# Guards — mustard-core

## Library boundaries

- DO keep this crate free of CLI/hook-specific logic. That belongs in `mustard-cli` / `mustard-rt`.
- DO NOT add `main.rs`, binary targets, or side effects at import time.
- DO build the full workspace (`cargo build`) after touching any publicly exported item — both consumer crates depend on this crate.

## Fail-open

- DO return `Result<T>` from every fallible operation. Never panic in library code.
- DO use `fail_open` / `fail_open_with` at call sites where an error should degrade silently.
- DO keep `Error::NotFound` distinct from `Error::Io` so callers can treat a missing file as "empty" without swallowing real failures.
- DO NOT propagate config parse errors from `EnforcementConfig::resolve` — a typo in `mustard.json` must never block a hook.

## Unsafe code

- DO NOT use unsafe code. `#![forbid(unsafe_code)]` is active crate-wide.
- DO use the thread-local overlay in `ProcessEnv` instead of `std::env::set_var` (unsafe since Rust 2024).

## Serde models

- DO use `#[serde(flatten)] pub raw: Value` on boundary structs that accept external JSON.
- DO mark extensible enums `#[non_exhaustive]`. Only use a closed enum when the vocabulary is complete (e.g. `Mode`).
- DO NOT use `#[serde(deny_unknown_fields)]` on boundary types — the harness adds fields.

## Traits

- DO program against traits (`EventSink`, `PipelineRepo`, `ContextSelector`, `Env`) not concrete types in hooks and tests.
- DO NOT implement side effects (I/O, env mutation) inside trait `evaluate` / `observe` implementations in the `model` layer — side effects belong in `io`.
- DO make `Observer::observe` infallible from the dispatcher's view (swallow errors internally).

## Testing

- DO use `MapEnv` and in-memory fakes for all env/IO tests — no real filesystem, no real process env.
- DO allow `clippy::unwrap_used` and `clippy::expect_used` in `#[cfg(test)]` only. Production code must never `.unwrap()`.
- DO use `tempfile::tempdir()` for any test that genuinely needs a filesystem.

## Code language

- DO write all code, comments, and doc-comments in English. `Lang` in spec frontmatter does not affect code.
- DO NOT translate legacy comments — surgical edits only.

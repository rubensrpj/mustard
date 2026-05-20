---
name: core-trait-backed-io
description: "Trait-backed IO with in-memory test fakes. Use when adding a new IO capability to mustard-core, writing a hook that needs an injectable store, implementing a test for filesystem or DB logic, or wiring a new EventSink / PipelineRepo consumer. Even if the user just says 'add an event store' or 'make the repo testable'."
source: scan
---
<!-- mustard:generated at:2026-05-19T00:00:00Z role:general -->

## Convention

- Every side-effecting capability is a **trait** first. The concrete struct (FS or SQLite) is a separate impl.
- Trait methods return `Result<T>` (using `crate::error::Result`). Implementations never panic.
- An in-memory fake for the trait lives inside `#[cfg(test)]` in the same file. It uses `RefCell<Vec<_>>` (single-threaded) or `Mutex<HashMap<_,_>>` (when Send is needed).
- The production struct holds a `PathBuf` root; `for_project(dir)` is the standard constructor.
- Atomic writes use `io::fs::write_atomic` — a rename guarantees readers never see a torn file.
- Missing files are `Error::NotFound`, not `Error::Io` — callers can fail-open on absence without swallowing real failures.

## Real examples in this codebase

- Trait definition: `packages/core/src/io/event_store.rs` — `EventSink`
- Trait + FS implementation: `packages/core/src/io/pipeline_repo.rs` — `PipelineRepo` / `FsPipelineRepo`
- Env abstraction: `packages/core/src/env.rs` — `Env` trait, `ProcessEnv`, `MapEnv`
- Knowledge selector: `packages/core/src/knowledge.rs` — `ContextSelector`, `PassthroughSelector`
- Atomic write primitive: `packages/core/src/io/fs.rs` — `write_atomic`, `append_line`

## References

See `packages/core/.claude/skills/core-trait-backed-io/references/examples.md` for verbatim code excerpts.

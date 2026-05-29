<!-- mustard:generated at:2026-05-29T00:00:00Z role:general -->
# Guards — mustard-core

DO / DON'T rules. Code examples live in `patterns.md`.

## Safety / fail-open

| | Rule |
|---|---|
| DO | Return `Result<T, Error>` from every fallible op; readers return `None` / empty on failure. |
| DO | Keep `Error::NotFound` distinct from `Error::Io` so callers can treat absence as "empty". |
| DO | Use `fail_open` / `fail_open_with` where the JS hooks wrapped a body in `try { … } catch (_) {}`. |
| DON'T | Call `.unwrap()` / `.expect()` outside `#[cfg(test)]` — `clippy::unwrap_used` is `deny`. |
| DON'T | Add `unsafe` — the crate root is `#![forbid(unsafe_code)]`. |
| DON'T | Let telemetry be load-bearing — a missing/corrupt event file must never abort a projection. |

## Projections (`view::projection`)

| | Rule |
|---|---|
| DO | Make every `project_*` total (always returns a view) and deterministic (same input → same output). |
| DO | Keep projections IO-free — fold over the supplied `&[HarnessEvent]` only. |
| DO | Read `payload` defensively (`get(...).and_then(Value::as_str)`) with safe defaults. |
| DON'T | Read the filesystem, touch the event store, or call `now()` inside a projection — pass `now_ms` in. |
| DON'T | Use `HashMap` when output order is observable; use `BTreeMap`/`BTreeSet` for determinism. |

## Serde boundary types

| | Rule |
|---|---|
| DO | Add `#[serde(flatten)] pub raw: Value` to any type parsed from harness/on-disk JSON. |
| DO | Give every field `#[serde(default)]` so partial / future JSON still deserializes. |
| DO | Mark boundary enums `#[non_exhaustive]` and keep a wildcard `match` arm downstream. |
| DON'T | Use `#[serde(deny_unknown_fields)]` on a boundary type — it rejects new harness fields. |

## Filesystem (`io::fs`)

| | Rule |
|---|---|
| DO | Route all filesystem access through `io::fs` (free fns by default). |
| DO | Use `write_atomic` for any write that must not tear on crash. |
| DO | Inject `&dyn Fs` (with `FakeFs`) only for hot paths / logic-heavy code under unit test. |
| DON'T | Call `std::fs::*` directly in crate code (the lone exception is `io::fs::real`). |
| DON'T | Virally thread a `&dyn Fs` port through leaf helpers — that is pure ceremony. |

## Crate API stability

| | Rule |
|---|---|
| DO | Add new root re-exports in `src/lib.rs` so consumers keep `use mustard_core::…`. |
| DO | Declare dependency versions once in the workspace `[workspace.dependencies]`; reference with `{ workspace = true }`. |
| DON'T | Reshape the frozen hook contract (`model::contract`) — extend additively via `#[non_exhaustive]`. |
| DON'T | Read or write spec lifecycle metadata from the `.md` body — it lives in the `meta.json` sidecar. |
| DON'T | Read `.claude/entity-registry.json` directly (~half a MB); use `mustard-rt run registry-query`. |

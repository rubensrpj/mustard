<!-- mustard:generated at:2026-05-19T00:00:00Z role:general -->
# Patterns — mustard-core

## 1. Trait-backed IO (Dependency Inversion)

Every side-effecting capability is exposed behind a trait. The concrete FS/SQLite
implementation is a separate struct. Tests inject an in-memory fake — no real
filesystem needed.

| Trait | Production impl | Test fake technique |
|---|---|---|
| `EventSink` | `SqliteEventStore` | Struct with `RefCell<Vec<_>>` |
| `PipelineRepo` | `FsPipelineRepo` | Struct with `Mutex<HashMap<_,_>>` |
| `ContextSelector` | `PassthroughSelector` | Closure or custom struct |
| `Env` | `ProcessEnv` | `MapEnv::new().with(k, v)` |

All trait methods return `Result<_>` or `()`. Implementations never panic.

Ref: `packages/core/src/io/event_store.rs`, `packages/core/src/io/pipeline_repo.rs`,
     `packages/core/src/env.rs`, `packages/core/src/knowledge.rs`

---

## 2. Fail-open error handling

The rule: hooks must never crash. Every fallible path returns `Result`; callers
use `fail_open` / `fail_open_with` to degrade to a safe default. `Error::NotFound`
is kept distinct from `Error::Io` so an absent file can be treated as "empty"
without swallowing real failures.

Pattern:

- Trait method returns `Result<T>`.
- Caller wraps with `fail_open(result, default)` where dropping the error is safe.
- Only genuine structural failures (`Error::Config`, `Error::InvalidInput`) bubble up.
- `emit_metric` returns `bool` (not `Result`) — fail-silent at the outermost call site.

Ref: `packages/core/src/error.rs`, `packages/core/src/metrics.rs`

---

## 3. Lenient serde model with raw catch-all

Boundary structs that accept external JSON (harness stdin, pipeline-state files)
use `#[serde(flatten)] pub raw: Value` to absorb unknown fields. Known fields are
typed for ergonomic access; new harness fields land in `raw` without a crate
release.

Enums and types that are expected to grow use `#[non_exhaustive]`. Types that are
the closed, complete vocabulary (e.g. `Mode`) do not.

| Struct | Lenient | Why |
|---|---|---|
| `HookInput` | `#[serde(flatten)] raw: Value` | Harness adds fields over time |
| `PipelineState` | `#[serde(flatten)] raw: Value` | Pipeline-states vary by pipeline type |
| `HarnessEvent` | `payload: Value` (event-specific) | New event names must never break deserialization |

Ref: `packages/core/src/model/contract.rs`, `packages/core/src/model/pipeline.rs`,
     `packages/core/src/model/event.rs`

---

## 4. Builder pattern on pure-value structs

Cross-cutting structs that have many optional fields expose a builder API via
`#[must_use]` chaining methods. The struct implements `Default`; the builder
methods return `self`. No separate builder type needed.

| Struct | Builder methods |
|---|---|
| `MetricLine` | `.tokens_affected(n)`, `.tokens_saved(n)`, `.note(s)`, `.extras(v)` |
| `EnforcementConfig` | `.with_check(name, mode)`, `.with_disabled(name)` |
| `MapEnv` | `.with(key, value)` |

All builder methods are `#[must_use]`; forgetting to use the return value is a
compiler warning.

Ref: `packages/core/src/metrics.rs`, `packages/core/src/config.rs`,
     `packages/core/src/env.rs`

---

## 5. Port parity comments

Every module ported from a JS file begins its doc-comment with the JS source
path (e.g. `hook-env.js`, `metrics-emit.js`, `knowledge-extract.js`). Functions
that differ from the JS in a deliberate, non-obvious way carry a `Parity note:`
inline comment. This lets reviewers verify fidelity without keeping both files
open.

Ref: `packages/core/src/env.rs`, `packages/core/src/metrics.rs`,
     `packages/core/src/knowledge.rs`

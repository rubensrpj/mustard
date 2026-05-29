<!-- mustard:generated at:2026-05-29T00:00:00Z role:general -->
# Patterns — mustard-core

The recurring conventions the crate is built from. Each maps to a granular skill (see `.claude/skills/`).

## 1. Pure projection fold (`project_*`) — skill `core-projection-fold`

One free function per ViewModel: `fn project_X(spec: &str, events: &[HarnessEvent]) -> XView`. Total (always returns *something*, never `None`/panic), deterministic (same input → same output), and IO-free. Production callers in `apps/rt` and `apps/dashboard` supply the slice via `view::projection::read_workspace_events` (the NDJSON walker).

```rust
#[must_use]
pub fn project_quality(spec_name: &str, events: &[HarnessEvent]) -> QualityRollup {
    let mut latest: BTreeMap<String, AcceptanceCriterion> = BTreeMap::new();
    for ev in events.iter()
        .filter(|e| e.spec.as_deref() == Some(spec_name))
        .filter(|e| e.event == "qa.result") { /* fold … */ }
    // … returns QualityRollup::empty() shape when no events
}
```

- Empty input → `XView::empty()` (or empty `Vec`), never an error. Ref: `src/view/projection/quality.rs:17`.
- Newer event wins per key via `BTreeMap::insert` (sorted + dedup for free). Ref: `quality.rs:64`.
- Payload reads are defensive: `payload.get(k).and_then(Value::as_str)` with safe defaults. Ref: `workspace.rs:69`.
- `BTreeMap`/`BTreeSet` over `HashMap` where output order must be deterministic. Ref: `workspace.rs:175`.

## 2. Lenient serde boundary type — skill `core-lenient-serde-model`

Any type deserialized from harness-controlled or on-disk JSON carries a `#[serde(flatten)] pub raw: Value` catch-all so a field added by a newer Mustard never breaks an older reader. Known fields are typed; everything else round-trips through `raw`.

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Meta {
    pub stage: Option<String>,
    // … six always-serialized lifecycle fields …
    #[serde(flatten)]
    pub raw: Value,
}
```

- Every required field has `#[serde(default)]` so partial JSON still parses. Ref: `src/domain/meta.rs:55`.
- Boundary enums use `#[non_exhaustive]` so new variants don't break downstream `match`. Ref: `src/domain/model/contract.rs:28`.
- Used by: `Meta` (`meta.rs`), `HookInput` (`contract.rs`), `Event` (`io/events/types.rs:17`), pipeline types (`model/pipeline.rs`).

## 3. Filesystem port + free functions — skill `core-fs-port`

`io::fs` is the single seam for all `std::fs` access. An object-safe `Fs` trait (Dependency Inversion) with `RealFs` (production) and `FakeFs` (tests), plus module-level free functions that mirror the `std::fs` surface and delegate to a process-wide `const RealFs`.

```rust
pub trait Fs {
    fn read_to_string(&self, path: &Path) -> Result<String>;
    fn write_atomic(&self, path: &Path, contents: &[u8]) -> Result<()>;
    fn append_line(&self, path: &Path, line: &str) -> Result<()>;
    // … exists / read_dir / create_dir_all / rename / canonicalize …
}
```

- Default: call the free fns (`fs::read_to_string`, `fs::write_atomic`). Inject `&dyn Fs` only where a unit test needs `FakeFs`. Ref: `src/io/fs/mod.rs:23`.
- Writes are atomic (sibling tempfile + fsync + rename) — a crash never leaves a torn file. Ref: `fs/mod.rs:91`.
- A missing file on read is `Error::NotFound`, distinct from `Error::Io`, so callers fail open on absence. Ref: `fs/mod.rs:33`.

## 4. Fail-open error handling — skill `core-fail-open`

Hooks must never crash. Every fallible op returns `Result<T, Error>`; the `fail_open` / `fail_open_with` helpers collapse a `Result` to a fallback value, and readers return `None`/empty on any failure.

```rust
pub fn fail_open<T>(result: Result<T>, fallback: T) -> T {
    result.unwrap_or(fallback)
}

#[must_use]
pub fn read_meta(path: &Path) -> Option<Meta> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str::<Meta>(&text).ok()  // unparseable → None
}
```

- `Error` is `#[non_exhaustive]`; `NotFound` is split out from `Io`. Ref: `src/platform/error.rs:28`.
- `#[forbid(unsafe_code)]` + `clippy::unwrap_used = deny` (except `cfg(test)`). Ref: `src/lib.rs:1`.
- Telemetry is never load-bearing: the NDJSON walker silently skips unreadable files / malformed lines. Ref: `src/view/projection/mod.rs:83`.

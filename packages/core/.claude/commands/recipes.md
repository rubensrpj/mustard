<!-- mustard:generated at:2026-05-19T00:00:00Z role:general -->
# Recipes — mustard-core

## Add a new IO trait + FS implementation

1. Create `src/io/<name>.rs`.
2. Declare a trait with all methods returning `Result<T>`.
3. Implement it on a struct that holds a `PathBuf` root.
4. Add an in-memory fake as a `#[cfg(test)]` struct implementing the same trait.
5. Re-export via `src/io/mod.rs`: `pub mod <name>;`.

Complexity: Medium. Ref: `packages/core/src/io/pipeline_repo.rs`

---

## Add a new model type

1. Choose the submodule: `model::contract` (hook contract), `model::event` (event schema), `model::pipeline` (pipeline state), or create a new submodule.
2. Derive `Debug, Clone, PartialEq, Serialize, Deserialize`.
3. If the type accepts external JSON, add `#[serde(flatten)] pub raw: Value` to absorb unknowns.
4. If the type may grow new variants, mark it `#[non_exhaustive]`.
5. Re-export from `model/mod.rs` if it is public.

Complexity: Simple. Ref: `packages/core/src/model/event.rs`

---

## Port a JS `hook-env.js`-style module

1. Start the module doc-comment with the JS source path.
2. Add an `Env` generic parameter (`<E: Env>`) to every function that reads/writes env vars.
3. In `ProcessEnv`, use the thread-local overlay for any `set` — do not call `std::env::set_var`.
4. Add a `MapEnv`-based test for every parity case. Add a `Parity note:` comment where the Rust behaviour differs from the JS.

Complexity: Medium. Ref: `packages/core/src/env.rs`

---

## Add a new `EnforcementConfig` check

1. Pick a kebab-case check name (e.g. `"spec-size"`).
2. Pass it to `EnforcementConfig::resolve` in the `checks` slice.
3. The environment variable is derived automatically: `MUSTARD_SPEC_SIZE_MODE`.
4. Call `config.mode_of("spec-size")` to read the resolved mode.

Complexity: Simple. Ref: `packages/core/src/config.rs`

---

## Emit a metric from a hook

1. Build a `MetricLine` using the builder:

   `MetricLine::new(ts, "my-event").tokens_saved(n).note("blocked")`

2. Call `emit_metric(cwd, &line)` — the return value (`bool`) can be ignored.
3. The shard file is `<cwd>/.claude/.metrics/my-event.jsonl`.

Complexity: Simple. Ref: `packages/core/src/metrics.rs`

---

## Implement a custom `ContextSelector`

1. Create a struct.
2. Implement `ContextSelector::select(&self, request, candidates) -> Vec<ContextItem>`.
3. Filter `candidates` by `request.agent`, `request.phase`, or `item.tags`.
4. Return only the relevant slice — that is the token saving.

Complexity: Simple. Ref: `packages/core/src/knowledge.rs` (`KindFilter` test example)

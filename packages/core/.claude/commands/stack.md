<!-- mustard:generated at:2026-05-29T00:00:00Z role:general -->
# Stack — mustard-core

Shared foundation crate (`mustard-core`) for the Mustard hook/script/CLI Rust migration. Pure-Rust library: event model, hook contract, pipeline-state types, the filesystem seam, and pure projections. No binary, `publish = false`.

## Language / edition

| Item | Value | Ref |
|---|---|---|
| Crate | `mustard-core` v0.1.0 | `Cargo.toml` |
| Edition | 2024 | `Cargo.toml` |
| MSRV | 1.85 | `Cargo.toml` |
| `unsafe_code` | `forbid` (crate root) | `src/lib.rs:1` |
| `clippy::unwrap_used` | `deny` workspace-wide (allowed only under `cfg(test)`) | `src/lib.rs:7` |
| Lints | inherit workspace (`pedantic = warn`) | `Cargo.toml` `[lints] workspace = true` |

## Dependencies (declared via workspace table)

| Crate | Version | Used by | Ref |
|---|---|---|---|
| serde / serde_json | workspace | every `model` / boundary type | `Cargo.toml` |
| thiserror | workspace | `platform::error::Error` | `src/platform/error.rs` |
| sha2 | 0.10.9 | `model::provenance::tree_checksum` | `Cargo.toml` |
| tiktoken-rs | workspace | `economy::estimator` (cl100k_base, ±5%) | `Cargo.toml` |
| aho-corasick | 1.1 | `vocabulary::VocabularyMatcher` | `src/domain/vocabulary/aho.rs` |
| toml | 0.8 | `vocabulary` (regression.toml) | `Cargo.toml` |
| tree-sitter (+loader) | 0.26 | `domain::ast` (agnostic AST) | `src/domain/ast/` |
| tree-sitter-{rust,typescript,python,go,java,c-sharp} | pinned | in-crate grammars | `Cargo.toml` |
| similar | 2 | `regression_check` textual fallback | `src/domain/regression_check/` |
| rayon | workspace, **optional** | `atomic_md::store::scan_dir` (>50 files) | `Cargo.toml` |
| ureq | workspace, **optional** | WASM grammar acquisition only | `src/domain/ast/wasm_acquire.rs` |

## Features

| Feature | Default | Effect | Ref |
|---|---|---|---|
| `wasm-grammars` | OFF | `tree-sitter/wasm` + `dep:ureq`; pulls wasmtime (heavy). Native + textual floor identical when off | `Cargo.toml:78` |

## Dev dependencies

| Crate | Use | Ref |
|---|---|---|
| insta | snapshot tests | workspace |
| tempfile | io round-trip / config tests | workspace |

## Commands

| Goal | Command | Notes |
|---|---|---|
| Build | `rtk cargo build -p mustard-core` | default features (no wasm) |
| Check | `rtk cargo check -p mustard-core` | |
| Test | `rtk cargo test -p mustard-core` | unit + `tests/*.rs` |
| Lint | `rtk cargo clippy -p mustard-core` | pedantic=warn, unwrap_used=deny |
| Feature build | `rtk cargo build -p mustard-core --features wasm-grammars` | acquires WASM grammars at runtime |

## Layer map

| Layer | Responsibility | Ref |
|---|---|---|
| `domain::model` | pure serde data types, zero side effects (event schema, hook contract, ViewModels) | `src/domain/model/` |
| `io::fs` | the single canonical filesystem seam (port + RealFs + FakeFs + free fns) | `src/io/fs/` |
| `io::events` | NDJSON `Event` / `EventReader` + workspace walker | `src/io/events/` |
| `view::projection` | pure folds over `&[HarnessEvent]` → one fn per ViewModel | `src/view/projection/` |
| `platform::error` | typed error + fail-open helpers | `src/platform/error.rs` |
| `domain` (cross-cut) | `config`, `meta`, `vocabulary`, `ast`, `economy`, `knowledge`, `skill` | `src/domain/` |
| `platform` (cross-cut) | `env`, `i18n`, `metrics`, `process`, `time` | `src/platform/` |

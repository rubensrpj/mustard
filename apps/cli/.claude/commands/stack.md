<!-- mustard:generated at:2026-05-19T00-00-00 role:general -->
# Stack: mustard-cli

| Dimension | Value |
|-----------|-------|
| Language | Rust (edition 2024, MSRV 1.85) |
| Crate name | `mustard-cli` |
| Binary | `mustard` |
| Version | 3.1.36 |
| Cargo workspace | root `Cargo.toml` (single source for deps/lints) |

## Dependencies

| Crate | Purpose |
|-------|---------|
| `clap` (workspace) | CLI argument parsing — derive API |
| `anyhow` (workspace) | Error propagation via `?` + context chaining |
| `serde` + `serde_json` (workspace) | JSON read/write for `mustard.json` / settings |
| `dialoguer` (workspace) | Interactive prompts (`Select`, `Confirm`, `Input`) |
| `ureq` (workspace) | HTTP client — Claude API calls + npm tarball fetch |
| `tar` + `flate2` (workspace) | `.tgz` extraction for `mustard add` |
| `zip` (workspace) | ZIP extraction for `mustard add` |
| `mustard-core` (path) | Shared library crate from `packages/core/` |
| `tempfile` (dev) | Temp dirs in unit tests |

## Workspace lint policy

`[workspace.lints]` in root `Cargo.toml` — `clippy::pedantic = warn`, `clippy::unwrap_used = deny`, `unsafe_code = forbid`. Test modules opt-out of `unwrap_used`/`expect_used` via `#![cfg_attr(test, allow(...))]`.

## Two faces (binary + library)

The crate ships both a `[[bin]]` (`mustard`) and a `[lib]` (`mustard_cli`). The library face is consumed by the Tauri dashboard backend (`apps/dashboard/`) so it can call `mustard_cli::init` / `mustard_cli::update` without spawning a sidecar.

Ref: `apps/cli/Cargo.toml`, `apps/cli/src/lib.rs`

//! `mustard-rt` library face — re-exports for integration tests.
//!
//! This crate is primarily a binary (`[[bin]]`). The `[lib]` target exists
//! solely so integration tests under `tests/` can import internal modules
//! without subprocess overhead. The lib crate must declare all modules that
//! `run/` submodules reference via `crate::` paths.
//!
//! Only `dead_code` is suppressed at crate level, and only because the
//! `hooks`/`report`/`registry`/`dispatch`/`mcp` modules are reached **only**
//! from the binary face (`main.rs`), so the lib build sees them as unused —
//! false positives inherent to this test-only re-export face. The real
//! dead-code signal is the bin build (`cargo build --bin mustard-rt`), which
//! declares the same modules without this allow. `unused_imports` /
//! `unused_variables` / `unused_mut` are NOT suppressed: the bin build already
//! keeps the module tree clean of them, so enforcing them in the lib build too
//! (covering `#[cfg(test)]` code the bin build skips) is free and catches
//! regressions.

#![forbid(unsafe_code)]
#![allow(dead_code)]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::float_cmp,
        clippy::len_zero,
        clippy::format_push_string,
        clippy::needless_range_loop,
    )
)]

pub mod commands;
pub mod shared;
pub mod util;
mod dispatch;
mod hooks;
mod mcp;
mod registry;
mod report;

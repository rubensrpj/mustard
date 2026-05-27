//! `mustard-rt` library face — re-exports for integration tests.
//!
//! This crate is primarily a binary (`[[bin]]`). The `[lib]` target exists
//! solely so integration tests under `tests/` can import internal modules
//! without subprocess overhead. The lib crate must declare all modules that
//! `run/` submodules reference via `crate::` paths. Dead-code warnings are
//! suppressed at crate level because the hooks/report/registry/dispatch
//! modules are used only from the binary face (`main.rs`).

#![forbid(unsafe_code)]
#![allow(dead_code, unused_imports, unused_variables, unused_mut)]
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

pub mod run;
pub mod util;
mod dispatch;
mod hooks;
mod mcp;
mod registry;
mod report;

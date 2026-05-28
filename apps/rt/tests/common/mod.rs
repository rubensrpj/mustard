//! Shared test fixtures for `mustard-rt` integration tests.
//!
//! ## `test_workspace()`
//!
//! Many integration tests need to run a `run` subcommand against a clean
//! workspace without leaking writes back into the on-disk `apps/rt/.claude/`
//! (which does not exist in this repository, and must never come into
//! existence as a side effect of `cargo test`). The legacy approach — `cd
//! tempdir; cargo test` — does not work inside a single Rust test binary
//! because `cargo test` runs every test in parallel inside the same process,
//! so a `set_current_dir` from one test races with every other test.
//!
//! [`test_workspace`] solves this by:
//!
//! 1. Creating a `tempfile::TempDir` and planting the workspace anchor
//!    (`mustard.json` + `.claude/`) inside it.
//! 2. Setting `MUSTARD_WORKSPACE_ROOT` to the tempdir path so
//!    [`mustard_core::io::workspace::workspace_root`] short-circuits there.
//! 3. Returning a [`TestWorkspace`] RAII guard that restores the previous
//!    value of `MUSTARD_WORKSPACE_ROOT` (or unsets it) when dropped.
//!
//! A process-wide [`Mutex`] (`ENV_LOCK`) serialises the env-var swap so two
//! tests cannot interleave their installs.
//!
//! ## Safety
//!
//! `std::env::set_var` is `unsafe` under Rust 2024 because environment
//! mutation is process-global and unsynchronised. The crate's `main.rs`
//! enables `#![forbid(unsafe_code)]`, but integration tests live in their own
//! crate root and are not bound by that lint. The mutex above is the
//! synchronisation that makes the call sound for the duration of the test.

use std::env;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use tempfile::TempDir;

/// Name of the env var consumed by `mustard_core::io::workspace::workspace_root`.
const OVERRIDE_ENV: &str = "MUSTARD_WORKSPACE_ROOT";

/// Process-wide mutex serialising env-var swaps.
fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// RAII guard returned by [`test_workspace`].
///
/// Holds the tempdir and a snapshot of the prior `MUSTARD_WORKSPACE_ROOT` so
/// it can restore the env when the guard is dropped.
pub struct TestWorkspace {
    /// Backing temporary directory. Kept alive for the lifetime of the guard
    /// so the workspace files remain accessible.
    _dir: TempDir,
    /// The resolved path of the tempdir (canonical when possible).
    path: PathBuf,
    /// The previous value of `MUSTARD_WORKSPACE_ROOT`, restored on drop.
    prior_override: Option<String>,
}

impl TestWorkspace {
    /// The path to the workspace root the fixture installed.
    #[allow(dead_code)]
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for TestWorkspace {
    fn drop(&mut self) {
        let _guard = env_lock().lock();
        // SAFETY: process-global env mutation is serialised through `env_lock()`.
        unsafe {
            if let Some(prev) = self.prior_override.take() {
                env::set_var(OVERRIDE_ENV, prev);
            } else {
                env::remove_var(OVERRIDE_ENV);
            }
        }
    }
}

/// Build a tempdir-backed workspace with `MUSTARD_WORKSPACE_ROOT` pointing at
/// it. Returns a guard that restores the prior env on drop.
///
/// Panics on tempdir failure — tests should fail loudly here.
#[allow(dead_code)]
pub fn test_workspace() -> TestWorkspace {
    let dir = tempfile::tempdir().expect("tempdir creation");
    let path = dir.path().to_path_buf();
    std::fs::write(path.join("mustard.json"), b"{}").expect("plant mustard.json");
    std::fs::create_dir_all(path.join(".claude")).expect("plant .claude/");

    let _guard = env_lock().lock();
    let prior = env::var(OVERRIDE_ENV).ok();
    // SAFETY: process-global env mutation is serialised through `env_lock()`.
    unsafe {
        env::set_var(OVERRIDE_ENV, &path);
    }
    TestWorkspace {
        _dir: dir,
        path,
        prior_override: prior,
    }
}

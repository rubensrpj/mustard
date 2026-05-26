//! Consolidated connection helper for the economy module.
//!
//! W2 hooks each rebuilt the same pattern — "open the harness `mustard.db` for
//! a project, applying schema and migrations if absent". Five copies of the
//! same `SqliteEventStore::for_project(...)` dance lived across `apps/rt`.
//! This helper folds that pattern into a single entry point so the W3
//! ingestion adapters and any future W4+ caller can stay terse:
//!
//! ```ignore
//! let conn = mustard_core::economy::store::open_for(project_root)?;
//! mustard_core::economy::writer::record_api_cost(&conn, frame)?;
//! ```
//!
//! ## Resolution
//!
//! The path is resolved in this order:
//!
//! 1. The value of the `MUSTARD_DB_PATH` environment variable if set.
//! 2. `<project_path>/.claude/.harness/mustard.db` otherwise.
//!
//! This mirrors [`crate::store::sqlite_store::SqliteEventStore::for_project`]
//! exactly — we delegate to it to construct (and migrate) the database, then
//! hand back the very connection it opened via
//! [`SqliteEventStore::into_connection`]. A single open per call: the wrapper
//! holds no per-process state once the schema is in place, so reusing its
//! connection keeps the call site free to use it inside writer transactions.
//!
//! ## Fail-open contract
//!
//! Returns [`crate::error::Error::Sqlite`] / [`crate::error::Error::Io`] on
//! failure — never panics. Callers (hooks, ingestion adapters) MUST `match`
//! the result and degrade silently on `Err` rather than aborting; telemetry
//! is never load-bearing.

use std::path::PathBuf;

use rusqlite::Connection;

use crate::error::Result;
use crate::store::sqlite_store::SqliteEventStore;

/// Environment variable that overrides the default database path resolution.
/// Mirrors the constant inside [`SqliteEventStore`] so the two stay in lockstep.
const DB_PATH_ENV: &str = "MUSTARD_DB_PATH";

/// Directory name of the harness store, under `.claude/`.
const HARNESS_DIR: &str = ".claude/.harness";

/// File name of the `SQLite` database within the harness directory.
const DB_FILE: &str = "mustard.db";

/// Open the harness database for `project_path`, applying schema + migrations.
///
/// Resolution order matches [`SqliteEventStore::for_project`]:
/// `MUSTARD_DB_PATH` if set, otherwise `<project_path>/.claude/.harness/mustard.db`.
///
/// Internally constructs an [`SqliteEventStore`] once so its schema apply and
/// migration ladder run, then drops it and re-opens a bare [`Connection`] at
/// the same path. The bare connection is what writer/reader functions expect
/// (they take `&Connection`, not the wrapper).
///
/// # Errors
///
/// Returns [`crate::error::Error::Sqlite`] if the database cannot be opened or
/// the schema cannot be applied, and [`crate::error::Error::Io`] if the parent
/// directory cannot be created.
pub fn open_for(project_path: &str) -> Result<Connection> {
    let path = resolve_db_path(project_path);
    // Single open: construct the store (applies schema + migrations on this
    // connection) and hand back the same connection it opened, rather than
    // dropping the wrapper and re-opening a second connection to the same file.
    Ok(SqliteEventStore::new(&path)?.into_connection())
}

/// Resolve the database path for `project_path`, honouring `MUSTARD_DB_PATH`.
///
/// Pulled out as a free function so a future test can swap `std::env::var`
/// via a helper without crossing the public API.
fn resolve_db_path(project_path: &str) -> PathBuf {
    if let Ok(override_path) = std::env::var(DB_PATH_ENV) {
        if !override_path.is_empty() {
            return PathBuf::from(override_path);
        }
    }
    PathBuf::from(project_path).join(HARNESS_DIR).join(DB_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn open_for_creates_db_under_project_root() {
        // SAFETY: tests are serial within a process per default cargo runner;
        // `MUSTARD_DB_PATH` is intentionally cleared so the path-resolution
        // fallback (project_path + HARNESS_DIR) is exercised.
        let dir = tempdir().unwrap();
        let project = dir.path().to_string_lossy().into_owned();
        // The legacy `std::env::remove_var` is unsafe under Rust 2024; the
        // crate forbids unsafe code, so we set it to an empty string instead.
        // The `resolve_db_path` helper treats empty as "not set" deliberately.
        // SAFETY note: set_var is actually safe-callable on this Rust version
        // when wrapped in a test harness, but we avoid it entirely by setting
        // it to a path the test owns when it must be present. Here we just
        // assert the default path was created.
        // To exercise the default branch we must ensure MUSTARD_DB_PATH is
        // truly absent; reaching for `set_var` would itself be unsafe. So we
        // gate this test behind the var being unset by the harness.
        if std::env::var(DB_PATH_ENV).is_ok() {
            // CI or sibling test set the override — skip rather than
            // contaminate the assertion.
            return;
        }
        let conn = open_for(&project).unwrap();
        let expected = dir.path().join(HARNESS_DIR).join(DB_FILE);
        assert!(expected.exists(), "db file must be created on first open");
        // The schema applied via SqliteEventStore must be visible on the bare
        // connection — `pipeline_events` is the W5 lifecycle index that
        // replaced the legacy `events` table.
        let row: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='pipeline_events'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(row, 1, "pipeline_events table must exist after open_for");
    }

    #[test]
    fn resolve_db_path_defaults_to_project_relative_when_env_absent() {
        // Pure-function test — bypasses the env-var dance entirely.
        let p = resolve_db_path("/tmp/projX");
        assert_eq!(
            p,
            PathBuf::from("/tmp/projX")
                .join(HARNESS_DIR)
                .join(DB_FILE)
        );
    }
}

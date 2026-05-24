//! The dedicated telemetry store — `.claude/.harness/telemetry.db`.
//!
//! [`TelemetryStore`] owns a `SQLite` database that is **independent** of
//! `mustard.db`. The harness opens `mustard.db` on every tool use; keeping the
//! high-volume telemetry tables (`usage_totals`, `run_usage`,
//! `run_attribution`) in a separate file means that hot path never pays to
//! open or lock them.
//!
//! Opening mirrors [`SqliteEventStore::new`](crate::store::sqlite_store::SqliteEventStore::new):
//! WAL journal mode, a generous `busy_timeout`, `synchronous = NORMAL`, and a
//! `PRAGMA user_version` fast-path so a steady-state open skips the DDL. The
//! path resolves from `MUSTARD_TELEMETRY_DB_PATH` (env override, analogous to
//! `MUSTARD_DB_PATH`) or `{project}/.claude/.harness/telemetry.db`.
//!
//! Every method is fail-open: it returns [`Result`] and never panics.

use crate::error::Result;
use rusqlite::Connection;
use std::path::{Path, PathBuf};

/// Directory name of the harness store, under `.claude/`.
const HARNESS_DIR: &str = ".harness";

/// File name of the dedicated telemetry database.
const DB_FILE: &str = "telemetry.db";

/// Environment variable overriding the resolved telemetry database path.
///
/// Analogous to `MUSTARD_DB_PATH` for the harness store. When set and
/// non-blank, [`TelemetryStore::for_project`] uses its value verbatim.
const DB_PATH_ENV: &str = "MUSTARD_TELEMETRY_DB_PATH";

/// How long a contended write waits for the lock before failing, in
/// milliseconds. Matches the harness store — telemetry writes are
/// sub-millisecond, so this only ever absorbs a transient pile-up.
const BUSY_TIMEOUT_MS: u32 = 5_000;

/// Schema version stamped into `PRAGMA user_version` once the DDL is applied.
/// Bump only if the embedded `schema.sql` changes shape.
const SCHEMA_VERSION: i64 = 1;

/// The idempotent telemetry schema, embedded via `include_str!`. Every
/// `CREATE` is `IF NOT EXISTS`, so applying it on every open is safe.
const SCHEMA_SQL: &str = include_str!("schema.sql");

/// SQLite-backed store over a single `telemetry.db` file.
///
/// Construct with [`TelemetryStore::new`] from an explicit path or
/// [`TelemetryStore::for_project`] from a project root. The connection is held
/// for the lifetime of the store; it is not `Clone`.
#[derive(Debug)]
pub struct TelemetryStore {
    /// The open `SQLite` connection. WAL + pragmas are set on open.
    conn: Connection,
    /// The database path, kept for diagnostics.
    path: PathBuf,
}

impl TelemetryStore {
    /// Open (creating if absent) a telemetry store at `path`.
    ///
    /// Applies the per-connection pragmas (WAL, busy timeout,
    /// `synchronous = NORMAL`) and the idempotent schema, gated by
    /// `PRAGMA user_version` so a materialized database skips the DDL. The
    /// parent directory is created if it does not exist.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Sqlite`](crate::error::Error::Sqlite) if the database
    /// cannot be opened or the schema cannot be applied, and
    /// [`Error::Io`](crate::error::Error::Io) if the parent directory cannot be
    /// created.
    pub fn new(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                crate::fs::create_dir_all(parent)?;
            }
        }
        let conn = Connection::open(&path)?;
        // Per-connection pragmas — set on every open (they do not persist with
        // the file). WAL gives concurrent readers + a single writer; NORMAL is
        // safe under WAL and avoids a full fsync per commit; the busy timeout
        // makes a contended write wait rather than error.
        conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
        conn.execute_batch("PRAGMA synchronous = NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_millis(u64::from(BUSY_TIMEOUT_MS)))?;

        // Fast-path: skip the DDL when the database is already materialized at
        // the current schema version. `user_version` is a header field, so
        // reading it touches no table.
        let user_version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if user_version != SCHEMA_VERSION {
            conn.execute_batch(SCHEMA_SQL)?;
            conn.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION}"))?;
        }
        Ok(Self { conn, path })
    }

    /// Open the standard telemetry database for a project.
    ///
    /// Resolves the path as: the value of `MUSTARD_TELEMETRY_DB_PATH` if set
    /// and non-blank, otherwise `{project_dir}/.claude/.harness/telemetry.db`.
    ///
    /// # Errors
    ///
    /// Same as [`TelemetryStore::new`].
    pub fn for_project(project_dir: impl AsRef<Path>) -> Result<Self> {
        let env_override = std::env::var(DB_PATH_ENV).ok();
        Self::new(resolve_db_path(project_dir.as_ref(), env_override.as_deref()))
    }

    /// The path of the backing database file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Borrow the store's open [`Connection`] for the writer / reader functions.
    #[must_use]
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Consume the store, yielding its already-opened [`Connection`].
    #[must_use]
    pub fn into_connection(self) -> Connection {
        self.conn
    }
}

/// Resolve the telemetry database path for a project.
///
/// `env_override` is the raw value of `MUSTARD_TELEMETRY_DB_PATH` (the caller
/// reads the environment); when present and non-blank it wins verbatim.
/// Otherwise the path defaults to `{project_dir}/.claude/.harness/telemetry.db`.
/// Kept pure — no environment access — so it is unit-testable without mutating
/// process state.
fn resolve_db_path(project_dir: &Path, env_override: Option<&str>) -> PathBuf {
    match env_override {
        Some(value) if !value.trim().is_empty() => PathBuf::from(value),
        _ => project_dir.join(".claude").join(HARNESS_DIR).join(DB_FILE),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::OptionalExtension as _;
    use tempfile::tempdir;

    #[test]
    fn new_materializes_three_tables() {
        let dir = tempdir().unwrap();
        let store = TelemetryStore::new(dir.path().join("telemetry.db")).unwrap();
        for table in ["usage_totals", "run_usage", "run_attribution"] {
            let exists: bool = store
                .conn()
                .query_row(
                    "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1",
                    rusqlite::params![table],
                    |r| r.get::<_, i64>(0),
                )
                .optional()
                .unwrap()
                .is_some();
            assert!(exists, "table {table} must exist after open");
        }
    }

    #[test]
    fn second_open_takes_fast_path() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("telemetry.db");
        {
            let store = TelemetryStore::new(&db).unwrap();
            let uv: i64 = store
                .conn()
                .query_row("PRAGMA user_version", [], |r| r.get(0))
                .unwrap();
            assert_eq!(uv, SCHEMA_VERSION);
        }
        // Re-open: user_version already at the schema version → DDL is skipped
        // but the store still opens and the version is preserved.
        let store = TelemetryStore::new(&db).unwrap();
        let uv: i64 = store
            .conn()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(uv, SCHEMA_VERSION);
    }

    #[test]
    fn resolve_db_path_honors_env_override() {
        let resolved = resolve_db_path(Path::new("/unused/project"), Some("/custom/tele.db"));
        assert_eq!(resolved, PathBuf::from("/custom/tele.db"));
    }

    #[test]
    fn resolve_db_path_falls_back_to_standard_path() {
        for env in [None, Some("   ")] {
            let resolved = resolve_db_path(Path::new("/proj"), env);
            assert!(resolved.ends_with("telemetry.db"));
            assert!(resolved.components().any(|c| c.as_os_str() == ".harness"));
        }
    }

    #[test]
    fn for_project_opens_a_usable_store() {
        let dir = tempdir().unwrap();
        let store = TelemetryStore::for_project(dir.path()).unwrap();
        assert!(store.path().ends_with("telemetry.db"));
    }
}

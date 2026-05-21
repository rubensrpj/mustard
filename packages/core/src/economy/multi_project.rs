//! Fan-out reader across multiple project databases.
//!
//! Backs [`EconomyScope::AllProjects`](super::scope::EconomyScope::AllProjects)
//! — given a list of project roots, open each `.claude/.harness/mustard.db`
//! read-only, run the same single-project query against it, and merge the
//! per-project results into one aggregate.
//!
//! ## Why a struct
//!
//! The fan-out logic is stateless (just a loop over project paths), but
//! keeping it inside [`MultiProjectReader`] lets the W5 dashboard inject a
//! mocked reader for tests, and lets a later wave swap the sequential loop
//! for `rayon` parallelism without touching every call site. The struct has
//! no fields today; future state (a connection cache, a per-project
//! timeout) can land here without an API break.
//!
//! ## Read-only guarantee
//!
//! Every connection is opened with `SQLITE_OPEN_READ_ONLY` so the fan-out
//! cannot corrupt a sibling project's DB. A project whose DB cannot be
//! opened — missing, permissions denied, locked — is skipped (fail-open)
//! rather than aborting the merge; the merged result simply omits that
//! project.

use std::collections::HashMap;

use rusqlite::{Connection, OpenFlags};

use crate::error::{Result, fail_open};

use super::scope::ProjectPath;

/// Suffix of the harness DB relative to a project root.
const DB_REL_PATH: &str = ".claude/.harness/mustard.db";

/// Stateless fan-out helper. See module docs.
#[derive(Debug, Default)]
pub struct MultiProjectReader;

impl MultiProjectReader {
    /// Build a fresh reader. No I/O — opening the per-project connections is
    /// deferred to [`Self::fan_out`].
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Run `query` against every project in `projects`, returning a map of
    /// per-project results.
    ///
    /// Projects whose database cannot be opened (or whose query returns an
    /// error) are silently skipped — the call is fail-open. A successful run
    /// with zero entries in the map means every project failed to open, not
    /// that every project returned no rows.
    ///
    /// The loop is sequential by design (W1 — paralleling is a W7+ debt).
    pub fn fan_out<T, F>(&self, projects: &[ProjectPath], query: F) -> HashMap<ProjectPath, T>
    where
        F: Fn(&Connection) -> Result<T>,
    {
        let mut out: HashMap<ProjectPath, T> = HashMap::new();
        for project in projects {
            let db_path = project.as_path().join(DB_REL_PATH);
            let conn = match Connection::open_with_flags(
                &db_path,
                OpenFlags::SQLITE_OPEN_READ_ONLY,
            ) {
                Ok(c) => c,
                Err(_) => continue, // Fail-open: missing project DB is not fatal.
            };
            // Fail-open per project: a query failure on one DB must not
            // poison the rest. The fallback (`None`) is filtered out below.
            let result: Option<T> = fail_open(query(&conn).map(Some), None);
            if let Some(value) = result {
                out.insert(project.clone(), value);
            }
        }
        out
    }

    /// Like [`Self::fan_out`], but also produce an aggregate via `merge`.
    ///
    /// `merge` is called with the per-project values in iteration order;
    /// projects that failed to open contribute nothing. The aggregate is
    /// `None` if every project failed.
    pub fn fan_out_merge<T, F, M>(
        &self,
        projects: &[ProjectPath],
        query: F,
        merge: M,
    ) -> (HashMap<ProjectPath, T>, Option<T>)
    where
        T: Clone,
        F: Fn(&Connection) -> Result<T>,
        M: Fn(T, T) -> T,
    {
        let per_project = self.fan_out(projects, query);
        let aggregate = per_project
            .values()
            .cloned()
            .reduce(|acc, v| merge(acc, v));
        (per_project, aggregate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::sqlite_store::SqliteEventStore;
    use tempfile::tempdir;

    fn make_project(name: &str) -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        let project_root = dir.path().join(name);
        std::fs::create_dir_all(project_root.join(".claude/.harness")).unwrap();
        let _store = SqliteEventStore::new(project_root.join(DB_REL_PATH)).unwrap();
        // Re-pack: callers want the project root, not the temp root.
        // Hand back the original TempDir; the project lives under `name`.
        dir
    }

    #[test]
    fn fan_out_skips_missing_projects_silently() {
        let reader = MultiProjectReader::new();
        let projects = vec![
            ProjectPath::new("/definitely/not/a/path/at/all"),
        ];
        let out = reader.fan_out(&projects, |_| Ok::<_, crate::error::Error>(1u32));
        assert!(out.is_empty(), "missing project DB must be silently skipped");
    }

    #[test]
    fn fan_out_returns_one_row_per_real_project() {
        let dir_a = make_project("a");
        let dir_b = make_project("b");
        let path_a = ProjectPath::new(dir_a.path().join("a"));
        let path_b = ProjectPath::new(dir_b.path().join("b"));

        let reader = MultiProjectReader::new();
        let out = reader.fan_out(
            &[path_a.clone(), path_b.clone()],
            |conn| {
                // Trivial query: how many tables does the DB have?
                let count: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table'",
                        [],
                        |r| r.get(0),
                    )
                    .unwrap_or(0);
                Ok(count)
            },
        );
        assert_eq!(out.len(), 2);
        assert!(out[&path_a] > 0);
        assert!(out[&path_b] > 0);
    }

    #[test]
    fn fan_out_merge_aggregates_when_provided_a_merge_fn() {
        let dir_a = make_project("a");
        let dir_b = make_project("b");
        let path_a = ProjectPath::new(dir_a.path().join("a"));
        let path_b = ProjectPath::new(dir_b.path().join("b"));

        let reader = MultiProjectReader::new();
        let (per_project, aggregate) = reader.fan_out_merge(
            &[path_a.clone(), path_b.clone()],
            |_| Ok::<_, crate::error::Error>(5i64),
            |a, b| a + b,
        );
        assert_eq!(per_project.len(), 2);
        assert_eq!(aggregate, Some(10));
    }
}

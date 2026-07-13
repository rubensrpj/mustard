//! Fan-out reader across multiple project roots (NDJSON edition).
//!
//! W7A of [[2026-05-26-no-sqlite-git-source-of-truth]] retired the
//! `Connection`-based fan-out. The new shape takes a closure receiving a
//! project root [`Path`] (no SQLite open) so it composes with the rest of the
//! NDJSON readers in [`super::reader`].
//!
//! ## Why a struct
//!
//! Statefulness today is zero. The struct exists so a future wave can swap
//! the sequential loop for `rayon` parallelism, add a per-project cache, or
//! plug in a deduplication step without an API break.
//!
//! ## Fail-open guarantee
//!
//! A project whose root cannot be read — missing, permissions denied — is
//! silently skipped, exactly as the SQLite-era reader skipped missing DBs.
//! The aggregate result simply omits that project.

use std::collections::HashMap;
use std::path::Path;

use crate::platform::error::{fail_open, Result};

use super::scope::ProjectPath;

/// Stateless fan-out helper. See module docs.
#[derive(Debug, Default)]
pub struct MultiProjectReader;

impl MultiProjectReader {
    /// Build a fresh reader.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Run `query` against every project in `projects`, returning a map of
    /// per-project results.
    ///
    /// The closure receives the absolute project root path AND the originating
    /// [`ProjectPath`] so callers can recurse into per-scope readers without
    /// mis-attributing the inner scope.
    ///
    /// Projects whose query returns `Err` are silently skipped (fail-open).
    /// A successful run with zero entries in the map means every project
    /// errored, not that every project produced an empty result.
    pub fn fan_out<T, F>(&self, projects: &[ProjectPath], query: F) -> HashMap<ProjectPath, T>
    where
        F: Fn(&Path, &ProjectPath) -> Result<T>,
    {
        let mut out: HashMap<ProjectPath, T> = HashMap::new();
        for project in projects {
            let root = project.as_path();
            let result: Option<T> = fail_open(query(root, project).map(Some), None);
            if let Some(value) = result {
                out.insert(project.clone(), value);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn fan_out_skips_projects_whose_query_errors() {
        let reader = MultiProjectReader::new();
        let projects = vec![ProjectPath::new("/definitely/not/a/path/at/all")];
        let out = reader.fan_out(&projects, |_, _| {
            Err::<u32, _>(crate::platform::error::Error::NotFound("nope".into()))
        });
        assert!(out.is_empty(), "every project errored → map must be empty");
    }

    #[test]
    fn fan_out_returns_one_row_per_real_project() {
        let dir_a = tempdir().unwrap();
        let dir_b = tempdir().unwrap();
        let path_a = ProjectPath::new(dir_a.path());
        let path_b = ProjectPath::new(dir_b.path());

        let reader = MultiProjectReader::new();
        let out = reader.fan_out(&[path_a.clone(), path_b.clone()], |_root, _proj| {
            Ok::<_, crate::platform::error::Error>(42u32)
        });
        assert_eq!(out.len(), 2);
        assert_eq!(out[&path_a], 42);
        assert_eq!(out[&path_b], 42);
    }

}

//! `QuerySet` — loads `.scm` tree-sitter queries from a project-local
//! directory.
//!
//! Queries live under `.claude/grammars/{lang_id}/queries/*.scm` in the
//! target project. The file stem becomes the query key (e.g.
//! `stub_detect.scm` → `"stub_detect"`).
//!
//! Project layout:
//!
//! ```text
//! .claude/
//!   grammars/
//!     rust/
//!       queries/
//!         stub_detect.scm
//!         function_signature.scm
//!     typescript/
//!       queries/
//!         stub_detect.scm
//!         function_signature.scm
//! ```
//!
//! ## Fail-open
//!
//! - The directory not existing is **not** an error — [`QuerySet::load_for`]
//!   returns an empty [`QuerySet`].
//! - An individual `.scm` file that fails to compile is dropped from the
//!   set with a `Vec<PathBuf>` of failures recorded in [`QuerySet::failures`].
//!   The caller may surface those failures in telemetry but the set is still
//!   usable for whichever queries did compile.

use super::AstError;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tree_sitter::{Language, Query};

/// Compiled tree-sitter queries for a single language.
#[derive(Default)]
pub struct QuerySet {
    /// File stem → compiled `Query`.
    queries: HashMap<String, Query>,
    /// Files that existed but failed to compile. Surfaced so callers can
    /// emit telemetry; the set itself stays usable.
    failures: Vec<(PathBuf, String)>,
    /// Language id this set was loaded for. Empty for `QuerySet::default()`.
    lang_id: String,
}

impl std::fmt::Debug for QuerySet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.queries.keys().map(String::as_str).collect();
        f.debug_struct("QuerySet")
            .field("lang_id", &self.lang_id)
            .field("queries", &names)
            .field("failures", &self.failures.len())
            .finish()
    }
}

impl QuerySet {
    /// Build a set for `lang_id` by reading every `.scm` file under
    /// `{project_root}/.claude/grammars/{lang_id}/queries/`.
    ///
    /// `language` is required to compile the queries — the same `Language`
    /// returned by [`super::GrammarLoader::language`]. A `None` `language`
    /// short-circuits to [`QuerySet::default`]; the caller is responsible
    /// for resolving the grammar before calling this.
    ///
    /// Never returns an error: a missing directory yields an empty set, and
    /// individual compile failures land in [`QuerySet::failures`]. This is
    /// intentional — the caller decides whether to surface the failures or
    /// fail open silently.
    #[must_use]
    pub fn load_for(lang_id: &str, project_root: &Path, language: Option<&Language>) -> Self {
        let Some(language) = language else {
            return Self {
                lang_id: lang_id.to_string(),
                ..Self::default()
            };
        };

        let queries_dir = project_root
            .join(".claude")
            .join("grammars")
            .join(lang_id)
            .join("queries");

        let Ok(entries) = std::fs::read_dir(&queries_dir) else {
            return Self {
                lang_id: lang_id.to_string(),
                ..Self::default()
            };
        };

        let mut queries: HashMap<String, Query> = HashMap::new();
        let mut failures: Vec<(PathBuf, String)> = Vec::new();

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("scm") {
                continue;
            }
            let stem = match path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };
            let source = match std::fs::read_to_string(&path) {
                Ok(s) => s,
                Err(e) => {
                    failures.push((path.clone(), format!("read failed: {e}")));
                    continue;
                }
            };
            match Query::new(language, &source) {
                Ok(q) => {
                    queries.insert(stem, q);
                }
                Err(e) => failures.push((path.clone(), format!("compile failed: {e}"))),
            }
        }

        Self {
            queries,
            failures,
            lang_id: lang_id.to_string(),
        }
    }

    /// Look up a compiled query by file stem (e.g. `"stub_detect"`).
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Query> {
        self.queries.get(name)
    }

    /// `true` when the set carries no compiled queries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.queries.is_empty()
    }

    /// Number of compiled queries in the set.
    #[must_use]
    pub fn len(&self) -> usize {
        self.queries.len()
    }

    /// Files that existed but failed to read or compile.
    #[must_use]
    pub fn failures(&self) -> &[(PathBuf, String)] {
        &self.failures
    }

    /// Language id this set was loaded for.
    #[must_use]
    pub fn lang_id(&self) -> &str {
        &self.lang_id
    }

    /// Strongly-typed accessor for the canonical `stub_detect.scm` query.
    /// Returns `None` when the file is absent or failed to compile.
    #[must_use]
    pub fn stub_detect(&self) -> Option<&Query> {
        self.get("stub_detect")
    }

    /// Strongly-typed accessor for the canonical
    /// `function_signature.scm` query. Returns `None` when the file is
    /// absent or failed to compile.
    #[must_use]
    pub fn function_signature(&self) -> Option<&Query> {
        self.get("function_signature")
    }
}

impl QuerySet {
    /// Surface a [`QuerySet`] load error as an [`AstError`]. Today
    /// [`load_for`] never returns errors directly; this helper exists so
    /// future strict callers can promote a `failures` entry to a typed
    /// error without re-implementing the formatting.
    ///
    /// [`load_for`]: Self::load_for
    #[must_use]
    pub fn first_failure_as_error(&self) -> Option<AstError> {
        self.failures
            .first()
            .map(|(path, _)| AstError::QueryLoadFailed(path.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_for_missing_dir_returns_empty_set() {
        let tmp = tempfile::tempdir().unwrap();
        let set = QuerySet::load_for("rust", tmp.path(), None);
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
        assert!(set.failures().is_empty());
        assert_eq!(set.lang_id(), "rust");
    }

    #[test]
    fn load_for_returns_empty_when_language_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        // Even with a queries directory present, lang=None ⇒ empty set.
        let dir = tmp
            .path()
            .join(".claude")
            .join("grammars")
            .join("rust")
            .join("queries");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("stub_detect.scm"), "(call_expression) @hit").unwrap();
        let set = QuerySet::load_for("rust", tmp.path(), None);
        assert!(set.is_empty());
    }

    #[test]
    fn default_set_has_no_queries_no_failures() {
        let set = QuerySet::default();
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
        assert!(set.failures().is_empty());
        assert_eq!(set.lang_id(), "");
        assert!(set.stub_detect().is_none());
        assert!(set.function_signature().is_none());
    }
}

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
//! ## Built-in queries (embedded)
//!
//! For the six in-crate grammars (`rust`, `typescript`, `tsx`, `python`,
//! `go`, `java`, `c-sharp`) the canonical `entity_definitions.scm` and
//! `import_edges.scm` queries are embedded into the binary via
//! [`include_str!`] from `queries_builtin/{lang_id}/`. They form the
//! *guaranteed base*: a query stem resolves even on a machine with no
//! `.claude/grammars/{lang}/queries/` directory.
//!
//! An on-disk `.scm` of the same stem **overrides** the built-in (last write
//! wins inside [`QuerySet::load_for`]): the built-in source is compiled
//! first, then the on-disk directory is walked and any compiled query
//! replaces the built-in entry under the same stem.
//!
//! ## Fail-open
//!
//! - The directory not existing is **not** an error — [`QuerySet::load_for`]
//!   returns a set carrying only the built-in queries (if any).
//! - An individual `.scm` file that fails to compile is dropped from the
//!   set with a `Vec<PathBuf>` of failures recorded in [`QuerySet::failures`].
//!   The caller may surface those failures in telemetry but the set is still
//!   usable for whichever queries did compile.

use super::AstError;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tree_sitter::{Language, Query};

/// Embedded built-in `.scm` query sources for the in-crate grammars.
///
/// Each entry is `(lang_id, query_stem, scm_source)`. The sources are
/// compiled into the binary via [`include_str!`] so the canonical
/// `entity_definitions` / `import_edges` queries resolve offline, with no
/// project-local `.claude/grammars/{lang}/queries/` directory present.
///
/// On-disk queries override these — see [`QuerySet::load_for`].
const BUILTIN_QUERIES: &[(&str, &str, &str)] = &[
    (
        "rust",
        "entity_definitions",
        include_str!("queries_builtin/rust/entity_definitions.scm"),
    ),
    (
        "rust",
        "import_edges",
        include_str!("queries_builtin/rust/import_edges.scm"),
    ),
    (
        "typescript",
        "entity_definitions",
        include_str!("queries_builtin/typescript/entity_definitions.scm"),
    ),
    (
        "typescript",
        "import_edges",
        include_str!("queries_builtin/typescript/import_edges.scm"),
    ),
    (
        "tsx",
        "entity_definitions",
        include_str!("queries_builtin/tsx/entity_definitions.scm"),
    ),
    (
        "tsx",
        "import_edges",
        include_str!("queries_builtin/tsx/import_edges.scm"),
    ),
    (
        "python",
        "entity_definitions",
        include_str!("queries_builtin/python/entity_definitions.scm"),
    ),
    (
        "python",
        "import_edges",
        include_str!("queries_builtin/python/import_edges.scm"),
    ),
    (
        "go",
        "entity_definitions",
        include_str!("queries_builtin/go/entity_definitions.scm"),
    ),
    (
        "go",
        "import_edges",
        include_str!("queries_builtin/go/import_edges.scm"),
    ),
    (
        "java",
        "entity_definitions",
        include_str!("queries_builtin/java/entity_definitions.scm"),
    ),
    (
        "java",
        "import_edges",
        include_str!("queries_builtin/java/import_edges.scm"),
    ),
    (
        "c-sharp",
        "entity_definitions",
        include_str!("queries_builtin/c-sharp/entity_definitions.scm"),
    ),
    (
        "c-sharp",
        "import_edges",
        include_str!("queries_builtin/c-sharp/import_edges.scm"),
    ),
];

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
    /// Build a set for `lang_id`.
    ///
    /// Two layers, in order:
    ///
    /// 1. **Built-in base.** Every [`BUILTIN_QUERIES`] entry for `lang_id` is
    ///    compiled first against `language`. For the six in-crate grammars
    ///    this guarantees `entity_definitions` / `import_edges` resolve with
    ///    no project-local files present.
    /// 2. **On-disk override.** Every `.scm` under
    ///    `{project_root}/.claude/grammars/{lang_id}/queries/` is then
    ///    compiled; a successful compile **replaces** any built-in entry under
    ///    the same stem (last write wins).
    ///
    /// `language` is required to compile the queries — the same `Language`
    /// returned by [`super::GrammarLoader::language`]. A `None` `language`
    /// short-circuits to [`QuerySet::default`]; the caller is responsible
    /// for resolving the grammar before calling this.
    ///
    /// Never returns an error: a missing directory yields a built-in-only
    /// set, and individual compile failures land in [`QuerySet::failures`].
    /// This is intentional — the caller decides whether to surface the
    /// failures or fail open silently.
    #[must_use]
    pub fn load_for(lang_id: &str, project_root: &Path, language: Option<&Language>) -> Self {
        let Some(language) = language else {
            return Self {
                lang_id: lang_id.to_string(),
                ..Self::default()
            };
        };

        let mut queries: HashMap<String, Query> = HashMap::new();
        let mut failures: Vec<(PathBuf, String)> = Vec::new();

        // (1) Built-in base: compile embedded sources for this language id.
        // A built-in that fails to compile is recorded as a failure under a
        // synthetic `<builtin>/{lang}/{stem}.scm` path; the rest of the set
        // stays usable.
        for (b_lang, b_stem, b_src) in BUILTIN_QUERIES {
            if *b_lang != lang_id {
                continue;
            }
            match Query::new(language, b_src) {
                Ok(q) => {
                    queries.insert((*b_stem).to_string(), q);
                }
                Err(e) => failures.push((
                    PathBuf::from("<builtin>")
                        .join(lang_id)
                        .join(format!("{b_stem}.scm")),
                    format!("builtin compile failed: {e}"),
                )),
            }
        }

        let queries_dir = project_root
            .join(".claude")
            .join("grammars")
            .join(lang_id)
            .join("queries");

        let Ok(entries) = std::fs::read_dir(&queries_dir) else {
            return Self {
                queries,
                failures,
                lang_id: lang_id.to_string(),
            };
        };

        // (2) On-disk override: a compiled file replaces the built-in entry
        // sharing its stem.
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

    /// Strongly-typed accessor for the canonical `entity_definitions.scm`
    /// query (named type declarations). Resolves to the built-in query for
    /// the six in-crate grammars unless overridden on disk. Returns `None`
    /// when neither a built-in nor an on-disk query compiled.
    #[must_use]
    pub fn entity_definitions(&self) -> Option<&Query> {
        self.get("entity_definitions")
    }

    /// Strongly-typed accessor for the canonical `import_edges.scm` query
    /// (imported module / path). Resolves to the built-in query for the six
    /// in-crate grammars unless overridden on disk. Returns `None` when
    /// neither a built-in nor an on-disk query compiled.
    #[must_use]
    pub fn import_edges(&self) -> Option<&Query> {
        self.get("import_edges")
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
        assert!(set.entity_definitions().is_none());
        assert!(set.import_edges().is_none());
    }

    /// Every built-in `entity_definitions` / `import_edges` query must compile
    /// against its in-crate grammar with no on-disk files present. A compile
    /// failure (wrong node type for the grammar version) lands in `failures`;
    /// asserting an empty `failures` proves all built-in `.scm` are valid.
    #[test]
    fn builtin_queries_compile_for_every_in_crate_grammar() {
        use crate::domain::ast::GrammarLoader;
        let tmp = tempfile::tempdir().expect("temp dir");
        let loader = GrammarLoader::with_builtins(tmp.path());
        for lang in ["rust", "typescript", "tsx", "python", "go", "java", "c-sharp"] {
            let language = loader.language(lang).expect("built-in grammar resolves");
            let set = QuerySet::load_for(lang, tmp.path(), Some(&language));
            assert!(
                set.failures().is_empty(),
                "built-in queries for `{lang}` failed to compile: {:?}",
                set.failures()
            );
            assert!(
                set.entity_definitions().is_some(),
                "`{lang}` must expose a built-in entity_definitions query"
            );
            assert!(
                set.import_edges().is_some(),
                "`{lang}` must expose a built-in import_edges query"
            );
        }
    }

    /// An on-disk `.scm` of the same stem must override the built-in.
    #[test]
    fn on_disk_query_overrides_builtin() {
        use crate::domain::ast::GrammarLoader;
        let tmp = tempfile::tempdir().expect("temp dir");
        let loader = GrammarLoader::with_builtins(tmp.path());
        let language = loader.language("rust").expect("rust grammar");

        // Built-in resolves before any file is written.
        let base = QuerySet::load_for("rust", tmp.path(), Some(&language));
        assert!(base.entity_definitions().is_some());

        // Write a trivially-different valid override; the set must still
        // resolve `entity_definitions` (now from disk) with no failures.
        let dir = tmp
            .path()
            .join(".claude")
            .join("grammars")
            .join("rust")
            .join("queries");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("entity_definitions.scm"),
            "(struct_item name: (type_identifier) @name)\n",
        )
        .unwrap();
        let overridden = QuerySet::load_for("rust", tmp.path(), Some(&language));
        assert!(
            overridden.failures().is_empty(),
            "override compile should succeed: {:?}",
            overridden.failures()
        );
        assert!(overridden.entity_definitions().is_some());
        // The built-in import_edges is still present (not overridden).
        assert!(overridden.import_edges().is_some());
    }

    /// A broken on-disk override is recorded as a failure but does NOT drop
    /// the built-in already compiled under the same stem (last *successful*
    /// write wins; a failed compile leaves the prior entry intact).
    #[test]
    fn broken_on_disk_override_keeps_builtin() {
        use crate::domain::ast::GrammarLoader;
        let tmp = tempfile::tempdir().expect("temp dir");
        let loader = GrammarLoader::with_builtins(tmp.path());
        let language = loader.language("rust").expect("rust grammar");

        let dir = tmp
            .path()
            .join(".claude")
            .join("grammars")
            .join("rust")
            .join("queries");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("entity_definitions.scm"),
            "(this_node_type_does_not_exist) @name\n",
        )
        .unwrap();
        let set = QuerySet::load_for("rust", tmp.path(), Some(&language));
        // The broken override is recorded...
        assert!(!set.failures().is_empty());
        // ...but the built-in entity_definitions survives.
        assert!(set.entity_definitions().is_some());
    }
}

//! `GrammarLoader` — agnostic grammar discovery via `tree_sitter_loader`.
//!
//! The loader is the **single owner** of `tree_sitter::Language` handles in
//! `mustard-core`. Every other module under `ast::*` asks the loader for a
//! language by id; nobody else constructs a `Language` from a hardcoded
//! `tree_sitter_*::language()` symbol — that is the v1 shape this redesign
//! exists to eliminate.
//!
//! ## Discovery
//!
//! Grammar discovery is delegated entirely to `tree_sitter_loader::Loader`:
//!
//! 1. Build a `tree_sitter_loader::Config` whose `parser_directories` either
//!    come from `~/.config/tree-sitter/config.json` (when present) or from
//!    `Config::initial()` defaults.
//! 2. Run `Loader::find_all_languages(&config)`.
//! 3. Iterate `Loader::get_all_language_configurations()` and resolve each
//!    `LanguageConfiguration` to its `Language` via
//!    `Loader::language_for_configuration`. Index by `language_name` (also
//!    by `scope` short-tail, e.g. `source.rust` → `rust`).
//!
//! The result is a `HashMap<String, Language>` plus a list of file-type
//! globs the caller can use to map paths back to a language id.
//!
//! ## Fail-open contract
//!
//! - A missing `~/.config/tree-sitter/config.json` is **not** an error; the
//!   loader simply discovers nothing and `available_languages()` is empty.
//! - Any `LoaderError` during `find_all_languages` is surfaced as
//!   [`AstError::LoaderConfigFailed`] **only when constructing**; consumers
//!   that need fail-open behaviour wrap the constructor in
//!   [`crate::platform::error::fail_open_with`].
//! - The lookup function `language()` is infallible — a missing id returns
//!   `None`, never an error.

use super::AstError;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tree_sitter::Language;
use tree_sitter_loader::{Config, Loader};

/// Resolver of `tree_sitter::Language` handles by opaque string id.
///
/// One instance per workspace root. Built once at startup (the discovery
/// step touches the filesystem) and reused for every parse.
pub struct GrammarLoader {
    /// All grammars discovered by `tree_sitter_loader::Loader::find_all_languages`,
    /// indexed by their `language_name` and by the short tail of their TextMate
    /// scope (e.g. `source.rust` → `rust`). Both keys point at the same handle.
    languages: HashMap<String, Language>,
    /// File-extension → language-id map. Built from each
    /// `LanguageConfiguration::file_types`. Used by the stub-detector to
    /// resolve `path.rs → "rust"` without enumerating extensions anywhere
    /// in `mustard-core`.
    extensions: HashMap<String, String>,
    /// The project root the loader was built for. Kept so the caller can
    /// build a [`super::QuerySet`] for the same root without threading the
    /// path back through every call.
    project_root: PathBuf,
}

impl std::fmt::Debug for GrammarLoader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `languages` holds raw `tree_sitter::Language` handles which don't
        // implement `Debug` cheaply; surface the count via
        // `available_languages()` instead. Use `finish_non_exhaustive` to
        // signal the omission.
        f.debug_struct("GrammarLoader")
            .field("project_root", &self.project_root)
            .field("available_languages", &self.available_languages())
            .field("extensions", &self.extensions.len())
            .finish_non_exhaustive()
    }
}

impl GrammarLoader {
    /// Build a grammar loader for `project_root`.
    ///
    /// Discovers every grammar reachable through
    /// `tree_sitter_loader::Config::initial()` (which honours
    /// `TREE_SITTER_DIR` / `~/.config/tree-sitter/config.json`). Languages
    /// whose `tree_sitter_loader` configuration cannot be promoted to a
    /// `tree_sitter::Language` are silently skipped — fail-open is the
    /// rule.
    ///
    /// # Errors
    ///
    /// Returns [`AstError::LoaderConfigFailed`] only when
    /// `tree_sitter_loader::Config::initial()` itself errors out (typically
    /// a corrupt `config.json`). A `config.json` that simply does not exist
    /// is **not** an error — the loader returns successfully with an empty
    /// `available_languages()` list.
    pub fn from_project(project_root: &Path) -> Result<Self, AstError> {
        let config = Self::load_config();
        let mut loader = Loader::new()
            .map_err(|e| AstError::LoaderConfigFailed(format!("Loader::new: {e}")))?;

        // `find_all_languages` walks `parser_directories` and registers every
        // grammar it finds. An error here means the loader could not even
        // walk the directories — surface it; the caller decides whether to
        // fail open or propagate.
        loader
            .find_all_languages(&config)
            .map_err(|e| AstError::LoaderConfigFailed(format!("find_all_languages: {e}")))?;

        let mut languages: HashMap<String, Language> = HashMap::new();
        let mut extensions: HashMap<String, String> = HashMap::new();

        // Pair each `LanguageConfiguration` with the resolved `Language`.
        // `get_all_language_configurations` is read-only; the borrow lives
        // only for this loop.
        let configs: Vec<(String, Option<String>, Vec<String>)> = loader
            .get_all_language_configurations()
            .iter()
            .map(|(cfg, _path)| {
                // `language_name` is the canonical id; `scope` carries the
                // TextMate-style id (e.g. `source.rust`). Both go into the
                // map so callers that know either spelling resolve cleanly.
                (
                    cfg.language_name.clone(),
                    cfg.scope.clone(),
                    cfg.file_types.clone(),
                )
            })
            .collect();

        for (name, scope, file_types) in configs {
            // Promote the configuration to a `Language` handle. This is the
            // step that actually links the parser shared library — any
            // failure here means the grammar is broken; we skip it rather
            // than abort discovery for the others.
            let Some(language) = Self::language_for_name(&mut loader, &name) else {
                continue;
            };

            languages.insert(name.clone(), language.clone());

            if let Some(scope) = scope {
                // `source.rust` → `rust`; `source.cpp.embedded.macro` → `macro`.
                // Index the tail so callers can look up by the short id too.
                if let Some(tail) = scope.rsplit('.').next() {
                    if !tail.is_empty() {
                        languages
                            .entry(tail.to_string())
                            .or_insert_with(|| language.clone());
                    }
                }
            }

            for ext in file_types {
                // Normalise so callers can look up by raw extension regardless
                // of the leading dot the grammar manifest chose to use.
                let key = ext.trim_start_matches('.').to_ascii_lowercase();
                if !key.is_empty() {
                    extensions.entry(key).or_insert_with(|| name.clone());
                }
            }
        }

        Ok(Self {
            languages,
            extensions,
            project_root: project_root.to_path_buf(),
        })
    }

    /// Build an empty loader — used by tests and by the textual-only
    /// fallback path when the caller wants to skip filesystem discovery.
    ///
    /// All lookups return `None`. `available_languages()` is empty.
    #[must_use]
    pub fn empty(project_root: &Path) -> Self {
        Self {
            languages: HashMap::new(),
            extensions: HashMap::new(),
            project_root: project_root.to_path_buf(),
        }
    }

    /// Look up a language by id. Accepts both the grammar's `language_name`
    /// (e.g. `rust`) and the tail of its TextMate scope (e.g. `rust` for
    /// `source.rust`). Returns `None` when no match exists — never panics,
    /// never errors.
    #[must_use]
    pub fn language(&self, lang_id: &str) -> Option<Language> {
        self.languages.get(lang_id).cloned()
    }

    /// Map a filesystem path to a language id via the discovered
    /// `file_types` glob list. Returns `None` when the extension is not
    /// associated with any installed grammar.
    #[must_use]
    pub fn language_id_for_path(&self, path: &Path) -> Option<String> {
        let ext = path.extension().and_then(|s| s.to_str())?;
        self.extensions.get(&ext.to_ascii_lowercase()).cloned()
    }

    /// Every language id the loader has resolved. Sorted alphabetically for
    /// stable diagnostics. Includes both `language_name` keys and scope
    /// tails — so a single grammar may appear twice (once under its name,
    /// once under its scope tail).
    #[must_use]
    pub fn available_languages(&self) -> Vec<String> {
        let mut v: Vec<String> = self.languages.keys().cloned().collect();
        v.sort();
        v
    }

    /// Project root this loader was built for. Kept so callers can build a
    /// matching [`super::QuerySet`] without threading the path through.
    #[must_use]
    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    /// **Test-only.** Inject a synthetic `(extension → language id)` entry.
    /// Used by `ast::stub_detect::tests` to drive the textual fallback path
    /// through the public surface without touching the user's
    /// `~/.config/tree-sitter/config.json`. Never compiled into release
    /// builds.
    #[cfg(test)]
    pub(crate) fn inject_extension_for_test(&mut self, extension: &str, lang_id: &str) {
        self.extensions
            .insert(extension.to_ascii_lowercase(), lang_id.to_string());
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Build the `tree_sitter_loader::Config` that drives grammar
    /// discovery. We try `Config::initial()` first (which seeds the
    /// default `parser_directories` under the user's home dir) and fall
    /// back to `Config::default()` if `initial` panics — `initial` will
    /// panic when `etcetera::home_dir()` cannot resolve a home directory,
    /// which would otherwise propagate up and break the fail-open
    /// contract.
    fn load_config() -> Config {
        // `Config::initial()` panics on missing home dir. `catch_unwind`
        // turns that into a recoverable degradation: a process running
        // without a home (e.g. some containers / CI) still gets an empty
        // `parser_directories` list — `find_all_languages` then discovers
        // nothing and the loader reports `available_languages.is_empty()`.
        std::panic::catch_unwind(Config::initial).unwrap_or_default()
    }

    /// Try to resolve a `Language` for a given grammar name. The loader
    /// requires a `LanguageConfiguration` borrow which we cannot hold while
    /// also keeping a `&mut Loader` for resolution, so we pull the
    /// configuration out by index and resolve it inline.
    fn language_for_name(loader: &mut Loader, name: &str) -> Option<Language> {
        // Hold the configuration borrow only long enough to clone the bits
        // we need; immediately drop it before the resolver call so the
        // borrow checker is satisfied.
        let configs = loader.get_all_language_configurations();
        let cfg = configs.iter().find(|(c, _)| c.language_name == name)?;
        // SAFETY: `language_for_configuration` is `&self` so the immutable
        // borrow held by `configs` is compatible.
        loader.language_for_configuration(cfg.0).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AC-A-17 — empty discovery + missing grammar fail-open.
    ///
    /// Builds a loader over a temp directory (no installed grammars), checks
    /// that:
    ///
    /// - `from_project` returns `Ok` (no panic, no error).
    /// - `available_languages()` is empty.
    /// - `language("rust")` returns `None`.
    /// - `TreeSitterParser::for_language(&loader, "rust")` returns
    ///   `Err(AstError::GrammarNotInstalled("rust"))`.
    #[test]
    fn test_agnostic_discovery_and_missing_grammar_fail_open() {
        let tmp = tempfile::tempdir().expect("temp dir");

        // We cannot guarantee the host has zero grammars installed (the dev
        // machine may have a populated `~/.config/tree-sitter/config.json`),
        // so we exercise both paths: the real loader for the "discovery
        // works" half, and `GrammarLoader::empty` for the "no grammars"
        // half. AC-A-17 requires both to fail-open and never panic.

        // Real loader — must succeed even when discovery turns up nothing.
        let real = GrammarLoader::from_project(tmp.path()).expect("real loader builds");
        // We cannot assert `is_empty()` against the host; only that the
        // accessor is non-panicking and returns a Vec (possibly empty).
        let _ = real.available_languages();

        // Forced-empty loader — the agnostic contract: lookup returns None.
        let loader = GrammarLoader::empty(tmp.path());
        assert!(loader.available_languages().is_empty());
        assert!(loader.language("rust").is_none());
        assert!(loader.language("typescript").is_none());
        assert!(loader.language("anything").is_none());

        let err = crate::domain::ast::TreeSitterParser::for_language(&loader, "rust").unwrap_err();
        match err {
            AstError::GrammarNotInstalled(id) => assert_eq!(id, "rust"),
            other => panic!("expected GrammarNotInstalled(\"rust\"), got {other:?}"),
        }
    }

    #[test]
    fn empty_loader_carries_project_root_through() {
        let tmp = tempfile::tempdir().unwrap();
        let loader = GrammarLoader::empty(tmp.path());
        assert_eq!(loader.project_root(), tmp.path());
    }

    #[test]
    fn language_id_for_path_returns_none_on_empty_loader() {
        let tmp = tempfile::tempdir().unwrap();
        let loader = GrammarLoader::empty(tmp.path());
        assert!(loader.language_id_for_path(Path::new("foo.rs")).is_none());
        assert!(loader.language_id_for_path(Path::new("foo")).is_none());
    }
}

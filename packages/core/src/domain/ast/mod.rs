//! `ast` — agnostic AST primitives backed by `tree-sitter`.
//!
//! Spec A / Wave 1.5 introduces the second `mustard-core` primitive: a
//! language-agnostic AST layer the regression gate (W4) uses to detect
//! stub-fail-open patterns inside the bodies of declared touched functions.
//!
//! ## Why this module exists
//!
//! Wave 1 ships a four-layer vocabulary that scans diffs as text. That is
//! enough for the gate when no grammar is installed locally, but it cannot
//! distinguish `return None;` inside a function body from `None` mentioned
//! inside a doc-comment. Wave 1.5 lifts that limitation by resolving grammars
//! through `tree_sitter_loader::Loader::find_all_languages` — the same
//! mechanism the `tree-sitter` CLI uses — so the layer adapts to whatever
//! the user has installed without `mustard-core` itself enumerating any
//! language.
//!
//! ## Design (agnostic from birth)
//!
//! - **Zero hardcoded languages.** No `match lang { "rust" => …, … }` lives
//!   anywhere under `ast::*`. Language ids are opaque strings that bottom
//!   out at the `Loader`'s `language_configurations`. The fallback regex in
//!   [`signature`] is a single agnostic expression matching the universal
//!   "public function" lexical shape across languages — it is documented as
//!   imprecise and never enumerates language ids.
//! - **Fail-open in runtime, NOT in design.** Every public function returns
//!   a real value: when a grammar is missing the layer falls back to
//!   [`crate::domain::vocabulary::VocabularyMatcher::scan`] on the `Pattern` layer
//!   and tags hits with [`DetectionMode::Textual`]. When the grammar is
//!   available it tags hits with [`DetectionMode::Ast`].
//! - **SOLID.** `GrammarLoader`, `TreeSitterParser`, `QuerySet`, the
//!   stub-detector and the signature-extractor each own a single
//!   responsibility; the loader is the only owner of `tree_sitter::Language`
//!   handles; consumers receive `Option<Language>` lookups, never raw
//!   handles.
//!
//! ## Public surface
//!
//! - [`GrammarLoader`] — discover and look up installed grammars.
//! - [`TreeSitterParser`] — parse source text for a given language id.
//! - [`QuerySet`] — load `.scm` queries from a project-local directory.
//! - [`detect_stub_patterns`] — Camera 2 of the regression gate.
//! - [`extract_function_signatures`] — public-fn signatures from a source
//!   blob, AST or fallback.
//! - [`extract_entities`] — named type declarations (struct/class/enum/…)
//!   from a source blob, AST (via the built-in `entity_definitions` query)
//!   or agnostic textual floor.
//! - [`Tree`], [`FunctionSig`], [`ExtractedEntity`], [`StubMatch`],
//!   [`StubPattern`], [`DetectionMode`], [`AstError`].

pub mod entity;
pub mod loader;
pub mod parser;
pub mod queries;
pub mod signature;
pub mod stub_detect;

/// On-demand WASM grammar acquisition (third tier of the grammar strategy).
/// Entire module is gated behind the optional `wasm-grammars` feature so the
/// default build never pulls `wasmtime`/`ureq`; see [`wasm_acquire`].
#[cfg(feature = "wasm-grammars")]
pub mod wasm_acquire;

#[cfg(test)]
pub(crate) mod loader_test_helpers;

use std::ops::Range;
use std::path::PathBuf;

// Re-exports for the canonical W1.5 public surface.
pub use entity::{extract_entities, ExtractedEntity};
pub use loader::GrammarLoader;
pub use parser::TreeSitterParser;
pub use queries::QuerySet;
pub use signature::extract_function_signatures;
pub use stub_detect::detect_stub_patterns;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Newtype wrapper over a parsed [`tree_sitter::Tree`].
///
/// Wraps the underlying handle so future waves can carry extra metadata
/// (source-id, parse duration, etc.) without breaking the public surface.
#[derive(Debug)]
pub struct Tree {
    inner: tree_sitter::Tree,
}

impl Tree {
    /// Construct a [`Tree`] from a raw `tree_sitter::Tree`.
    #[must_use]
    pub fn new(inner: tree_sitter::Tree) -> Self {
        Self { inner }
    }

    /// Borrow the underlying `tree_sitter::Tree`. Callers that want to run
    /// queries directly use this to access the root node.
    #[must_use]
    pub fn as_tree_sitter(&self) -> &tree_sitter::Tree {
        &self.inner
    }

    /// Take ownership of the inner `tree_sitter::Tree`.
    #[must_use]
    pub fn into_tree_sitter(self) -> tree_sitter::Tree {
        self.inner
    }
}

/// Public function signature extracted from a source file.
///
/// Field semantics intentionally mirror the on-disk markdown contract from
/// `## Funções tocadas` (see [`crate::domain::spec::touched_functions`]): `name` is
/// the final identifier, never the qualified path, so equality against the
/// declared list is a simple string compare.
///
/// `Serialize` / `Deserialize` are derived so callers can persist a
/// signature alongside a captured function body (see
/// [`crate::domain::regression_check::FunctionCapture`]). The on-disk shape matches
/// the field names verbatim; the W2 snapshot artefact picks camelCase
/// rename at its own boundary via `#[serde(rename_all = "camelCase")]`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FunctionSig {
    /// Final identifier — the function name with no namespace / path prefix.
    pub name: String,
    /// Raw parameter list as it appears in source — preserved verbatim so
    /// the regression gate can flag signature drift inside `Extended`/
    /// `Modified` functions without re-parsing.
    pub params: String,
    /// Return type or annotation, when present in the source language.
    /// Empty string when the language has no syntactic return type or when
    /// the extractor could not resolve one.
    pub return_type: String,
    /// Byte span of the signature inside the original source. Half-open:
    /// `[start, end)`. Useful for the gate's diff localisation.
    pub span: Range<usize>,
}

/// Concrete stub-fail-open pattern.
///
/// Closed enum: the regression gate is the sole consumer and the W4 contract
/// pins the five variants. Adding a new pattern is a deliberate breaking
/// change — callers must extend the gate scoring at the same time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StubPattern {
    /// Return `None` (or its language equivalent: `null` / `nil` / `undefined`).
    NoneLiteral,
    /// Empty collection literal (`vec![]`, `Vec::new()`, `[]`, `{}`).
    EmptyCollection,
    /// `Default::default()` or equivalent factory of a zero-value.
    DefaultDefault,
    /// `unimplemented!()` macro / `NotImplementedException` raise.
    UnimplementedMarker,
    /// `todo!()` macro / `TODO`-tagged early return.
    TodoMarker,
}

impl StubPattern {
    /// Canonical lowercase identifier — used in telemetry payloads.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoneLiteral => "none_literal",
            Self::EmptyCollection => "empty_collection",
            Self::DefaultDefault => "default_default",
            Self::UnimplementedMarker => "unimplemented_marker",
            Self::TodoMarker => "todo_marker",
        }
    }

    /// All five variants in declaration order. Used by tests and by the
    /// gate-scoring loop.
    #[must_use]
    pub fn all() -> [Self; 5] {
        [
            Self::NoneLiteral,
            Self::EmptyCollection,
            Self::DefaultDefault,
            Self::UnimplementedMarker,
            Self::TodoMarker,
        ]
    }
}

/// How a [`StubMatch`] was produced.
///
/// The gate emits this so the operator can tell whether a hit is AST-precise
/// (grammar was available) or textual fallback (grammar missing, hit came from
/// the vocabulary scanner).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectionMode {
    /// Grammar resolved via [`GrammarLoader`]; match came from a tree-sitter
    /// `Query` against the parsed AST.
    Ast,
    /// Grammar unavailable; match came from
    /// [`crate::domain::vocabulary::VocabularyMatcher::scan`] over the diff text.
    Textual,
}

impl DetectionMode {
    /// Canonical lowercase identifier — used in telemetry payloads.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ast => "ast",
            Self::Textual => "textual",
        }
    }
}

/// One stub-fail-open hit emitted by [`detect_stub_patterns`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StubMatch {
    /// Final identifier of the declared function the hit landed inside —
    /// matches [`FunctionSig::name`] so callers can join against the
    /// touched-functions list directly.
    pub function_name: String,
    /// Which pattern fired.
    pub pattern: StubPattern,
    /// Byte span of the hit inside the source / diff text. Half-open.
    pub span: Range<usize>,
    /// Whether the hit came from the AST path or the textual fallback.
    pub mode: DetectionMode,
}

/// Typed error surface for the `ast` module.
///
/// `#[non_exhaustive]` so later waves can add variants without breaking
/// downstream matches.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AstError {
    /// A language id was requested but the grammar is not installed locally.
    /// The string carries the requested id verbatim so callers can name it
    /// in diagnostics.
    #[error("grammar not installed: {0}")]
    GrammarNotInstalled(String),

    /// The tree-sitter parser returned `None` — typically a parser-state
    /// reset or an internal grammar bug. Distinct from
    /// [`AstError::GrammarNotInstalled`] so callers can decide whether to
    /// fall back to the textual path.
    #[error("tree-sitter parser produced no tree")]
    ParseFailed,

    /// A `.scm` query file existed but could not be compiled. The path is
    /// the offending file.
    #[error("query load failed: {0}")]
    QueryLoadFailed(PathBuf),

    /// The `tree_sitter_loader::Config` / `Loader` constructor failed.
    /// The string carries the underlying error message.
    #[error("loader config failed: {0}")]
    LoaderConfigFailed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_pattern_round_trips_via_as_str() {
        for p in StubPattern::all() {
            assert!(!p.as_str().is_empty());
        }
    }

    #[test]
    fn detection_mode_strings_are_distinct() {
        assert_ne!(DetectionMode::Ast.as_str(), DetectionMode::Textual.as_str());
    }

    #[test]
    fn ast_error_grammar_not_installed_carries_id_verbatim() {
        let e = AstError::GrammarNotInstalled("rust".into());
        assert!(e.to_string().contains("rust"));
    }
}

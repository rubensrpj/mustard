//! `TreeSitterParser` — thin per-language wrapper around `tree_sitter::Parser`.
//!
//! Single responsibility: hold a `tree_sitter::Parser` already configured
//! with a `Language` resolved by [`GrammarLoader`], and feed it source bytes.
//!
//! Resolution of the `Language` is **delegated** to the loader; this struct
//! never matches on a language id, never falls back to a hardcoded grammar
//! crate. When the loader returns `None` the constructor errors out — the
//! caller decides whether to use the textual fallback or propagate.

use super::{AstError, GrammarLoader, Tree};
use tree_sitter::Parser;

/// Per-language tree-sitter parser.
pub struct TreeSitterParser {
    parser: Parser,
    /// Language id this parser was built for, kept for diagnostics.
    lang_id: String,
}

impl std::fmt::Debug for TreeSitterParser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `Parser` does not implement `Debug`; surface only the lang id.
        f.debug_struct("TreeSitterParser")
            .field("lang_id", &self.lang_id)
            .finish_non_exhaustive()
    }
}

impl TreeSitterParser {
    /// Build a parser for `lang_id` using the grammar resolved by `loader`.
    ///
    /// Resolution: a `Language` registered with the loader (in-crate built-in
    /// or `tree_sitter_loader`-discovered).
    ///
    /// # Errors
    ///
    /// - [`AstError::GrammarNotInstalled`] when neither path resolves a grammar
    ///   for `lang_id` (the caller then falls back to the textual floor).
    /// - [`AstError::LoaderConfigFailed`] when `tree_sitter::Parser::set_language`
    ///   rejects the language — typically an ABI mismatch.
    pub fn for_language(loader: &GrammarLoader, lang_id: &str) -> Result<Self, AstError> {
        // Native grammar.
        if let Some(language) = loader.language(lang_id) {
            let mut parser = Parser::new();
            parser.set_language(&language).map_err(|e| {
                AstError::LoaderConfigFailed(format!("set_language({lang_id}): {e}"))
            })?;
            return Ok(Self {
                parser,
                lang_id: lang_id.to_string(),
            });
        }


        Err(AstError::GrammarNotInstalled(lang_id.to_string()))
    }

    /// Parse `source` and return the resulting [`Tree`].
    ///
    /// # Errors
    ///
    /// Returns [`AstError::ParseFailed`] when `tree_sitter::Parser::parse`
    /// returns `None` — typically a parser-state reset or an internal
    /// grammar issue. Hostile input does **not** cause a failure: the
    /// parser is robust against arbitrary bytes and produces an error-rich
    /// tree instead.
    pub fn parse(&mut self, source: &str) -> Result<Tree, AstError> {
        let tree = self
            .parser
            .parse(source.as_bytes(), None)
            .ok_or(AstError::ParseFailed)?;
        Ok(Tree::new(tree))
    }

    /// Language id this parser was constructed for.
    #[must_use]
    pub fn lang_id(&self) -> &str {
        &self.lang_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn for_language_returns_grammar_not_installed_on_empty_loader() {
        let tmp = tempfile::tempdir().unwrap();
        let loader = GrammarLoader::empty(tmp.path());
        let err = TreeSitterParser::for_language(&loader, "rust").unwrap_err();
        match err {
            AstError::GrammarNotInstalled(id) => assert_eq!(id, "rust"),
            other => panic!("expected GrammarNotInstalled, got {other:?}"),
        }
    }

    #[test]
    fn for_language_carries_id_verbatim_in_error() {
        let tmp = tempfile::tempdir().unwrap();
        let loader = GrammarLoader::empty(tmp.path());
        for id in ["typescript", "python", "go", "unknown-lang"] {
            let err = TreeSitterParser::for_language(&loader, id).unwrap_err();
            assert!(err.to_string().contains(id));
        }
    }
}

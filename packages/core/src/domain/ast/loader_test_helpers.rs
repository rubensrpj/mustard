//! Test-only helpers for the `ast` module.
//!
//! These exist so `ast::stub_detect::tests` can drive the textual fallback
//! path through the public surface. They never compile into a release
//! build.

use super::GrammarLoader;

/// Return a [`GrammarLoader`] with a single synthetic `(extension → lang_id)`
/// mapping injected. No `tree_sitter::Language` is registered, so
/// `language()` lookups still return `None` — exactly the shape the
/// textual fallback path needs.
pub(crate) fn with_extension(
    mut loader: GrammarLoader,
    extension: &str,
    lang_id: &str,
) -> GrammarLoader {
    loader.inject_extension_for_test(extension, lang_id);
    loader
}

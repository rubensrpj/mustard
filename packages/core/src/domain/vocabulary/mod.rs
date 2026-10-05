//! `vocabulary` — the stack registry and the multi-pattern engine behind it.
//!
//! The stack-inference engine and its TOML registry live in [`stacks`]; the
//! `aho-corasick` engine that matches every signature in one linear pass lives
//! in [`aho`]. This module owns the error both share, [`VocabError`], and its
//! mapping onto the crate-wide [`CoreError`].

pub mod aho;
pub mod stacks;

use crate::platform::error::Error as CoreError;

/// Typed error surface for the vocabulary module.
///
/// `#[non_exhaustive]` so later waves can add variants without breaking
/// downstream matches.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum VocabError {
    /// The TOML file does not exist on disk. Carries the path that was
    /// looked up. Kept distinct from [`VocabError::Io`] so callers can
    /// treat absence as "empty vocabulary" while still surfacing real I/O
    /// failures.
    #[error("vocabulary file not found: {0}")]
    FileNotFound(String),

    /// An underlying I/O operation failed (permissions, broken symlink,
    /// disk error).
    #[error("vocabulary io error: {0}")]
    Io(String),

    /// The TOML content failed to deserialise — invalid table-array shape or
    /// malformed UTF-8 escape — or the automaton could not be built.
    #[error("invalid vocabulary toml: {0}")]
    InvalidToml(String),

    /// The constructor was handed an empty term list. Surfacing this as a
    /// typed error (rather than silently building an empty automaton) catches
    /// misconfigured vocab files early.
    #[error("vocabulary has no terms across any layer")]
    NoTerms,
}

impl From<VocabError> for CoreError {
    fn from(e: VocabError) -> Self {
        match e {
            VocabError::FileNotFound(p) => CoreError::NotFound(p),
            VocabError::Io(m) => CoreError::Config(format!("vocab io: {m}")),
            VocabError::InvalidToml(m) => CoreError::Parse(m),
            VocabError::NoTerms => CoreError::Config("vocab has no terms".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_not_found_maps_to_core_not_found() {
        let core: CoreError = VocabError::FileNotFound("/tmp/x".into()).into();
        assert!(matches!(core, CoreError::NotFound(p) if p == "/tmp/x"));
    }

    #[test]
    fn invalid_toml_maps_to_core_parse() {
        let core: CoreError = VocabError::InvalidToml("bad".into()).into();
        assert!(matches!(core, CoreError::Parse(_)));
    }
}

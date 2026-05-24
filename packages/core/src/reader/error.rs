//! Error type for the read layer — kept distinct from [`crate::error`].
//!
//! Consumers (dashboard Tauri commands, mustard-rt `run` subcommands) program
//! against this enum and never have to know about `rusqlite::Error` directly.
//! The variants reflect the only ways a reader call can legitimately fail:
//!
//! - `Io` — opening or accessing the `SQLite` database failed
//! - `Decode` — a row could not be deserialized into a `ViewModel`
//! - `Invalid` — a caller passed a malformed argument (empty spec name, etc.)
//!
//! Missing data is **never** an error: a spec with zero events resolves to
//! `Ok(None)` or an empty collection, matching the fail-open contract of the
//! rest of the workspace.

/// Read-side `Result` alias. Named `ReadResult` at the crate root to avoid
/// colliding with [`crate::error::Result`] when both are in scope.
pub type Result<T> = std::result::Result<T, ReadError>;

/// Error from a [`SpecReader`](crate::reader::SpecReader) call.
///
/// `#[non_exhaustive]` so later waves can add variants (e.g. `Stale` for cache
/// invalidation) without breaking a downstream `match`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ReadError {
    /// Underlying IO failure — usually `SqliteEventStore::for_project` could
    /// not open the database file. The string carries the original message.
    #[error("io error: {0}")]
    Io(String),

    /// A row was found but could not be decoded into a `ViewModel` — a malformed
    /// JSON payload or an event schema mismatch. The reader skips the row and
    /// continues; this variant only surfaces when the failure is fatal to the
    /// whole query.
    #[error("decode error: {0}")]
    Decode(String),

    /// The caller passed a malformed argument (empty spec name, an invalid
    /// time window, etc.). Reserved for programming errors, not data errors.
    #[error("invalid argument: {0}")]
    Invalid(String),
}

impl From<crate::error::Error> for ReadError {
    fn from(err: crate::error::Error) -> Self {
        // Every `mustard-core` error is, from this layer's point of view, an
        // IO failure. The original message survives in the wrapped string so
        // callers can still surface the root cause if they care.
        Self::Io(err.to_string())
    }
}

impl From<rusqlite::Error> for ReadError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Io(err.to_string())
    }
}

impl From<serde_json::Error> for ReadError {
    fn from(err: serde_json::Error) -> Self {
        Self::Decode(err.to_string())
    }
}

impl ReadError {
    /// Construct a [`ReadError::Invalid`] from anything string-like.
    pub fn invalid(msg: impl Into<String>) -> Self {
        Self::Invalid(msg.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_constructor_carries_message() {
        let err = ReadError::invalid("empty spec name");
        assert!(matches!(err, ReadError::Invalid(m) if m == "empty spec name"));
    }

    #[test]
    fn rusqlite_error_converts_to_io() {
        let rs = rusqlite::Error::SqliteSingleThreadedMode;
        let read: ReadError = rs.into();
        assert!(matches!(read, ReadError::Io(_)));
    }

    #[test]
    fn serde_error_converts_to_decode() {
        let serde_err = serde_json::from_str::<String>("not json").unwrap_err();
        let read: ReadError = serde_err.into();
        assert!(matches!(read, ReadError::Decode(_)));
    }
}

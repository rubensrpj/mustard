//! Crate-level error type and fail-open helpers.
//!
//! [`model`](crate::domain::model) is pure and error-free; the moment a layer touches
//! the filesystem ([`fs`](crate::io::fs)) or parses config it needs a typed
//! error. This is that type.
//!
//! **Fail-open is the rule.** No function in the crate panics on these
//! errors; every fallible operation returns [`Result`]. Callers (hooks) treat
//! an [`Error`] as a signal to degrade safely, never to crash. In particular
//! [`Error::NotFound`] is kept distinct from [`Error::Io`] so a caller can
//! treat an absent file as "empty" (the common fail-open case) while still
//! surfacing a genuine I/O failure.
//!
//! This enum is `#[non_exhaustive]`; later waves can add variants without
//! breaking a downstream `match` (consumers keep a wildcard arm).

/// The crate's [`Result`] alias.
pub type Result<T> = std::result::Result<T, Error>;

/// An error from a side-effecting `mustard-core` operation.
///
/// `#[non_exhaustive]`: later waves can add variants without breaking a
/// downstream `match` (consumers keep a wildcard arm).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// An underlying I/O operation failed (permissions, disk, rename, …).
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// A required file did not exist. Separated from [`Error::Io`] so callers
    /// can fail open on absence without swallowing real I/O failures. The
    /// string is the path that was missing.
    #[error("not found: {0}")]
    NotFound(String),

    /// JSON serialization or deserialization failed. The string is the
    /// underlying message; the offending input is intentionally not retained.
    #[error("parse error: {0}")]
    Parse(String),

    /// A configuration value was malformed — a non-object `mustard.json`, or
    /// an entry of the wrong shape. The string describes what was wrong.
    /// Callers fall back to the default config (fail-open) rather than
    /// crashing.
    #[error("config error: {0}")]
    Config(String),

    /// A [`Check`](crate::domain::model::contract::Check) failed for a reason specific
    /// to its own logic.
    #[error("check failed: {0}")]
    CheckFailed(String),

    /// A PRIVATE install could not hide its own footprint from the host
    /// repository it is being installed into.
    ///
    /// The one place in this crate where fail-open is the wrong answer. Every
    /// other degradation here costs a feature; this one costs the operator's
    /// belief: they asked for an install nothing in the client's git can see,
    /// and a quiet "nothing was excluded" would let the seeds land VISIBLY
    /// under exactly that belief. So the install refuses before writing
    /// anything. The string is the reason, from
    /// [`crate::platform::git_exclude::ExcludeFailure`].
    #[error("private install cannot hide its footprint: {0}")]
    NotHidden(String),
}

impl Error {
    /// Construct an [`Error::Config`] from anything string-like.
    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config(msg.into())
    }

    /// Construct an [`Error::CheckFailed`] from anything string-like.
    pub fn check_failed(msg: impl Into<String>) -> Self {
        Self::CheckFailed(msg.into())
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Self::Parse(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_and_check_failed_constructors_carry_message() {
        assert!(matches!(Error::config("x"), Error::Config(m) if m == "x"));
        assert!(matches!(Error::check_failed("y"), Error::CheckFailed(m) if m == "y"));
    }
}

//! Crate-level error type and fail-open helpers.
//!
//! [`model`](crate::domain::model) is pure and error-free; the moment a layer touches
//! the filesystem ([`fs`](crate::io::fs)) or parses config
//! ([`config`](crate::platform::config)) it needs a typed error. This
//! is that type.
//!
//! **Fail-open is the rule.** No function in the crate panics on these
//! errors; every fallible operation returns [`Result`]. Callers (hooks) treat
//! an [`Error`] as a signal to degrade safely, never to crash. In particular
//! [`Error::NotFound`] is kept distinct from [`Error::Io`] so a caller can
//! treat an absent file as "empty" (the common fail-open case) while still
//! surfacing a genuine I/O failure.
//!
//! The [`fail_open`] helper makes that pattern explicit: collapse a `Result`
//! to a fallback value without ever propagating the error.
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

    /// A configuration value was malformed — an unrecognised enforcement
    /// `mode`, a non-object `mustard.json`, or a check entry of the wrong
    /// shape. The string describes what was wrong. Callers fall back to the
    /// default config (fail-open) rather than crashing.
    #[error("config error: {0}")]
    Config(String),

    /// An environment lookup or resolution failed in a way the caller treats
    /// as recoverable (e.g. a required variable could not be read). The
    /// string is a short description.
    #[error("env error: {0}")]
    Env(String),

    /// A [`Check`](crate::domain::model::contract::Check) could not reach a decision
    /// because its [`HookInput`](crate::domain::model::contract::HookInput) was
    /// malformed or missing a field it required.
    #[error("invalid hook input: {0}")]
    InvalidInput(String),

    /// A [`Check`](crate::domain::model::contract::Check) failed for a reason specific
    /// to its own logic.
    #[error("check failed: {0}")]
    CheckFailed(String),
}

impl Error {
    /// Construct an [`Error::Config`] from anything string-like.
    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config(msg.into())
    }

    /// Construct an [`Error::Env`] from anything string-like.
    pub fn env(msg: impl Into<String>) -> Self {
        Self::Env(msg.into())
    }

    /// Construct an [`Error::CheckFailed`] from anything string-like.
    pub fn check_failed(msg: impl Into<String>) -> Self {
        Self::CheckFailed(msg.into())
    }
}

/// Collapse a [`Result`] to a fallback value, discarding any error.
///
/// This is the fail-open primitive: hooks must never crash, so an operation
/// that could fail is reduced to "the value, or a safe default". Use it where
/// the JS hooks wrap a body in `try { … } catch (_) { /* swallow */ }`.
///
/// ```
/// use mustard_core::platform::error::{fail_open, Error, Result};
/// let ok: Result<i32> = Ok(7);
/// let bad: Result<i32> = Err(Error::env("nope"));
/// assert_eq!(fail_open(ok, 0), 7);
/// assert_eq!(fail_open(bad, 0), 0);
/// ```
pub fn fail_open<T>(result: Result<T>, fallback: T) -> T {
    result.unwrap_or(fallback)
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
    fn fail_open_returns_value_on_ok() {
        let r: Result<u8> = Ok(42);
        assert_eq!(fail_open(r, 0), 42);
    }

    #[test]
    fn fail_open_returns_fallback_on_err() {
        let r: Result<u8> = Err(Error::config("bad mode"));
        assert_eq!(fail_open(r, 7), 7);
    }

    #[test]
    fn config_and_env_constructors_carry_message() {
        assert!(matches!(Error::config("x"), Error::Config(m) if m == "x"));
        assert!(matches!(Error::env("y"), Error::Env(m) if m == "y"));
    }
}

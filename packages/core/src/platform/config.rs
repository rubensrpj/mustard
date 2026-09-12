//! Enforcement mode — how strongly a check acts: off, advisory or blocking.
//!
//! [`Mode`] is the one vocabulary the `MUSTARD_*_MODE` readers parse into
//! (the rtk and commit gates). Parsing never panics: an unrecognised string
//! is `None`, and the caller falls back to its own default, because a hook
//! must never be blocked by a config typo.

/// How strongly an enforcement check acts.
///
/// Mirrors the three values every `MUSTARD_*_MODE` variable accepts in the JS
/// hooks. `#[non_exhaustive]` is deliberately *not* used: these three modes
/// are the complete, closed vocabulary — a check is off, advisory, or blocking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    /// The check does not run at all.
    Off,
    /// The check runs and may print an advisory message, but never blocks.
    Warn,
    /// The check runs and blocks the action on failure.
    Strict,
}

impl Mode {
    /// Parse a mode string case-insensitively.
    ///
    /// Accepts `off`, `warn`, `strict`. Whitespace is trimmed. Returns `None`
    /// for anything else so callers fail open to a default rather than panic.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "off" => Some(Self::Off),
            "warn" => Some(Self::Warn),
            "strict" => Some(Self::Strict),
            _ => None,
        }
    }

    /// The canonical lowercase string for this mode.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Warn => "warn",
            Self::Strict => "strict",
        }
    }

    /// `true` if a check in this mode runs at all (`Warn` or `Strict`).
    #[must_use]
    pub fn is_active(self) -> bool {
        !matches!(self, Self::Off)
    }

    /// `true` if a check in this mode can block an action (`Strict` only).
    #[must_use]
    pub fn is_blocking(self) -> bool {
        matches!(self, Self::Strict)
    }
}

impl Default for Mode {
    /// An unset check defaults to [`Mode::Strict`] — the JS hooks treat a
    /// missing `MUSTARD_*_MODE` variable as `strict`.
    fn default() -> Self {
        Self::Strict
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_parse_is_case_insensitive_and_trims() {
        assert_eq!(Mode::parse("  STRICT "), Some(Mode::Strict));
        assert_eq!(Mode::parse("Warn"), Some(Mode::Warn));
        assert_eq!(Mode::parse("off"), Some(Mode::Off));
        assert_eq!(Mode::parse("bogus"), None);
    }

}

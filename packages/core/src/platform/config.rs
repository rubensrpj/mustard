//! Enforcement configuration — a typed table that replaces the scattered
//! `MUSTARD_*_MODE` environment variables.
//!
//! Today every gate hook reads its own `MUSTARD_<NAME>_GATE_MODE` variable
//! with an ad-hoc `(process.env.X || 'strict').toLowerCase()` call (see
//! `close-gate.js`, `bash-native-redirect.js`, `model-routing-gate.js`, …).
//! That spreads the same parsing logic across a dozen hooks and gives no
//! single place to see what is on. [`EnforcementConfig`] is that single
//! place: a map from check name to [`Mode`].
//!
//! ## Resolution order
//!
//! [`EnforcementConfig::resolve`] layers three sources, last-wins:
//!
//! 1. **Defaults** — every check defaults to [`Mode::Strict`]; the JS hooks
//!    treat an unset variable as `strict`.
//! 2. **`mustard.json`** — an optional `enforcement` object: a `{ checkName:
//!    mode }` map, plus an optional `disabledChecks` array.
//! 3. **Environment** — `MUSTARD_<CHECK>_MODE` for each check (highest
//!    precedence; env always wins over the file), plus `MUSTARD_DISABLED_HOOKS`
//!    (comma-separated).
//!
//! ## Fail-open
//!
//! Parsing never panics. An unrecognised mode string, a malformed
//! `mustard.json`, or a check entry of the wrong type is skipped (and, for the
//! file, surfaced as [`Error::Config`] from [`EnforcementConfig::from_json`]).
//! [`EnforcementConfig::resolve`] swallows a bad file and proceeds with
//! defaults + env, because a hook must never be blocked by a config typo.

use crate::platform::error::{Error, Result};
use serde_json::Value;
use std::collections::BTreeMap;

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

/// The enforcement table: per-check [`Mode`] plus a disabled-check set.
///
/// A check absent from [`EnforcementConfig::modes`] resolves to
/// [`Mode::default`] (strict) via [`EnforcementConfig::mode_of`]. A check named
/// in [`EnforcementConfig::disabled`] resolves to [`Mode::Off`] regardless of
/// its entry in `modes` — `MUSTARD_DISABLED_HOOKS` is an override, matching
/// `_lib/hook-env.js#shouldRun`.
#[derive(Debug, Clone, Default)]
pub struct EnforcementConfig {
    /// Explicit per-check modes. Keys are normalised lowercase check names.
    modes: BTreeMap<String, Mode>,
    /// Checks disabled outright (`MUSTARD_DISABLED_HOOKS` / `disabledChecks`).
    /// Normalised lowercase. Wins over any entry in `modes`.
    disabled: Vec<String>,
}

/// Normalise a check name: trimmed, lowercased. Keeps lookups case-insensitive
/// and consistent between the file, the env, and callers.
fn normalize(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

impl EnforcementConfig {
    /// An empty config — every check resolves to its default mode (strict).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the mode for a single check. Returns `self` for chaining.
    #[must_use]
    pub fn with_check(mut self, name: &str, mode: Mode) -> Self {
        self.modes.insert(normalize(name), mode);
        self
    }

    /// Mark a check disabled. Returns `self` for chaining.
    #[must_use]
    pub fn with_disabled(mut self, name: &str) -> Self {
        let key = normalize(name);
        if !self.disabled.contains(&key) {
            self.disabled.push(key);
        }
        self
    }

    /// The effective [`Mode`] for `name`.
    ///
    /// A disabled check is always [`Mode::Off`]. Otherwise the explicit entry
    /// is returned, falling back to [`Mode::default`] (strict) when the check
    /// has no entry — the JS hooks treat an unset variable as strict.
    #[must_use]
    pub fn mode_of(&self, name: &str) -> Mode {
        let key = normalize(name);
        if self.disabled.contains(&key) {
            return Mode::Off;
        }
        self.modes.get(&key).copied().unwrap_or_default()
    }

    /// `true` if `name` is in the disabled set.
    #[must_use]
    pub fn is_disabled(&self, name: &str) -> bool {
        self.disabled.contains(&normalize(name))
    }

    /// `true` if the check runs at all (mode is `Warn` or `Strict`).
    /// Shorthand for `self.mode_of(name).is_active()`.
    #[must_use]
    pub fn is_enabled(&self, name: &str) -> bool {
        self.mode_of(name).is_active()
    }

    /// Build a config from a parsed `mustard.json` value.
    ///
    /// Reads the optional `enforcement` object:
    /// - each `{ "checkName": "off|warn|strict" }` pair becomes a mode entry;
    /// - an optional `disabledChecks` array of strings populates the disabled
    ///   set.
    ///
    /// A check whose value is not a recognised mode string is skipped (it then
    /// resolves to the strict default). Resolution is lenient on individual
    /// entries.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Config`] if `enforcement` is present but is not a JSON
    /// object — that is a structural mistake the caller should know about
    /// (though [`EnforcementConfig::resolve`] swallows it and falls back).
    pub fn from_json(root: &Value) -> Result<Self> {
        let mut config = Self::new();

        let Some(enforcement) = root.get("enforcement") else {
            return Ok(config);
        };
        let enforcement = enforcement
            .as_object()
            .ok_or_else(|| Error::config("`enforcement` must be a JSON object"))?;

        for (key, value) in enforcement {
            if key == "disabledChecks" {
                if let Some(list) = value.as_array() {
                    for item in list {
                        if let Some(name) = item.as_str() {
                            config = config.with_disabled(name);
                        }
                    }
                }
                continue;
            }
            // A check entry: value must be a recognised mode string. Anything
            // else is skipped (fail-open: the check keeps its strict default).
            if let Some(mode) = value.as_str().and_then(Mode::parse) {
                config = config.with_check(key, mode);
            }
        }

        Ok(config)
    }

    /// Apply environment overrides on top of this config.
    ///
    /// For each `(check, env_var)` pair in `env_overrides`, if `env_var` is set
    /// to a recognised mode string the check's mode is replaced. Additionally,
    /// `MUSTARD_DISABLED_HOOKS` (read from `disabled_hooks_raw`) is split on
    /// commas and each entry is added to the disabled set.
    ///
    /// Environment always wins over the file — this is the highest-precedence
    /// layer. An unrecognised mode string is ignored (the file value or the
    /// strict default stands).
    #[must_use]
    pub fn apply_env_overrides<'a, I>(
        mut self,
        env_overrides: I,
        disabled_hooks_raw: Option<&str>,
    ) -> Self
    where
        I: IntoIterator<Item = (&'a str, Option<&'a str>)>,
    {
        for (check, raw_mode) in env_overrides {
            if let Some(mode) = raw_mode.and_then(Mode::parse) {
                self.modes.insert(normalize(check), mode);
            }
        }
        if let Some(raw) = disabled_hooks_raw {
            for name in raw.split(',') {
                let key = normalize(name);
                if !key.is_empty() && !self.disabled.contains(&key) {
                    self.disabled.push(key);
                }
            }
        }
        self
    }

    /// Resolve the full config from `mustard.json` text and the live process
    /// environment.
    ///
    /// Layers, last-wins: defaults → `mustard.json` `enforcement` block →
    /// `MUSTARD_<CHECK>_MODE` env vars + `MUSTARD_DISABLED_HOOKS`.
    ///
    /// Fail-open: a `mustard.json` that is absent, not valid JSON, or has a
    /// malformed `enforcement` block is treated as empty — resolution proceeds
    /// with defaults + env. A hook must never be blocked by a config typo.
    ///
    /// `mustard_json` is the raw file contents (`None` when the file is
    /// absent). `env_var` is a lookup closure — pass `|k| std::env::var(k).ok()`
    /// in production, or a fake map in tests. `env_var` is also used to read
    /// each `MUSTARD_<CHECK>_MODE` variable: the variable name is derived from
    /// the check name as `MUSTARD_` + uppercased, `-` → `_`, + `_MODE`.
    #[must_use]
    pub fn resolve<F>(mustard_json: Option<&str>, checks: &[&str], env_var: F) -> Self
    where
        F: Fn(&str) -> Option<String>,
    {
        // Layer 1+2: defaults + mustard.json. A parse failure falls back to an
        // empty config rather than propagating — fail-open.
        let from_file = mustard_json
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .and_then(|value| Self::from_json(&value).ok())
            .unwrap_or_default();

        // Layer 3: env. Derive `MUSTARD_<CHECK>_MODE` for each known check;
        // an env value always overrides the file value for that check.
        let mut config = from_file;
        for check in checks {
            let var = env_var_name_for(check);
            if let Some(mode) = env_var(&var).as_deref().and_then(Mode::parse) {
                config.modes.insert(normalize(check), mode);
            }
        }
        let disabled = env_var("MUSTARD_DISABLED_HOOKS");
        config.apply_env_overrides(std::iter::empty(), disabled.as_deref())
    }
}

/// Derive the environment variable name for a check.
///
/// `close-gate` → `MUSTARD_CLOSE_GATE_MODE`, matching the existing JS naming
/// (`MUSTARD_CLOSE_GATE_MODE`, `MUSTARD_BASH_REDIRECT_MODE`, …).
#[must_use]
pub fn env_var_name_for(check: &str) -> String {
    let body = check.trim().to_ascii_uppercase().replace('-', "_");
    format!("MUSTARD_{body}_MODE")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn mode_parse_is_case_insensitive_and_trims() {
        assert_eq!(Mode::parse("  STRICT "), Some(Mode::Strict));
        assert_eq!(Mode::parse("Warn"), Some(Mode::Warn));
        assert_eq!(Mode::parse("off"), Some(Mode::Off));
        assert_eq!(Mode::parse("bogus"), None);
    }

    #[test]
    fn unset_check_defaults_to_strict() {
        let config = EnforcementConfig::new();
        assert_eq!(config.mode_of("close-gate"), Mode::Strict);
    }

    #[test]
    fn disabled_check_is_always_off() {
        let config = EnforcementConfig::new()
            .with_check("close-gate", Mode::Strict)
            .with_disabled("close-gate");
        assert_eq!(config.mode_of("close-gate"), Mode::Off);
        assert!(!config.is_enabled("close-gate"));
    }

    #[test]
    fn from_json_reads_modes_and_disabled() {
        let json = serde_json::json!({
            "enforcement": {
                "close-gate": "warn",
                "model-gate": "off",
                "bad-check": "nonsense",
                "disabledChecks": ["spec-size", "convention"]
            }
        });
        let config = EnforcementConfig::from_json(&json).expect("valid enforcement");
        assert_eq!(config.mode_of("close-gate"), Mode::Warn);
        assert_eq!(config.mode_of("model-gate"), Mode::Off);
        // Unrecognised mode string is skipped → falls back to strict default.
        assert_eq!(config.mode_of("bad-check"), Mode::Strict);
        assert!(config.is_disabled("spec-size"));
        assert!(config.is_disabled("convention"));
    }

    #[test]
    fn from_json_rejects_non_object_enforcement() {
        let json = serde_json::json!({ "enforcement": "strict" });
        assert!(matches!(
            EnforcementConfig::from_json(&json),
            Err(Error::Config(_))
        ));
    }

    #[test]
    fn from_json_absent_enforcement_is_empty() {
        let json = serde_json::json!({ "git": { "main": "main" } });
        let config = EnforcementConfig::from_json(&json).expect("no enforcement block");
        assert_eq!(config.mode_of("anything"), Mode::Strict);
    }

    #[test]
    fn env_var_name_derivation() {
        assert_eq!(env_var_name_for("close-gate"), "MUSTARD_CLOSE_GATE_MODE");
        assert_eq!(
            env_var_name_for("bash-redirect"),
            "MUSTARD_BASH_REDIRECT_MODE"
        );
    }

    #[test]
    fn resolve_layers_file_then_env() {
        let mustard_json = r#"{ "enforcement": { "close-gate": "warn" } }"#;
        let env: HashMap<&str, &str> = HashMap::from([
            // Env overrides the file: close-gate file=warn, env=strict → strict.
            ("MUSTARD_CLOSE_GATE_MODE", "strict"),
            ("MUSTARD_DISABLED_HOOKS", "spec-size, model-gate"),
        ]);
        let config = EnforcementConfig::resolve(
            Some(mustard_json),
            &["close-gate", "model-gate", "spec-size"],
            |k| env.get(k).map(|s| (*s).to_string()),
        );
        assert_eq!(config.mode_of("close-gate"), Mode::Strict);
        // Disabled via env wins regardless of any mode entry.
        assert!(config.is_disabled("spec-size"));
        assert!(config.is_disabled("model-gate"));
        assert_eq!(config.mode_of("model-gate"), Mode::Off);
    }

    #[test]
    fn resolve_with_malformed_json_falls_back_to_defaults() {
        let config = EnforcementConfig::resolve(
            Some("{ not valid json"),
            &["close-gate"],
            |_| None,
        );
        // Malformed file is swallowed; defaults stand.
        assert_eq!(config.mode_of("close-gate"), Mode::Strict);
    }
}

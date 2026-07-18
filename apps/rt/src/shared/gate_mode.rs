//! `gate_mode` — the shared three-state gate mode (`off` / `warn` / `strict`)
//! and its cascade resolver, consumed by every `MUSTARD_*_MODE` gate across
//! **both** faces: the `hooks` size/validate gates and the `commands`
//! close-gate policy engine.
//!
//! Keeping the enum + resolver here (rather than in either the size gate or the
//! close-gate module) preserves the clean dependency DAG documented on
//! [`super`]: `hooks` and `commands` both depend on `shared`, never the reverse.
//! A size gate reaching into the close-gate command module for a generic mode
//! enum would blur that layering — this module exists so it does not have to.
//!
//! Before this module the enum and its cascade were copy-pasted three times
//! (the size gate, the close sub-gates, the QA-composition gate), each re-
//! implementing the same env → config → default resolution with a different
//! hard-coded default. The single [`resolve_mode`] now takes the default as a
//! parameter, so each call-site keeps its own (`warn` for the size and
//! composition gates, `strict` for the close sub-gates).

/// A three-state gate mode, `off` / `warn` / `strict`. The JS `resolveMode`
/// lowercases the env var and falls back to the caller's default for any
/// unrecognised value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GateMode {
    Off,
    Warn,
    Strict,
}

/// Resolve a `MUSTARD_*_MODE` mode in cascade: env var → `mustard.json`
/// (`gates.<field>`, supplied as `config_override`) → the caller's `default`.
///
/// An env var set to a non-empty value wins; otherwise the config override is
/// tried; an absent string OR an unrecognised value falls through to `default`
/// — matching `resolveMode` / `getMode` in the JS hooks. Each call-site passes
/// the default its family uses: the size/validate gates and the QA-composition
/// gate `warn`, the close sub-gates `strict`.
pub(crate) fn resolve_mode(
    env_var: &str,
    config_override: Option<&str>,
    default: GateMode,
) -> GateMode {
    let s = std::env::var(env_var)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| config_override.map(str::to_string));
    match s.unwrap_or_default().to_ascii_lowercase().as_str() {
        "off" => GateMode::Off,
        "warn" => GateMode::Warn,
        "strict" => GateMode::Strict,
        _ => default,
    }
}

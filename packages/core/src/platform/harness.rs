//! The version of the RUNNING harness — the single number every stamp, drift
//! check and statusline segment compares against.
//!
//! It is baked into the binary AT BUILD TIME, so a running binary always knows
//! its own version without reading anything at runtime. Truth order:
//! 1. `MUSTARD_RELEASE_VERSION` — injected by the release workflow from the git
//!    tag (`vX.Y.Z`, which the release gate verifies equals
//!    `plugin.json#version`, the line `bump-on-main` advances on every main
//!    merge). A SHIPPED binary carries this — the version follows the release.
//! 2. `CARGO_PKG_VERSION` — the unified `[workspace.package]` version, for a
//!    local / dev / CI build the release env did not stamp. An honest "this is
//!    a dev build" answer, not a stale lie.
//!
//! `mustard.json#version` therefore records which harness last set the project
//! up. The 3.1.x stamps in the field are the pre-plugin CLI era: they read as
//! drift once, and the first `/mustard:upsert` realigns them to this line.
//!
//! The module answers a SECOND question, which the first cannot:
//! [`installed_harness_version`] reads what Claude Code's plugin registry
//! records as installed. A running binary and the stamp it writes always agree,
//! so the pair above can never see the window between an update landing on disk
//! and the operator reloading — during which the session runs the old prose and
//! looks perfectly aligned. Only the registry knows.

/// Resolve the running harness version — release-stamped when shipped, the
/// workspace version otherwise. Never empty.
#[must_use]
pub fn harness_version() -> String {
    option_env!("MUSTARD_RELEASE_VERSION")
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or(env!("CARGO_PKG_VERSION"))
        .to_string()
}

/// The plugin name this harness ships under, as it appears on the left of the
/// `{plugin}@{marketplace}` key in the registry. The marketplace half varies by
/// how the operator added it (`mustard@mustard`, `mustard@mustard-local`), so
/// only this half is matched.
const PLUGIN_NAME: &str = "mustard";

/// Claude Code's registry of installed plugins, relative to its config
/// directory.
const INSTALLED_PLUGINS: &str = "plugins/installed_plugins.json";

/// The version the plugin registry records as INSTALLED — what the next
/// session would load, as opposed to [`harness_version`], which is what THIS
/// process is.
///
/// The two are the same number almost always, and differ in exactly the window
/// that matters: between an update landing on disk and the operator reloading,
/// the session keeps running the old plugin. Nothing else in the product can
/// see that window — the stamp in `mustard.json` is written BY the running
/// harness, so it agrees with the running harness by construction and a stale
/// session reads as perfectly aligned.
///
/// `None` when the registry cannot be located, read or parsed, or when it lists
/// no install of this plugin — every one of which means "we cannot prove the
/// session is behind", the only safe direction for an advisory. Never panics.
#[must_use]
pub fn installed_harness_version() -> Option<String> {
    let raw = std::fs::read_to_string(claude_config_dir()?.join(INSTALLED_PLUGINS)).ok()?;
    installed_harness_version_from(&raw)
}

/// The testable half of [`installed_harness_version`]: the registry's JSON text
/// in, the recorded version out.
///
/// The registry keys installs by `{plugin}@{marketplace}` and maps each to an
/// ARRAY — one record per scope (user, project). The highest version among this
/// plugin's records wins: "installed" is what a reload would pick up, and a
/// lower-scoped leftover must not make a current session look stale.
#[must_use]
pub fn installed_harness_version_from(raw: &str) -> Option<String> {
    let doc: serde_json::Value = serde_json::from_str(raw).ok()?;
    let plugins = doc.get("plugins")?.as_object()?;
    plugins
        .iter()
        .filter(|(key, _)| key.split('@').next() == Some(PLUGIN_NAME))
        .filter_map(|(_, records)| records.as_array())
        .flatten()
        .filter_map(|record| record.get("version")?.as_str())
        .filter(|version| !version.is_empty())
        .max_by(|a, b| compare_versions(a, b))
        .map(str::to_string)
}

/// Whether `running` names a version strictly OLDER than `installed`.
///
/// Dotted numeric components, compared left to right and zero-padded to the
/// longer side, so `0.1.9 < 0.1.10` (a plain string comparison gets that
/// backwards). A component that is not a number compares as 0 — a pre-release
/// suffix must never READ as an upgrade, and the honest answer for "we cannot
/// order these" is "not behind", which stays silent.
#[must_use]
pub fn is_behind(running: &str, installed: &str) -> bool {
    compare_versions(running, installed) == std::cmp::Ordering::Less
}

/// Order two dotted version strings by their numeric components. See
/// [`is_behind`] for why a non-numeric component counts as 0.
fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let parts = |v: &str| -> Vec<u64> {
        v.split(['.', '-', '+'])
            .map(|p| p.parse::<u64>().unwrap_or(0))
            .collect()
    };
    let (left, right) = (parts(a), parts(b));
    let width = left.len().max(right.len());
    for i in 0..width {
        let ordering = left.get(i).copied().unwrap_or(0).cmp(&right.get(i).copied().unwrap_or(0));
        if ordering != std::cmp::Ordering::Equal {
            return ordering;
        }
    }
    std::cmp::Ordering::Equal
}

/// Claude Code's config directory: `CLAUDE_CONFIG_DIR` when the operator moved
/// it, `~/.claude` otherwise (`HOME`, or `USERPROFILE` on Windows). `None` when
/// neither resolves — this crate reads no home directory through a dependency.
fn claude_config_dir() -> Option<std::path::PathBuf> {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return Some(std::path::PathBuf::from(dir));
    }
    let home = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(home)
        .filter(|h| !h.is_empty())
        .map(|h| std::path::PathBuf::from(h).join(".claude"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The entry never yields an empty string — either the release stamp or
    /// this crate's compiled-in workspace version.
    #[test]
    fn harness_version_is_never_empty() {
        assert!(!harness_version().is_empty());
    }

    /// Absent the release env, the answer is the crate's own `CARGO_PKG_VERSION`
    /// (the workspace version) — the dev/local build path.
    #[test]
    fn harness_version_falls_back_to_cargo_pkg_version() {
        // In the test build `MUSTARD_RELEASE_VERSION` is unset (nothing stamps
        // it), so the fallback is exercised directly.
        if option_env!("MUSTARD_RELEASE_VERSION").is_none() {
            assert_eq!(harness_version(), env!("CARGO_PKG_VERSION"));
        }
    }

    /// The registry keys installs by `{plugin}@{marketplace}` and the
    /// marketplace half varies by how the operator added it, so only the plugin
    /// name is matched — and a foreign plugin's version is never mistaken for
    /// ours.
    #[test]
    fn the_registry_answers_this_plugins_version_whatever_the_marketplace() {
        let raw = r#"{
          "version": 2,
          "plugins": {
            "mustard@mustard-local": [{"scope":"user","version":"0.1.43"}],
            "rust-analyzer-lsp@claude-plugins-official": [{"scope":"user","version":"9.9.9"}]
          }
        }"#;
        assert_eq!(installed_harness_version_from(raw).as_deref(), Some("0.1.43"));
    }

    /// Several records (one per scope) resolve to the HIGHEST: "installed" is
    /// what a reload would pick up, and a lower-scoped leftover must not make a
    /// current session read as stale.
    #[test]
    fn the_highest_recorded_install_wins() {
        let raw = r#"{"plugins":{"mustard@mustard":[
          {"scope":"project","version":"0.1.9"},
          {"scope":"user","version":"0.1.10"}
        ]}}"#;
        assert_eq!(installed_harness_version_from(raw).as_deref(), Some("0.1.10"));
    }

    /// Every unreadable shape is one answer — `None`, "we cannot prove the
    /// session is behind" — because the caller is an advisory.
    #[test]
    fn an_unreadable_registry_proves_nothing() {
        assert_eq!(installed_harness_version_from("not json").as_deref(), None);
        assert_eq!(installed_harness_version_from("{}").as_deref(), None);
        assert_eq!(installed_harness_version_from(r#"{"plugins":{}}"#).as_deref(), None);
        assert_eq!(
            installed_harness_version_from(r#"{"plugins":{"other@m":[{"version":"1.0.0"}]}}"#)
                .as_deref(),
            None
        );
    }

    /// Dotted components compare NUMERICALLY: `0.1.9` is behind `0.1.10`, which
    /// a plain string comparison gets backwards. Equal and ahead are both
    /// "not behind" — the advisory only ever fires on a session running old
    /// prose.
    #[test]
    fn behind_is_a_numeric_component_comparison() {
        assert!(is_behind("0.1.9", "0.1.10"));
        assert!(is_behind("0.1.42", "0.2.0"));
        assert!(!is_behind("0.1.42", "0.1.42"));
        assert!(!is_behind("0.1.43", "0.1.42"));
        assert!(!is_behind("0.1.42", "0.1.42-rc1"), "a pre-release never reads as an upgrade");
    }
}

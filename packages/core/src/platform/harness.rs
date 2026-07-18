//! The version of the RUNNING harness — the single source every stamp, drift
//! check and statusline segment compares against.
//!
//! Truth order:
//! 1. The installed plugin's manifest —
//!    `$CLAUDE_PLUGIN_ROOT/.claude-plugin/plugin.json#version`. Hook and `run`
//!    processes launched by the plugin carry that env var, and the manifest is
//!    what the `bump-on-main` automation advances on every main merge (the
//!    canonical release line).
//! 2. This crate's own `CARGO_PKG_VERSION` — the CLI / dev / CI path. Same
//!    0.1.x line; at worst a patch behind, which costs one realign advisory
//!    (the next `/mustard:upsert` run via the plugin restamps).
//!
//! `mustard.json#version` therefore records "which harness last set this
//! project up" — no longer the CLI's crate version. The 3.1.x stamps in the
//! field are the pre-plugin era: they read as drift once, and the first
//! `/mustard:upsert` realigns them.

use std::path::Path;

use crate::io::fs;

/// Resolve the running harness version. Total: degrades to this crate's
/// version when no plugin manifest is reachable.
#[must_use]
pub fn harness_version() -> String {
    plugin_manifest_version().unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
}

/// The version of the plugin manifest the current process was launched from,
/// via `CLAUDE_PLUGIN_ROOT`. Absent env var / file / key ⇒ `None`.
fn plugin_manifest_version() -> Option<String> {
    let root = std::env::var("CLAUDE_PLUGIN_ROOT").ok()?;
    manifest_version_at(Path::new(&root))
}

/// `<plugin_root>/.claude-plugin/plugin.json#version`, when readable.
fn manifest_version_at(plugin_root: &Path) -> Option<String> {
    let path = plugin_root.join(".claude-plugin").join("plugin.json");
    let text = fs::read_to_string(&path).ok()?;
    let manifest: serde_json::Value = serde_json::from_str(&text).ok()?;
    manifest
        .get("version")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// The inner reader is exercised directly (no process-global env mutation,
    /// so parallel tests cannot race).
    #[test]
    fn manifest_version_reads_the_plugin_json() {
        let dir = tempdir().unwrap();
        let manifest_dir = dir.path().join(".claude-plugin");
        std::fs::create_dir_all(&manifest_dir).unwrap();
        std::fs::write(
            manifest_dir.join("plugin.json"),
            r#"{"name":"mustard","version":"9.8.7"}"#,
        )
        .unwrap();
        assert_eq!(manifest_version_at(dir.path()).as_deref(), Some("9.8.7"));
    }

    #[test]
    fn manifest_version_degrades_on_missing_or_malformed() {
        let dir = tempdir().unwrap();
        // No .claude-plugin/ at all.
        assert_eq!(manifest_version_at(dir.path()), None);
        // Malformed JSON.
        let manifest_dir = dir.path().join(".claude-plugin");
        std::fs::create_dir_all(&manifest_dir).unwrap();
        std::fs::write(manifest_dir.join("plugin.json"), "{not json").unwrap();
        assert_eq!(manifest_version_at(dir.path()), None);
    }

    /// The public entry never yields an empty string — either the manifest's
    /// version or this crate's own.
    #[test]
    fn harness_version_is_never_empty() {
        assert!(!harness_version().is_empty());
    }
}

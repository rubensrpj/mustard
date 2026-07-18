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
}

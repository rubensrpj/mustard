//! The workspace version and the plugin manifest are ONE line — proven here.
//!
//! Mustard publishes its version from `plugin/.claude-plugin/plugin.json`: the
//! `bump-on-main` workflow advances it on every merge to main, tags `vX.Y.Z`
//! from it, and the release gate refuses a tag that disagrees with it. But the
//! Rust side reads a different number. A binary built without
//! `MUSTARD_RELEASE_VERSION` — which is every local build, every `install.ps1`
//! run and every CI build — resolves
//! [`mustard_core::harness_version`] to `CARGO_PKG_VERSION`, i.e. the
//! `[workspace.package] version` in the root `Cargo.toml`.
//!
//! Nothing connected the two, so the workspace version sat at `0.1.0` while the
//! plugin reached `0.1.28`. The consequence was not cosmetic: that number is
//! what `mustard-rt --version` prints, what the statusline renders as
//! `m{version}`, and what every install stamps into a project's
//! `mustard.json#version`. A stale value there does not read as "this is a dev
//! build" — it reads as a real, old release, and the harness's own drift
//! warning starts firing against itself.
//!
//! ## Why `Cargo.lock` is NOT checked here
//!
//! There is a third file carrying this version — `Cargo.lock` pins one per
//! workspace member — and leaving it behind is what broke the v0.1.29 release
//! on all three operating systems ("cannot update the lock file because
//! `--locked` was passed", before a line compiled).
//!
//! A test for it was written, and then measured: it CANNOT fail. Plain
//! `cargo test` repairs a stale lock before running anything, so the assertion
//! never sees the divergence; `cargo test --locked` fails in cargo itself,
//! before any test binary starts. Either way the assertion is decoration, and
//! decoration that looks like a guard is worse than no guard.
//!
//! The real guard is `--locked`, which CI and the release already pass. The gap
//! was never detection — it was that the ONE commit which moved the version
//! without the lock is the `bump-on-main` commit, and a push made with
//! `GITHUB_TOKEN` does not trigger workflows (anti-recursion), so it is the one
//! commit CI never sees. Fixed where it belongs: that workflow now runs
//! `cargo update --workspace` and commits the lock with the manifest.

use std::path::{Path, PathBuf};

/// The plugin manifest, relative to the workspace root.
const MANIFEST_REL: &str = "plugin/.claude-plugin/plugin.json";

#[test]
fn the_workspace_version_equals_the_published_plugin_version() {
    let Some(manifest) = find_manifest() else {
        // Compiled outside the repository (a vendored or packaged source tree):
        // there is no manifest to agree with, and inventing a failure there
        // would break a legitimate build. Absence is missing evidence.
        return;
    };
    let raw = std::fs::read_to_string(&manifest).expect("plugin manifest is readable");
    let published = json_string_field(&raw, "version")
        .unwrap_or_else(|| panic!("no `version` field in {}", manifest.display()));

    assert_eq!(
        env!("CARGO_PKG_VERSION"),
        published,
        "the workspace version and {MANIFEST_REL} disagree.\n\
         `bump-on-main` advances the manifest on every merge to main; the root \
         Cargo.toml must move with it, because a locally built binary falls back \
         to the workspace version for `--version`, the statusline mark and the \
         `mustard.json` stamp. Set `[workspace.package] version` to {published}."
    );
}

/// …and the running harness agrees with both. This is the value every consumer
/// actually reads, so proving the file matches is not enough on its own —
/// `harness_version` could grow a third source tomorrow.
#[test]
fn the_running_harness_reports_that_same_line() {
    let running = mustard_core::harness_version();
    assert!(!running.is_empty(), "the harness version is never empty");

    // A release build stamps `MUSTARD_RELEASE_VERSION` from the tag, and the
    // release gate already proved the tag equals the manifest — so that path is
    // covered elsewhere and would only be re-asserted here by duplicating the
    // gate. The path this file exists for is the UNSTAMPED one.
    if option_env!("MUSTARD_RELEASE_VERSION").is_none() {
        assert_eq!(
            running,
            env!("CARGO_PKG_VERSION"),
            "an unstamped build must report the workspace version"
        );
    }
}

/// Walk up from this crate to the workspace root — the directory holding the
/// plugin manifest. `None` when there is none.
fn workspace_root() -> Option<PathBuf> {
    let mut dir: &Path = Path::new(env!("CARGO_MANIFEST_DIR"));
    loop {
        if dir.join(MANIFEST_REL).is_file() {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}

/// The plugin manifest itself. `None` when there is none — see the caller for
/// why that is not a failure.
fn find_manifest() -> Option<PathBuf> {
    workspace_root().map(|root| root.join(MANIFEST_REL))
}

/// Read one top-level `"key": "value"` string out of a JSON document without
/// pulling a parser into this crate's dev-dependencies for one field.
fn json_string_field(raw: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let rest = raw.split_once(&needle)?.1;
    let rest = rest.trim_start().strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let (value, _) = rest.split_once('"')?;
    Some(value.to_string())
}

#[cfg(test)]
mod helper_tests {
    use super::json_string_field;

    #[test]
    fn reads_the_field_and_ignores_the_rest() {
        let doc = "{\n  \"name\": \"mustard\",\n  \"version\": \"1.2.3\",\n  \"x\": 1\n}";
        assert_eq!(json_string_field(doc, "version").as_deref(), Some("1.2.3"));
        assert_eq!(json_string_field(doc, "name").as_deref(), Some("mustard"));
        assert_eq!(json_string_field(doc, "absent"), None);
    }
}

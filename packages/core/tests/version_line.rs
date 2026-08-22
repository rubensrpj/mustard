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
//! ## Which `Cargo.lock` is checked here, and which is not
//!
//! A third kind of file carries this version: `Cargo.lock` pins one per local
//! package. There are TWO of them in this repository, and they need opposite
//! treatment.
//!
//! **The root lock is NOT checked.** A test for it was written, and then
//! measured: it CANNOT fail. Plain `cargo test` repairs a stale root lock
//! before running anything, so the assertion never sees the divergence;
//! `cargo test --locked` fails in cargo itself, before any test binary starts.
//! Either way the assertion is decoration, and decoration that looks like a
//! guard is worse than no guard. The real guard there is `--locked`, which CI
//! and the release already pass, plus the `cargo update --workspace` that
//! `bump-on-main` runs — leaving that lock behind is what broke the v0.1.29
//! release on all three operating systems ("cannot update the lock file because
//! `--locked` was passed", before a line compiled).
//!
//! **The dashboard lock IS checked**, and the paragraph above is exactly why it
//! has to be. `apps/dashboard/src-tauri` is deliberately its own workspace root,
//! so no root build ever resolves it: nothing repairs it before this test reads
//! it as a plain file, which is what makes the assertion able to fail. Nothing
//! else looks either — CI excludes the dashboard on purpose (it needs per-OS
//! system libraries), and the release builds it through `tauri build` WITHOUT
//! `--locked`, so a stale lock is silently repaired at build time and the repair
//! is thrown away instead of committed. Measured on 2026-08-22: that lock still
//! named `mustard-cli` and `mustard-core` at `0.1.41` while the workspace was at
//! `0.1.44` — three releases behind, and nothing in the repository had noticed.
//! `bump-on-main` now advances that lock too; this test is what says so out loud
//! if it ever stops.

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

/// The dashboard's own workspace lock, relative to the workspace root.
const DASHBOARD_LOCK_REL: &str = "apps/dashboard/src-tauri/Cargo.lock";

/// The dashboard's own manifest, relative to the workspace root.
const DASHBOARD_MANIFEST_REL: &str = "apps/dashboard/src-tauri/Cargo.toml";

/// One `[[package]]` record of a `Cargo.lock`, reduced to the three fields this
/// file asks about.
struct LockPackage {
    name: String,
    version: String,
    /// `None` for a package that lives in this repository (reached by path);
    /// a package pulled from a registry carries a `source` line.
    source: Option<String>,
}

/// Every `[[package]]` record of a lock file.
///
/// Hand-rolled rather than pulling a TOML parser into this crate's
/// dev-dependencies for three fields — the same trade `json_string_field` above
/// already makes. Only column-0 `key = "value"` lines count, so the quoted
/// entries inside a `dependencies = [...]` list are never mistaken for fields.
fn lock_packages(raw: &str) -> Vec<LockPackage> {
    let mut out = Vec::new();
    for block in raw.split("[[package]]").skip(1) {
        let field = |key: &str| {
            block
                .lines()
                .take_while(|line| !line.starts_with('['))
                .find_map(|line| line.strip_prefix(key)?.strip_prefix(" = \"")?.strip_suffix('"'))
                .map(str::to_string)
        };
        if let (Some(name), Some(version)) = (field("name"), field("version")) {
            out.push(LockPackage { name, version, source: field("source") });
        }
    }
    out
}

/// The `[package] name` of a manifest — the one package in the dashboard's lock
/// that is NOT ours to version.
fn package_name(manifest: &Path) -> Option<String> {
    std::fs::read_to_string(manifest).ok()?.lines().find_map(|line| {
        line.strip_prefix("name = \"")?.strip_suffix('"').map(str::to_string)
    })
}

/// Every crate of THIS repository that the dashboard consumes must be pinned at
/// THIS repository's version in the dashboard's own lock.
///
/// This is the assertion the module doc argues for: the dashboard is its own
/// workspace root, so no root build resolves its lock, and this test reads that
/// file as data — nothing repairs it first, which is what lets the assertion
/// fail. `mustard-dashboard` itself is excluded because it carries a version of
/// its own on purpose; every other local package is one of ours.
#[test]
fn the_dashboard_lock_pins_this_repositorys_crates_at_this_version() {
    let Some(root) = workspace_root() else {
        // Compiled outside the repository — see the sibling test for why absence
        // is missing evidence rather than a failure.
        return;
    };
    let lock_path = root.join(DASHBOARD_LOCK_REL);
    let Ok(raw) = std::fs::read_to_string(&lock_path) else {
        eprintln!("[skip] {} is absent from this source tree", lock_path.display());
        return;
    };

    let own = package_name(&root.join(DASHBOARD_MANIFEST_REL)).unwrap_or_default();
    let ours: Vec<LockPackage> = lock_packages(&raw)
        .into_iter()
        .filter(|p| p.source.is_none() && p.name != own)
        .collect();

    // Without this the filters could silently match nothing — a reworded lock
    // format would turn the guard into a test that always passes, which is the
    // decoration the module doc refuses.
    assert!(
        !ours.is_empty(),
        "{} names no local package other than `{own}` — either the dashboard \
         stopped depending on this repository (then delete this test) or the \
         lock format moved and the parser above no longer reads it",
        lock_path.display()
    );

    let expected = env!("CARGO_PKG_VERSION");
    let stale: Vec<String> = ours
        .iter()
        .filter(|p| p.version != expected)
        .map(|p| format!("{} {}", p.name, p.version))
        .collect();
    assert!(
        stale.is_empty(),
        "{} still pins {stale:?}, but this repository is at {expected}.\n\
         Nothing repairs that file on its own: the dashboard is its own workspace \
         root, CI excludes it (per-OS system libraries) and the release builds it \
         without `--locked`, so a stale pin is patched at build time and thrown \
         away. Advance it with\n  \
         cargo update --workspace --manifest-path {DASHBOARD_MANIFEST_REL}\n\
         and commit the lock. `bump-on-main` runs that same line on every release \
         — if this is red after a release, that step is what broke.",
        lock_path.display()
    );
}

//! Artifact provenance — the shape of `apps/cli/templates/.artifacts.json`.
//!
//! Mustard vendors dozens of artifacts under `templates/` (skills, recipes,
//! refs, commands, hooks) and pins external tools such as RTK. Several of
//! those have an external upstream that keeps evolving; the manifest records
//! where each artifact came from, at which version, and (for vendored trees)
//! a checksum, so a maintainer-side `artifact-update --check` can flag drift
//! instead of comparing by hand.
//!
//! The manifest is **maintainer-side only** — it is not a `CORE_FOLDER` and is
//! never copied into a user installation. The types here are plain `serde`
//! data with no side effects; [`tree_checksum`] is the one helper that touches
//! the filesystem, and it is consumed by the `artifact-update` engine.

use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The full managed-artifact manifest (`apps/cli/templates/.artifacts.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactManifest {
    /// Manifest schema version. `1` today.
    pub schema_version: u32,
    /// Every managed artifact, one record each.
    pub artifacts: Vec<ArtifactRecord>,
}

/// One managed artifact: a vendored tree or a pinned external tool.
///
/// For vendored artifacts (skill / recipe / ref / command / hook) `path` and
/// `checksum` are populated; for a `tool` both are absent — the tool is not
/// vendored, only tracked by version.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactRecord {
    /// Stable identifier, e.g. `skill:design-craft`, `tool:rtk`.
    pub id: String,
    /// Which kind of artifact this is.
    pub category: ArtifactCategory,
    /// Where the artifact came from.
    pub source: ArtifactSource,
    /// Vendored version / tag, when the source carries one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// ISO-8601 date the artifact was last vendored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendored_at: Option<String>,
    /// Folder path relative to `apps/cli/templates/` (vendored artifacts only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// SHA-256 of the vendored tree (vendored artifacts only). Computed on
    /// demand by the `artifact-update` engine via [`tree_checksum`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
}

/// The kind of a managed artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactCategory {
    /// A foundation skill under `templates/skills/`.
    Skill,
    /// A structured recipe under `templates/recipes/`.
    Recipe,
    /// A progressive-disclosure ref tree under `templates/refs/`.
    Ref,
    /// A namespaced slash command under `templates/commands/mustard/`.
    Command,
    /// The enforcement / scripts payload.
    Hook,
    /// An external tool pinned by version, e.g. RTK.
    Tool,
}

/// Where a managed artifact originates.
///
/// The `kind` tag selects the variant; `first-party` / `manual` carry no
/// extra fields, while the external sources carry the coordinates needed to
/// check the upstream for newer versions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ArtifactSource {
    /// Authored in this repo; versions with the CLI. No external upstream.
    FirstParty,
    /// Vendored from a Git repository subtree.
    Git {
        /// Clone URL of the upstream repository.
        repo: String,
        /// Subdirectory within the repository, when the artifact is nested.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subdir: Option<String>,
        /// Git ref (branch / tag) the artifact was vendored from.
        #[serde(rename = "ref")]
        git_ref: String,
    },
    /// Vendored from the skills directory registry.
    SkillsDirectory {
        /// Registry slug, e.g. `nutlope/hallmark`.
        slug: String,
    },
    /// An external tool installed from a Cargo crate.
    Cargo {
        /// Crate name on crates.io.
        #[serde(rename = "crate")]
        crate_name: String,
    },
    /// Vendored from an upstream with no machine-checkable provenance.
    Manual,
}

/// SHA-256 over every file in `dir`, walked recursively.
///
/// Paths are sorted before hashing so the digest is stable regardless of
/// directory-iteration order. Each file contributes its relative path (as
/// bytes) followed by its contents, so a rename changes the digest. Written
/// for the `artifact-update` engine to detect drift between a vendored tree
/// and its upstream.
///
/// # Errors
///
/// Returns an [`io::Error`] if `dir` cannot be read or a file under it cannot
/// be opened.
pub fn tree_checksum(dir: &Path) -> io::Result<String> {
    let mut files = Vec::new();
    collect_files(dir, dir, &mut files)?;
    files.sort();

    let mut hasher = Sha256::new();
    for (rel, abs) in files {
        hasher.update(rel.as_bytes());
        hasher.update([0u8]);
        let bytes = crate::fs::read(&abs).map_err(|e| io::Error::other(e.to_string()))?;
        hasher.update(bytes);
    }
    Ok(hex_encode(&hasher.finalize()))
}

/// Recursively collect `(relative-path, absolute-path)` pairs for every file
/// under `dir`. Relative paths use `/` so the digest is platform-stable.
fn collect_files(
    root: &Path,
    dir: &Path,
    out: &mut Vec<(String, std::path::PathBuf)>,
) -> io::Result<()> {
    let entries = crate::fs::read_dir(dir).map_err(|e| io::Error::other(e.to_string()))?;
    for entry in entries {
        let path = entry.path;
        if entry.is_dir {
            collect_files(root, &path, out)?;
        } else {
            let rel = path
                .strip_prefix(root)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?
                .to_string_lossy()
                .replace('\\', "/");
            out.push((rel, path));
        }
    }
    Ok(())
}

/// Lower-case hex encoding of a byte slice.
fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real manifest must round-trip into [`ArtifactManifest`].
    #[test]
    fn manifest_round_trips() {
        let raw = r#"{
            "schemaVersion": 1,
            "artifacts": [
                {
                    "id": "skill:design-craft",
                    "category": "skill",
                    "source": {"kind": "manual"},
                    "version": null,
                    "vendoredAt": "2026-05-19",
                    "path": "skills/design-craft",
                    "checksum": null
                },
                {
                    "id": "skill:hallmark",
                    "category": "skill",
                    "source": {"kind": "skills-directory", "slug": "nutlope/hallmark"},
                    "vendoredAt": "2026-05-19",
                    "path": "skills/hallmark"
                },
                {
                    "id": "tool:rtk",
                    "category": "tool",
                    "source": {"kind": "cargo", "crate": "rtk"},
                    "version": null,
                    "vendoredAt": "2026-05-19"
                }
            ]
        }"#;
        let manifest: ArtifactManifest = serde_json::from_str(raw).expect("parse manifest");
        assert_eq!(manifest.schema_version, 1);
        assert_eq!(manifest.artifacts.len(), 3);
        assert_eq!(manifest.artifacts[0].category, ArtifactCategory::Skill);
        assert_eq!(manifest.artifacts[2].category, ArtifactCategory::Tool);
        assert!(manifest.artifacts[2].path.is_none());
    }

    /// The tagged `ArtifactSource` enum must serialize with `kind` + kebab-case.
    #[test]
    fn source_serializes_with_kind_tag() {
        let git = ArtifactSource::Git {
            repo: "https://github.com/mattpocock/skills".to_string(),
            subdir: Some("diagnose".to_string()),
            git_ref: "main".to_string(),
        };
        let json = serde_json::to_value(&git).expect("serialize git source");
        assert_eq!(json["kind"], "git");
        assert_eq!(json["ref"], "main");

        let cargo = ArtifactSource::Cargo { crate_name: "rtk".to_string() };
        let json = serde_json::to_value(&cargo).expect("serialize cargo source");
        assert_eq!(json["kind"], "cargo");
        assert_eq!(json["crate"], "rtk");

        let fp = serde_json::to_value(ArtifactSource::FirstParty).expect("serialize");
        assert_eq!(fp["kind"], "first-party");
    }

    /// `tree_checksum` is deterministic and path-sensitive.
    #[test]
    fn tree_checksum_is_stable_and_path_sensitive() {
        let dir = tempfile::tempdir().unwrap();
        crate::fs::write_atomic(&dir.path().join("a.txt"), b"alpha").unwrap();
        crate::fs::create_dir_all(&dir.path().join("sub")).unwrap();
        crate::fs::write_atomic(&dir.path().join("sub/b.txt"), b"beta").unwrap();

        let first = tree_checksum(dir.path()).unwrap();
        let second = tree_checksum(dir.path()).unwrap();
        assert_eq!(first, second, "checksum must be deterministic");
        assert_eq!(first.len(), 64, "sha256 hex is 64 chars");

        crate::fs::write_atomic(&dir.path().join("sub/b.txt"), b"gamma").unwrap();
        assert_ne!(first, tree_checksum(dir.path()).unwrap(), "content change shifts digest");
    }
}

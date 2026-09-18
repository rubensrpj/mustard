//! Artifact provenance — the shape of `apps/cli/templates/.artifacts.json`.
//!
//! Mustard vendors dozens of artifacts under `templates/` (skills, refs,
//! commands, hooks) and pins external tools such as RTK. The manifest records
//! where each artifact came from and at which version.
//!
//! The manifest is **maintainer-side only** — it is not a `CORE_FOLDER` and is
//! never copied into a user installation. What still reads it is the install,
//! which looks up the pinned RTK revision before offering to install the tool.
//! The types here are plain `serde` data with no side effects.

use serde::{Deserialize, Serialize};

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
/// For vendored artifacts (skill / ref / command / hook) `path` and
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
    /// SHA-256 of the vendored tree, as recorded when the tree was last
    /// vendored (vendored artifacts only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
}

/// The kind of a managed artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactCategory {
    /// A foundation skill under `templates/skills/`.
    Skill,
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
}

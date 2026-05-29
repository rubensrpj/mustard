//! `entity_registry` — the single owner of `.claude/entity-registry.json` (v4):
//! both the typed read surface ([`EntityRegistry`]) and the write/serialize
//! model ([`RegistryDoc`]).
//!
//! Before this module every consumer (`skill-resolve`, `knowledge`, `status`,
//! the `enforce-registry` hook, the `sync-registry` writer's populated-check)
//! hand-parsed the registry `serde_json::Value` with its own `.get("e")` /
//! `.get("_patterns")` / top-level-key walk, and the `sync-registry` writer
//! defined the v4 shape a *second* time in a local struct. Three of those read
//! walks predated the v4 move of entities under the `e` key and still iterated
//! the document root — in a v4 registry that walks the literal `"e"` key as if
//! it were an entity, so entity matching / counting silently broke. This module
//! is now the one home for the v4 shape: readers go through [`EntityRegistry`];
//! the writer assembles a [`RegistryDoc`] and calls [`RegistryDoc::write`].
//!
//! ## v4 schema
//!
//! ```json
//! {
//!   "_meta":     { "version": "4.0", "generated": "...", "generator": "..." },
//!   "_patterns": { "<stack>": { "discovered": [ { "label": "...", "subprojectName": "..." } ], ... } },
//!   "_enums":    { ... },
//!   "e":         { "<EntityName>": { "description": "...", "refs": [...], ... } }
//! }
//! ```
//!
//! The writer (`mustard-rt run sync-registry`) only ever emits `version: "4.0"`
//! and auto-upgrades anything older, so this reader targets v4 exclusively — no
//! hypothetical v3 fallback.
//!
//! ## Fail-open
//!
//! [`EntityRegistry::load`] degrades a missing / unreadable / unparseable file
//! to an empty registry (every accessor then returns the empty answer). Callers
//! that must distinguish "missing" from "parse error" (the status command, the
//! enforce-registry hook) keep their own read + branch and wrap the parsed
//! value via [`EntityRegistry::from_value`].

use std::path::Path;

use serde::Serialize;
use serde_json::{Map, Value};

use crate::io::claude_paths::ClaudePaths;
use crate::io::fs;
use crate::platform::error::{Error, Result};

/// Read-only view over a parsed v4 entity-registry document.
pub struct EntityRegistry {
    doc: Value,
}

impl EntityRegistry {
    /// Load `<project_root>/.claude/entity-registry.json`. Fail-open: a missing,
    /// unreadable, or invalid file yields an empty registry.
    #[must_use]
    pub fn load(project_root: &Path) -> Self {
        let doc = ClaudePaths::for_project(project_root)
            .ok()
            .map(|p| p.entity_registry_json_path())
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
            .unwrap_or(Value::Null);
        Self { doc }
    }

    /// Wrap an already-parsed document. Used by callers that read the file
    /// themselves to preserve a bespoke missing-vs-parse-error contract.
    #[must_use]
    pub fn from_value(doc: Value) -> Self {
        Self { doc }
    }

    /// `_meta.version`, when present.
    #[must_use]
    pub fn version(&self) -> Option<&str> {
        self.doc.get("_meta")?.get("version")?.as_str()
    }

    /// `_meta.generatedAt`, when present.
    #[must_use]
    pub fn generated_at(&self) -> Option<&str> {
        self.doc.get("_meta")?.get("generatedAt")?.as_str()
    }

    /// The `e` entity map (v4). `None` when the key is absent or not an object.
    #[must_use]
    pub fn entities(&self) -> Option<&Map<String, Value>> {
        self.doc.get("e").and_then(Value::as_object)
    }

    /// The `_enums` map (v4). `None` when the key is absent or not an object.
    ///
    /// Each value is either a bare member array (`["A","B"]`) or a rich object
    /// (`{ "values": [...], "file": "...", ... }`) — the writer emits either
    /// shape depending on whether file/decorator metadata was recovered.
    #[must_use]
    pub fn enums(&self) -> Option<&Map<String, Value>> {
        self.doc.get("_enums").and_then(Value::as_object)
    }

    /// Entity names — keys of `e`, excluding any `_`-prefixed sentinel (e.g. the
    /// `_placeholder` an empty registry carries).
    #[must_use]
    pub fn entity_names(&self) -> Vec<&str> {
        self.entities().map_or_else(Vec::new, |e| {
            e.keys()
                .filter(|k| !k.starts_with('_'))
                .map(String::as_str)
                .collect()
        })
    }

    /// Number of real entities (excludes `_`-prefixed sentinels).
    #[must_use]
    pub fn entity_count(&self) -> usize {
        self.entities().map_or(0, |e| {
            e.keys().filter(|k| !k.starts_with('_')).count()
        })
    }

    /// The `_patterns` map (v4) — `{stack}` → `{ discovered[], folderFrequency,
    /// conventions, architecture, ... }`. `None` when the key is absent or not
    /// an object.
    #[must_use]
    pub fn patterns(&self) -> Option<&Map<String, Value>> {
        self.doc.get("_patterns").and_then(Value::as_object)
    }

    /// Whether `_patterns` is a present, non-empty object.
    #[must_use]
    pub fn has_patterns(&self) -> bool {
        self.doc
            .get("_patterns")
            .and_then(Value::as_object)
            .is_some_and(|o| !o.is_empty())
    }

    /// Lowercased cluster labels declared in `_patterns.{stack}.discovered[]`.
    ///
    /// When `subproject` is `Some`, a cluster is kept only when its
    /// `subprojectName` matches (the subproject path ends with the name, or they
    /// are equal). Clusters with no `subprojectName` always match. Deduplicated
    /// and returned in sorted order.
    #[must_use]
    pub fn cluster_labels(&self, subproject: Option<&str>) -> Vec<String> {
        use std::collections::BTreeSet;
        let mut labels: BTreeSet<String> = BTreeSet::new();
        let Some(patterns) = self.doc.get("_patterns").and_then(Value::as_object) else {
            return Vec::new();
        };
        for body in patterns.values() {
            let Some(arr) = body.get("discovered").and_then(Value::as_array) else {
                continue;
            };
            for cluster in arr {
                if let (Some(sub), Some(name)) = (
                    subproject,
                    cluster.get("subprojectName").and_then(Value::as_str),
                ) {
                    if !sub.ends_with(name) && name != sub {
                        continue;
                    }
                }
                if let Some(label) = cluster.get("label").and_then(Value::as_str) {
                    labels.insert(label.to_ascii_lowercase());
                }
            }
        }
        labels.into_iter().collect()
    }
}

// ===========================================================================
// Write model — the single owner of the on-disk v4 shape
// ===========================================================================

/// The `_meta` block of a v4 registry. Field order is pinned (`version`,
/// `generated`, `generator`) to keep the serialized bytes stable.
#[derive(Debug, Serialize)]
pub struct RegistryMeta {
    /// Schema version — always [`RegistryDoc::VERSION`].
    pub version: &'static str,
    /// Producer timestamp (the writer uses a `YYYY-MM-DD` date).
    pub generated: String,
    /// Human-readable producer tag.
    pub generator: &'static str,
}

/// The whole `entity-registry.json` v4 document, in canonical key order
/// (`_meta`, `_patterns`, `_enums`, `e`).
///
/// The `sync-registry` writer assembles the three payload objects (`_patterns`,
/// `_enums`, `e`) from its scan results and hands them here; this type owns the
/// envelope, the version, the byte-stable serialization, and the atomic write.
#[derive(Debug, Serialize)]
pub struct RegistryDoc {
    #[serde(rename = "_meta")]
    pub meta: RegistryMeta,
    #[serde(rename = "_patterns")]
    pub patterns: Value,
    #[serde(rename = "_enums")]
    pub enums: Value,
    /// Entity map — keyed by entity name.
    pub e: Value,
}

impl RegistryDoc {
    /// The only schema version this crate emits.
    pub const VERSION: &'static str = "4.0";

    /// Assemble a v4 document from its three payload objects. `generated` is the
    /// producer timestamp string; `generator` the producer tag.
    #[must_use]
    pub fn new(
        generated: String,
        generator: &'static str,
        patterns: Value,
        enums: Value,
        e: Value,
    ) -> Self {
        Self {
            meta: RegistryMeta {
                version: Self::VERSION,
                generated,
                generator,
            },
            patterns,
            enums,
            e,
        }
    }

    /// Serialize to the canonical pretty JSON with a trailing newline.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Parse`] if serialization fails (never expected for a
    /// well-formed document).
    pub fn to_pretty_json(&self) -> Result<String> {
        Ok(format!("{}\n", serde_json::to_string_pretty(self)?))
    }

    /// Write atomically to `<project_root>/.claude/entity-registry.json`.
    ///
    /// # Errors
    ///
    /// [`Error::Config`] when `project_root` has no `.claude` anchor,
    /// [`Error::Parse`] on a serialization failure, or [`Error::Io`] on a write
    /// failure.
    pub fn write(&self, project_root: &Path) -> Result<()> {
        let paths =
            ClaudePaths::for_project(project_root).map_err(|e| Error::config(e.to_string()))?;
        let json = self.to_pretty_json()?;
        fs::write_atomic(paths.entity_registry_json_path(), json.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn v4() -> EntityRegistry {
        EntityRegistry::from_value(json!({
            "_meta": { "version": "4.0", "generatedAt": "2026-05-28T00:00:00Z" },
            "_patterns": {
                "drizzle": {
                    "discovered": [
                        { "label": "User-CRUD", "subprojectName": "api" },
                        { "label": "Auth", "subprojectName": "web" },
                        { "label": "Shared" }
                    ]
                }
            },
            "_enums": {},
            "e": {
                "User": { "description": "the user", "refs": [{ "path": "src/user.rs" }] },
                "Post": { "ref": "src/post.rs" },
                "_placeholder": {}
            }
        }))
    }

    #[test]
    fn reads_meta_fields() {
        let r = v4();
        assert_eq!(r.version(), Some("4.0"));
        assert_eq!(r.generated_at(), Some("2026-05-28T00:00:00Z"));
    }

    #[test]
    fn entities_under_e_excluding_placeholder() {
        let r = v4();
        assert_eq!(r.entity_count(), 2);
        let mut names = r.entity_names();
        names.sort_unstable();
        assert_eq!(names, vec!["Post", "User"]);
    }

    #[test]
    fn has_patterns_true_when_non_empty() {
        assert!(v4().has_patterns());
        assert!(!EntityRegistry::from_value(json!({ "_patterns": {} })).has_patterns());
    }

    #[test]
    fn patterns_exposes_stack_map() {
        let r = v4();
        let patterns = r.patterns().expect("_patterns object");
        assert!(patterns.contains_key("drizzle"));
        let discovered = patterns["drizzle"]["discovered"].as_array().unwrap();
        assert_eq!(discovered.len(), 3);
        // Absent / non-object `_patterns` ⇒ None.
        assert!(EntityRegistry::from_value(Value::Null).patterns().is_none());
    }

    #[test]
    fn cluster_labels_filter_by_subproject() {
        let r = v4();
        // `api` subproject → its own cluster + the un-scoped "Shared".
        let api = r.cluster_labels(Some("apps/api"));
        assert_eq!(api, vec!["shared".to_string(), "user-crud".to_string()]);
        // No filter → every label.
        let all = r.cluster_labels(None);
        assert_eq!(all, vec!["auth".to_string(), "shared".to_string(), "user-crud".to_string()]);
    }

    #[test]
    fn empty_registry_is_fail_open() {
        let r = EntityRegistry::from_value(Value::Null);
        assert_eq!(r.version(), None);
        assert_eq!(r.entity_count(), 0);
        assert!(r.entity_names().is_empty());
        assert!(!r.has_patterns());
        assert!(r.cluster_labels(None).is_empty());
    }

    #[test]
    fn registry_doc_pins_v4_key_order() {
        let doc = RegistryDoc::new(
            "2026-05-28".to_string(),
            "mustard-rt run sync-registry",
            json!({ "drizzle": {} }),
            json!({}),
            json!({ "User": {} }),
        );
        assert_eq!(doc.meta.version, "4.0");
        let json = serde_json::to_string(&doc).unwrap();
        let meta = json.find("\"_meta\"").unwrap();
        let patterns = json.find("\"_patterns\"").unwrap();
        let enums = json.find("\"_enums\"").unwrap();
        let e = json.find("\"e\"").unwrap();
        assert!(meta < patterns && patterns < enums && enums < e);
    }

    #[test]
    fn registry_doc_write_round_trips_through_reader() {
        let dir = tempdir().unwrap();
        // Plant the workspace anchor so `for_project` accepts the path.
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
        std::fs::write(dir.path().join("mustard.json"), b"{}").unwrap();

        let doc = RegistryDoc::new(
            "2026-05-28".to_string(),
            "mustard-rt run sync-registry",
            json!({ "drizzle": { "discovered": [] } }),
            json!({}),
            json!({ "User": {}, "Post": {} }),
        );
        doc.write(dir.path()).expect("write succeeds");

        let reloaded = EntityRegistry::load(dir.path());
        assert_eq!(reloaded.version(), Some("4.0"));
        assert_eq!(reloaded.entity_count(), 2);
        assert!(reloaded.has_patterns());
    }
}

//! Deterministic `patternsOverlay` assembly + architectural-style detection
//! for the scan subsystem (F1-b).
//!
//! This module replaces the two things the cold-path LLM used to emit:
//!
//! 1. **`patternsOverlay`** — the `{ clusterLabels, dominant, edges }` object
//!    merged into `_patterns.{stack}`. All three are now derived
//!    deterministically:
//!    - `clusterLabels` ← the `label` of every cluster `cluster_discovery`
//!      produced (the suffix / folder / base-class / decorator name *is* the
//!      label).
//!    - `dominant` ← the dominant naming convention `project_conventions`
//!      inferred (`PascalCase` / `kebab-case` / …), when one crossed the
//!      threshold.
//!    - `edges` ← a deterministic join: an entity field whose **type** matches
//!      a known entity name yields a `Entity -> Type` edge; plus the import
//!      `refs` the structural extractor attached to each entity.
//!
//! 2. **`architecture`** — the `ScanResult.architecture` tag (legacy `"unknown"`).
//!    Detected by classifying every path **segment** into an architectural role
//!    via [`mustard_core::domain::vocabulary::architecture`] (Aho-Corasick over
//!    an embedded role vocabulary — the *same* engine the framework detector
//!    uses) and combining the role-presence set with the **direction of the
//!    import graph between roles** (does a central layer import an outer one?).
//!    Agnostic — nothing assumes a stack or an architecture; an
//!    `mustard.json#architecture` pin overrides the inference.
//!
//! No LLM anywhere. Byte-stable: the overlay key set + value shapes match what
//! the registry consumer already expected from the model.

use super::file_utils::VisitedFile;
use super::{EntityInfo, EnumInfo};
use crate::util::mustard_config;
use mustard_core::domain::vocabulary::architecture::{
    detect_architecture, ArchitectureVocabulary, LayerEdge, LayerRole, DEFAULT_ARCHITECTURE_NAME,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Assemble the deterministic `patternsOverlay` object for one subproject.
///
/// Shape (any subset present): `{ "clusterLabels": [..], "dominant": "..",
/// "edges": [{ "from": "..", "to": ".." }, ..] }`. Keys are only inserted when
/// non-empty so the merged `_patterns.{stack}` stays compact (the model used to
/// omit empty keys too). The returned value is always a `Value::Object`.
#[must_use]
pub fn build_patterns_overlay(
    clusters: &[Value],
    conventions: &Value,
    entities: &BTreeMap<String, EntityInfo>,
) -> Value {
    let mut overlay = serde_json::Map::new();

    // clusterLabels — the deduplicated, sorted `label` of every discovered
    // cluster (the suffix / decorator / base-class IS the label).
    let labels = cluster_labels(clusters);
    if !labels.is_empty() {
        overlay.insert("clusterLabels".to_string(), json!(labels));
    }

    // dominant — the naming convention that crossed the dominance threshold.
    if let Some(dominant) = conventions
        .get("naming")
        .and_then(|n| n.get("dominant"))
        .and_then(Value::as_str)
    {
        if !dominant.is_empty() {
            overlay.insert("dominant".to_string(), json!(dominant));
        }
    }

    // edges — deterministic type/import join across the structural entities.
    let edges = entity_edges(entities);
    if !edges.is_empty() {
        overlay.insert("edges".to_string(), Value::Array(edges));
    }

    Value::Object(overlay)
}

/// Deduplicated, sorted cluster labels.
fn cluster_labels(clusters: &[Value]) -> Vec<String> {
    let mut labels: BTreeSet<String> = BTreeSet::new();
    for c in clusters {
        if let Some(label) = c.get("label").and_then(Value::as_str) {
            if !label.is_empty() {
                labels.insert(label.to_string());
            }
        }
    }
    labels.into_iter().collect()
}

/// Build deterministic `{from, to}` edges between entities.
///
/// Two sources, both deterministic:
/// 1. **Type join** — a field declared on entity `E` whose type token names a
///    *known* entity `T` (case-insensitive, singular/plural tolerant) yields an
///    `E -> T` edge. This recovers foreign-key / navigation relationships the
///    model used to guess.
/// 2. **Import refs** — the `refs` the structural extractor attached to `E`
///    (final import path segments). When a ref matches a known entity name it
///    becomes an `E -> ref` edge; otherwise it is still emitted as an outbound
///    edge so the import surface is not lost.
///
/// Edges are sorted + deduplicated for byte-stability.
fn entity_edges(entities: &BTreeMap<String, EntityInfo>) -> Vec<Value> {
    // Lowercased index of known entity names for the type/ref join.
    let known: BTreeMap<String, String> = entities
        .keys()
        .map(|n| (n.to_ascii_lowercase(), n.clone()))
        .collect();

    let mut edges: BTreeSet<(String, String)> = BTreeSet::new();

    for (name, info) in entities {
        // 1. Type join — scan each property for a type token that names a known
        //    entity. Properties are stored as the field NAME by the extractor;
        //    when the field name itself singularises to a known entity (e.g.
        //    `user_id` → `User`, `order` → `Order`) we record the edge. This is
        //    the deterministic floor; richer type extraction can layer on later.
        for prop in &info.properties {
            if let Some(target) = resolve_known_entity(prop, &known) {
                if &target != name {
                    edges.insert((name.clone(), target));
                }
            }
        }
        // 2. Import refs — the structural import edges. Every ref is an outbound
        //    edge; when it resolves to a known entity it is canonicalised to the
        //    declared name, otherwise kept verbatim.
        for r in &info.refs {
            let target = resolve_known_entity(r, &known).unwrap_or_else(|| r.clone());
            if !target.is_empty() && &target != name {
                edges.insert((name.clone(), target));
            }
        }
    }

    edges
        .into_iter()
        .map(|(from, to)| json!({ "from": from, "to": to }))
        .collect()
}

/// Resolve a raw token (field name / import tail) to a known entity name when
/// it matches one case-insensitively, tolerating a trailing `_id` / `Id` and a
/// trailing plural `s`. Returns the canonical declared name, or `None`.
fn resolve_known_entity(raw: &str, known: &BTreeMap<String, String>) -> Option<String> {
    let token = raw.trim().trim_end_matches(['_', '.']);
    if token.is_empty() {
        return None;
    }
    let lc = token.to_ascii_lowercase();
    // Direct hit.
    if let Some(name) = known.get(&lc) {
        return Some(name.clone());
    }
    // Strip a trailing `_id` / `id` suffix (`user_id` / `userId` → `user`).
    let stripped = lc
        .strip_suffix("_id")
        .or_else(|| lc.strip_suffix("id").filter(|s| !s.is_empty() && *s != lc))
        .unwrap_or(&lc)
        .trim_end_matches('_');
    if let Some(name) = known.get(stripped) {
        return Some(name.clone());
    }
    // Strip a trailing plural `s` (`orders` → `order`).
    if let Some(singular) = stripped.strip_suffix('s') {
        if let Some(name) = known.get(singular) {
            return Some(name.clone());
        }
    }
    None
}

/// Detect the architectural-style tag for one subproject.
///
/// Resolution order:
/// 1. `mustard.json#architecture` pin (explicit user override — wins outright).
/// 2. Deterministic inference: classify every path segment into an
///    architectural role via the embedded Aho vocabulary, build the
///    role-dependency edges from the structural import `refs`, and run the pure
///    [`detect_architecture`] decision rule.
///
/// Returns the lowercase style tag (`clean` / `hexagonal` / `layered` / `ddd` /
/// `unknown`). Fail-open: a vocab build error degrades to `"unknown"`.
#[must_use]
pub fn detect_subproject_architecture(
    sub_root: &Path,
    visited: &[VisitedFile],
    entities: &BTreeMap<String, EntityInfo>,
    enums: &BTreeMap<String, EnumInfo>,
) -> String {
    // 1. Explicit override.
    if let Some(pin) =
        mustard_config::load(sub_root).and_then(|cfg| mustard_config::architecture(&cfg))
    {
        return pin;
    }

    // 2. Deterministic inference.
    let Ok(vocab) = ArchitectureVocabulary::load(DEFAULT_ARCHITECTURE_NAME, sub_root) else {
        return "unknown".to_string();
    };

    // 2a. Role presence — classify every segment of every visited path. The
    //     visited set already contains every entity / enum source file (the
    //     structural extractor only ever sees `visited`), so this single pass is
    //     the authoritative layout signal — language- and stack-agnostic.
    let paths: Vec<String> = visited.iter().map(|v| v.rel.clone()).collect();
    let roles: BTreeSet<LayerRole> =
        mustard_core::domain::vocabulary::architecture::roles_in_paths(&vocab, &paths);
    let _ = enums; // presence is already covered via `visited`; kept for the API.

    // 2b. Layer-dependency edges — map each entity's own role (from its file
    //     path) to the role of each of its import `refs`. The refs are short
    //     import tails the structural extractor attached; we classify them as
    //     path segments too. Only cross-role edges carry direction signal.
    let mut layer_edges: Vec<LayerEdge> = Vec::new();
    let mut seen_edges: BTreeSet<(LayerRole, LayerRole)> = BTreeSet::new();
    for info in entities.values() {
        let Some(from) = role_of_path(&vocab, &info.file) else {
            continue;
        };
        for r in &info.refs {
            if let Some(to) = vocab.classify_segment(r) {
                if from != to && seen_edges.insert((from, to)) {
                    layer_edges.push(LayerEdge { from, to });
                }
            }
        }
    }

    detect_architecture(&roles, &layer_edges).style.as_str().to_string()
}

/// Classify the role of a file from the first of its path segments that maps to
/// an architectural role. Returns `None` when no segment classifies.
fn role_of_path(vocab: &ArchitectureVocabulary, rel: &str) -> Option<LayerRole> {
    for segment in rel.replace('\\', "/").split('/') {
        if let Some(role) = vocab.classify_segment(segment) {
            return Some(role);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn vf(rel: &str) -> VisitedFile {
        VisitedFile {
            abs: PathBuf::from(rel),
            rel: rel.to_string(),
            content: Some(String::new()),
        }
    }

    fn entity(file: &str, props: &[&str], refs: &[&str]) -> EntityInfo {
        EntityInfo {
            file: file.to_string(),
            properties: props.iter().map(|s| s.to_string()).collect(),
            refs: refs.iter().map(|s| s.to_string()).collect(),
            ..EntityInfo::default()
        }
    }

    #[test]
    fn overlay_carries_cluster_labels_dominant_edges() {
        let clusters = vec![
            json!({ "label": "Service" }),
            json!({ "label": "Repository" }),
            json!({ "label": "Service" }), // dup — collapses
        ];
        let conventions = json!({ "naming": { "dominant": "PascalCase" } });
        let mut entities = BTreeMap::new();
        entities.insert("User".to_string(), entity("src/user.rs", &[], &[]));
        entities.insert(
            "Order".to_string(),
            entity("src/order.rs", &["user_id", "total"], &[]),
        );

        let overlay = build_patterns_overlay(&clusters, &conventions, &entities);
        assert_eq!(
            overlay["clusterLabels"],
            json!(["Repository", "Service"])
        );
        assert_eq!(overlay["dominant"], json!("PascalCase"));
        // `Order.user_id` joins to the known `User` entity.
        assert_eq!(overlay["edges"], json!([{ "from": "Order", "to": "User" }]));
    }

    #[test]
    fn overlay_omits_empty_keys() {
        let entities: BTreeMap<String, EntityInfo> = BTreeMap::new();
        let overlay = build_patterns_overlay(&[], &json!({}), &entities);
        assert_eq!(overlay, json!({}));
    }

    #[test]
    fn edges_from_import_refs() {
        let mut entities = BTreeMap::new();
        entities.insert("User".to_string(), entity("src/user.rs", &[], &[]));
        entities.insert(
            "Account".to_string(),
            entity("src/account.rs", &[], &["User", "external_api"]),
        );
        let overlay = build_patterns_overlay(&[], &json!({}), &entities);
        let edges = overlay["edges"].as_array().unwrap();
        // Account -> User (resolved to known) and Account -> external_api (kept).
        assert!(edges.contains(&json!({ "from": "Account", "to": "User" })));
        assert!(edges.contains(&json!({ "from": "Account", "to": "external_api" })));
    }

    #[test]
    fn architecture_clean_layout_detected() {
        let visited = vec![
            vf("src/domain/user.rs"),
            vf("src/application/create_user.rs"),
            vf("src/infrastructure/pg_user_repo.rs"),
        ];
        let mut entities = BTreeMap::new();
        // infrastructure repo imports the domain user — inward dependency.
        entities.insert(
            "PgUserRepo".to_string(),
            entity("src/infrastructure/pg_user_repo.rs", &[], &["domain"]),
        );
        let arch = detect_subproject_architecture(
            std::path::Path::new("/nonexistent"),
            &visited,
            &entities,
            &BTreeMap::new(),
        );
        assert_eq!(arch, "clean");
    }

    #[test]
    fn architecture_hexagonal_layout_detected() {
        let visited = vec![
            vf("src/domain/order.rs"),
            vf("src/ports/order_repository.rs"),
            vf("src/adapters/pg_order_repository.rs"),
        ];
        let arch = detect_subproject_architecture(
            std::path::Path::new("/nonexistent"),
            &visited,
            &BTreeMap::new(),
            &BTreeMap::new(),
        );
        assert_eq!(arch, "hexagonal");
    }

    #[test]
    fn architecture_unknown_when_flat_layout() {
        let visited = vec![vf("src/main.rs"), vf("src/helpers.rs")];
        let arch = detect_subproject_architecture(
            std::path::Path::new("/nonexistent"),
            &visited,
            &BTreeMap::new(),
            &BTreeMap::new(),
        );
        assert_eq!(arch, "unknown");
    }

    #[test]
    fn architecture_override_wins() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("mustard.json"),
            r#"{ "architecture": "Hexagonal" }"#,
        )
        .unwrap();
        // A flat layout would infer `unknown`, but the pin forces `hexagonal`.
        let visited = vec![vf("src/main.rs")];
        let arch = detect_subproject_architecture(
            tmp.path(),
            &visited,
            &BTreeMap::new(),
            &BTreeMap::new(),
        );
        assert_eq!(arch, "hexagonal");
    }
}

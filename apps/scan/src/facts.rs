//! Deterministic model FACTS — the small, stable projection the ORCHESTRATOR
//! (Mustard) consumes instead of parsing `grain.model.json` itself.
//!
//! Two facts an orchestrator needs without reading source or the (large) model:
//! the subproject list (one per build manifest) and the known declaration names
//! (entities/types/functions). Keeping this here makes `scan` the single owner
//! of the model schema — consumers depend only on this tiny JSON shape, never on
//! the model's internals. A pure projection of the deterministic model, so it is
//! deterministic too. Nothing here is language- or framework-specific.

use crate::model::{Manifest, ProjectModel, ProjectUnit};
use serde::Serialize;

/// How many ranked values `rank_by_frequency` surfaces. A fixed projection
/// constant, not user config — tuning the model shape does not belong here.
const STACK_RANK_LIMIT: usize = 12;

#[derive(Serialize)]
pub struct ModelFacts {
    /// Subprojects (one per build manifest) — the deterministic discovery the
    /// orchestrator splits work by. Kept in the model's stable order.
    pub projects: Vec<ProjectUnit>,
    /// Distinct declaration names (entities/types/functions), sorted + deduped —
    /// the "known entities" set (answers "is X new or already in the repo?").
    pub entities: Vec<String>,
}

/// Project the model down to its orchestrator FACTS. Deterministic: `projects`
/// keep the model's stable order; `entities` are sorted + deduped. Each project
/// is enriched with the frameworks/dependencies/scripts mined from the manifests
/// that live under it (a per-unit slice of the same agnostic projection).
#[must_use]
pub fn build(model: &ProjectModel) -> ModelFacts {
    let mut entities: Vec<String> = model
        .modules
        .iter()
        .flat_map(|m| m.declarations.iter().map(|d| d.name.clone()))
        .filter(|n| !n.is_empty())
        .collect();
    entities.sort();
    entities.dedup();

    let mut projects = model.projects.clone();
    enrich_projects(&mut projects, &model.projects, &model.manifests);

    ModelFacts { projects, entities }
}

/// Enrich each unit in `projects` with the frameworks/dependencies/scripts
/// aggregated from the manifests it owns. `all` is the full (immutable) project
/// list used for the longest-prefix ownership test (`owned_manifests`), passed
/// separately so the caller can mutate `projects` while reading `all`.
///
/// Single source of the manifest→project projection: `build_projects` calls it
/// so the grain `projects[]` carry the fields (`scan_claude` reads `scripts` for
/// `## Commands` and `frameworks` for the Guards facts), and [`build`] calls it
/// for the facts view. Idempotent — re-running over already-enriched units
/// reproduces the same values.
pub(crate) fn enrich_projects(projects: &mut [ProjectUnit], all: &[ProjectUnit], manifests: &[Manifest]) {
    for project in projects.iter_mut() {
        let owned: Vec<&Manifest> = owned_manifests(project, all, manifests);
        project.dependencies = aggregate_field(owned.iter().flat_map(|m| m.dependencies.iter()));
        project.scripts = aggregate_field(owned.iter().flat_map(|m| m.scripts.iter()));
        project.frameworks = rank_by_frequency(owned.iter().flat_map(|m| m.dependencies.iter()));
    }
}

/// The manifests owned by `project`: those whose path sits under `project.dir`
/// but NOT under a more-specific sibling unit. A manifest belongs to the unit
/// with the longest matching `dir` prefix, so a nested subproject's manifests
/// never leak up into its parent (and an empty/root `dir` does not swallow all).
pub(crate) fn owned_manifests<'a>(
    project: &ProjectUnit,
    all: &[ProjectUnit],
    manifests: &'a [Manifest],
) -> Vec<&'a Manifest> {
    manifests
        .iter()
        .filter(|m| dir_contains(&project.dir, &m.path))
        .filter(|m| {
            // Excluded if some other unit with a strictly longer dir also owns it
            // (the more-specific unit wins).
            !all.iter().any(|other| {
                other.dir.len() > project.dir.len()
                    && dir_contains(&other.dir, &m.path)
            })
        })
        .collect()
}

/// True when `path` lives under directory `dir` (paths are `/`-normalized and
/// relative, per `ingest`). An empty `dir` is the workspace root and contains
/// everything; otherwise the path must equal `dir` or start with `dir/`.
pub(crate) fn dir_contains(dir: &str, path: &str) -> bool {
    if dir.is_empty() {
        return true;
    }
    path == dir || path.starts_with(&format!("{dir}/"))
}

/// Aggregate string values, deduped + sorted — a deterministic projection.
fn aggregate_field<'a>(values: impl Iterator<Item = &'a String>) -> Vec<String> {
    let mut out: Vec<String> = values.cloned().collect();
    out.sort();
    out.dedup();
    out
}

/// Rank values by frequency (desc), breaking ties by first-appearance order, and
/// take the top `STACK_RANK_LIMIT` — the same agnostic projection
/// `ingest::infer_frameworks` applies repo-wide, here restricted to one unit's
/// manifests. No curated catalog. Ties resolve by declaration order (the order
/// the value is first seen in `values`), never alphabetically: an ASCII tiebreak
/// would hide a relevant dependency behind a lexically-smaller neighbour, and it
/// would not be honest to the manifest the project actually wrote.
pub fn rank_by_frequency<'a>(values: impl Iterator<Item = &'a String>) -> Vec<String> {
    use std::collections::HashMap;
    // freq + the index at which each value was first observed (document order).
    let mut stats: HashMap<String, (usize, usize)> = HashMap::new();
    for (idx, v) in values.enumerate() {
        let entry = stats.entry(v.clone()).or_insert((0, idx));
        entry.0 += 1;
    }
    let mut ranked: Vec<(String, usize, usize)> =
        stats.into_iter().map(|(v, (freq, first_seen))| (v, freq, first_seen)).collect();
    // (Reverse(freq), first_seen) — higher frequency first, then earliest seen.
    ranked.sort_by_key(|(_, freq, first_seen)| (std::cmp::Reverse(*freq), *first_seen));
    ranked.into_iter().map(|(v, _, _)| v).take(STACK_RANK_LIMIT).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Decl, Module};

    fn model_with(decls: &[&str], projects: &[&str]) -> ProjectModel {
        ProjectModel {
            modules: vec![Module {
                declarations: decls.iter().map(|n| Decl { name: (*n).to_string(), ..Default::default() }).collect(),
                ..Default::default()
            }],
            projects: projects.iter().map(|n| ProjectUnit { name: (*n).to_string(), ..Default::default() }).collect(),
            ..Default::default()
        }
    }

    fn unit(name: &str, dir: &str) -> ProjectUnit {
        ProjectUnit { name: name.into(), dir: dir.into(), ..Default::default() }
    }

    fn manifest(path: &str, deps: &[&str], scripts: &[&str]) -> Manifest {
        Manifest {
            path: path.into(),
            dependencies: deps.iter().map(|d| (*d).to_string()).collect(),
            scripts: scripts.iter().map(|s| (*s).to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn entities_are_sorted_deduped_and_nonempty() {
        let f = build(&model_with(&["User", "Invoice", "User", ""], &[]));
        assert_eq!(f.entities, vec!["Invoice", "User"]);
    }

    #[test]
    fn projects_preserve_model_order() {
        let f = build(&model_with(&[], &["api", "web"]));
        let names: Vec<String> = f.projects.iter().map(|p| p.name.clone()).collect();
        assert_eq!(names, vec!["api", "web"]);
    }

    #[test]
    fn crossing_by_dir_prefix_fills_frameworks_scripts_and_deps() {
        let model = ProjectModel {
            projects: vec![unit("api", "apps/api"), unit("web", "apps/web")],
            manifests: vec![
                manifest("apps/api/Cargo.toml", &["serde", "tokio"], &["gen: build.rs"]),
                manifest("apps/web/package.json", &["react"], &["build: vite"]),
            ],
            ..Default::default()
        };
        let f = build(&model);
        let api = f.projects.iter().find(|p| p.name == "api").unwrap();
        assert_eq!(api.dependencies, vec!["serde", "tokio"]);
        assert_eq!(api.scripts, vec!["gen: build.rs"]);
        assert_eq!(api.frameworks, vec!["serde", "tokio"]);
        let web = f.projects.iter().find(|p| p.name == "web").unwrap();
        assert_eq!(web.dependencies, vec!["react"]);
        assert_eq!(web.scripts, vec!["build: vite"]);
    }

    #[test]
    fn unmatched_dir_stays_empty() {
        let model = ProjectModel {
            projects: vec![unit("api", "apps/api")],
            manifests: vec![manifest("apps/other/Cargo.toml", &["serde"], &[])],
            ..Default::default()
        };
        let f = build(&model);
        let api = &f.projects[0];
        assert!(api.dependencies.is_empty(), "deps should be empty: {:?}", api.dependencies);
        assert!(api.frameworks.is_empty(), "frameworks should be empty: {:?}", api.frameworks);
        assert!(api.scripts.is_empty());
    }

    #[test]
    fn nested_subproject_does_not_leak_into_parent() {
        // The parent unit must NOT absorb the nested unit's manifest — the
        // more-specific (longer dir) unit owns it.
        let model = ProjectModel {
            projects: vec![unit("root", ""), unit("api", "apps/api")],
            manifests: vec![
                manifest("Cargo.toml", &["workspace-dep"], &[]),
                manifest("apps/api/Cargo.toml", &["serde"], &[]),
            ],
            ..Default::default()
        };
        let f = build(&model);
        let root = f.projects.iter().find(|p| p.name == "root").unwrap();
        let api = f.projects.iter().find(|p| p.name == "api").unwrap();
        // Root keeps only its own root manifest, not the nested one.
        assert_eq!(root.dependencies, vec!["workspace-dep"]);
        assert_eq!(api.dependencies, vec!["serde"]);
    }

    #[test]
    fn aggregated_fields_are_sorted_and_deduped() {
        let model = ProjectModel {
            projects: vec![unit("api", "apps/api")],
            manifests: vec![
                manifest("apps/api/Cargo.toml", &["tokio", "serde"], &[]),
                manifest("apps/api/crate/Cargo.toml", &["serde", "anyhow"], &[]),
            ],
            ..Default::default()
        };
        let f = build(&model);
        let api = &f.projects[0];
        // Both manifests are under apps/api (no more-specific sibling unit), so
        // deps merge, dedupe and sort.
        assert_eq!(api.dependencies, vec!["anyhow", "serde", "tokio"]);
        // serde appears twice → ranks first by frequency.
        assert_eq!(api.frameworks.first().map(String::as_str), Some("serde"));
    }

    #[test]
    fn equal_frequency_ties_keep_first_appearance_not_alphabetical() {
        // Both deps appear exactly once, so the tiebreak decides the order. The
        // honest answer is the order the manifest declared them ("zebra" before
        // "alpha"), never the ASCII order that would surface "alpha" first.
        let deps = ["zebra".to_string(), "alpha".to_string()];
        let ranked = rank_by_frequency(deps.iter());
        assert_eq!(ranked, vec!["zebra", "alpha"]);
    }

    #[test]
    fn json_manifest_deps_rank_in_document_order_not_alphabetical() {
        // End-to-end guard for the serde_json `preserve_order` feature: a
        // package.json lists deps "zebra" then "alpha" (both freq 1). Without
        // preserve_order, json_deps reads them from a BTreeMap and alphabetizes
        // to ["alpha", "zebra"] — and the tie would resolve wrong. With the
        // feature, document order survives and rank_by_frequency keeps it.
        let pkg = r#"{ "dependencies": { "zebra": "1.0.0", "alpha": "1.0.0" } }"#;
        let parsed = crate::manifests::parse("app/package.json", "package.json", pkg)
            .expect("package.json should parse");
        assert_eq!(parsed.deps, vec!["zebra", "alpha"], "json_deps must preserve document order");
        let ranked = rank_by_frequency(parsed.deps.iter());
        assert_eq!(ranked, vec!["zebra", "alpha"]);
    }
}

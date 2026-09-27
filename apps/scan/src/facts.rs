//! A projeção dos manifestos em cada subprojeto: as dependências, os scripts
//! e os frameworks de cada unidade saem dos manifestos que ficam sob a pasta
//! dela. O manifesto pertence à unidade de pasta mais específica que o contém,
//! de modo que o de um subprojeto aninhado não sobe para o pai. É uma projeção
//! pura e determinística do modelo; nada aqui depende de linguagem ou
//! framework.

use crate::model::{Manifest, ProjectUnit};

/// How many ranked values `rank_by_frequency` surfaces. A fixed projection
/// constant, not user config — tuning the model shape does not belong here.
const STACK_RANK_LIMIT: usize = 12;

/// Enrich each unit in `projects` with the frameworks/dependencies/scripts
/// aggregated from the manifests it owns. `all` is the full (immutable) project
/// list used for the longest-prefix ownership test (`owned_manifests`), passed
/// separately so the caller can mutate `projects` while reading `all`.
///
/// Single source of the manifest→project projection: `build_projects` calls it
/// so the grain `projects[]` carry the fields (`scan_claude` reads `scripts` for
/// `## Commands` and `frameworks` for the Guards facts). Idempotent — re-running
/// over already-enriched units reproduces the same values.
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

    /// As unidades depois da projeção, cada uma lida contra a lista inteira.
    fn enriched(projects: Vec<ProjectUnit>, manifests: &[Manifest]) -> Vec<ProjectUnit> {
        let mut out = projects.clone();
        enrich_projects(&mut out, &projects, manifests);
        out
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
    fn crossing_by_dir_prefix_fills_frameworks_scripts_and_deps() {
        let projects = enriched(
            vec![unit("api", "apps/api"), unit("web", "apps/web")],
            &[
                manifest("apps/api/Cargo.toml", &["serde", "tokio"], &["gen: build.rs"]),
                manifest("apps/web/package.json", &["react"], &["build: vite"]),
            ],
        );
        let api = projects.iter().find(|p| p.name == "api").unwrap();
        assert_eq!(api.dependencies, vec!["serde", "tokio"]);
        assert_eq!(api.scripts, vec!["gen: build.rs"]);
        assert_eq!(api.frameworks, vec!["serde", "tokio"]);
        let web = projects.iter().find(|p| p.name == "web").unwrap();
        assert_eq!(web.dependencies, vec!["react"]);
        assert_eq!(web.scripts, vec!["build: vite"]);
    }

    #[test]
    fn unmatched_dir_stays_empty() {
        let projects = enriched(vec![unit("api", "apps/api")], &[manifest("apps/other/Cargo.toml", &["serde"], &[])]);
        let api = &projects[0];
        assert!(api.dependencies.is_empty(), "deps should be empty: {:?}", api.dependencies);
        assert!(api.frameworks.is_empty(), "frameworks should be empty: {:?}", api.frameworks);
        assert!(api.scripts.is_empty());
    }

    #[test]
    fn nested_subproject_does_not_leak_into_parent() {
        // The parent unit must NOT absorb the nested unit's manifest — the
        // more-specific (longer dir) unit owns it.
        let projects = enriched(
            vec![unit("root", ""), unit("api", "apps/api")],
            &[manifest("Cargo.toml", &["workspace-dep"], &[]), manifest("apps/api/Cargo.toml", &["serde"], &[])],
        );
        let root = projects.iter().find(|p| p.name == "root").unwrap();
        let api = projects.iter().find(|p| p.name == "api").unwrap();
        // Root keeps only its own root manifest, not the nested one.
        assert_eq!(root.dependencies, vec!["workspace-dep"]);
        assert_eq!(api.dependencies, vec!["serde"]);
    }

    #[test]
    fn aggregated_fields_are_sorted_and_deduped() {
        let projects = enriched(
            vec![unit("api", "apps/api")],
            &[
                manifest("apps/api/Cargo.toml", &["tokio", "serde"], &[]),
                manifest("apps/api/crate/Cargo.toml", &["serde", "anyhow"], &[]),
            ],
        );
        let api = &projects[0];
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

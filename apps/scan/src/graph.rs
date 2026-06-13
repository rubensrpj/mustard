//! Layer 3 — Dependency graph.
//!
//! The declared architecture lives in folder names; the *real* architecture
//! lives in the import edges. We resolve imports to internal modules and ask
//! objective, vocabulary-free questions: are there cycles? god modules? and
//! what is the *emergent* layering — i.e. how deep is each module in the
//! dependency order the code itself defines?
//!
//! Layering is derived, not named: condense cycles into a DAG, then take each
//! module's longest dependency chain as its depth (L0 = most depended-upon /
//! innermost). The only direction-violation topology can prove without a
//! hardcoded layer vocabulary is a dependency cycle, so that is what we count.
//!
//! Resolution is heuristic and language-agnostic: imports and declared
//! namespaces are first normalized to one canonical segment form (`\`, `::`
//! and the dots of a dotted namespace all become `/`), then every import is
//! tried against a union of resolution shapes and the ones that don't apply
//! return nothing.
//!   * namespace/package match — an import that names a declared namespace
//!     (the shape used by C#, Java, Kotlin, ...), retried with the final
//!     segment dropped when the import names a TYPE inside a namespace
//!     (the fully-qualified-name shape used by PHP);
//!   * module-prefixed path — strip a declared module prefix, match a directory
//!     (the shape used by Go);
//!   * file path — resolve a relative/path-ish import to a module file (the
//!     shape used by TS/JS, Dart, Python, ...);
//!   * root-alias path — only for imports whose first segment is one of the
//!     importer language's declared `root_aliases` (registry data, e.g. Rust's
//!     `crate`/`self`/`super`): drop the alias segment and probe the tail
//!     against the importer's ancestor dirs. Languages that declare no aliases
//!     never take this branch, so an external package path (`std::...`) can
//!     never be mistaken for an internal module.
//! Nothing here switches on a language name, so a new language needs no change.
//! Imports that resolve to nothing internal are treated as external deps.

use crate::model::{GraphStats, LayerInfo, Module, NodeDegree, Touchpoint};
use petgraph::graph::{DiGraph, NodeIndex};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Catalog cap for `top_fan_in` / `top_fan_out`. The digest filters these BY
/// QUERY VOCABULARY, so the catalog must be wide enough for a domain-specific
/// hub to exist in it at all — 8 global slots starved every query on a large
/// monorepo. Bounded: ~a few KB of model.
const TOP_DEGREE_CAP: usize = 64;

/// Longest dependency chain below an SCC = its emergent depth (L0 = innermost).
fn scc_depth(c: usize, succ: &[HashSet<usize>], memo: &mut [Option<usize>]) -> usize {
    if let Some(d) = memo[c] {
        return d;
    }
    memo[c] = Some(0); // DAG guard; condensed graph has no cycles
    let mut d = 0;
    for &s in &succ[c] {
        d = d.max(1 + scc_depth(s, succ, memo));
    }
    memo[c] = Some(d);
    d
}


pub fn build(modules: &[Module], go_module: &Option<String>) -> (GraphStats, HashMap<String, (usize, usize)>, HashMap<String, usize>) {
    let mut g: DiGraph<String, ()> = DiGraph::new();
    let mut idx: HashMap<String, NodeIndex> = HashMap::new();
    for m in modules {
        let n = g.add_node(m.path.clone());
        idx.insert(m.path.clone(), n);
    }

    // Indexes for resolution.
    let mut ns_index: HashMap<String, Vec<String>> = HashMap::new(); // namespace -> module paths
    let mut dir_index: HashMap<String, Vec<String>> = HashMap::new(); // dir -> module paths
    for m in modules {
        for ns in &m.namespaces {
            // Index namespaces in canonical segment form so a lookup never
            // depends on which separator the language writes (`\`, `.`, `::`).
            ns_index.entry(canon_segments(ns)).or_default().push(m.path.clone());
        }
        let dir = parent_dir(&m.path);
        dir_index.entry(dir).or_default().push(m.path.clone());
    }
    let module_paths: HashSet<&str> = modules.iter().map(|m| m.path.as_str()).collect();
    let stem_index = build_stem_index(modules);

    // Edge weight ×1024 by resolution SPECIFICITY: an import that lands on ONE
    // module is full evidence (1024); a namespace/package bucket of N modules
    // spreads the same single import across N files, so each target gets 1/N
    // (floored at 1). One `using` of a 40-file namespace must not mint 40
    // units of centrality — the field case had every file of the most-used
    // C# namespace at a uniform fan-in 478, saturating `top_fan_in` with
    // alphabetical enums and starving every query's `hubs` of real signal.
    // The strongest evidence per (src, dst) pair wins; re-imports never inflate.
    let mut edge_w: HashMap<(NodeIndex, NodeIndex), u64> = HashMap::new();

    for m in modules {
        let src = idx[&m.path];
        let root_aliases = crate::extract::root_aliases(&m.language);
        for imp in &m.imports {
            let targets = resolve(imp, &m.path, root_aliases, &ns_index, &stem_index, &dir_index, &module_paths, go_module);
            let w = (1024 / targets.len().max(1) as u64).max(1);
            for t in targets {
                if let Some(&dst) = idx.get(&t) {
                    if dst != src {
                        let e = edge_w.entry((src, dst)).or_insert(0);
                        *e = (*e).max(w);
                    }
                }
            }
        }
    }
    let edge_set: HashSet<(NodeIndex, NodeIndex)> = edge_w.keys().copied().collect();
    for (a, b) in &edge_set {
        g.add_edge(*a, *b, ());
    }

    // Cycles via SCC.
    let sccs = petgraph::algo::tarjan_scc(&g);
    let mut cycles: Vec<Vec<String>> = Vec::new();
    for scc in &sccs {
        if scc.len() > 1 {
            cycles.push(scc.iter().map(|n| g[*n].clone()).collect());
        }
    }
    let self_loop = edge_set.iter().any(|(a, b)| a == b);
    let cyclic = !cycles.is_empty() || self_loop;

    // Fan-in / fan-out — specificity-weighted (see `edge_w`): the published
    // degree is the rounded sum of edge weights, i.e. "specific-import
    // equivalents". A namespace-broadcast target keeps a small honest degree
    // (478 importers / 40-file bucket ≈ 12) instead of a minted 478, so real
    // hubs — modules imported by precise evidence — rank above diffuse glue.
    let mut win: HashMap<NodeIndex, u64> = HashMap::new();
    let mut wout: HashMap<NodeIndex, u64> = HashMap::new();
    for ((a, b), w) in &edge_w {
        *wout.entry(*a).or_insert(0) += w;
        *win.entry(*b).or_insert(0) += w;
    }
    // Rounded units, floored at 1 for any node with at least one in/out edge
    // — "has dependents" must survive the rounding of a tiny diffuse weight.
    let units = |x: u64| (((x + 512) >> 10) as usize).max(1);
    let mut fan_in: Vec<(u64, NodeDegree)> = Vec::new();
    let mut fan_out: Vec<(u64, NodeDegree)> = Vec::new();
    let mut degree_map: HashMap<String, (usize, usize)> = HashMap::new();
    for n in g.node_indices() {
        let wi = win.get(&n).copied().unwrap_or(0);
        let wo = wout.get(&n).copied().unwrap_or(0);
        degree_map.insert(
            g[n].clone(),
            (if wi > 0 { units(wi) } else { 0 }, if wo > 0 { units(wo) } else { 0 }),
        );
        if wi > 0 {
            fan_in.push((wi, NodeDegree { module: g[n].clone(), degree: units(wi) }));
        }
        if wo > 0 {
            fan_out.push((wo, NodeDegree { module: g[n].clone(), degree: units(wo) }));
        }
    }
    // Order by the RAW weighted sum (full discrimination), path asc on ties —
    // deterministic regardless of HashMap iteration order.
    fan_in.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.module.cmp(&b.1.module)));
    fan_out.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.module.cmp(&b.1.module)));
    let mut fan_in: Vec<NodeDegree> = fan_in.into_iter().map(|(_, d)| d).collect();
    let mut fan_out: Vec<NodeDegree> = fan_out.into_iter().map(|(_, d)| d).collect();
    // 64, not 8: the digest filters this catalog BY QUERY VOCABULARY — with
    // only 8 global slots a 4k-module monorepo never surfaces the hub of any
    // specific domain (the field case answered every financial query with
    // zero hubs). Still bounded; a few KB on the model.
    fan_in.truncate(TOP_DEGREE_CAP);
    fan_out.truncate(TOP_DEGREE_CAP);

    // Emergent layering: condense cycles into a DAG, then depth = longest
    // dependency chain. No layer names — just the order the imports define.
    let mut scc_of = vec![0usize; g.node_count()];
    for (i, comp) in sccs.iter().enumerate() {
        for &n in comp {
            scc_of[n.index()] = i;
        }
    }
    let mut succ: Vec<HashSet<usize>> = vec![HashSet::new(); sccs.len()];
    let mut cyclic_edges = 0usize;
    for (a, b) in &edge_set {
        let (ca, cb) = (scc_of[a.index()], scc_of[b.index()]);
        if ca != cb {
            succ[ca].insert(cb);
        } else if sccs[ca].len() > 1 || a == b {
            cyclic_edges += 1; // a dependency that closes a cycle
        }
    }
    let mut memo = vec![None; sccs.len()];
    let mut per_depth: BTreeMap<usize, usize> = BTreeMap::new();
    let mut depth_by_path: HashMap<String, usize> = HashMap::new();
    for n in g.node_indices() {
        let d = scc_depth(scc_of[n.index()], &succ, &mut memo);
        *per_depth.entry(d).or_default() += 1;
        depth_by_path.insert(g[n].clone(), d);
    }
    let layers: Vec<LayerInfo> = per_depth
        .into_iter()
        .map(|(d, modules)| LayerInfo { name: format!("L{d}"), modules })
        .collect();

    // Touchpoints: hubs that import across many directories — the registration
    // points you edit when adding an entity (DI container, menu, barrels). Ranked
    // by breadth (distinct dirs imported) then fan-out; tests excluded because
    // they import broadly but register nothing. Frequency-derived, no catalog.
    let mut src_targets: HashMap<&str, Vec<&str>> = HashMap::new();
    for (a, b) in &edge_set {
        src_targets.entry(g[*a].as_str()).or_default().push(g[*b].as_str());
    }
    let mut touchpoints: Vec<Touchpoint> = src_targets
        .iter()
        .filter(|(src, _)| !is_test_path(src))
        .map(|(src, tgts)| {
            let breadth = tgts.iter().map(|t| parent_dir(t)).collect::<HashSet<_>>().len();
            Touchpoint { module: (*src).to_string(), fan_out: tgts.len(), breadth }
        })
        .collect();
    touchpoints.sort_by(|a, b| b.breadth.cmp(&a.breadth).then(b.fan_out.cmp(&a.fan_out)).then(a.module.cmp(&b.module)));
    touchpoints.truncate(120); // keep enough so per-project hubs (e.g. a frontend menu) aren't crowded out by a larger project

    let stats = GraphStats {
        nodes: g.node_count(),
        edges: edge_set.len(),
        cyclic,
        cycles,
        top_fan_in: fan_in,
        top_fan_out: fan_out,
        layers,
        cyclic_edges,
        total_edges: edge_set.len(),
        touchpoints,
    };
    (stats, degree_map, depth_by_path)
}

fn build_stem_index(modules: &[Module]) -> HashMap<String, Vec<String>> {
    let mut m: HashMap<String, Vec<String>> = HashMap::new();
    for module in modules {
        let stem = strip_ext(&module.path);
        m.entry(stem).or_default().push(module.path.clone());
    }
    m
}

#[allow(clippy::too_many_arguments)]
fn resolve(
    imp: &str,
    from: &str,
    root_aliases: &[&str],
    ns_index: &HashMap<String, Vec<String>>,
    stem_index: &HashMap<String, Vec<String>>,
    dir_index: &HashMap<String, Vec<String>>,
    module_paths: &HashSet<&str>,
    go_module: &Option<String>,
) -> Vec<String> {
    // Try every resolution shape; whichever applies wins. No language switch.
    // Lookups run on the canonical segment form so no shape ever cares which
    // separator the import was written with.
    let canon = canon_segments(imp);
    // 1) Namespace/package match: the import names a declared namespace (the
    //    common case for namespace languages — a using shared by many files).
    if let Some(v) = ns_index.get(&canon) {
        return v.clone();
    }
    // 1b) Fully-qualified-name match: the import names a TYPE inside a
    //     declared namespace — retry with the final segment dropped, narrowed
    //     to the file named after the type (the file-per-type convention) so
    //     one FQCN doesn't edge to every file in the namespace. When no file
    //     carries the type's name, keep the whole bucket: coupling at
    //     namespace granularity, the same evidence shape (1) accepts.
    if let Some((ns, type_name)) = canon.rsplit_once('/') {
        if let Some(v) = ns_index.get(ns) {
            let named: Vec<String> = v.iter().filter(|p| file_stem(p) == type_name).cloned().collect();
            return if named.is_empty() { v.clone() } else { named };
        }
    }
    // 2) Module-prefixed path: strip a declared module prefix and match the
    //    directory it points at (the import-as-package-path shape). Raw on
    //    both sides: these imports and the declared prefix are already
    //    slash-separated, and canonicalizing a dotted module domain would
    //    corrupt it.
    if let Some(modpath) = go_module {
        if let Some(rest) = imp.strip_prefix(modpath.as_str()) {
            let rest = rest.trim_start_matches('/');
            if let Some(v) = dir_index.get(rest) {
                return v.clone();
            }
        }
    }
    // 3) File path: a relative or path-ish import resolved to a module file.
    //    The canonical form means dotted / `::` module paths take this branch
    //    too — they are paths spelled with another separator.
    let cleaned = canon.strip_prefix("package:").unwrap_or(&canon);
    if cleaned.starts_with('.') {
        let joined = join_relative(from, cleaned);
        return resolve_path_candidate(&joined, stem_index, dir_index, module_paths);
    }
    if cleaned.contains('/') {
        let hits = resolve_path_candidate(cleaned, stem_index, dir_index, module_paths);
        if !hits.is_empty() {
            return hits;
        }
    }
    // 4) Root-alias path: only for imports whose FIRST segment is one of the
    //    importer language's declared root aliases (registry data — e.g. Rust's
    //    `crate`/`self`/`super`); any other first segment names an external
    //    package, never the project root. Drop the alias and probe the tail
    //    (and, because the final segment may name an ITEM inside the module,
    //    the tail minus its last segment) against the importer's ancestor
    //    directories, nearest first. The fixed probe order keeps resolution
    //    deterministic. No aliases declared -> this branch never runs.
    if let Some((alias, tail)) = canon.split_once('/') {
        if root_aliases.contains(&alias) {
            let mut tails = vec![tail.to_string()];
            if let Some((head, _)) = tail.rsplit_once('/') {
                tails.push(head.to_string());
            }
            for t in &tails {
                let mut dir = parent_dir(from);
                loop {
                    let cand = if dir.is_empty() { t.clone() } else { format!("{dir}/{t}") };
                    let hits = resolve_path_candidate(&cand, stem_index, dir_index, module_paths);
                    if !hits.is_empty() {
                        return hits;
                    }
                    if dir.is_empty() {
                        break;
                    }
                    dir = parent_dir(&dir);
                }
            }
        }
    }
    Vec::new()
}

fn resolve_path_candidate(
    cand: &str,
    stem_index: &HashMap<String, Vec<String>>,
    dir_index: &HashMap<String, Vec<String>>,
    module_paths: &HashSet<&str>,
) -> Vec<String> {
    let cand = normalize(cand);
    if module_paths.contains(cand.as_str()) {
        return vec![cand];
    }
    let stem = strip_ext(&cand);
    if let Some(v) = stem_index.get(&stem) {
        return v.clone();
    }
    // directory import -> index file
    for index in ["index", "main", "mod"] {
        let probe = format!("{stem}/{index}");
        if let Some(v) = stem_index.get(&probe) {
            return v.clone();
        }
    }
    // dart package suffix: match any module whose path ends with the candidate
    let mut suffix_matches: Vec<String> = stem_index
        .iter()
        .filter(|(k, _)| k.ends_with(&stem))
        .flat_map(|(_, v)| v.clone())
        .collect();
    if !suffix_matches.is_empty() {
        suffix_matches.sort(); // stable output: HashMap iteration order varies per run
        return suffix_matches;
    }
    let _ = dir_index;
    Vec::new()
}

fn parent_dir(path: &str) -> String {
    match path.rfind('/') {
        Some(i) => path[..i].to_string(),
        None => String::new(),
    }
}

/// Path-segment test detection (language-agnostic): a file under a test/mock/
/// fixture folder, or named `*.test.*`/`*.spec.*`.
fn is_test_path(p: &str) -> bool {
    let l = p.to_lowercase();
    if l.contains(".test.") || l.contains(".spec.") {
        return true;
    }
    l.split('/').any(|s| matches!(s, "test" | "tests" | "__tests__" | "mocks" | "fixtures" | "spec" | "specs"))
}

fn strip_ext(path: &str) -> String {
    match path.rfind('.') {
        Some(i) if !path[i..].contains('/') => path[..i].to_string(),
        _ => path.to_string(),
    }
}

/// Normalize qualified-name separators to one canonical segment form: `\` and
/// `::` always become `/`; dots become `/` only when the string is not already
/// path-ish (no `/` present, no leading `.` — a dotted namespace never starts
/// with a dot, while a relative import always does).
fn canon_segments(s: &str) -> String {
    let flat = s.replace('\\', "/").replace("::", "/");
    if !flat.contains('/') && !flat.starts_with('.') && flat.contains('.') {
        flat.replace('.', "/")
    } else {
        flat
    }
}

/// Final path segment without its extension: `app/Models/User.php` -> `User`.
fn file_stem(path: &str) -> String {
    let stem = strip_ext(path);
    match stem.rfind('/') {
        Some(i) => stem[i + 1..].to_string(),
        None => stem,
    }
}

fn join_relative(from: &str, rel: &str) -> String {
    let base = parent_dir(from);
    let mut parts: Vec<&str> = base.split('/').filter(|s| !s.is_empty()).collect();
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

//! `mustard-rt run wave-dependency` — a port of `scripts/wave-dependency.js`.
//!
//! Builds a dependency DAG from a list of files (via import/require parsing)
//! and groups files into waves using topological level assignment.
//!
//! Input arrives as JSON on stdin (`{ files, projectRoot }`); output is one
//! JSON object on stdout. Fail-open: an unrecoverable error emits
//! `{ "error": "error-fallback" }`.

use crate::commands::wave::wave_lib::{
    detect_role_with, load_role_patterns, load_wave_layer_order,
};
use mustard_core::{io::fs, RolePattern};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};

/// Extensions that can be appended when resolving a relative import.
const RESOLVABLE_EXTENSIONS: &[&str] = &[
    ".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs", ".vue", ".svelte", ".py", ".go", ".cs",
];
/// Index basenames probed when an import resolves to a directory.
const INDEX_BASENAMES: &[&str] = &[
    "index.ts",
    "index.tsx",
    "index.js",
    "index.jsx",
    "index.mjs",
    "__init__.py",
];

/// Extract import/require specifiers from file content.
///
/// Mirrors the four JS regexes: ES `import ... from '...'`, bare
/// `import '...'`, `require('...')`, and Python `from x import`.
fn extract_imports(content: &str) -> Vec<String> {
    let mut imports: BTreeSet<String> = BTreeSet::new();

    // Capture every `'...'` / `"..."` string literal that follows `from` or
    // `require(` or a bare `import`, plus Python `from <mod> import`.
    for (idx, _) in content.match_indices("from ") {
        let after = &content[idx + 5..];
        if let Some(spec) = leading_quoted(after) {
            imports.insert(spec);
        } else {
            // Python `from <mod> import` — `[.\w]+`.
            let module: String = after
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '.' || *c == '_')
                .collect();
            let rest = &after[module.len()..];
            if !module.is_empty() && rest.trim_start().starts_with("import ") {
                imports.insert(module);
            }
        }
    }
    for (idx, _) in content.match_indices("import ") {
        let after = &content[idx + 7..];
        if let Some(spec) = leading_quoted(after) {
            imports.insert(spec);
        }
    }
    for (idx, _) in content.match_indices("require") {
        let after = content[idx + 7..].trim_start();
        if let Some(after) = after.strip_prefix('(') {
            if let Some(spec) = leading_quoted(after.trim_start()) {
                imports.insert(spec);
            }
        }
    }

    imports.into_iter().collect()
}

/// If `s` begins with a quoted string (`'...'` or `"..."`), return its content.
fn leading_quoted(s: &str) -> Option<String> {
    let q = s.chars().next()?;
    if q != '\'' && q != '"' {
        return None;
    }
    let rest = &s[1..];
    let end = rest.find(q)?;
    Some(rest[..end].to_string())
}

/// Resolve a relative import to an absolute path in `candidate_set`.
fn resolve_import(
    import_path: &str,
    current_file: &Path,
    candidate_set: &BTreeSet<PathBuf>,
) -> Option<PathBuf> {
    if !import_path.starts_with('.') && !import_path.starts_with('/') {
        return None;
    }
    let base_dir = current_file.parent()?;
    let abs_target = normalize(&base_dir.join(import_path));

    if candidate_set.contains(&abs_target) {
        return Some(abs_target);
    }
    for ext in RESOLVABLE_EXTENSIONS {
        let with_ext = PathBuf::from(format!("{}{ext}", abs_target.display()));
        if candidate_set.contains(&with_ext) {
            return Some(with_ext);
        }
    }
    // Strip an existing extension and retry.
    if let Some(stem) = abs_target.file_stem() {
        if abs_target.extension().is_some() {
            let stripped = abs_target.with_file_name(stem);
            if candidate_set.contains(&stripped) {
                return Some(stripped);
            }
            for ext in RESOLVABLE_EXTENSIONS {
                let swapped = PathBuf::from(format!("{}{ext}", stripped.display()));
                if candidate_set.contains(&swapped) {
                    return Some(swapped);
                }
            }
        }
    }
    for basename in INDEX_BASENAMES {
        let index_path = abs_target.join(basename);
        if candidate_set.contains(&index_path) {
            return Some(index_path);
        }
    }
    None
}

/// Lexically normalize a path (resolve `.` / `..`) without touching the disk.
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Build the dependency graph.
fn build_graph(
    files: &[String],
    project_root: &Path,
) -> BTreeMap<PathBuf, BTreeSet<PathBuf>> {
    let abs_files: Vec<PathBuf> = files
        .iter()
        .map(|f| {
            let p = Path::new(f);
            if p.is_absolute() {
                normalize(p)
            } else {
                normalize(&project_root.join(p))
            }
        })
        .collect();
    let candidate_set: BTreeSet<PathBuf> = abs_files.iter().cloned().collect();
    let mut graph: BTreeMap<PathBuf, BTreeSet<PathBuf>> = BTreeMap::new();

    for abs_file in &abs_files {
        let deps = graph.entry(abs_file.clone()).or_default();
        let Ok(content) = fs::read_to_string(abs_file) else {
            continue;
        };
        for imp in extract_imports(&content) {
            if let Some(resolved) = resolve_import(&imp, abs_file, &candidate_set) {
                if &resolved != abs_file {
                    deps.insert(resolved);
                }
            }
        }
    }
    graph
}

/// Result of the topological pass.
enum TopoResult {
    Waves(Vec<Vec<PathBuf>>),
    Cycle(Vec<PathBuf>),
}

/// Assign files to waves by topological level. Files with no in-graph
/// dependencies are wave 1; cyclic files yield `Cycle`.
fn topological_waves(graph: &BTreeMap<PathBuf, BTreeSet<PathBuf>>) -> TopoResult {
    let mut indegree: BTreeMap<&PathBuf, usize> = graph.keys().map(|k| (k, 0)).collect();
    let mut dependents: BTreeMap<&PathBuf, BTreeSet<&PathBuf>> =
        graph.keys().map(|k| (k, BTreeSet::new())).collect();

    for (node, deps) in graph {
        for dep in deps {
            if graph.contains_key(dep) {
                *indegree.entry(node).or_insert(0) += 1;
                dependents.entry(dep).or_default().insert(node);
            }
        }
    }

    let mut waves: Vec<Vec<PathBuf>> = Vec::new();
    let mut visited: BTreeSet<&PathBuf> = BTreeSet::new();
    let mut current: Vec<&PathBuf> = indegree
        .iter()
        .filter(|&(_, &d)| d == 0)
        .map(|(&n, _)| n)
        .collect();

    while !current.is_empty() {
        waves.push(current.iter().map(|p| (*p).clone()).collect());
        for node in &current {
            visited.insert(node);
        }
        let mut next: Vec<&PathBuf> = Vec::new();
        for node in &current {
            if let Some(deps) = dependents.get(node) {
                for &dependent in deps {
                    let entry = indegree.entry(dependent).or_insert(0);
                    *entry = entry.saturating_sub(1);
                    if *entry == 0 && !visited.contains(dependent) {
                        next.push(dependent);
                    }
                }
            }
        }
        current = next;
    }

    if visited.len() < graph.len() {
        let stuck: Vec<PathBuf> = graph
            .keys()
            .filter(|k| !visited.contains(k))
            .cloned()
            .collect();
        return TopoResult::Cycle(stuck);
    }
    TopoResult::Waves(waves)
}

/// Relativize `abs` against `project_root`, with forward slashes.
fn to_relative(abs: &Path, project_root: &Path) -> String {
    abs.strip_prefix(project_root).map_or_else(
        |_| abs.to_string_lossy().replace('\\', "/"),
        |p| p.to_string_lossy().replace('\\', "/"),
    )
}

/// Compute the wave-DAG result JSON for a list of files.
///
/// Shared with `exec-rewave-check`, which used to shell to `wave-dependency.js`
/// — it now calls this directly. The shape matches the JS stdout exactly.
pub fn compute_waves(files: &[String], project_root: &Path) -> Value {
    if files.is_empty() {
        return json!({ "error": "empty-input" });
    }
    let graph = build_graph(files, project_root);
    // F0-e: role-classification overrides from `mustard.json#rolePatterns`.
    let role_patterns = load_role_patterns(project_root);
    match topological_waves(&graph) {
        TopoResult::Cycle(stuck) => {
            let cycle: Vec<String> = stuck.iter().map(|f| to_relative(f, project_root)).collect();
            json!({ "error": "cyclic-dependency", "cycle": cycle })
        }
        TopoResult::Waves(wave_files) => {
            // Net-new features have no import edges yet, so the DAG flattens to a
            // single level even when the files span multiple architectural
            // layers. When that happens, derive the waves from the files' roles
            // ordered by `mustard.json#waveLayerOrder` (documented default), so a
            // backend->core->ui net-new feature still decomposes deterministically
            // instead of collapsing to one wave.
            if wave_files.len() == 1 {
                let layer_order = load_wave_layer_order(project_root);
                if let Some(fallback) =
                    role_layered_fallback(&wave_files[0], project_root, &role_patterns, &layer_order)
                {
                    return fallback;
                }
            }
            let mut widest = 0usize;
            let waves: Vec<Value> = wave_files
                .iter()
                .enumerate()
                .map(|(idx, files)| {
                    let rel: Vec<String> =
                        files.iter().map(|f| to_relative(f, project_root)).collect();
                    widest = widest.max(rel.len());
                    let mut roles: Vec<String> = Vec::new();
                    for r in rel.iter().map(|f| detect_role_with(f, &role_patterns)) {
                        if !roles.contains(&r) {
                            roles.push(r);
                        }
                    }
                    json!({
                        "wave": idx + 1,
                        "files": rel,
                        "roles": roles,
                        "dependsOn": if idx == 0 { json!([]) } else { json!([idx]) },
                    })
                })
                .collect();
            json!({
                "waves": waves,
                "metadata": {
                    "totalWaves": wave_files.len(),
                    "totalFiles": files.len(),
                    "widestWave": widest,
                },
            })
        }
    }
}

/// Deterministic role-layered decomposition for a flat (no-edge) DAG.
///
/// Returns `Some(wavesJson)` ONLY when the files span >= 2 architectural layers
/// — applying the same lib-folding rule the scope decider uses (a lone `lib`
/// bucket is one layer, never split). Otherwise `None` and the caller keeps the
/// single import-DAG wave. Roles are scheduled in `layer_order` (case-insensitive
/// match), each wave depending on the previous; roles absent from the order fall
/// to the tail in lexical order. The emitted shape is byte-identical to the
/// import-DAG path (`{wave, files, roles, dependsOn}` + `metadata`).
fn role_layered_fallback(
    files: &[PathBuf],
    project_root: &Path,
    role_patterns: &[RolePattern],
    layer_order: &[String],
) -> Option<Value> {
    let mut by_role: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for f in files {
        let rel = to_relative(f, project_root);
        let role = detect_role_with(&rel, role_patterns);
        by_role.entry(role).or_default().push(rel);
    }
    // Same lib-folding rule as scope_decompose::decide / exec_rewave_check: a
    // lone "lib" bucket counts as one layer, so a purely-generic net-new slice
    // is not split.
    let layer_count = if by_role.len() == 1 && by_role.contains_key("lib") {
        1
    } else {
        by_role.len()
    };
    if layer_count < 2 {
        return None;
    }
    // Schedule the role buckets: ordered roles first (case-insensitive match
    // against the config/default order), then any remainder in lexical order.
    let mut ordered: Vec<String> = Vec::new();
    for want in layer_order {
        for key in by_role.keys() {
            if key.eq_ignore_ascii_case(want) && !ordered.iter().any(|o| o == key) {
                ordered.push(key.clone());
            }
        }
    }
    for key in by_role.keys() {
        if !ordered.iter().any(|o| o == key) {
            ordered.push(key.clone());
        }
    }
    // Backstop: an architectural decomposition is a handful of layers, not a
    // dozen. If role detection fragments the census into many buckets it is
    // mislabeling (random path prefixes read as bespoke roles), not a real
    // layering — keep the single import-DAG wave rather than emit one wave per
    // noise role (field report: a flat net-new census fanned out to 11 waves).
    const MAX_FALLBACK_LAYERS: usize = 6;
    if ordered.len() > MAX_FALLBACK_LAYERS {
        return None;
    }
    let mut widest = 0usize;
    let mut total_files = 0usize;
    let waves: Vec<Value> = ordered
        .iter()
        .enumerate()
        .map(|(idx, role)| {
            let wave_files = by_role.get(role).cloned().unwrap_or_default();
            widest = widest.max(wave_files.len());
            total_files += wave_files.len();
            json!({
                "wave": idx + 1,
                "files": wave_files,
                "roles": [role],
                "dependsOn": if idx == 0 { json!([]) } else { json!([idx]) },
            })
        })
        .collect();
    Some(json!({
        "waves": waves,
        "metadata": {
            "totalWaves": ordered.len(),
            "totalFiles": total_files,
            "widestWave": widest,
        },
    }))
}

/// Extract the file list from a parsed input document.
///
/// Accepts BOTH input shapes — the prose⇄binary drift that made the documented
/// `wave-dependency < plan.json` form answer `empty-input` (the refs said to
/// feed the plan JSON while the binary only parsed `{files}`; first recorded as
/// a follow-up in spec `redesenho-agnostico-indice-termos-digest`):
///
/// - **derivation shape**: top-level `files: [...]` (+ optional `projectRoot`);
/// - **plan JSON** (the same document `plan-materialize --plan` consumes):
///   `waves: [{files: [...]}]` — the per-wave censuses are unioned in wave
///   order, first occurrence wins (dedup), so shared files create no phantom
///   duplicate nodes in the DAG.
fn files_from_value(parsed: &Value) -> Vec<String> {
    if let Some(arr) = parsed.get("files").and_then(Value::as_array) {
        return arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    }
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut out: Vec<String> = Vec::new();
    for wave in parsed
        .get("waves")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        for f in wave
            .get("files")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(Value::as_str)
        {
            if seen.insert(f.to_string()) {
                out.push(f.to_string());
            }
        }
    }
    out
}

/// Trust an explicit plan's wave boundaries (Option D). When the input is the
/// rich PLAN shape (`waves: [{files:[...]}]`), emit the canonical
/// `{waves, metadata}` from the planner's own boundaries — renumbered, with a
/// linear `dependsOn` chain and per-wave roles — instead of flattening to a file
/// union and re-deriving (which lets the flat-DAG role fallback fan a 2-wave plan
/// out to one wave per role). Files are deduped across waves (first occurrence
/// wins, matching the DAG's no-phantom-node rule); a wave whose files all
/// appeared earlier is dropped. Returns `None` for the bare `{files}` derivation
/// shape (no `waves` key) or an all-empty plan, so the import-DAG path runs.
fn passthrough_plan_waves(parsed: &Value, role_patterns: &[RolePattern]) -> Option<Value> {
    let waves_in = parsed.get("waves").and_then(Value::as_array)?;
    if waves_in.is_empty() {
        return None;
    }
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut out_waves: Vec<Value> = Vec::new();
    let mut widest = 0usize;
    let mut total_files = 0usize;
    for wave in waves_in {
        let mut rel: Vec<String> = Vec::new();
        for f in wave
            .get("files")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(Value::as_str)
        {
            if seen.insert(f.to_string()) {
                rel.push(f.to_string());
            }
        }
        if rel.is_empty() {
            continue;
        }
        let mut roles: Vec<String> = Vec::new();
        for r in rel.iter().map(|f| detect_role_with(f, role_patterns)) {
            if !roles.contains(&r) {
                roles.push(r);
            }
        }
        widest = widest.max(rel.len());
        total_files += rel.len();
        let idx = out_waves.len();
        out_waves.push(json!({
            "wave": idx + 1,
            "files": rel,
            "roles": roles,
            "dependsOn": if idx == 0 { json!([]) } else { json!([idx]) },
        }));
    }
    if out_waves.is_empty() {
        return None;
    }
    let total_waves = out_waves.len();
    Some(json!({
        "waves": out_waves,
        "metadata": {
            "totalWaves": total_waves,
            "totalFiles": total_files,
            "widestWave": widest,
            "source": "input-plan",
        },
    }))
}

/// Dispatch `mustard-rt run wave-dependency [--plan <file>]`.
///
/// `--plan` reads the input JSON from a file — the reliable transport (stdin
/// does not survive the `rtk` wrapper, and a sandboxed/background shell may
/// hand the process a closed stdin; field report 2026-06-12: four wasted calls
/// before the orchestrator gave up). Without the flag, the legacy stdin
/// contract still applies. Both transports accept both input shapes — see
/// [`files_from_value`].
pub fn run(plan: Option<&str>) {
    let raw = match plan {
        Some(path) => match fs::read_to_string(Path::new(path)) {
            Ok(s) => s,
            Err(_) => {
                println!("{}", json!({ "error": "plan-unreadable", "path": path }));
                return;
            }
        },
        None => {
            let mut buf = String::new();
            if std::io::stdin().read_to_string(&mut buf).is_err() {
                println!("{}", json!({ "error": "empty-input" }));
                return;
            }
            buf
        }
    };
    if raw.trim().is_empty() {
        println!("{}", json!({ "error": "empty-input" }));
        return;
    }
    let parsed: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => {
            println!("{}", json!({ "error": "error-fallback" }));
            return;
        }
    };
    let project_root = parsed
        .get("projectRoot")
        .and_then(Value::as_str)
        .map_or_else(
            || std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            PathBuf::from,
        );
    let root_abs = if project_root.is_absolute() {
        project_root
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(project_root)
    };
    let root = normalize(&root_abs);

    // Option D — trust an explicit plan's wave boundaries instead of flattening
    // to a file union and re-deriving. The role-layered fallback below would
    // otherwise shred a sensible 2-wave plan into one wave per detected role
    // (field report: a 2-wave plan came back as 11). Bare `{files}` inputs carry
    // no `waves` key and fall through to the import-DAG path.
    if let Some(out) = passthrough_plan_waves(&parsed, &load_role_patterns(&root)) {
        println!("{out}");
        return;
    }

    let files = files_from_value(&parsed);
    if files.is_empty() {
        println!("{}", json!({ "error": "empty-input" }));
        return;
    }
    println!("{}", compute_waves(&files, &root));
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn flat_dag_multi_role_falls_back_to_role_layers() {
        // Net-new files with no imports → flat DAG. Three distinct roles →
        // deterministic role-layered fallback, ordered by the default layer
        // order (schema → api → ui), each wave depending on the previous.
        let dir = tempdir().unwrap();
        let files = vec![
            "src/schema/user.sql".to_string(),
            "src/api/handler.ts".to_string(),
            "src/ui/page.tsx".to_string(),
        ];
        let out = compute_waves(&files, dir.path());
        let waves = out["waves"].as_array().expect("waves array");
        assert_eq!(waves.len(), 3, "flat 3-role net-new must split: {out}");
        assert_eq!(waves[0]["roles"][0].as_str(), Some("schema"));
        assert_eq!(waves[1]["roles"][0].as_str(), Some("api"));
        assert_eq!(waves[2]["roles"][0].as_str(), Some("ui"));
        assert_eq!(waves[0]["dependsOn"].as_array().map(Vec::len), Some(0));
        assert_eq!(waves[1]["dependsOn"][0].as_u64(), Some(1));
        assert_eq!(waves[2]["dependsOn"][0].as_u64(), Some(2));
    }

    #[test]
    fn flat_dag_lone_lib_stays_single_wave() {
        // Two net-new files that both fall to the generic "lib" bucket: the
        // lib-folding rule keeps them in one layer — no over-split.
        let dir = tempdir().unwrap();
        let files = vec!["src/util/a.ts".to_string(), "src/util/b.ts".to_string()];
        let out = compute_waves(&files, dir.path());
        assert_eq!(
            out["waves"].as_array().map(Vec::len),
            Some(1),
            "lone-lib net-new must not split: {out}"
        );
    }

    #[test]
    fn rich_plan_waves_are_trusted_not_reinflated() {
        // Option D regression: a planner's explicit 2-wave plan must come back as
        // 2 waves. Before the fix the union was flattened and the flat-DAG role
        // fallback shredded it into one wave per role (field report: 2 → 11).
        let parsed = json!({
            "waves": [
                { "files": ["src/api/x.ts", "src/api/y.ts"] },
                { "files": ["src/ui/z.tsx"] },
            ]
        });
        let out = passthrough_plan_waves(&parsed, &[]).expect("rich plan → Some");
        assert_eq!(
            out["waves"].as_array().map(Vec::len),
            Some(2),
            "explicit 2-wave plan must stay 2: {out}"
        );
        assert_eq!(out["metadata"]["source"].as_str(), Some("input-plan"));
        assert_eq!(out["metadata"]["totalWaves"].as_u64(), Some(2));
        // Linear dependency chain across the trusted boundaries.
        assert_eq!(out["waves"][0]["dependsOn"].as_array().map(Vec::len), Some(0));
        assert_eq!(out["waves"][1]["dependsOn"][0].as_u64(), Some(1));
    }

    #[test]
    fn bare_files_input_is_not_treated_as_a_plan() {
        // The derivation shape ({files} only) has no `waves` key → passthrough
        // declines so the import-DAG path still runs.
        let parsed = json!({ "files": ["a.ts", "b.ts"] });
        assert!(passthrough_plan_waves(&parsed, &[]).is_none());
    }

    #[test]
    fn import_dag_depth_is_not_overridden_by_fallback() {
        // A real import edge gives the DAG depth, so the fallback (guarded on a
        // single flat wave) never fires — the import topology wins.
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src/schema")).unwrap();
        std::fs::create_dir_all(dir.path().join("src/api")).unwrap();
        std::fs::write(dir.path().join("src/schema/m.ts"), "export const m = 1;").unwrap();
        std::fs::write(dir.path().join("src/api/h.ts"), "import '../schema/m.ts';").unwrap();
        let files = vec!["src/schema/m.ts".to_string(), "src/api/h.ts".to_string()];
        let out = compute_waves(&files, dir.path());
        assert_eq!(
            out["waves"].as_array().map(Vec::len),
            Some(2),
            "import depth preserved: {out}"
        );
    }

    #[test]
    fn extract_imports_handles_es_and_cjs() {
        let src = "import { x } from './a';\nconst y = require('./b');\n";
        let imps = extract_imports(src);
        assert!(imps.contains(&"./a".to_string()));
        assert!(imps.contains(&"./b".to_string()));
    }

    #[test]
    fn topological_waves_orders_dependencies() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("a.ts"), "export const a = 1;").unwrap();
        std::fs::write(root.join("b.ts"), "import { a } from './a';").unwrap();
        let result = compute_waves(
            &["a.ts".to_string(), "b.ts".to_string()],
            root,
        );
        let waves = result["waves"].as_array().unwrap();
        assert_eq!(waves.len(), 2);
        assert_eq!(waves[0]["files"][0], json!("a.ts"));
        assert_eq!(waves[1]["files"][0], json!("b.ts"));
    }

    #[test]
    fn empty_files_is_error() {
        let dir = tempdir().unwrap();
        assert_eq!(compute_waves(&[], dir.path())["error"], json!("empty-input"));
    }

    #[test]
    fn files_from_value_accepts_derivation_shape() {
        let v = json!({ "files": ["a.ts", "b.ts"], "projectRoot": "." });
        assert_eq!(files_from_value(&v), vec!["a.ts", "b.ts"]);
    }

    /// The documented `< plan.json` form: a plan JSON (`{waves: [{files}]}`)
    /// must yield the union of the per-wave censuses — this was the prose⇄binary
    /// drift that answered `empty-input` to the exact input the refs prescribed
    /// (field report 2026-06-12 + follow-up note in spec
    /// `redesenho-agnostico-indice-termos-digest`).
    #[test]
    fn files_from_value_accepts_plan_json_shape_with_dedup() {
        let v = json!({
            "waves": [
                { "role": "backend", "files": ["src/api/h.ts", "src/shared/t.ts"] },
                { "role": "ui", "files": ["src/ui/p.tsx", "src/shared/t.ts"] },
            ]
        });
        // Union in wave order; the shared file appears once (first occurrence).
        assert_eq!(
            files_from_value(&v),
            vec!["src/api/h.ts", "src/shared/t.ts", "src/ui/p.tsx"],
        );
    }

    #[test]
    fn files_from_value_empty_for_unknown_shape() {
        assert!(files_from_value(&json!({ "foo": 1 })).is_empty());
        assert!(files_from_value(&json!({ "waves": [] })).is_empty());
    }
}

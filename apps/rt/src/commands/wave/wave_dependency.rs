//! `mustard-rt run wave-dependency` — a port of `scripts/wave-dependency.js`.
//!
//! Builds a dependency DAG from a list of files (via import/require parsing)
//! and groups files into waves using topological level assignment.
//!
//! Input arrives as JSON on stdin (`{ files, projectRoot }`); output is one
//! JSON object on stdout. Fail-open: an unrecoverable error emits
//! `{ "error": "error-fallback" }`.

use crate::commands::wave::wave_lib::detect_role;
use mustard_core::fs;
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
    match topological_waves(&graph) {
        TopoResult::Cycle(stuck) => {
            let cycle: Vec<String> = stuck.iter().map(|f| to_relative(f, project_root)).collect();
            json!({ "error": "cyclic-dependency", "cycle": cycle })
        }
        TopoResult::Waves(wave_files) => {
            let mut widest = 0usize;
            let waves: Vec<Value> = wave_files
                .iter()
                .enumerate()
                .map(|(idx, files)| {
                    let rel: Vec<String> =
                        files.iter().map(|f| to_relative(f, project_root)).collect();
                    widest = widest.max(rel.len());
                    let mut roles: Vec<String> = Vec::new();
                    for r in rel.iter().map(|f| detect_role(f)) {
                        let r = r.to_string();
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

/// Dispatch `mustard-rt run wave-dependency`.
pub fn run() {
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() || raw.trim().is_empty() {
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
    let files: Vec<String> = parsed
        .get("files")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    if files.is_empty() {
        println!("{}", json!({ "error": "empty-input" }));
        return;
    }
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
    println!("{}", compute_waves(&files, &normalize(&root_abs)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

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
}

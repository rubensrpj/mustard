//! `mustard-rt run scan-recipes-validate` — validate `.claude/recipes/<sub>/*.json`
//! shape for the deep-refactor W3 contract.
//!
//! Checks:
//!
//! - **Schema shape.** Every file under `.claude/recipes/` is JSON with the
//!   keys: `id` (string), `entity` (string), `operation` (string), `files`
//!   (array), `imports` (array, may be empty), `subproject` (string).
//! - **Real paths.** Every `files[].path` resolves to an existing path inside
//!   the recipe's subproject root.
//! - **No literal placeholders.** Strings containing `{Entity}` / `{entity}` /
//!   `{ClusterLabel}` / `{cluster}` (the canonical placeholder catalogue from
//!   the generator) are forbidden — they signal a recipe written from the
//!   abstract template instead of derived from real cluster samples.
//!
//! Output is byte-stable pretty JSON: `{ recipes, hits, ok }`. Exit code is
//! always `0` (fail-open) unless `--strict` is set and any hit is found.

use mustard_core::io::claude_paths::ClaudePaths;
use mustard_core::io::fs as mfs;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const FORBIDDEN_PLACEHOLDERS: &[&str] = &[
    "{Entity}",
    "{entity}",
    "{ClusterLabel}",
    "{clusterLabel}",
    "{cluster}",
    "{Cluster}",
    "{Field}",
    "{field}",
    "{Subproject}",
    "{subproject}",
];

/// Collect every `*.json` recipe under `<repo>/.claude/recipes/`.
fn collect_recipes(repo_root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(paths) = ClaudePaths::for_project(repo_root) else { return out; };
    let root = paths.claude_dir().join("recipes");
    walk(&root, &mut out);
    out.sort();
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = mfs::read_dir(dir) else { return };
    for entry in entries {
        if entry.is_dir {
            walk(&entry.path, out);
        } else if entry.file_name.to_ascii_lowercase().ends_with(".json") {
            out.push(entry.path);
        }
    }
}

/// True when any forbidden placeholder appears anywhere in the serialised value.
fn has_literal_placeholder(serialised: &str) -> bool {
    FORBIDDEN_PLACEHOLDERS
        .iter()
        .any(|p| serialised.contains(p))
}

/// Resolve the subproject root for a recipe path.
///
/// Path shape: `<repo>/.claude/recipes/<sub>/<file>.json`. The subproject root
/// is then `<repo>/<recipe.subproject>` when the recipe carries the canonical
/// `subproject: "apps/<x>"` field, falling back to `<repo>` for top-level
/// recipes.
fn subproject_root(repo_root: &Path, recipe: &Value) -> PathBuf {
    if let Some(sub) = recipe.get("subproject").and_then(Value::as_str) {
        return repo_root.join(sub);
    }
    repo_root.to_path_buf()
}

/// One validator pass. Walks recipes and emits per-file hits.
pub fn validate(repo_root: &Path, strict: bool) -> (Value, bool) {
    let recipes = collect_recipes(repo_root);
    let mut hits: Vec<Value> = Vec::new();

    for path in &recipes {
        let rel = path
            .strip_prefix(repo_root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");

        let Ok(body) = mfs::read_to_string(path) else {
            hits.push(json!({ "kind": "read-error", "file": rel }));
            continue;
        };

        if has_literal_placeholder(&body) {
            hits.push(json!({
                "kind": "literal-placeholder",
                "file": rel,
            }));
        }

        let parsed: Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(e) => {
                hits.push(json!({
                    "kind": "json-parse",
                    "file": rel,
                    "error": e.to_string(),
                }));
                continue;
            }
        };

        // Shape check — required keys.
        for key in ["id", "entity", "operation", "files", "subproject"] {
            if parsed.get(key).is_none() {
                hits.push(json!({
                    "kind": "missing-key",
                    "file": rel,
                    "key": key,
                }));
            }
        }
        if !parsed.get("files").is_some_and(|f| f.is_array()) {
            hits.push(json!({
                "kind": "files-not-array",
                "file": rel,
            }));
            continue;
        }
        if let Some(imports) = parsed.get("imports") {
            if !imports.is_array() {
                hits.push(json!({
                    "kind": "imports-not-array",
                    "file": rel,
                }));
            }
        }

        // Real-path check for every `files[].path`.
        let sub_root = subproject_root(repo_root, &parsed);
        if let Some(files) = parsed.get("files").and_then(Value::as_array) {
            for entry in files {
                let Some(rel_path) = entry.get("path").and_then(Value::as_str) else {
                    hits.push(json!({
                        "kind": "file-entry-missing-path",
                        "file": rel,
                    }));
                    continue;
                };
                let candidate = sub_root.join(rel_path);
                if !candidate.exists() {
                    hits.push(json!({
                        "kind": "path-missing",
                        "file": rel,
                        "ref": rel_path,
                        "expected": candidate.to_string_lossy(),
                    }));
                }
            }
        }
    }

    let ok = hits.is_empty();
    let exit_fail = strict && !ok;
    (
        json!({
            "recipes": recipes.len(),
            "hits": hits,
            "ok": ok,
            "strict": strict,
        }),
        exit_fail,
    )
}

/// Dispatch `mustard-rt run scan-recipes-validate`.
pub fn run(strict: bool) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let (report, exit_fail) = validate(&cwd, strict);
    println!(
        "{}",
        serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into())
    );
    if exit_fail {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn flags_literal_placeholder() {
        let dir = tempdir().unwrap();
        let rdir = dir.path().join(".claude").join("recipes").join("cli");
        std::fs::create_dir_all(&rdir).unwrap();
        let body = r#"{"id":"x","entity":"{Entity}","operation":"add","files":[],"subproject":"."}"#;
        std::fs::write(rdir.join("add-foo.json"), body).unwrap();
        let (report, _) = validate(dir.path(), false);
        let hits = report["hits"].as_array().unwrap();
        assert!(hits.iter().any(|h| h["kind"] == "literal-placeholder"));
    }

    #[test]
    fn flags_missing_path() {
        let dir = tempdir().unwrap();
        let rdir = dir.path().join(".claude").join("recipes").join("cli");
        std::fs::create_dir_all(&rdir).unwrap();
        let body = r#"{"id":"x","entity":"User","operation":"add","files":[{"path":"src/nope.rs"}],"subproject":"apps/cli"}"#;
        std::fs::write(rdir.join("add-user.json"), body).unwrap();
        let (report, _) = validate(dir.path(), false);
        let hits = report["hits"].as_array().unwrap();
        assert!(hits.iter().any(|h| h["kind"] == "path-missing"));
    }

    #[test]
    fn missing_key_reported() {
        let dir = tempdir().unwrap();
        let rdir = dir.path().join(".claude").join("recipes").join("cli");
        std::fs::create_dir_all(&rdir).unwrap();
        std::fs::write(rdir.join("a.json"), "{}").unwrap();
        let (report, _) = validate(dir.path(), false);
        let hits = report["hits"].as_array().unwrap();
        assert!(hits.iter().any(|h| h["kind"] == "missing-key"));
    }
}

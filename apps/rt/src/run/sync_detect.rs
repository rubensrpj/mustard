//! `mustard-rt run sync-detect` — a port of `scripts/sync-detect.js`.
//!
//! Discovers subprojects in a monorepo by scanning for **build manifests**
//! (`Cargo.toml` with `[package]`, `package.json`, `*.csproj`, `go.mod`,
//! `pyproject.toml`, `pubspec.yaml`) — the language-agnostic signal that a
//! directory is an independent buildable unit. `.claude/mustard.json` may
//! override the result via `subprojects.exclude` / `.include` (paths relative
//! to the repo root). It then detects each subproject's role via file-based
//! scoring, maps role → agent, lists commands, and computes a SHA-256 source
//! hash (keyed by `path`) so the pipeline can skip recompilation when nothing
//! changed.
//!
//! The stdout JSON must stay byte-compatible with the JS version — the pipeline
//! parses it. Field order, the two-space `serde_json` pretty layout, and the
//! `hashChanged` / optional-`warnings` semantics all mirror `sync-detect.js`.
//!
//! Two behaviours of the JS script are deliberately *not* ported here because
//! they only affect performance, never the emitted shape: the 5-minute
//! early-exit cache gate, and the per-module `moduleHashes` map (the JS script
//! emits `{}` for it on the common single-root project, which this port always
//! does — a later wave can add fine-grained module hashing).

use crate::util::sha256::Sha256;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Role-scoring weights — mirrors `ROLE_WEIGHTS` in `sync-detect.js`.
const WEIGHT_HIGH: i32 = 10;
const WEIGHT_MEDIUM: i32 = 5;
const WEIGHT_LOW: i32 = 3;

/// Source-file extensions whose content feeds the SHA-256 hash.
const SOURCE_EXTENSIONS: &[&str] = &[".cs", ".ts", ".tsx", ".js", ".jsx", ".dart", ".rs"];

/// Manifest file names that invalidate the source hash on change.
const MANIFEST_FILES: &[&str] = &[
    "pubspec.yaml",
    "pubspec.lock",
    "package.json",
    "pnpm-lock.yaml",
    "package-lock.json",
    "yarn.lock",
    "Directory.Packages.props",
    "Directory.Build.props",
    "nuget.config",
    "go.mod",
    "go.sum",
    "Cargo.toml",
    "Cargo.lock",
    "pyproject.toml",
    "requirements.txt",
    "poetry.lock",
];

/// One subproject entry — serializes with the exact JSON keys the JS script
/// emits (camelCase, optional `gitDirty*`).
#[derive(Debug, Serialize)]
struct Subproject {
    name: String,
    path: String,
    role: String,
    agent: String,
    commands: Vec<String>,
    #[serde(rename = "stackSummary")]
    stack_summary: String,
    #[serde(rename = "hashChanged")]
    hash_changed: bool,
    #[serde(rename = "gitDirty", skip_serializing_if = "Option::is_none")]
    git_dirty: Option<bool>,
    #[serde(rename = "gitDirtyCount", skip_serializing_if = "Option::is_none")]
    git_dirty_count: Option<usize>,
}

/// The full `sync-detect` output — field order mirrors the JS `result` object.
#[derive(Debug, Serialize)]
struct DetectOutput {
    subprojects: Vec<Subproject>,
    agents: Vec<String>,
    #[serde(rename = "detectedAgents")]
    detected_agents: Vec<String>,
    #[serde(rename = "promptsDir")]
    prompts_dir: String,
    #[serde(rename = "promptsCompiledDir")]
    prompts_compiled_dir: String,
    #[serde(rename = "sourceHashes")]
    source_hashes: BTreeMap<String, String>,
    #[serde(rename = "moduleHashes")]
    module_hashes: BTreeMap<String, serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    warnings: Option<Vec<String>>,
}

/// `true` if the file/glob `pattern` matches an entry directly inside `dir`.
fn file_exists(dir: &Path, pattern: &str) -> bool {
    if !pattern.contains('*') {
        return dir.join(pattern).exists();
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() != 2 {
        return false;
    }
    std::fs::read_dir(dir)
        .map(|entries| {
            entries.flatten().any(|e| {
                e.file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with(parts[0]) && name.ends_with(parts[1]))
            })
        })
        .unwrap_or(false)
}

/// `true` if `dir_name` exists as a directory inside `base` (or one level deep).
fn dir_exists(base: &Path, dir_name: &str) -> bool {
    if base.join(dir_name).is_dir() {
        return true;
    }
    let Ok(entries) = std::fs::read_dir(base) else {
        return false;
    };
    for e in entries.flatten() {
        let Some(name) = e.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if name.starts_with('.') || matches!(name.as_str(), "node_modules" | "bin" | "obj") {
            continue;
        }
        if e.path().join(dir_name).is_dir() {
            return true;
        }
    }
    false
}

/// Recursively find `.csproj` files up to `max_depth` levels deep.
fn find_csproj_files(dir: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut results = Vec::new();
    fn walk(dir: &Path, remaining: usize, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let Some(name) = e.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if e.path().is_file() && name.ends_with(".csproj") {
                out.push(e.path());
            } else if e.path().is_dir()
                && remaining > 0
                && !name.starts_with('.')
                && !matches!(name.as_str(), "node_modules" | "bin" | "obj")
            {
                walk(&e.path(), remaining - 1, out);
            }
        }
    }
    walk(dir, max_depth, &mut results);
    results
}

/// Read a file, returning an empty string on any error.
fn read_safe(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// `true` if any `.csproj` under `dir` targets `Microsoft.NET.Sdk.Web`.
fn is_csproj_web(dir: &Path) -> bool {
    find_csproj_files(dir, 2)
        .iter()
        .any(|f| read_safe(f).contains("Microsoft.NET.Sdk.Web"))
}

/// `true` if any `.csproj` under `dir` is *not* a web project.
fn is_csproj_library(dir: &Path) -> bool {
    find_csproj_files(dir, 2)
        .iter()
        .any(|f| !read_safe(f).contains("Microsoft.NET.Sdk.Web"))
}

/// Read `package.json` dependency + devDependency names.
fn package_json_deps(dir: &Path) -> Vec<String> {
    let content = read_safe(&dir.join("package.json"));
    if content.is_empty() {
        return Vec::new();
    }
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else {
        return Vec::new();
    };
    let mut deps = Vec::new();
    for section in ["dependencies", "devDependencies"] {
        if let Some(obj) = json.get(section).and_then(serde_json::Value::as_object) {
            deps.extend(obj.keys().cloned());
        }
    }
    deps
}

/// `true` if any dependency matches a pattern (exact, or namespaced).
fn has_any_dep(deps: &[String], patterns: &[&str]) -> bool {
    deps.iter().any(|dep| {
        patterns.iter().any(|p| {
            dep == p
                || dep.starts_with(&format!("{p}/"))
                || dep.starts_with(&format!("@{p}/"))
                || *dep == format!("@{p}")
        })
    })
}

/// `true` if a manifest file contains any of `patterns`.
fn manifest_has(dir: &Path, file: &str, patterns: &[&str]) -> bool {
    let content = read_safe(&dir.join(file));
    !content.is_empty() && patterns.iter().any(|p| content.contains(p))
}

/// Detect a subproject's role via file-based scoring — a port of `detectRole()`.
fn detect_role(abs_path: &Path) -> String {
    let mut scores: BTreeMap<&str, i32> = [
        ("api", 0),
        ("ui", 0),
        ("database", 0),
        ("library", 0),
        ("mobile", 0),
    ]
    .into_iter()
    .collect();
    let add = |scores: &mut BTreeMap<&str, i32>, key: &'static str, n: i32| {
        *scores.entry(key).or_insert(0) += n;
    };

    // --- Config files (HIGH) ---
    if is_csproj_web(abs_path) {
        add(&mut scores, "api", WEIGHT_HIGH);
    }
    if file_exists(abs_path, "next.config.*") || file_exists(abs_path, "vite.config.*") {
        add(&mut scores, "ui", WEIGHT_HIGH);
    }
    if file_exists(abs_path, "drizzle.config.*") || dir_exists(abs_path, "prisma") {
        add(&mut scores, "database", WEIGHT_HIGH);
    }
    if is_csproj_library(abs_path) && !is_csproj_web(abs_path) {
        add(&mut scores, "library", WEIGHT_HIGH);
    }
    if file_exists(abs_path, "pubspec.yaml") {
        add(&mut scores, "mobile", WEIGHT_HIGH);
    }

    // --- package.json deps (MEDIUM) ---
    let deps = package_json_deps(abs_path);
    if !deps.is_empty() {
        if has_any_dep(&deps, &["express", "fastify", "hono", "koa", "nestjs"]) {
            add(&mut scores, "api", WEIGHT_MEDIUM);
        }
        if has_any_dep(&deps, &["react", "next", "vue", "nuxt", "svelte", "angular"]) {
            add(&mut scores, "ui", WEIGHT_MEDIUM);
        }
        if has_any_dep(&deps, &["drizzle-orm", "prisma", "typeorm", "knex", "sequelize"]) {
            add(&mut scores, "database", WEIGHT_MEDIUM);
        }
    }

    // --- pubspec.yaml deps (MEDIUM) ---
    if read_safe(&abs_path.join("pubspec.yaml")).contains("flutter:") {
        add(&mut scores, "mobile", WEIGHT_MEDIUM);
    }

    // --- go.mod / pyproject.toml / Cargo.toml (HIGH) ---
    if file_exists(abs_path, "go.mod")
        && manifest_has(abs_path, "go.mod", &["net/http", "gin", "echo", "fiber"])
    {
        add(&mut scores, "api", WEIGHT_HIGH);
    }
    if file_exists(abs_path, "pyproject.toml")
        && manifest_has(
            abs_path,
            "pyproject.toml",
            &["fastapi", "django", "flask", "starlette"],
        )
    {
        add(&mut scores, "api", WEIGHT_HIGH);
    }
    if file_exists(abs_path, "Cargo.toml")
        && manifest_has(abs_path, "Cargo.toml", &["actix", "axum", "rocket", "warp"])
    {
        add(&mut scores, "api", WEIGHT_HIGH);
    }

    // --- Directories (LOW) ---
    if dir_exists(abs_path, "Controllers")
        || dir_exists(abs_path, "Modules")
        || dir_exists(abs_path, "routes")
    {
        add(&mut scores, "api", WEIGHT_LOW);
    }
    if dir_exists(abs_path, "app") && dir_exists(abs_path, "components") {
        add(&mut scores, "ui", WEIGHT_LOW);
    }
    if abs_path.join("src/app").is_dir() && abs_path.join("src/components").is_dir() {
        add(&mut scores, "ui", WEIGHT_LOW);
    }
    if dir_exists(abs_path, "migrations") || dir_exists(abs_path, "schema") {
        add(&mut scores, "database", WEIGHT_LOW);
    }
    if abs_path.join("src/migrations").is_dir() || abs_path.join("src/schema").is_dir() {
        add(&mut scores, "database", WEIGHT_LOW);
    }
    if dir_exists(abs_path, "lib")
        && (dir_exists(abs_path, "android") || dir_exists(abs_path, "ios"))
    {
        add(&mut scores, "mobile", WEIGHT_LOW);
    }

    // Highest score wins; the JS iteration order is api, ui, database, library,
    // mobile — replicated by checking that fixed order on ties.
    let order = ["api", "ui", "database", "library", "mobile"];
    let mut role = "general";
    let mut max = 0;
    for key in order {
        let s = scores.get(key).copied().unwrap_or(0);
        if s > max {
            max = s;
            role = key;
        }
    }
    role.to_string()
}

/// Map a role to its agent — a port of `ROLE_AGENT_MAP` / `roleToAgent`.
fn role_to_agent(role: &str) -> &'static str {
    match role {
        "api" | "library" => "backend",
        "ui" => "frontend",
        "database" => "database",
        "mobile" => "mobile",
        _ => "general",
    }
}

/// Compact stack summary — a trimmed port of `getStackSummary()` covering the
/// `package.json` and `.csproj` cases the registry consumes.
fn stack_summary(abs_path: &Path) -> String {
    let mut parts: Vec<String> = Vec::new();
    let csproj = find_csproj_files(abs_path, 2);
    if !csproj.is_empty() {
        let joined: String = csproj.iter().map(|f| read_safe(f)).collect::<Vec<_>>().join("\n");
        if let Some(tfm) = extract_between(&joined, "<TargetFramework>", "</TargetFramework>") {
            if let Some(ver) = tfm.strip_prefix("net") {
                parts.push(format!(".NET {ver}"));
            }
        }
        if joined.contains("Microsoft.NET.Sdk.Web") {
            parts.push("Web API".to_string());
        }
    }
    let pkg = read_safe(&abs_path.join("package.json"));
    if !pkg.is_empty() {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&pkg) {
            let mut all: BTreeMap<String, String> = BTreeMap::new();
            for section in ["dependencies", "devDependencies"] {
                if let Some(obj) = json.get(section).and_then(serde_json::Value::as_object) {
                    for (k, v) in obj {
                        all.insert(k.clone(), v.as_str().unwrap_or("").to_string());
                    }
                }
            }
            let interesting = [
                "react", "next", "vue", "nuxt", "svelte", "angular", "express", "fastify",
                "hono", "tailwindcss", "drizzle-orm", "prisma", "typeorm", "typescript",
            ];
            for dep in interesting {
                if let Some(raw) = all.get(dep) {
                    let name = if dep == "tailwindcss" {
                        "Tailwind".to_string()
                    } else {
                        let mut chars = dep.chars();
                        chars
                            .next()
                            .map(|c| c.to_ascii_uppercase().to_string() + chars.as_str())
                            .unwrap_or_default()
                    };
                    let ver: String = raw
                        .chars()
                        .filter(|c| !matches!(c, '^' | '~' | '>' | '=' | '<'))
                        .collect();
                    let short: Vec<&str> = ver.split('.').take(2).collect();
                    parts.push(format!("{name} {}", short.join(".")));
                }
            }
        }
    }
    parts.join(", ")
}

/// Extract the text between two markers — a small helper for `stack_summary`.
fn extract_between(content: &str, open: &str, close: &str) -> Option<String> {
    let start = content.find(open)? + open.len();
    let end = content[start..].find(close)? + start;
    Some(content[start..end].to_string())
}

/// List the `.md` command files inside `<subproject>/.claude/commands/`.
fn get_commands(abs_path: &Path) -> Vec<String> {
    let dir = abs_path.join(".claude").join("commands");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter(|n| n.ends_with(".md"))
        .collect();
    names.sort();
    names
}

/// Build the agents list from `.claude/prompts/*.md` — a port of `getAgents()`.
fn get_agents(root: &Path) -> Vec<String> {
    let mut agents = vec!["orchestrator".to_string()];
    let dir = root.join(".claude").join("prompts");
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            if let Some(name) = e.file_name().to_str() {
                if name.ends_with(".md") && !name.starts_with('_') {
                    agents.push(name.trim_end_matches(".md").to_string());
                }
            }
        }
    }
    agents.sort();
    agents.dedup();
    agents
}

/// `true` if `dir` directly contains a recognised build manifest — the
/// language-agnostic signal that the directory is an independent subproject.
/// A `Cargo.toml` counts only when it declares a `[package]`: a virtual
/// workspace root (`[workspace]` only) is not itself a subproject.
fn has_build_manifest(dir: &Path) -> bool {
    if dir.join("package.json").is_file()
        || dir.join("go.mod").is_file()
        || dir.join("pyproject.toml").is_file()
        || dir.join("pubspec.yaml").is_file()
        || file_exists(dir, "*.csproj")
    {
        return true;
    }
    let cargo = dir.join("Cargo.toml");
    cargo.is_file() && read_safe(&cargo).contains("[package]")
}

/// Read `subprojects.exclude` / `.include` from `.claude/mustard.json`.
/// Entries are repo-root-relative paths; returned normalised (forward slash,
/// no surrounding slashes).
fn read_overrides(root: &Path) -> (Vec<String>, Vec<String>) {
    let mut exclude = Vec::new();
    let mut include = Vec::new();
    let content = read_safe(&root.join(".claude").join("mustard.json"));
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
        if let Some(sp) = json.get("subprojects") {
            for (field, out) in [("exclude", &mut exclude), ("include", &mut include)] {
                if let Some(arr) = sp.get(field).and_then(serde_json::Value::as_array) {
                    for v in arr.iter().filter_map(serde_json::Value::as_str) {
                        out.push(v.replace('\\', "/").trim_matches('/').to_string());
                    }
                }
            }
        }
    }
    (exclude, include)
}

/// Apply the `mustard.json` override to a detected path list: drop excluded
/// entries, append included ones not already present.
fn apply_overrides(root: &Path, paths: &mut Vec<String>) {
    let (exclude, include) = read_overrides(root);
    paths.retain(|p| !exclude.contains(p));
    for inc in include {
        if !inc.is_empty() && !paths.contains(&inc) {
            paths.push(inc);
        }
    }
}

/// Discover subprojects by scanning for directories carrying a build manifest
/// (BFS, max depth 3) — the agnostic successor to `scanForSubprojects()`.
fn scan_for_subprojects(root: &Path) -> Vec<String> {
    const IGNORE: &[&str] = &[
        "node_modules",
        "bin",
        "obj",
        "dist",
        ".next",
        "_backup",
        "migrations",
        ".claude",
        ".git",
    ];
    let mut results = Vec::new();
    fn walk(
        abs_dir: &Path,
        rel_dir: &str,
        depth: usize,
        ignore: &[&str],
        out: &mut Vec<String>,
    ) {
        if depth > 3 {
            return;
        }
        if depth > 0 && has_build_manifest(abs_dir) {
            out.push(rel_dir.replace('\\', "/"));
            return;
        }
        let Ok(entries) = std::fs::read_dir(abs_dir) else {
            return;
        };
        for e in entries.flatten() {
            if !e.path().is_dir() {
                continue;
            }
            let Some(name) = e.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if name.starts_with('.') || ignore.contains(&name.as_str()) {
                continue;
            }
            let next_rel = if rel_dir.is_empty() {
                name.clone()
            } else {
                format!("{rel_dir}/{name}")
            };
            walk(&e.path(), &next_rel, depth + 1, ignore, out);
        }
    }
    walk(root, "", 0, IGNORE, &mut results);
    results
}

/// Recursively collect source + manifest files under `dir`, returned as paths
/// relative to `root` with forward slashes — a port of `collectSourceFiles()`.
fn collect_source_files(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let Some(name) = e.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if name.starts_with('.')
            || matches!(
                name.as_str(),
                "node_modules" | "bin" | "obj" | "dist" | "migrations" | "_backup"
            )
        {
            continue;
        }
        let path = e.path();
        if path.is_dir() {
            collect_source_files(root, &path, out);
        } else if path.is_file() {
            let is_source = SOURCE_EXTENSIONS
                .iter()
                .any(|ext| name.to_ascii_lowercase().ends_with(ext));
            let is_manifest = MANIFEST_FILES.contains(&name.as_str());
            if is_source || is_manifest {
                let rel = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push(rel);
            }
        }
    }
}

/// Compute the SHA-256 hash of every source file under a subproject — a port of
/// `computeSourceHash()`. Files are sorted; each file's path is mixed into the
/// hash before its content (so renames are detected).
fn compute_source_hash(root: &Path, subproject_rel: &str) -> String {
    let abs = if subproject_rel == "." {
        root.to_path_buf()
    } else {
        root.join(subproject_rel)
    };
    let mut files = Vec::new();
    collect_source_files(root, &abs, &mut files);
    files.sort();
    let mut hash = Sha256::new();
    for file in &files {
        hash.update(file.as_bytes());
        if let Ok(bytes) = std::fs::read(root.join(file)) {
            hash.update(&bytes);
        }
    }
    hash.hex_digest()
}

/// Run `mustard-rt run sync-detect` rooted at `root`, writing the JSON to stdout.
///
/// Fail-open: discovery errors degrade to empty results rather than aborting.
pub fn run(root: &Path) {
    // 1. Discover subprojects via the build-manifest scan, apply the
    //    `mustard.json` override, then fall back to the repo root for a
    //    single-root project (no nested subprojects).
    let mut subproject_paths = scan_for_subprojects(root);
    apply_overrides(root, &mut subproject_paths);
    if subproject_paths.is_empty()
        && (root.join("CLAUDE.md").exists() || has_build_manifest(root))
    {
        subproject_paths.push(".".to_string());
    }

    // 2. Build subproject entries.
    let mut subprojects = Vec::new();
    let mut detected_agents: Vec<String> = Vec::new();
    let mut source_hashes: BTreeMap<String, String> = BTreeMap::new();

    for rel_path in &subproject_paths {
        let abs = if rel_path == "." {
            root.to_path_buf()
        } else {
            root.join(rel_path)
        };
        // Detection keys off the build manifest, not a `CLAUDE.md` — a
        // subproject need not carry one. Only guard against a bogus path
        // (e.g. a stale `mustard.json` `include` entry).
        if !abs.is_dir() {
            continue;
        }
        let name = if rel_path == "." {
            root.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("project")
                .to_string()
        } else {
            rel_path.rsplit('/').next().unwrap_or(rel_path).to_string()
        };
        let role = detect_role(&abs);
        let agent = role_to_agent(&role).to_string();
        if agent != "general" && !detected_agents.contains(&agent) {
            detected_agents.push(agent.clone());
        }
        let normalized = rel_path.replace('\\', "/");
        let hash = compute_source_hash(root, &normalized);
        // Keyed by `path`, not `name`: two subprojects can share a folder
        // name (`apps/a/core` vs `packages/core`) but never a path.
        source_hashes.insert(normalized.clone(), hash);

        subprojects.push(Subproject {
            name,
            path: normalized,
            role,
            agent,
            commands: get_commands(&abs),
            stack_summary: stack_summary(&abs),
            // No previous cache is consulted (the cache gate is not ported), so
            // every subproject is reported as changed — the safe default.
            hash_changed: true,
            git_dirty: None,
            git_dirty_count: None,
        });
    }

    detected_agents.sort();

    let output = DetectOutput {
        subprojects,
        agents: get_agents(root),
        detected_agents,
        prompts_dir: ".claude/prompts".to_string(),
        prompts_compiled_dir: ".claude/prompts_compiled".to_string(),
        source_hashes,
        module_hashes: BTreeMap::new(),
        warnings: None,
    };

    // The JS script prints `JSON.stringify(result, null, 2)` followed by `\n`.
    match serde_json::to_string_pretty(&output) {
        Ok(json) => println!("{json}"),
        Err(_) => println!("{{}}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn detect_role_scores_ui_for_next_config() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("next.config.js"), "").unwrap();
        assert_eq!(detect_role(dir.path()), "ui");
    }

    #[test]
    fn detect_role_general_when_no_signals() {
        let dir = tempdir().unwrap();
        assert_eq!(detect_role(dir.path()), "general");
    }

    #[test]
    fn role_to_agent_maps_known_roles() {
        assert_eq!(role_to_agent("api"), "backend");
        assert_eq!(role_to_agent("library"), "backend");
        assert_eq!(role_to_agent("ui"), "frontend");
        assert_eq!(role_to_agent("unknown"), "general");
    }

    #[test]
    fn source_hash_changes_when_file_content_changes() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.ts"), "const x = 1;").unwrap();
        let h1 = compute_source_hash(dir.path(), ".");
        std::fs::write(dir.path().join("a.ts"), "const x = 2;").unwrap();
        let h2 = compute_source_hash(dir.path(), ".");
        assert_ne!(h1, h2, "hash must change when a source file changes");
    }

    #[test]
    fn source_hash_stable_when_nothing_changes() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.ts"), "const x = 1;").unwrap();
        assert_eq!(
            compute_source_hash(dir.path(), "."),
            compute_source_hash(dir.path(), ".")
        );
    }

    #[test]
    fn scan_for_subprojects_finds_manifest_dirs() {
        let dir = tempdir().unwrap();
        let app = dir.path().join("apps").join("web");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(app.join("package.json"), "{}").unwrap();
        let found = scan_for_subprojects(dir.path());
        assert_eq!(found, vec!["apps/web".to_string()]);
    }

    #[test]
    fn scan_ignores_manifestless_dir_even_with_claude_md() {
        // A payload dir (e.g. `templates/`) carries a `CLAUDE.md` but no build
        // manifest — it must not be mistaken for a subproject.
        let dir = tempdir().unwrap();
        let payload = dir.path().join("apps").join("cli").join("templates");
        std::fs::create_dir_all(&payload).unwrap();
        std::fs::write(payload.join("CLAUDE.md"), "# template").unwrap();
        let crate_dir = dir.path().join("apps").join("cli");
        std::fs::write(crate_dir.join("Cargo.toml"), "[package]\nname = \"cli\"").unwrap();
        let found = scan_for_subprojects(dir.path());
        assert_eq!(found, vec!["apps/cli".to_string()]);
    }

    #[test]
    fn has_build_manifest_rejects_virtual_workspace_root() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[workspace]\nmembers = []").unwrap();
        assert!(!has_build_manifest(dir.path()));
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"x\"").unwrap();
        assert!(has_build_manifest(dir.path()));
    }

    #[test]
    fn apply_overrides_excludes_and_includes_by_path() {
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(
            claude.join("mustard.json"),
            r#"{"subprojects":{"exclude":["apps/drop"],"include":["infra/edge"]}}"#,
        )
        .unwrap();
        let mut paths = vec!["apps/keep".to_string(), "apps/drop".to_string()];
        apply_overrides(dir.path(), &mut paths);
        assert_eq!(paths, vec!["apps/keep".to_string(), "infra/edge".to_string()]);
    }
}

//! `mustard-rt run sync-detect` — a port of `scripts/sync-detect.js`.
//!
//! Discovers subprojects in a monorepo by scanning for **build manifests**
//! (`Cargo.toml` with `[package]`, `package.json`, `*.csproj`, `go.mod`,
//! `pyproject.toml`, `pubspec.yaml`) — the language-agnostic signal that a
//! directory is an independent buildable unit. The root `mustard.json` may
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

use super::subproject_discovery::{self, DiscoveryOptions};
use crate::util::sha256::Sha256;
use mustard_core::io::fs;
use mustard_core::ClaudePaths;
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
    fs::read_dir(dir).is_ok_and(|entries| {
        entries.into_iter().any(|e| {
            e.file_name.starts_with(parts[0]) && e.file_name.ends_with(parts[1])
        })
    })
}

/// `true` if `dir_name` exists as a directory inside `base` (or one level deep).
fn dir_exists(base: &Path, dir_name: &str) -> bool {
    if base.join(dir_name).is_dir() {
        return true;
    }
    let Ok(entries) = fs::read_dir(base) else {
        return false;
    };
    for e in entries {
        if e.file_name.starts_with('.') || matches!(e.file_name.as_str(), "node_modules" | "bin" | "obj") {
            continue;
        }
        if e.path.join(dir_name).is_dir() {
            return true;
        }
    }
    false
}

/// Recursively find `.csproj` files up to `max_depth` levels deep.
fn find_csproj_files(dir: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut results = Vec::new();
    fn walk(dir: &Path, remaining: usize, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for e in entries {
            if !e.is_dir && e.file_name.ends_with(".csproj") {
                out.push(e.path.clone());
            } else if e.is_dir
                && remaining > 0
                && !e.file_name.starts_with('.')
                && !matches!(e.file_name.as_str(), "node_modules" | "bin" | "obj")
            {
                walk(&e.path, remaining - 1, out);
            }
        }
    }
    walk(dir, max_depth, &mut results);
    results
}

/// Read a file, returning an empty string on any error.
fn read_safe(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
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
    let Ok(paths) = ClaudePaths::for_project(abs_path) else {
        return Vec::new();
    };
    let dir = paths.commands_dir();
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .into_iter()
        .map(|e| e.file_name)
        .filter(|n| n.ends_with(".md"))
        .collect();
    names.sort();
    names
}

/// Build the agents list from `.claude/prompts/*.md` — a port of `getAgents()`.
fn get_agents(root: &Path) -> Vec<String> {
    let mut agents = vec!["orchestrator".to_string()];
    // Legacy `.claude/prompts/` is not surfaced by `ClaudePaths`; route via
    // `claude_dir()` so the canonical handle still owns the boundary without
    // forcing a new accessor for a deprecated subdirectory.
    let Ok(paths) = ClaudePaths::for_project(root) else {
        agents.sort();
        agents.dedup();
        return agents;
    };
    let dir = paths.claude_dir().join("prompts");
    if let Ok(entries) = fs::read_dir(&dir) {
        for e in entries {
            if e.file_name.ends_with(".md") && !e.file_name.starts_with('_') {
                agents.push(e.file_name.trim_end_matches(".md").to_string());
            }
        }
    }
    agents.sort();
    agents.dedup();
    agents
}

/// Recursively collect source + manifest files under `dir`, returned as paths
/// relative to `root` with forward slashes — a port of `collectSourceFiles()`.
fn collect_source_files(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries {
        if e.file_name.starts_with('.')
            || matches!(
                e.file_name.as_str(),
                "node_modules" | "bin" | "obj" | "dist" | "migrations" | "_backup"
            )
        {
            continue;
        }
        if e.is_dir {
            collect_source_files(root, &e.path, out);
        } else {
            let is_source = SOURCE_EXTENSIONS
                .iter()
                .any(|ext| e.file_name.to_ascii_lowercase().ends_with(ext));
            let is_manifest = MANIFEST_FILES.contains(&e.file_name.as_str());
            if is_source || is_manifest {
                let rel = e.path
                    .strip_prefix(root)
                    .unwrap_or(&e.path)
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
        if let Ok(bytes) = fs::read(root.join(file)) {
            hash.update(&bytes);
        }
    }
    hash.hex_digest()
}

/// Run `mustard-rt run sync-detect` rooted at `root`, writing the JSON to stdout.
///
/// Fail-open: discovery errors degrade to empty results rather than aborting.
pub fn run(root: &Path) {
    // 1. Discover subprojects via the canonical build-manifest BFS (the same
    //    source of truth `sync-registry` uses). It applies the `mustard.json`
    //    override, performs the single-root fallback, and drops bogus paths
    //    (e.g. a stale `mustard.json` `include`) internally. Detection keys off
    //    the build manifest, not a `CLAUDE.md` — a subproject need not carry one.
    let discovered =
        subproject_discovery::discover_subprojects(root, &DiscoveryOptions::default());

    // 2. Build subproject entries.
    let mut subprojects = Vec::new();
    let mut detected_agents: Vec<String> = Vec::new();
    let mut source_hashes: BTreeMap<String, String> = BTreeMap::new();

    for sub in &discovered {
        let abs = if sub.rel_path == "." {
            root.to_path_buf()
        } else {
            root.join(&sub.rel_path)
        };
        let name = sub.name.clone();
        let role = detect_role(&abs);
        let agent = role_to_agent(&role).to_string();
        if agent != "general" && !detected_agents.contains(&agent) {
            detected_agents.push(agent.clone());
        }
        // `rel_path` is already forward-slash normalised by discovery.
        let normalized = sub.rel_path.clone();
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

    // Subproject discovery (build-manifest BFS, `mustard.json` overrides,
    // single-root fallback) now lives in `super::subproject_discovery` and is
    // tested there — this module keeps only the role-scoring + source-hash tests.
}

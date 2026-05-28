//! `mustard-rt run scan-structural --subproject <path>` — agnostic manifest +
//! source structural scan, Rust-only (no LLM).
//!
//! Wave 3 (deep-refactor) extracts the structural facts a `/scan` agent used to
//! re-derive from prose: which manifests are present, which dependencies they
//! declare (verbatim, no normalisation), the dominant file extensions in the
//! subproject's source tree, and the cluster shapes the agnostic
//! [`crate::commands::scan::cluster_discovery`] pass produced. The agent receives a
//! structured digest instead of re-walking the tree.
//!
//! The user-facing output is a single `stack.md` ≤60 lines written to
//! `<sub>/.claude/commands/stack.md` (idempotent, atomic). The machine-readable
//! mirror is the pretty JSON printed to stdout.
//!
//! Manifests parsed (verbatim, fail-open per parser):
//!
//! | Manifest | Stack flavour |
//! |---|---|
//! | `Cargo.toml` | compiled-strongly-typed |
//! | `package.json` | transpiled-typed (when `tsconfig.json` present) / dynamic-scripting |
//! | `requirements.txt` / `pyproject.toml` | dynamic-scripting |
//! | `go.mod` | compiled-strongly-typed |
//! | `pom.xml` / `build.gradle*` | compiled-strongly-typed |
//! | `composer.json` | dynamic-scripting |
//! | `Gemfile` | dynamic-scripting |
//! | `*.csproj` | compiled-strongly-typed |
//! | `pubspec.yaml` | compiled-strongly-typed |

use mustard_core::fs as mfs;
use mustard_core::ClaudePaths;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The structural digest produced for a single subproject.
#[derive(Debug, Default)]
struct StructuralReport {
    subproject: String,
    manifests: Vec<String>,
    dependencies: Vec<(String, String)>, // (manifest, dep name)
    ext_counts: BTreeMap<String, usize>,
    cluster_count: usize,
    cluster_labels: Vec<String>,
    file_count: usize,
}

impl StructuralReport {
    fn to_json(&self) -> Value {
        json!({
            "subproject": self.subproject,
            "manifests": self.manifests,
            "dependencies": self.dependencies.iter().map(|(m, d)| json!({"manifest": m, "name": d})).collect::<Vec<_>>(),
            "extensions": self.ext_counts.iter().map(|(k, v)| json!({"ext": k, "count": v})).collect::<Vec<_>>(),
            "fileCount": self.file_count,
            "clusters": self.cluster_count,
            "clusterLabels": self.cluster_labels,
        })
    }

    fn to_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str("<!-- mustard:generated -->\n");
        let _ = writeln!(out, "# Stack — `{}`\n", self.subproject);
        if !self.manifests.is_empty() {
            out.push_str("## Manifests\n");
            for m in &self.manifests {
                let _ = writeln!(out, "- `{m}`");
            }
            out.push('\n');
        }
        if !self.dependencies.is_empty() {
            out.push_str("## Dependencies\n");
            // Cap at 30 to respect the 60-line budget.
            for (manifest, dep) in self.dependencies.iter().take(30) {
                let _ = writeln!(out, "- `{dep}` (from `{manifest}`)");
            }
            if self.dependencies.len() > 30 {
                let _ = writeln!(out, "- ... {} more", self.dependencies.len() - 30);
            }
            out.push('\n');
        }
        if !self.ext_counts.is_empty() {
            out.push_str("## Source extensions\n");
            let mut entries: Vec<_> = self.ext_counts.iter().collect();
            entries.sort_by(|a, b| b.1.cmp(a.1));
            for (ext, count) in entries.iter().take(6) {
                let _ = writeln!(out, "- `{ext}` — {count}");
            }
            out.push('\n');
        }
        let _ = writeln!(
            out,
            "## Clusters\n- {} clusters across {} source files",
            self.cluster_count, self.file_count
        );
        if !self.cluster_labels.is_empty() {
            for label in self.cluster_labels.iter().take(8) {
                let _ = writeln!(out, "- `{label}`");
            }
        }
        out
    }
}

/// Pretty-list of manifests we look for, in priority order.
const MANIFEST_NAMES: &[&str] = &[
    "Cargo.toml",
    "package.json",
    "requirements.txt",
    "pyproject.toml",
    "go.mod",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "composer.json",
    "Gemfile",
    "pubspec.yaml",
];

/// Parse a manifest into a verbatim list of dependency names.
fn parse_manifest(path: &Path, body: &str) -> Vec<String> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    match name.as_str() {
        "Cargo.toml" => parse_cargo_toml(body),
        "package.json" => parse_package_json(body),
        "requirements.txt" => parse_requirements_txt(body),
        "pyproject.toml" => parse_pyproject(body),
        "go.mod" => parse_go_mod(body),
        "composer.json" => parse_package_json(body), // same json shape — keys differ but `require` works
        "Gemfile" => parse_gemfile(body),
        "pubspec.yaml" => parse_pubspec(body),
        _ if name.ends_with(".csproj") => parse_csproj(body),
        _ if name == "pom.xml" => parse_pom_xml(body),
        _ if name.starts_with("build.gradle") => parse_gradle(body),
        _ => Vec::new(),
    }
}

fn parse_cargo_toml(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_deps = false;
    for line in body.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_deps = t == "[dependencies]"
                || t == "[dev-dependencies]"
                || t == "[build-dependencies]";
            continue;
        }
        if !in_deps || t.is_empty() || t.starts_with('#') {
            continue;
        }
        if let Some(eq) = t.find('=') {
            let name = t[..eq].trim().trim_matches('"');
            if !name.is_empty() {
                out.push(name.to_string());
            }
        }
    }
    out
}

fn parse_package_json(body: &str) -> Vec<String> {
    let parsed: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for key in ["dependencies", "devDependencies", "peerDependencies", "require"] {
        if let Some(obj) = parsed.get(key).and_then(Value::as_object) {
            for k in obj.keys() {
                out.push(k.clone());
            }
        }
    }
    out
}

fn parse_requirements_txt(body: &str) -> Vec<String> {
    body.lines()
        .filter_map(|line| {
            let t = line.trim();
            if t.is_empty() || t.starts_with('#') || t.starts_with('-') {
                return None;
            }
            let name = t
                .split(['=', '<', '>', ';', '['])
                .next()?
                .trim();
            (!name.is_empty()).then(|| name.to_string())
        })
        .collect()
}

fn parse_pyproject(body: &str) -> Vec<String> {
    // Best-effort: pull `name = "x"` keys under `[tool.poetry.dependencies]` /
    // `[project.dependencies]`. Bare TOML without a heavyweight dep.
    let mut out = Vec::new();
    let mut in_deps = false;
    for line in body.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_deps = t == "[tool.poetry.dependencies]"
                || t == "[project.dependencies]"
                || t == "[tool.poetry.dev-dependencies]";
            continue;
        }
        if !in_deps || t.is_empty() || t.starts_with('#') {
            continue;
        }
        if let Some(eq) = t.find('=') {
            let name = t[..eq].trim().trim_matches('"');
            if !name.is_empty() && name != "python" {
                out.push(name.to_string());
            }
        } else if t.starts_with('"') {
            // `[project] dependencies = ["x", "y"]` style
            let name = t.trim_matches(|c: char| c == ',' || c == '"').to_string();
            if !name.is_empty() {
                out.push(name);
            }
        }
    }
    out
}

fn parse_go_mod(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_require = false;
    for line in body.lines() {
        let t = line.trim();
        if t.starts_with("require (") {
            in_require = true;
            continue;
        }
        if in_require && t == ")" {
            in_require = false;
            continue;
        }
        if let Some(after) = t.strip_prefix("require ") {
            let after = after.trim();
            if let Some(name) = after.split_whitespace().next() {
                out.push(name.to_string());
            }
        } else if in_require && !t.is_empty() {
            if let Some(name) = t.split_whitespace().next() {
                out.push(name.to_string());
            }
        }
    }
    out
}

fn parse_gemfile(body: &str) -> Vec<String> {
    body.lines()
        .filter_map(|line| {
            let t = line.trim();
            if !t.starts_with("gem ") {
                return None;
            }
            let after = t["gem ".len()..].trim_start();
            let quote = after.chars().next()?;
            if quote != '"' && quote != '\'' {
                return None;
            }
            after[1..].split(quote).next().map(str::to_string)
        })
        .collect()
}

fn parse_pubspec(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_deps = false;
    for line in body.lines() {
        let trimmed = line.trim_end();
        if trimmed == "dependencies:" || trimmed == "dev_dependencies:" {
            in_deps = true;
            continue;
        }
        if in_deps && !trimmed.starts_with(' ') && !trimmed.is_empty() {
            in_deps = false;
        }
        if !in_deps {
            continue;
        }
        let t = trimmed.trim_start();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if let Some(colon) = t.find(':') {
            let name = t[..colon].trim();
            if !name.is_empty() {
                out.push(name.to_string());
            }
        }
    }
    out
}

fn parse_csproj(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in body.lines() {
        let t = line.trim();
        if let Some(start) = t.find("PackageReference Include=\"") {
            let after = &t[start + "PackageReference Include=\"".len()..];
            if let Some(end) = after.find('"') {
                out.push(after[..end].to_string());
            }
        }
    }
    out
}

fn parse_pom_xml(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current_group: Option<String> = None;
    let mut current_artifact: Option<String> = None;
    for line in body.lines() {
        let t = line.trim();
        if let Some(start) = t.find("<groupId>") {
            if let Some(end) = t[start..].find("</groupId>") {
                current_group =
                    Some(t[start + "<groupId>".len()..start + end].to_string());
            }
        }
        if let Some(start) = t.find("<artifactId>") {
            if let Some(end) = t[start..].find("</artifactId>") {
                current_artifact =
                    Some(t[start + "<artifactId>".len()..start + end].to_string());
            }
        }
        if t.contains("</dependency>") {
            if let (Some(g), Some(a)) = (current_group.take(), current_artifact.take()) {
                out.push(format!("{g}:{a}"));
            }
        }
    }
    out
}

fn parse_gradle(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in body.lines() {
        let t = line.trim();
        // Match e.g. `implementation 'org.x:y:1.0'` / `api("org.x:y:1.0")`.
        for prefix in &[
            "implementation",
            "api",
            "compileOnly",
            "runtimeOnly",
            "testImplementation",
        ] {
            if let Some(after) = t.strip_prefix(prefix) {
                let after = after.trim_start_matches(|c: char| c == '(' || c.is_whitespace());
                let quote = after.chars().next().unwrap_or(' ');
                if quote != '"' && quote != '\'' {
                    continue;
                }
                if let Some(end) = after[1..].find(quote) {
                    let dep = &after[1..=end];
                    if !dep.is_empty() {
                        out.push(dep.to_string());
                    }
                }
            }
        }
    }
    out
}

/// Count source files by extension under `root`, ignoring common skip dirs.
fn count_extensions(root: &Path) -> (BTreeMap<String, usize>, usize) {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut total = 0_usize;
    walk_source(root, &mut counts, &mut total);
    (counts, total)
}

const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    "obj",
    "bin",
    ".git",
    ".next",
    "__pycache__",
    ".venv",
    "venv",
    "Pods",
    "vendor",
];

fn walk_source(dir: &Path, counts: &mut BTreeMap<String, usize>, total: &mut usize) {
    let Ok(entries) = mfs::read_dir(dir) else { return };
    for entry in entries {
        if entry.is_dir {
            let name = entry.file_name.clone();
            if SKIP_DIRS.contains(&name.as_str()) || name.starts_with('.') {
                continue;
            }
            walk_source(&entry.path, counts, total);
        } else {
            let name = entry.file_name.clone();
            if let Some(idx) = name.rfind('.') {
                let ext = &name[idx..];
                // Filter manifests/lockfiles/binary noise.
                if matches!(
                    ext,
                    ".lock" | ".log" | ".png" | ".jpg" | ".jpeg" | ".gif" | ".svg" | ".ico"
                ) {
                    continue;
                }
                *counts.entry(ext.to_string()).or_insert(0) += 1;
                *total += 1;
            }
        }
    }
}

/// Build the structural report for one subproject.
fn build_report(repo_root: &Path, subproject: &str) -> StructuralReport {
    let abs = if subproject == "." {
        repo_root.to_path_buf()
    } else {
        repo_root.join(subproject)
    };

    let mut report = StructuralReport {
        subproject: subproject.to_string(),
        ..Default::default()
    };

    // Manifests at the subproject root.
    for name in MANIFEST_NAMES {
        let candidate = abs.join(name);
        if candidate.exists() {
            report.manifests.push((*name).to_string());
            if let Ok(body) = mfs::read_to_string(&candidate) {
                for dep in parse_manifest(&candidate, &body) {
                    report.dependencies.push(((*name).to_string(), dep));
                }
            }
        }
    }
    // `.csproj` files (any filename).
    if let Ok(entries) = mfs::read_dir(&abs) {
        for entry in entries {
            if !entry.is_dir && entry.file_name.ends_with(".csproj") {
                report.manifests.push(entry.file_name.clone());
                if let Ok(body) = mfs::read_to_string(&entry.path) {
                    for dep in parse_csproj(&body) {
                        report.dependencies.push((entry.file_name.clone(), dep));
                    }
                }
            }
        }
    }

    let (counts, total) = count_extensions(&abs);
    report.ext_counts = counts;
    report.file_count = total;

    // Cluster discovery — reuse the agnostic pass.
    let stack_id = crate::commands::scan::detect_stack(&abs).unwrap_or("unknown").to_string();
    let clusters =
        crate::commands::scan::cluster_discovery::discover_clusters(&abs, &stack_id, Some(subproject));
    report.cluster_count = clusters.len();
    report.cluster_labels = clusters
        .iter()
        .filter_map(|c| c.get("label").and_then(Value::as_str).map(str::to_string))
        .collect();

    report
}

/// Write `stack.md` under `<sub>/.claude/commands/`. Best-effort, fail-open.
fn write_stack_md(repo_root: &Path, report: &StructuralReport) -> Option<String> {
    let abs = if report.subproject == "." {
        repo_root.to_path_buf()
    } else {
        repo_root.join(&report.subproject)
    };
    let dir = ClaudePaths::for_project(&abs).ok()?.commands_dir();
    if mfs::create_dir_all(&dir).is_err() {
        return None;
    }
    let path = dir.join("stack.md");
    let body = report.to_markdown();
    // Enforce 60-line budget conservatively (already paginated above).
    let trimmed: String = body
        .lines()
        .take(60)
        .collect::<Vec<_>>()
        .join("\n");
    if mfs::write_atomic(&path, trimmed.as_bytes()).is_err() {
        return None;
    }
    Some(path.to_string_lossy().replace('\\', "/"))
}

/// Dispatch `mustard-rt run scan-structural --subproject <path>`.
pub fn run(subproject: Option<&str>) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let subs: Vec<String> = match subproject {
        Some(s) => vec![s.to_string()],
        None => {
            // Walk every detected subproject.
            let mut out: Vec<String> = vec![".".to_string()];
            for top in ["apps", "packages"] {
                if let Ok(entries) = mfs::read_dir(cwd.join(top)) {
                    for entry in entries {
                        if entry.is_dir {
                            out.push(format!("{top}/{}", entry.file_name));
                        }
                    }
                }
            }
            out
        }
    };

    let mut all: Vec<Value> = Vec::new();
    for sub in &subs {
        let report = build_report(&cwd, sub);
        let written = write_stack_md(&cwd, &report);
        let mut entry = report.to_json();
        if let Value::Object(map) = &mut entry {
            map.insert(
                "stackMd".to_string(),
                written.map_or(Value::Null, Value::String),
            );
        }
        all.push(entry);
    }

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({ "subprojects": all })).unwrap_or_else(|_| "{}".into())
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn parses_cargo_toml() {
        let body = "[package]\nname = \"x\"\n[dependencies]\nserde = \"1\"\nclap = \"4\"\n";
        let deps = parse_cargo_toml(body);
        assert!(deps.contains(&"serde".to_string()));
        assert!(deps.contains(&"clap".to_string()));
    }

    #[test]
    fn parses_package_json() {
        let body = r#"{"dependencies":{"react":"19","next":"15"},"devDependencies":{"vite":"5"}}"#;
        let deps = parse_package_json(body);
        assert!(deps.contains(&"react".to_string()));
        assert!(deps.contains(&"vite".to_string()));
    }

    #[test]
    fn parses_requirements_txt() {
        let body = "# comment\nflask==2.0\nrequests>=2.20\n-r other.txt\n";
        let deps = parse_requirements_txt(body);
        assert_eq!(deps, vec!["flask".to_string(), "requests".to_string()]);
    }

    #[test]
    fn parses_go_mod() {
        let body = "module x\nrequire github.com/foo/bar v1.0.0\nrequire (\n  github.com/baz/qux v0.1\n)\n";
        let deps = parse_go_mod(body);
        assert!(deps.contains(&"github.com/foo/bar".to_string()));
        assert!(deps.contains(&"github.com/baz/qux".to_string()));
    }

    #[test]
    fn stack_md_capped_at_60_lines() {
        let dir = tempdir().unwrap();
        let sub = dir.path().join("apps").join("x");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(
            sub.join("Cargo.toml"),
            "[dependencies]\nfoo = \"1\"\nbar = \"2\"\n",
        )
        .unwrap();
        let report = build_report(dir.path(), "apps/x");
        let md = report.to_markdown();
        assert!(md.lines().count() <= 60);
        assert!(md.contains("<!-- mustard:generated -->"));
    }
}

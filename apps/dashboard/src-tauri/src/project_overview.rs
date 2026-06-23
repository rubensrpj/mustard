//! Project-overview projection for the dashboard overview card.
//!
//! [`dashboard_project_overview`] reads the grain model
//! (`.claude/grain.model.json`) via [`mustard_core::read_projects`] — never
//! parsing the model's own schema directly — and projects the small,
//! card-ready shape: whether the workspace is a monorepo, how many compilation
//! units it has, and the distinct languages, frameworks, and detected stacks
//! mined across them.
//!
//! Each per-unit summary is further enriched with the dependencies AND THEIR
//! VERSIONS, read straight from the unit's MANIFEST (`package.json` /
//! `Cargo.toml` / `*.csproj`) — the grain model only carries dependency names,
//! not version ranges. The unit also reports the relative paths to its
//! `README.md` / `CLAUDE.md`, so the frontend can open them in the code viewer.
//!
//! [`dashboard_deps_outdated`] is an on-demand companion that shells out to the
//! ecosystem's own outdated tool (`npm outdated` / `dotnet list package
//! --outdated` / `cargo outdated`) and classifies each stale dependency by
//! semver severity.
//!
//! FAIL-OPEN CONTRACT (mirrors every dashboard command): a missing model (no
//! scan yet) yields an all-empty overview — `read_projects` already returns an
//! empty vec on a missing/malformed model — so the card shows an empty state
//! rather than an error toast. A missing/unreadable manifest yields empty
//! `deps`; a missing/failing outdated tool yields an empty outdated list. No
//! IO/spawn failure ever becomes an `Err`.

use mustard_core::read_projects;
use mustard_core::ProjectConfig;
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::process_util::no_window_command;

/// Soft cap on dependencies projected per unit — protects the card from a
/// pathological manifest without truncating any realistic project.
const MAX_DEPS_PER_PROJECT: usize = 300;

/// Wall-clock budget for one on-demand outdated probe. The network-bound tools
/// (`npm outdated`, `dotnet list package`) can hang on a flaky registry; this
/// keeps the command from blocking the UI thread indefinitely.
const OUTDATED_TIMEOUT: Duration = Duration::from_secs(60);

/// One inferred stack, flattened for the frontend (the model's
/// `StackDetection` carries auditable `signals` the card does not render).
#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct StackSummary {
    pub name: String,
    pub confidence: f32,
}

/// One declared dependency with the version range as written in the manifest
/// (e.g. `"^4.0.1"`, `"~2.0.0"`, `"1.2.3"`). The version is the raw manifest
/// string, never resolved — `version` is empty when the manifest omits it
/// (e.g. a `*.csproj` under Central Package Management).
#[derive(Serialize, Default, Clone)]
#[serde(rename_all = "snake_case")]
pub struct DepVersion {
    pub name: String,
    pub version: String,
}

/// Per-unit projection so the card can list each subproject of a monorepo
/// rather than only the workspace-wide aggregates. One per `projects[]` entry.
#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct ProjectUnitSummary {
    /// Unit name from the model.
    pub name: String,
    /// Unit directory relative to the repo root; falls back to `name` when the
    /// model carries no `dir`.
    pub dir: String,
    /// The unit's `kind` (e.g. `cargo`, `npm`, `go`) — the only per-unit
    /// language signal the model carries.
    pub language: String,
    /// Frameworks/deps mined for this unit (frequency-ranked by the model).
    pub frameworks: Vec<String>,
    /// Stacks inferred for this unit, flattened to name + confidence.
    pub stacks: Vec<StackSummary>,
    /// Dependencies WITH version ranges, read from the unit's manifest (not the
    /// grain model, which only has names). Sorted by name, deduped, capped at
    /// [`MAX_DEPS_PER_PROJECT`]. Empty when the manifest is absent/unreadable.
    pub deps: Vec<DepVersion>,
    /// Repo-relative path to the unit's `README.md` (also matches lowercase
    /// `readme.md`), or `None` when it has no readme. The frontend opens it in
    /// the code viewer via `dashboard_read_file`.
    pub readme_path: Option<String>,
    /// Repo-relative path to the unit's `CLAUDE.md`, or `None` when absent.
    pub claude_md_path: Option<String>,
}

/// Card-ready projection of the workspace's grain model.
#[derive(Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct ProjectOverview {
    /// The Mustard CLI `version` stamped into `<repo>/mustard.json` (same source
    /// `detect_project_mustard` surfaces). `None` when the file is missing,
    /// malformed, or has no `version` key — the card then omits the version.
    pub version: Option<String>,
    /// `true` when the model declares more than one compilation unit.
    pub is_monorepo: bool,
    /// Number of compilation units (subprojects) in the model.
    pub project_count: usize,
    /// Distinct languages across units, derived from each unit's `kind`
    /// (e.g. `cargo`, `npm`, `go`) — the only language signal the model
    /// carries per unit. Sorted, deduped.
    pub languages: Vec<String>,
    /// Distinct frameworks/deps mined across units. Sorted, deduped.
    pub frameworks: Vec<String>,
    /// Distinct inferred stacks across units, keeping the highest confidence
    /// seen for each stack name.
    pub detected_stacks: Vec<StackSummary>,
    /// One entry per compilation unit, so the card can render the monorepo's
    /// members instead of only the workspace-wide aggregates. Stably ordered
    /// by `dir` then `name`.
    pub units: Vec<ProjectUnitSummary>,
}

/// Project the grain model at `repo_path` into a [`ProjectOverview`]. Always
/// returns `Ok`; a missing/unscanned model degrades to an empty overview.
#[tauri::command]
pub fn dashboard_project_overview(repo_path: String) -> Result<ProjectOverview, String> {
    let base = PathBuf::from(&repo_path);
    let model = base.join(".claude").join("grain.model.json");
    let projects = read_projects(&model);

    // Mustard CLI version, read from the project-root `mustard.json` — the same
    // source `detect_project_mustard` surfaces for the sidebar. Best-effort: a
    // missing/malformed config yields `None` (`load` is fail-open).
    let version = ProjectConfig::load(&base).version;

    let project_count = projects.len();
    let mut languages: BTreeSet<String> = BTreeSet::new();
    let mut frameworks: BTreeSet<String> = BTreeSet::new();
    // Highest confidence wins per stack name.
    let mut stacks: std::collections::BTreeMap<String, f32> = std::collections::BTreeMap::new();
    let mut units: Vec<ProjectUnitSummary> = Vec::with_capacity(project_count);

    for project in &projects {
        if !project.kind.is_empty() {
            languages.insert(project.kind.clone());
        }
        for framework in &project.frameworks {
            frameworks.insert(framework.clone());
        }
        for stack in &project.detected_stacks {
            stacks
                .entry(stack.name.clone())
                .and_modify(|c| {
                    if stack.confidence > *c {
                        *c = stack.confidence;
                    }
                })
                .or_insert(stack.confidence);
        }

        // The model carries `dir` per unit, but older models / a root unit may
        // leave it empty — fall back to the name so the card always has a key.
        let dir = if project.dir.is_empty() {
            project.name.clone()
        } else {
            project.dir.clone()
        };
        // The unit's manifest lives at `<repo>/<dir>`. `dir` may be the
        // name-fallback above for a root unit, in which case the manifest is at
        // the repo root anyway (an empty/`.` dir joins to `base`).
        let unit_root = base.join(&dir);
        let deps = read_manifest_deps(&unit_root, &project.kind);
        let readme_path = find_relative_doc(&base, &dir, &["README.md", "readme.md"]);
        let claude_md_path = find_relative_doc(&base, &dir, &["CLAUDE.md"]);

        units.push(ProjectUnitSummary {
            name: project.name.clone(),
            dir,
            language: project.kind.clone(),
            frameworks: project.frameworks.clone(),
            stacks: project
                .detected_stacks
                .iter()
                .map(|s| StackSummary {
                    name: s.name.clone(),
                    confidence: s.confidence,
                })
                .collect(),
            deps,
            readme_path,
            claude_md_path,
        });
    }

    // Stable order: by directory, then name, so the card list never jitters
    // between scans.
    units.sort_by(|a, b| a.dir.cmp(&b.dir).then_with(|| a.name.cmp(&b.name)));

    Ok(ProjectOverview {
        version,
        is_monorepo: project_count > 1,
        project_count,
        languages: languages.into_iter().collect(),
        frameworks: frameworks.into_iter().collect(),
        detected_stacks: stacks
            .into_iter()
            .map(|(name, confidence)| StackSummary { name, confidence })
            .collect(),
        units,
    })
}

/// Return the repo-relative path of the first `candidates` filename that exists
/// directly under `<base>/<dir>`, or `None`. Paths use forward slashes so the
/// frontend can hand them straight to `dashboard_read_file`.
fn find_relative_doc(base: &Path, dir: &str, candidates: &[&str]) -> Option<String> {
    let unit_root = base.join(dir);
    for name in candidates {
        if unit_root.join(name).is_file() {
            // Build the repo-relative path from the (already repo-relative)
            // `dir` so we never leak the absolute base; normalise separators.
            let rel = if dir.is_empty() || dir == "." {
                (*name).to_string()
            } else {
                format!("{}/{}", dir.replace('\\', "/").trim_end_matches('/'), name)
            };
            return Some(rel);
        }
    }
    None
}

/// Read the unit's manifest at `unit_root` and return its declared dependencies
/// with version ranges, dispatched by `kind`. Fail-open: an absent/unreadable
/// manifest yields an empty vec. The result is sorted by name, deduped, and
/// capped at [`MAX_DEPS_PER_PROJECT`].
fn read_manifest_deps(unit_root: &Path, kind: &str) -> Vec<DepVersion> {
    let mut deps = match kind {
        "npm" => std::fs::read_to_string(unit_root.join("package.json"))
            .map(|s| parse_npm_deps(&s))
            .unwrap_or_default(),
        "cargo" => std::fs::read_to_string(unit_root.join("Cargo.toml"))
            .map(|s| parse_cargo_deps(&s))
            .unwrap_or_default(),
        "dotnet" => find_csproj(unit_root)
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|s| parse_csproj_deps(&s))
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    finalize_deps(&mut deps);
    deps
}

/// Sort by name (case-insensitive), dedup by name (first wins), and cap.
fn finalize_deps(deps: &mut Vec<DepVersion>) {
    deps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    deps.dedup_by(|a, b| a.name.eq_ignore_ascii_case(&b.name));
    deps.truncate(MAX_DEPS_PER_PROJECT);
}

/// Parse `dependencies` + `devDependencies` from a `package.json` string into
/// `{name, version}` pairs (version = the raw range string). Lenient: a
/// malformed document yields an empty vec; a non-string version becomes empty.
fn parse_npm_deps(content: &str) -> Vec<DepVersion> {
    let value: serde_json::Value = match serde_json::from_str(content) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for section in ["dependencies", "devDependencies"] {
        if let Some(map) = value.get(section).and_then(|v| v.as_object()) {
            for (name, ver) in map {
                out.push(DepVersion {
                    name: name.clone(),
                    version: ver.as_str().unwrap_or("").to_string(),
                });
            }
        }
    }
    out
}

/// Lightweight `Cargo.toml` parser for the `[dependencies]` and
/// `[dev-dependencies]` tables. No `toml` crate dependency — we scan line by
/// line, tracking the current table header. A dependency value is either a bare
/// string (`serde = "1.0"`) or an inline table (`serde = { version = "1.0", ...
/// }`); in both cases we extract the `version` string. Entries with no version
/// (path/git deps, or an inline table without `version`) get an empty version.
fn parse_cargo_deps(content: &str) -> Vec<DepVersion> {
    let mut out = Vec::new();
    let mut in_deps = false;
    for raw in content.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            // A new table header — only the dependency tables interest us.
            let header = line.trim_start_matches('[').trim_end_matches(']').trim();
            in_deps = matches!(header, "dependencies" | "dev-dependencies");
            continue;
        }
        if !in_deps || line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((name_part, value_part)) = line.split_once('=') else {
            continue;
        };
        let name = name_part.trim();
        if name.is_empty() {
            continue;
        }
        let value = value_part.trim();
        let version = if value.starts_with('{') {
            // Inline table: pull `version = "x"` out of it, else empty.
            extract_inline_version(value)
        } else {
            // Bare string value: strip surrounding quotes.
            unquote(value)
        };
        out.push(DepVersion {
            name: name.to_string(),
            version,
        });
    }
    out
}

/// Extract the `version = "x"` field from a Cargo inline-table value
/// (`{ version = "1.0", features = [...] }`). Returns "" when absent.
fn extract_inline_version(inline: &str) -> String {
    let Some(idx) = inline.find("version") else {
        return String::new();
    };
    let after = &inline[idx + "version".len()..];
    let after = after.trim_start();
    let Some(after) = after.strip_prefix('=') else {
        return String::new();
    };
    // Take up to the next comma or closing brace, then unquote.
    let token: String = after
        .trim_start()
        .chars()
        .take_while(|&c| c != ',' && c != '}')
        .collect();
    unquote(token.trim())
}

/// Strip a single pair of surrounding single/double quotes from a token.
fn unquote(s: &str) -> String {
    let s = s.trim();
    let bytes = s.as_bytes();
    if bytes.len() >= 2
        && ((bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\''))
    {
        return s[1..s.len() - 1].to_string();
    }
    s.to_string()
}

/// Find the first `*.csproj` directly under `dir` (non-recursive — a .NET
/// project keeps its csproj at its own root). Returns `None` when none exists.
fn find_csproj(dir: &Path) -> Option<PathBuf> {
    let rd = std::fs::read_dir(dir).ok()?;
    let mut found: Vec<PathBuf> = rd
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("csproj"))
        })
        .collect();
    found.sort();
    found.into_iter().next()
}

/// Parse `<PackageReference Include="X" Version="Y" />` lines out of a `.csproj`
/// (line/attribute scan — no full XML parse). `Version` may be absent when the
/// project uses Central Package Management (`Directory.Packages.props`); those
/// references get an empty version. The attributes can appear in either order.
fn parse_csproj_deps(content: &str) -> Vec<DepVersion> {
    let mut out = Vec::new();
    for line in content.lines() {
        if !line.contains("PackageReference") {
            continue;
        }
        let Some(name) = xml_attr(line, "Include") else {
            continue;
        };
        let version = xml_attr(line, "Version").unwrap_or_default();
        out.push(DepVersion { name, version });
    }
    out
}

/// Pull the value of an XML attribute (`Attr="value"`) out of a single line.
/// Case-sensitive on the attribute name (matches MSBuild conventions); returns
/// `None` when the attribute is absent or unterminated.
fn xml_attr(line: &str, attr: &str) -> Option<String> {
    let needle = format!("{attr}=\"");
    let start = line.find(&needle)? + needle.len();
    let rest = &line[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

// ── On-demand outdated check ─────────────────────────────────────────────────

/// One stale dependency, with the installed/declared version, the latest
/// available version, and a semver-derived severity.
#[derive(Serialize, Default, Clone)]
#[serde(rename_all = "snake_case")]
pub struct OutdatedDep {
    pub name: String,
    pub current: String,
    pub latest: String,
    /// `"major" | "minor" | "patch" | "up-to-date"`, from comparing
    /// `current` × `latest` by semver.
    pub severity: String,
}

/// On-demand outdated check for one unit. Runs the ecosystem's own tool inside
/// `<repo_path>/<project_dir>` and classifies each stale dependency by semver
/// severity.
///
/// STRONGLY FAIL-OPEN: a missing tool, no network, a timeout, a non-zero exit
/// with no parseable body, or unexpected JSON all degrade to `Ok(vec![])` —
/// NEVER an `Err`. The 60s timeout keeps a flaky registry from hanging the UI.
#[tauri::command]
pub async fn dashboard_deps_outdated(
    repo_path: String,
    project_dir: String,
    kind: String,
) -> Result<Vec<OutdatedDep>, String> {
    let result = tauri::async_runtime::spawn_blocking(move || {
        outdated_impl(&repo_path, &project_dir, &kind)
    })
    .await
    .unwrap_or_default();
    Ok(result)
}

/// Synchronous body of [`dashboard_deps_outdated`], kept separate so the spawn
/// wrapper stays thin. Returns an empty vec on any failure path.
fn outdated_impl(repo_path: &str, project_dir: &str, kind: &str) -> Vec<OutdatedDep> {
    let cwd = PathBuf::from(repo_path).join(project_dir);
    if !cwd.is_dir() {
        return Vec::new();
    }
    match kind {
        "npm" => js_outdated(&cwd),
        "dotnet" => dotnet_outdated(&cwd),
        "cargo" => cargo_outdated(&cwd),
        _ => Vec::new(),
    }
}

/// Detect the JS package manager for `cwd` by walking up to the filesystem root
/// for a lockfile: `pnpm-lock.yaml` → pnpm, `yarn.lock` → yarn,
/// `package-lock.json` → npm. Defaults to npm when none is found. A monorepo
/// keeps the lock at its root, so the walk-up is what lets a nested workspace
/// package (e.g. `apps/sialia-admin`) resolve to the repo's real PM instead of
/// being probed with the wrong tool.
fn detect_js_pm(cwd: &Path) -> &'static str {
    let mut dir = Some(cwd);
    while let Some(d) = dir {
        if d.join("pnpm-lock.yaml").is_file() {
            return "pnpm";
        }
        if d.join("yarn.lock").is_file() {
            return "yarn";
        }
        if d.join("package-lock.json").is_file() {
            return "npm";
        }
        dir = d.parent();
    }
    "npm"
}

/// Run the workspace's JS package manager `outdated` and parse its
/// `{pkg: {current, latest, wanted}}` map. The PM is detected from the nearest
/// lockfile ([`detect_js_pm`]) so a pnpm/yarn monorepo isn't probed with `npm`
/// (which yields nothing there → the "couldn't check" note). `npm` and `pnpm`
/// share the JSON shape; pnpm needs `--format json`. The tool exits non-zero
/// when packages are stale — that is normal; we parse stdout regardless.
fn js_outdated(cwd: &Path) -> Vec<OutdatedDep> {
    let pm = detect_js_pm(cwd);
    let args: &[&str] = match pm {
        "pnpm" => &["outdated", "--format", "json"],
        // npm and yarn-classic both accept `--json` and emit the documented
        // `{pkg:{current,latest}}` map; yarn-berry differs and just yields
        // nothing parseable (fail-open empty).
        _ => &["outdated", "--json"],
    };
    match run_js_capture(cwd, pm, args) {
        Some(stdout) => parse_outdated_json(&stdout),
        None => Vec::new(),
    }
}

/// Parse the `{pkg: {current, latest}}` JSON emitted by both `npm outdated
/// --json` and `pnpm outdated --format json` into `OutdatedDep`s. Empty or
/// malformed stdout → empty (fail-open).
fn parse_outdated_json(stdout: &str) -> Vec<OutdatedDep> {
    if stdout.trim().is_empty() {
        return Vec::new();
    }
    let value: serde_json::Value = match serde_json::from_str(stdout) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let Some(map) = value.as_object() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (name, info) in map {
        let current = info
            .get("current")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let latest = info
            .get("latest")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let severity = classify_severity(&current, &latest);
        out.push(OutdatedDep {
            name: name.clone(),
            current,
            latest,
            severity,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Run `dotnet list <csproj> package --outdated --format json`. The JSON shape
/// is `{ projects: [{ frameworks: [{ topLevelPackages: [{ id,
/// resolvedVersion, latestVersion }] }] }] }`. Older `dotnet` builds reject
/// `--format json` (printed to stderr, no JSON body) → fail-open empty.
fn dotnet_outdated(cwd: &Path) -> Vec<OutdatedDep> {
    let Some(csproj) = find_csproj(cwd) else {
        return Vec::new();
    };
    let csproj_str = csproj.to_string_lossy().into_owned();
    let stdout = match run_capture(
        cwd,
        "dotnet",
        &[
            "list",
            &csproj_str,
            "package",
            "--outdated",
            "--format",
            "json",
        ],
    ) {
        Some(s) => s,
        None => return Vec::new(),
    };
    let value: serde_json::Value = match serde_json::from_str(&stdout) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    let projects = value.get("projects").and_then(|v| v.as_array());
    for project in projects.into_iter().flatten() {
        let frameworks = project.get("frameworks").and_then(|v| v.as_array());
        for fw in frameworks.into_iter().flatten() {
            let pkgs = fw.get("topLevelPackages").and_then(|v| v.as_array());
            for pkg in pkgs.into_iter().flatten() {
                let name = pkg.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if name.is_empty() {
                    continue;
                }
                let current = pkg
                    .get("resolvedVersion")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let latest = pkg
                    .get("latestVersion")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let severity = classify_severity(&current, &latest);
                out.push(OutdatedDep {
                    name,
                    current,
                    latest,
                    severity,
                });
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Run `cargo outdated --format json` IF the subcommand is installed (it is not
/// a built-in). A missing subcommand surfaces as a spawn/exit failure with no
/// JSON body → fail-open empty. The shape is
/// `{ dependencies: [{ name, project, latest }] }`.
fn cargo_outdated(cwd: &Path) -> Vec<OutdatedDep> {
    let stdout = match run_capture(cwd, "cargo", &["outdated", "--format", "json"]) {
        Some(s) => s,
        None => return Vec::new(),
    };
    let value: serde_json::Value = match serde_json::from_str(&stdout) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let deps = value.get("dependencies").and_then(|v| v.as_array());
    let mut out = Vec::new();
    for dep in deps.into_iter().flatten() {
        let name = dep.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if name.is_empty() {
            continue;
        }
        // `cargo outdated` prints the in-use version under `project` and the
        // newest under `latest`; either can be the literal "---" when N/A.
        let current = dep
            .get("project")
            .and_then(|v| v.as_str())
            .filter(|s| *s != "---")
            .unwrap_or("")
            .to_string();
        let latest = dep
            .get("latest")
            .and_then(|v| v.as_str())
            .filter(|s| *s != "---")
            .unwrap_or("")
            .to_string();
        let severity = classify_severity(&current, &latest);
        out.push(OutdatedDep {
            name,
            current,
            latest,
            severity,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Spawn `program args...` in `cwd` via [`no_window_command`] with a timeout,
/// returning its stdout (lossily decoded) regardless of exit code, or `None` on
/// spawn failure / timeout. The exit code is intentionally ignored: `npm
/// outdated` exits non-zero precisely when it has results to report.
fn capture_child(mut command: std::process::Command) -> Option<String> {
    use std::process::Stdio;

    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;

    // Bounded wait so a hung registry call cannot freeze the command. We poll
    // `try_wait` rather than blocking forever; on timeout we kill the child.
    let deadline = std::time::Instant::now() + OUTDATED_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => break,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return None,
        }
    }
    let output = child.wait_with_output().ok()?;
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn run_capture(cwd: &Path, program: &str, args: &[&str]) -> Option<String> {
    let mut cmd = no_window_command(program);
    cmd.args(args).current_dir(cwd);
    capture_child(cmd)
}

/// Spawn a JS package-manager command (`pnpm`/`npm`/`yarn`). On Windows these
/// are `.cmd` shims that `Command::new(name)` cannot resolve — Rust does not
/// search `PATHEXT` — so a bare `npm` spawn silently fails (returns an empty
/// outdated list → the "couldn't check" note) while `dotnet`/`cargo` (real
/// `.exe`s) work. We therefore invoke the PM through `cmd /C` on Windows; on
/// Unix the binary is spawned directly.
fn run_js_capture(cwd: &Path, pm: &str, args: &[&str]) -> Option<String> {
    let mut cmd = if cfg!(windows) {
        let mut c = no_window_command("cmd");
        c.arg("/C").arg(pm).args(args);
        c
    } else {
        let mut c = no_window_command(pm);
        c.args(args);
        c
    };
    cmd.current_dir(cwd);
    capture_child(cmd)
}

/// Classify the jump from `current` to `latest` by semantic version. Returns
/// `"up-to-date"` when they are equal (or either is unparseable/empty — we do
/// not fabricate a severity for versions we cannot compare). Otherwise the
/// first differing component decides: major → `"major"`, minor → `"minor"`,
/// patch (or anything below) → `"patch"`.
fn classify_severity(current: &str, latest: &str) -> String {
    let (Some(c), Some(l)) = (parse_semver(current), parse_semver(latest)) else {
        return "up-to-date".to_string();
    };
    if l.0 != c.0 {
        "major"
    } else if l.1 != c.1 {
        "minor"
    } else if l.2 != c.2 {
        "patch"
    } else {
        "up-to-date"
    }
    .to_string()
}

/// Parse a `major.minor.patch` triple, tolerating a leading `v`/range operator
/// and a `-prerelease`/`+build` suffix. Missing minor/patch default to 0.
/// Returns `None` when the major component is not a number.
fn parse_semver(v: &str) -> Option<(u64, u64, u64)> {
    let v = v.trim().trim_start_matches(['v', 'V', '^', '~', '=', '>', '<', ' ']);
    // Drop a prerelease/build suffix before splitting on dots.
    let core = v.split(['-', '+']).next().unwrap_or(v);
    let mut parts = core.split('.');
    let major = parts.next()?.trim().parse::<u64>().ok()?;
    let minor = parts.next().and_then(|s| s.trim().parse::<u64>().ok()).unwrap_or(0);
    let patch = parts.next().and_then(|s| s.trim().parse::<u64>().ok()).unwrap_or(0);
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_model_yields_empty_overview() {
        let dir = tempfile::tempdir().unwrap();
        let overview =
            dashboard_project_overview(dir.path().to_string_lossy().into_owned()).unwrap();
        assert!(!overview.is_monorepo);
        assert_eq!(overview.project_count, 0);
        assert!(overview.languages.is_empty());
        assert!(overview.frameworks.is_empty());
        assert!(overview.detected_stacks.is_empty());
        assert!(overview.units.is_empty());
    }

    #[test]
    fn npm_deps_parse_runtime_and_dev() {
        let content = r#"{
            "name": "demo",
            "dependencies": { "a": "^1.0.0" },
            "devDependencies": { "b": "~2.0.0" }
        }"#;
        let mut deps = parse_npm_deps(content);
        finalize_deps(&mut deps);
        assert_eq!(deps.len(), 2);
        let a = deps.iter().find(|d| d.name == "a").unwrap();
        assert_eq!(a.version, "^1.0.0");
        let b = deps.iter().find(|d| d.name == "b").unwrap();
        assert_eq!(b.version, "~2.0.0");
    }

    #[test]
    fn npm_deps_malformed_is_empty() {
        assert!(parse_npm_deps("not json").is_empty());
    }

    #[test]
    fn cargo_deps_string_and_inline_table() {
        let content = r#"
[package]
name = "x"

[dependencies]
serde = "1.0.190"
tokio = { version = "1.35", features = ["full"] }
local = { path = "../local" }

[dev-dependencies]
tempfile = "3"
"#;
        let mut deps = parse_cargo_deps(content);
        finalize_deps(&mut deps);
        let serde = deps.iter().find(|d| d.name == "serde").unwrap();
        assert_eq!(serde.version, "1.0.190");
        let tokio = deps.iter().find(|d| d.name == "tokio").unwrap();
        assert_eq!(tokio.version, "1.35");
        // Path dep has no version → empty string, still listed.
        let local = deps.iter().find(|d| d.name == "local").unwrap();
        assert_eq!(local.version, "");
        let tempfile = deps.iter().find(|d| d.name == "tempfile").unwrap();
        assert_eq!(tempfile.version, "3");
        // `[package]` keys must not leak in as deps.
        assert!(deps.iter().all(|d| d.name != "name"));
    }

    #[test]
    fn csproj_package_references() {
        let content = r#"<Project Sdk="Microsoft.NET.Sdk">
  <ItemGroup>
    <PackageReference Include="Newtonsoft.Json" Version="13.0.3" />
    <PackageReference Version="6.0.0" Include="Serilog" />
    <PackageReference Include="NoVersionPkg" />
  </ItemGroup>
</Project>"#;
        let mut deps = parse_csproj_deps(content);
        finalize_deps(&mut deps);
        let nj = deps.iter().find(|d| d.name == "Newtonsoft.Json").unwrap();
        assert_eq!(nj.version, "13.0.3");
        // Attributes can appear in either order.
        let serilog = deps.iter().find(|d| d.name == "Serilog").unwrap();
        assert_eq!(serilog.version, "6.0.0");
        // CPM/no-version reference → empty version, still listed.
        let nv = deps.iter().find(|d| d.name == "NoVersionPkg").unwrap();
        assert_eq!(nv.version, "");
    }

    #[test]
    fn read_manifest_deps_reads_package_json() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"dependencies":{"a":"^1.0.0"},"devDependencies":{"b":"~2.0.0"}}"#,
        )
        .unwrap();
        let deps = read_manifest_deps(dir.path(), "npm");
        assert_eq!(deps.len(), 2);
        assert!(deps.iter().any(|d| d.name == "a" && d.version == "^1.0.0"));
        assert!(deps.iter().any(|d| d.name == "b" && d.version == "~2.0.0"));
    }

    #[test]
    fn read_manifest_deps_missing_manifest_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_manifest_deps(dir.path(), "npm").is_empty());
        assert!(read_manifest_deps(dir.path(), "cargo").is_empty());
        assert!(read_manifest_deps(dir.path(), "dotnet").is_empty());
        assert!(read_manifest_deps(dir.path(), "unknown").is_empty());
    }

    #[test]
    fn find_relative_doc_detects_readme_and_claude() {
        let base = tempfile::tempdir().unwrap();
        let unit = base.path().join("apps").join("web");
        std::fs::create_dir_all(&unit).unwrap();
        std::fs::write(unit.join("README.md"), "# readme").unwrap();
        std::fs::write(unit.join("CLAUDE.md"), "# guards").unwrap();

        let readme = find_relative_doc(base.path(), "apps/web", &["README.md", "readme.md"]);
        assert_eq!(readme.as_deref(), Some("apps/web/README.md"));
        let claude = find_relative_doc(base.path(), "apps/web", &["CLAUDE.md"]);
        assert_eq!(claude.as_deref(), Some("apps/web/CLAUDE.md"));
        // Absent doc → None.
        assert!(find_relative_doc(base.path(), "apps/web", &["MISSING.md"]).is_none());
    }

    #[test]
    fn find_relative_doc_lowercase_readme() {
        let base = tempfile::tempdir().unwrap();
        let unit = base.path().join("pkg");
        std::fs::create_dir_all(&unit).unwrap();
        std::fs::write(unit.join("readme.md"), "# lower").unwrap();
        let readme = find_relative_doc(base.path(), "pkg", &["README.md", "readme.md"]);
        // A lowercase `readme.md` is detected. On a case-insensitive FS
        // (Windows/macOS) the first candidate `README.md` matches the same
        // file, so the returned name may carry either casing — both resolve
        // through `dashboard_read_file` on those platforms. Assert detection +
        // directory, not the exact letter-case.
        let rel = readme.expect("readme should be detected");
        assert!(rel.eq_ignore_ascii_case("pkg/readme.md"), "got {rel}");
    }

    #[test]
    fn severity_classification_by_semver() {
        assert_eq!(classify_severity("1.0.0", "2.0.0"), "major");
        assert_eq!(classify_severity("1.0.0", "1.1.0"), "minor");
        assert_eq!(classify_severity("1.0.0", "1.0.1"), "patch");
        assert_eq!(classify_severity("1.2.3", "1.2.3"), "up-to-date");
        // Tolerates range operators / v-prefix.
        assert_eq!(classify_severity("^1.0.0", "v2.0.0"), "major");
        // Unparseable → up-to-date (never fabricate a severity).
        assert_eq!(classify_severity("", "1.0.0"), "up-to-date");
        assert_eq!(classify_severity("garbage", "1.0.0"), "up-to-date");
    }

    #[test]
    fn outdated_impl_fail_open() {
        let dir = tempfile::tempdir().unwrap();
        // Unknown kind → empty.
        assert!(outdated_impl(&dir.path().to_string_lossy(), "", "weird").is_empty());
        // Missing project dir → empty.
        assert!(outdated_impl(&dir.path().to_string_lossy(), "nope", "npm").is_empty());
    }

    #[test]
    fn detect_js_pm_walks_up_to_the_repo_lockfile() {
        let dir = tempfile::tempdir().unwrap();
        // pnpm-lock at the repo root; the unit lives two levels down.
        std::fs::write(dir.path().join("pnpm-lock.yaml"), "").unwrap();
        let nested = dir.path().join("apps").join("admin");
        std::fs::create_dir_all(&nested).unwrap();
        assert_eq!(detect_js_pm(&nested), "pnpm");
    }

    #[test]
    fn detect_js_pm_prefers_pnpm_then_yarn_then_npm_and_defaults_npm() {
        let none = tempfile::tempdir().unwrap();
        assert_eq!(detect_js_pm(none.path()), "npm");

        let yarn = tempfile::tempdir().unwrap();
        std::fs::write(yarn.path().join("yarn.lock"), "").unwrap();
        assert_eq!(detect_js_pm(yarn.path()), "yarn");

        let npm = tempfile::tempdir().unwrap();
        std::fs::write(npm.path().join("package-lock.json"), "{}").unwrap();
        assert_eq!(detect_js_pm(npm.path()), "npm");
    }

    #[test]
    fn parse_outdated_json_reads_the_shared_npm_pnpm_shape() {
        // The exact shape `pnpm outdated --format json` and `npm outdated --json`
        // both emit (verified against a real pnpm workspace).
        let json = r#"{"@next/bundle-analyzer":{"current":"16.2.4","latest":"16.2.9","wanted":"16.2.4"}}"#;
        let out = parse_outdated_json(json);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "@next/bundle-analyzer");
        assert_eq!(out[0].current, "16.2.4");
        assert_eq!(out[0].latest, "16.2.9");
        assert_eq!(out[0].severity, "patch"); // 16.2.4 → 16.2.9 bumps only patch
        // Empty / malformed → empty, never a panic.
        assert!(parse_outdated_json("").is_empty());
        assert!(parse_outdated_json("not json").is_empty());
    }
}

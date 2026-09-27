//! `grain` — typed client for the external grain tool.
//!
//! grain is the deterministic codebase miner (it replaces Mustard's old scan
//! engine entirely). Mustard never reads project source to understand a repo;
//! it shells out to the grain binary and consumes its JSON/Markdown:
//!
//! - `grain scan <root> --out <model.json>` — the durable model (run once/repo).
//! - `grain facts <model>` — the subproject list and the known declaration
//!   names, so Mustard never parses the model's own schema.
//!
//! The boundary is a TOOL (process + JSON/MD), not a library link: no shared
//! build, no tree-sitter version coupling, grain stays standalone. This module
//! is the single owner of that boundary. Nothing here is language- or
//! framework-specific — grain is itself fully data-driven.
//!
//! Fail-open: spawning or parsing failures return [`Error`]; callers degrade
//! (e.g. an empty subproject list when the tool is missing).

use std::path::Path;
use std::process::Command;

use serde::Deserialize;

use crate::domain::vocabulary::stacks::StackDetection;
use crate::platform::error::{Error, Result};

/// Default tool name — resolved on `PATH`. A project can point at a pinned
/// binary later (e.g. via `mustard.json`); the locator is injected, never
/// hardcoded at a call site.
pub(crate) const DEFAULT_BINARY: &str = "scan";

/// A handle to the grain tool at a known location.
#[derive(Debug, Clone)]
pub struct Scan {
    binary: String,
}

impl Default for Scan {
    fn default() -> Self {
        Self { binary: DEFAULT_BINARY.to_string() }
    }
}

/// One compilation unit from grain's model (`grain.model.json` `projects[]`) —
/// the subproject list. Replaces the deleted sync-detect discovery: grain mines
/// the same build-manifest set deterministically.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Project {
    pub name: String,
    #[serde(default)]
    pub dir: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub code_files: usize,
    /// Frameworks/deps recurring across this unit's manifests (mined by `scan`,
    /// frequency-ranked, top-12). Empty when none mined / older model.
    #[serde(default)]
    pub frameworks: Vec<String>,
    /// Distinct dependencies declared by this unit's manifests (sorted, deduped).
    #[serde(default)]
    pub dependencies: Vec<String>,
    /// Build/codegen scripts declared by this unit's manifests (sorted, deduped).
    #[serde(default)]
    pub scripts: Vec<String>,
    /// Stacks inferred for this unit (registry-driven, see
    /// `domain::vocabulary::stacks`). Additive next to [`Self::frameworks`]
    /// (which stays the raw frequency-ranked dep list); empty when the model
    /// predates the field or nothing was inferred.
    #[serde(default)]
    pub detected_stacks: Vec<StackDetection>,
    /// `true` when this subproject's own directory is a NESTED git repository
    /// root (a submodule / linked repo: `.git` present as a directory OR a
    /// pointer file at [`Self::dir`]). The grain miner is git-blind, so this is
    /// stamped by Mustard ([`mark_own_git_roots`]) — never mined. Defaulted
    /// `false` for back-compat with any census that predates the field and for
    /// the superproject root itself (which is not a nested boundary).
    #[serde(default)]
    pub own_git_root: bool,
}

/// The small, stable FACTS the orchestrator consumes from a grain model — the
/// subproject list and the known declaration names. Produced by `scan facts`;
/// Mustard deserializes this tiny shape but never the model's own (large)
/// schema, so the scan tool stays the single owner of the model format.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ModelFacts {
    #[serde(default)]
    pub projects: Vec<Project>,
    #[serde(default)]
    pub entities: Vec<String>,
}

/// Read the `projects[]` (subproject list) from a grain model — via the scan
/// tool's `facts` command ([`Scan::facts`]), so this crate never parses the
/// model's own schema. Fail-open: a missing model (no scan yet) or any
/// spawn/parse error yields an empty list.
#[must_use]
pub fn read_projects(model_path: &std::path::Path) -> Vec<Project> {
    if !crate::io::project_map::exists_at(model_path) {
        return Vec::new();
    }
    Scan::locate().facts(model_path).map(|f| f.projects).unwrap_or_default()
}

/// Read the distinct declaration names (entities / types / functions) from a
/// grain model — the "known entities" set — via the scan tool's `facts` command.
/// Sorted + deduped by the tool. Fail-open: empty on a missing model or any
/// spawn/parse error.
#[must_use]
pub fn read_entity_names(model_path: &std::path::Path) -> Vec<String> {
    if !crate::io::project_map::exists_at(model_path) {
        return Vec::new();
    }
    Scan::locate().facts(model_path).map(|f| f.entities).unwrap_or_default()
}

/// Stamp [`Project::own_git_root`] on each census entry by probing whether its
/// directory is a NESTED git repository root (`.git` dir or pointer file),
/// relative to `repo_root`. The grain miner is git-blind (it walks source
/// only), so the git-boundary FACT — "this subproject is its own repo" — is
/// Mustard's to add; the single owner of that probe is
/// [`crate::io::workspace::is_git_repo_root`] (reused, not re-implemented).
///
/// The superproject root itself is never a nested boundary: an empty or `"."`
/// `dir` is skipped (left `false`) so the root project — whose `.git` is the
/// SUPERproject's — is not mistaken for a submodule. Purely a filesystem probe,
/// fail-open: an unreadable / absent dir stays `false`.
pub fn mark_own_git_roots(repo_root: &Path, projects: &mut [Project]) {
    for project in projects.iter_mut() {
        let dir = project.dir.trim();
        if dir.is_empty() || dir == "." {
            continue;
        }
        project.own_git_root = crate::io::workspace::is_git_repo_root(&repo_root.join(dir));
    }
}

impl Scan {
    /// A client for the grain binary at `binary` (a name on `PATH` or a path).
    #[must_use]
    pub fn new(binary: impl Into<String>) -> Self {
        Self { binary: binary.into() }
    }

    /// Locate the bundled grain binary — built as a sibling of the running
    /// executable in the same workspace `target/` dir — falling back to
    /// [`DEFAULT_BINARY`] on `PATH`. Fail-open: any probe error → the fallback.
    #[must_use]
    pub fn locate() -> Self {
        let sibling = std::env::current_exe().ok().and_then(|exe| {
            let dir = exe.parent()?;
            let cand = dir.join(if cfg!(windows) { "scan.exe" } else { "scan" });
            cand.is_file().then(|| cand.to_string_lossy().into_owned())
        });
        Self { binary: sibling.unwrap_or_else(|| DEFAULT_BINARY.to_string()) }
    }

    /// Mine `root` into the model file at `out` (`grain scan`). With a model
    /// of the same project already at `out`, the tool reads only what changed
    /// since; the report says which files it read.
    ///
    /// # Errors
    /// [`Error::Io`] if the tool cannot be spawned, [`Error::CheckFailed`] on a
    /// non-zero exit or a report that does not parse.
    pub fn scan(&self, root: &Path, out: &Path) -> Result<ScanReport> {
        parse_scan_report(&self.run(&scan_args(root, out))?)
    }

    /// Read the model's FACTS (subproject list + known declaration names) via
    /// `scan facts <model>` — so Mustard never parses the model's own schema.
    ///
    /// # Errors
    /// [`Error::Io`] / [`Error::CheckFailed`] on spawn/exit failure,
    /// [`Error::Parse`] if the output is not the expected JSON.
    pub fn facts(&self, model: &Path) -> Result<ModelFacts> {
        let out = self.run(&facts_args(model))?;
        Ok(serde_json::from_str(&out)?)
    }

    /// Run grain with `args`, returning stdout. Maps a non-zero exit (with
    /// stderr) to [`Error::CheckFailed`].
    fn run(&self, args: &[String]) -> Result<String> {
        let output = Command::new(&self.binary).args(args).output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::check_failed(format!("scan {}: {}", args.join(" "), stderr.trim())));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

// --- pure arg builders (unit-testable without the binary present) -----------

fn scan_args(root: &Path, out: &Path) -> Vec<String> {
    vec![
        "scan".to_string(),
        root.to_string_lossy().into_owned(),
        "--out".to_string(),
        out.to_string_lossy().into_owned(),
        "--json".to_string(),
    ]
}

/// What one scan pass reports on its last stdout line: whether it read every
/// file, the files it read, how many code files the map has, the commit it
/// read from, and whether it rewrote the dictionary.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ScanReport {
    pub full: bool,
    pub read: Vec<String>,
    pub files: usize,
    pub head: String,
    pub dictionary: bool,
}

/// The report a `scan --json` run printed: its last non-empty line.
fn parse_scan_report(stdout: &str) -> Result<ScanReport> {
    let line = stdout.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("{}");
    serde_json::from_str(line).map_err(|e| Error::check_failed(format!("scan report: {e}")))
}

fn facts_args(model: &Path) -> Vec<String> {
    vec!["facts".to_string(), model.to_string_lossy().into_owned()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn scan_args_shape() {
        let a = scan_args(&PathBuf::from("repo"), &PathBuf::from("m.json"));
        assert_eq!(a, vec!["scan", "repo", "--out", "m.json", "--json"]);
    }

    #[test]
    fn the_scan_report_is_the_last_line() {
        let report = parse_scan_report("noise\n{\"ok\":true,\"full\":false,\"read\":[\"src/b.rs\"],\"files\":3}\n\n")
            .expect("report");
        assert!(!report.full);
        assert_eq!(report.read, vec!["src/b.rs".to_string()]);
        assert_eq!(report.files, 3);
        assert!(parse_scan_report("not json").is_err());
    }

    #[test]
    fn facts_args_shape() {
        let a = facts_args(&PathBuf::from("m.json"));
        assert_eq!(a, vec!["facts", "m.json"]);
    }

    #[test]
    fn model_facts_deserializes_scan_output() {
        let json = r#"{"projects":[{"name":"api","dir":"apps/api","kind":"node","code_files":3}],"entities":["Invoice","User"]}"#;
        let f: ModelFacts = serde_json::from_str(json).expect("valid scan facts json");
        assert_eq!(f.projects.len(), 1);
        assert_eq!(f.projects[0].name, "api");
        assert_eq!(f.entities, vec!["Invoice", "User"]);
    }

    #[test]
    fn detected_stacks_serde_compat() {
        // An old payload without `detected_stacks` still deserialises, and
        // `frameworks` is untouched by the new field.
        let old = r#"{"name":"api","dir":"apps/api","kind":"node","code_files":3,"frameworks":["express"]}"#;
        let p: Project = serde_json::from_str(old).expect("old payload without detected_stacks");
        assert!(p.detected_stacks.is_empty());
        assert_eq!(p.frameworks, vec!["express"]);

        // A new payload carrying the field round-trips into the contract type.
        let new = r#"{"name":"web","frameworks":["laravel/framework"],"detected_stacks":[{"name":"laravel","confidence":0.9,"signals":["dep:laravel/framework"]}]}"#;
        let p: Project = serde_json::from_str(new).expect("payload with detected_stacks");
        assert_eq!(p.detected_stacks.len(), 1);
        assert_eq!(p.detected_stacks[0].name, "laravel");
        assert_eq!(p.detected_stacks[0].signals, vec!["dep:laravel/framework"]);
        assert_eq!(p.frameworks, vec!["laravel/framework"]);
    }

    #[test]
    fn model_facts_defaults_missing_fields() {
        let f: ModelFacts = serde_json::from_str("{}").expect("empty object ok");
        assert!(f.projects.is_empty());
        assert!(f.entities.is_empty());
    }

    #[test]
    fn own_git_root_serde_defaults_false_and_roundtrips() {
        // A census that predates the field (grain never mines it) deserialises
        // with `own_git_root == false` — the git boundary is Mustard's to stamp.
        let old = r#"{"name":"api","dir":"apps/api","kind":"node"}"#;
        let p: Project = serde_json::from_str(old).expect("old payload without own_git_root");
        assert!(!p.own_git_root, "absent field defaults false");

        // A payload carrying the flag round-trips into the contract type.
        let new = r#"{"name":"sub","dir":"backend/Sub","own_git_root":true}"#;
        let p: Project = serde_json::from_str(new).expect("payload with own_git_root");
        assert!(p.own_git_root);
    }

    #[test]
    fn mark_own_git_roots_flags_dir_with_dot_git_file() {
        use tempfile::tempdir;
        let root = tempdir().unwrap();
        // A submodule carries `.git` as a FILE (a `gitdir:` pointer), NOT a dir —
        // the exact sialia shape. It must be flagged its own git root.
        let sub = root.path().join("backend").join("Sialia.Backend");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join(".git"), b"gitdir: ../../.git/modules/Sialia.Backend\n").unwrap();
        // A plain subproject (no `.git`) stays false.
        std::fs::create_dir_all(root.path().join("apps").join("api")).unwrap();

        let mut projects = vec![
            Project { name: "backend".into(), dir: "backend/Sialia.Backend".into(), ..Default::default() },
            Project { name: "api".into(), dir: "apps/api".into(), ..Default::default() },
            // The superproject root itself (empty / ".") is never a nested boundary.
            Project { name: "root".into(), dir: ".".into(), ..Default::default() },
        ];
        mark_own_git_roots(root.path(), &mut projects);
        assert!(projects[0].own_git_root, "a `.git` FILE marks a nested git root");
        assert!(!projects[1].own_git_root, "a plain subproject is not a nested git root");
        assert!(!projects[2].own_git_root, "the superproject root `.` is never flagged");
    }
}

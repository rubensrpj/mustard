//! `grain` — typed client for the external grain tool.
//!
//! grain is the deterministic codebase miner (it replaces Mustard's old scan
//! engine entirely). Mustard never reads project source to understand a repo;
//! it shells out to the grain binary and consumes its JSON/Markdown:
//!
//! - `grain scan <root> --out <model.json>` — the durable model (run once/repo).
//!
//! What the map holds is read back through the map port
//! (`io/project_map.rs`), never through another run of the tool.
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

/// One compilation unit from the map (the `projects` table of `.claude/grain.db`) —
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

/// Os subprojetos do mapa em `model_path`, lidos só da tabela deles pela
/// porta do mapa ([`crate::io::project_map::projects_at`]), sem abrir outro
/// processo nem ler o resto do mapa. Falha aberta: sem mapa (o scan ainda não
/// rodou) ou com um que não se entende, a lista vem vazia.
#[must_use]
pub fn read_projects(model_path: &std::path::Path) -> Vec<Project> {
    crate::io::project_map::projects_at(model_path).unwrap_or_default()
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
        Self::located_from(std::env::current_exe().ok().as_deref())
    }

    /// [`Self::locate`] for the executable at `exe`. A test binary runs from
    /// `deps/`, one folder below the programs of the same build: there the
    /// folder above is searched too, before `PATH`, so a test never runs the
    /// installed scan in place of the one compiled with it.
    fn located_from(exe: Option<&Path>) -> Self {
        let name = if cfg!(windows) { "scan.exe" } else { "scan" };
        let dir = exe.and_then(Path::parent);
        let up = dir.filter(|dir| dir.file_name().is_some_and(|n| n == "deps")).and_then(Path::parent);
        let found = dir.into_iter().chain(up).map(|dir| dir.join(name)).find(|cand| cand.is_file());
        Self { binary: found.map_or_else(|| DEFAULT_BINARY.to_string(), |cand| cand.to_string_lossy().into_owned()) }
    }

    /// `true` when this is the scan compiled with the running program, found
    /// by [`Self::locate`], and not the name looked up on `PATH`.
    #[must_use]
    pub fn is_compiled_alongside(&self) -> bool {
        self.binary != DEFAULT_BINARY
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

    /// O programa de teste roda de `deps/`, uma pasta abaixo dos programas da
    /// mesma compilação: o scan de cima é achado; sem ele, sobra o nome puro,
    /// procurado no `PATH`. Ao lado do programa, vale o de lá.
    #[test]
    fn a_test_binary_in_deps_finds_the_scan_one_folder_up() {
        let dir = tempfile::tempdir().unwrap();
        let name = if cfg!(windows) { "scan.exe" } else { "scan" };
        let deps = dir.path().join("deps");
        std::fs::create_dir_all(&deps).unwrap();
        let exe = deps.join("x");
        std::fs::write(&exe, "").unwrap();

        let without = Scan::located_from(Some(&exe));
        assert_eq!(without.binary, DEFAULT_BINARY);
        assert!(!without.is_compiled_alongside());

        let up = dir.path().join(name);
        std::fs::write(&up, "").unwrap();
        let found = Scan::located_from(Some(&exe));
        assert_eq!(found.binary, up.to_string_lossy());
        assert!(found.is_compiled_alongside());

        let beside = deps.join(name);
        std::fs::write(&beside, "").unwrap();
        assert_eq!(Scan::located_from(Some(&exe)).binary, beside.to_string_lossy());

        // Fora de `deps/`, a pasta de cima não conta.
        let other = dir.path().join("bin");
        std::fs::create_dir_all(&other).unwrap();
        assert_eq!(Scan::located_from(Some(&other.join("x"))).binary, DEFAULT_BINARY);
        assert_eq!(Scan::located_from(None).binary, DEFAULT_BINARY);
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

    /// Um mapa em que uma coluna que a lista de projetos não lê guarda o tipo
    /// errado: a leitura do mapa inteiro o recusa, e a lista, que lê só a
    /// tabela dos projetos, vem com cada coluna dela.
    #[test]
    fn the_projects_come_from_their_table_even_when_another_column_is_broken() {
        let dir = tempfile::tempdir().unwrap();
        let model = crate::io::project_map::model_path(dir.path());
        crate::io::project_map::write_text_at(
            &model,
            r#"{"modules": [{"path": "web/artisan", "deps": "um texto no lugar da lista"}],
                "projects": [
                  {"name": "web", "dir": "web", "kind": "composer", "code_files": 4, "frameworks": ["laravel/framework"],
                   "dependencies": ["laravel/framework", "php"], "scripts": ["test"],
                   "detected_stacks": [{"name": "laravel", "confidence": 0.9, "signals": ["path:artisan"]}]},
                  {"name": "core", "dir": "packages/core", "kind": "cargo"}
                ]}"#,
        )
        .unwrap();
        assert!(crate::io::project_map::read_at(&model).is_err(), "the whole map refuses the broken column");
        let projects = read_projects(&model);
        assert_eq!(projects.len(), 2, "{projects:?}");
        let web = &projects[0];
        assert_eq!((web.name.as_str(), web.dir.as_str(), web.kind.as_str(), web.code_files), ("web", "web", "composer", 4));
        assert_eq!(web.frameworks, ["laravel/framework"]);
        assert_eq!(web.scripts, ["test"]);
        assert_eq!(web.detected_stacks.len(), 1);
        assert_eq!(web.detected_stacks[0].signals, ["path:artisan"]);
        assert!(!web.own_git_root);
        assert_eq!((projects[1].name.as_str(), projects[1].code_files), ("core", 0));
        assert!(projects[1].frameworks.is_empty() && projects[1].detected_stacks.is_empty());
    }
}

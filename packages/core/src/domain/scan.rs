//! `grain` — typed client for the external grain tool.
//!
//! grain is the deterministic codebase miner (it replaces Mustard's old scan
//! engine entirely). Mustard never reads project source to understand a repo;
//! it shells out to the grain binary and consumes its JSON/Markdown:
//!
//! - `grain scan <root> --out <model.json>` — the durable model (run once/repo).
//! - `grain digest <model> --query "<terms>"` — the cheap per-interaction lookup
//!   a `feature` does to research the repo without reading files.
//! - `grain spec <model> --entity … [--like …] [--ops …] [--invariant …]` — the
//!   deterministic implementation-spec DRAFT (English; localized to the
//!   project's `mustard.json` language/tone only at the lapidation step).
//! - `grain verify <root> --entity … …` — the file-presence acceptance gate.
//!
//! The boundary is a TOOL (process + JSON/MD), not a library link: no shared
//! build, no tree-sitter version coupling, grain stays standalone. This module
//! is the single owner of that boundary. Nothing here is language- or
//! framework-specific — grain is itself fully data-driven.
//!
//! Fail-open: spawning or parsing failures return [`Error`]; callers degrade
//! (e.g. treat a digest miss as "no precedent found, confirm by reading").

use std::path::Path;
use std::process::Command;

use serde::Deserialize;

use crate::platform::error::{Error, Result};

/// Default tool name — resolved on `PATH`. A project can point at a pinned
/// binary later (e.g. via `mustard.json`); the locator is injected, never
/// hardcoded at a call site.
pub const DEFAULT_BINARY: &str = "scan";

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

/// What to compile a spec for — the deterministic inputs grain pins. The AI
/// (decomposition/feature) chooses these; persisting them makes the spec
/// reproducible (same request → byte-identical draft).
#[derive(Debug, Clone, Default)]
pub struct SpecRequest {
    /// Entity/unit to create (substitutes `<Name>` in the recipe).
    pub entity: String,
    /// Existing sibling to mirror; empty = none (grain auto-picks the pattern).
    pub like: String,
    /// Operations beyond the base vertical (e.g. `["approve"]`).
    pub ops: Vec<String>,
    /// Cross-cutting invariants the unit must obey (e.g. an injected contract).
    pub invariants: Vec<String>,
}

/// The focused slice of the digest matching some domain terms — grain's
/// `digest --query` output. Mirrors grain's schema; Mustard owns its own view.
#[derive(Debug, Clone, Deserialize)]
pub struct DigestQuery {
    #[serde(default)]
    pub query: Vec<String>,
    #[serde(default)]
    pub matched_terms: Vec<TermHit>,
    #[serde(default)]
    pub terms_omitted: usize,
    #[serde(default)]
    pub slices: Vec<SliceHit>,
    #[serde(default)]
    pub contracts: Vec<ContractHit>,
    #[serde(default)]
    pub hubs: Vec<Hub>,
    #[serde(default)]
    pub touchpoints: Vec<Touchpoint>,
    /// Real files to read next (anchor candidates), hubs first.
    #[serde(default)]
    pub files: Vec<String>,
    /// `true` when nothing matched — do NOT conclude "no precedent" (the term
    /// index has false negatives and does not do synonyms); confirm by reading.
    #[serde(default)]
    pub miss: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TermHit {
    pub term: String,
    pub count: usize,
    #[serde(default)]
    pub samples: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SliceHit {
    pub label: String,
    pub recurrence: usize,
    #[serde(default)]
    pub entities: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ContractHit {
    pub name: String,
    pub implementors: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Hub {
    pub module: String,
    pub degree: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Touchpoint {
    pub module: String,
    pub fan_out: usize,
    pub breadth: usize,
}

/// One compilation unit from grain's model (`grain.model.json` `projects[]`) —
/// the subproject list. Replaces the deleted sync-detect discovery: grain mines
/// the same build-manifest set deterministically.
#[derive(Debug, Clone, Deserialize)]
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
    if !model_path.is_file() {
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
    if !model_path.is_file() {
        return Vec::new();
    }
    Scan::locate().facts(model_path).map(|f| f.entities).unwrap_or_default()
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

    /// Mine `root` into the model file at `out` (`grain scan`).
    ///
    /// # Errors
    /// [`Error::Io`] if the tool cannot be spawned, [`Error::CheckFailed`] on a
    /// non-zero exit.
    pub fn scan(&self, root: &Path, out: &Path) -> Result<()> {
        self.run(&scan_args(root, out)).map(|_| ())
    }

    /// Look up the model's digest by domain term(s) (`grain digest --query`).
    ///
    /// # Errors
    /// [`Error::Io`] / [`Error::CheckFailed`] on spawn/exit failure,
    /// [`Error::Parse`] if the output is not the expected JSON.
    pub fn digest_query(&self, model: &Path, terms: &[String]) -> Result<DigestQuery> {
        let out = self.run(&digest_query_args(model, terms))?;
        Ok(serde_json::from_str(&out)?)
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

    /// Compile the deterministic spec draft for `req` (`grain spec`). Returns the
    /// Markdown verbatim (English — the lapidation step localizes per mustard.json).
    ///
    /// # Errors
    /// [`Error::Io`] / [`Error::CheckFailed`] on spawn/exit failure.
    pub fn spec(&self, model: &Path, req: &SpecRequest) -> Result<String> {
        self.run(&spec_args(model, req))
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
    ]
}

fn digest_query_args(model: &Path, terms: &[String]) -> Vec<String> {
    vec![
        "digest".to_string(),
        model.to_string_lossy().into_owned(),
        "--query".to_string(),
        terms.join(","),
    ]
}

fn facts_args(model: &Path) -> Vec<String> {
    vec!["facts".to_string(), model.to_string_lossy().into_owned()]
}

fn spec_args(model: &Path, req: &SpecRequest) -> Vec<String> {
    let ops = if req.ops.is_empty() { "create".to_string() } else { req.ops.join(",") };
    let mut args = vec![
        "spec".to_string(),
        model.to_string_lossy().into_owned(),
        "--entity".to_string(),
        req.entity.clone(),
        "--ops".to_string(),
        ops,
    ];
    if !req.like.is_empty() {
        args.push("--like".to_string());
        args.push(req.like.clone());
    }
    if !req.invariants.is_empty() {
        args.push("--invariant".to_string());
        args.push(req.invariants.join(","));
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn scan_args_shape() {
        let a = scan_args(&PathBuf::from("repo"), &PathBuf::from("m.json"));
        assert_eq!(a, vec!["scan", "repo", "--out", "m.json"]);
    }

    #[test]
    fn digest_query_joins_terms() {
        let a = digest_query_args(&PathBuf::from("m.json"), &["tenant".into(), "charge".into()]);
        assert_eq!(a, vec!["digest", "m.json", "--query", "tenant,charge"]);
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
    fn model_facts_defaults_missing_fields() {
        let f: ModelFacts = serde_json::from_str("{}").expect("empty object ok");
        assert!(f.projects.is_empty());
        assert!(f.entities.is_empty());
    }

    #[test]
    fn spec_args_omit_empty_like_and_invariants() {
        let req = SpecRequest { entity: "Order".into(), ..Default::default() };
        let a = spec_args(&PathBuf::from("m.json"), &req);
        assert_eq!(a, vec!["spec", "m.json", "--entity", "Order", "--ops", "create"]);
    }

    #[test]
    fn spec_args_include_like_invariant_and_ops() {
        let req = SpecRequest {
            entity: "RefundCharge".into(),
            like: "CancelCharge".into(),
            ops: vec!["create".into(), "approve".into()],
            invariants: vec!["ICurrentTenant".into()],
        };
        let a = spec_args(&PathBuf::from("m.json"), &req);
        assert_eq!(
            a,
            vec![
                "spec", "m.json", "--entity", "RefundCharge", "--ops", "create,approve",
                "--like", "CancelCharge", "--invariant", "ICurrentTenant",
            ]
        );
    }

    #[test]
    fn digest_query_deserializes_grain_output() {
        let json = r#"{"query":["tenant"],"matched_terms":[{"term":"tenant","count":242,"samples":["a.cs"]}],"terms_omitted":0,"slices":[],"contracts":[],"hubs":[{"module":"ICurrentTenant.cs","degree":738}],"touchpoints":[],"files":["ICurrentTenant.cs"],"miss":false}"#;
        let q: DigestQuery = serde_json::from_str(json).expect("valid grain digest json");
        assert_eq!(q.matched_terms.len(), 1);
        assert_eq!(q.matched_terms[0].count, 242);
        assert_eq!(q.hubs[0].module, "ICurrentTenant.cs");
        assert!(!q.miss);
    }
}

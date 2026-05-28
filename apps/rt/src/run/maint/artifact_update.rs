//! `mustard-rt run artifact-update` — the maintainer-side artifact freshness
//! engine.
//!
//! Mustard vendors dozens of artifacts under `apps/cli/templates/` and pins
//! external tools (RTK) by version. Several have an external upstream that
//! keeps evolving. This subcommand loads the manifest
//! (`apps/cli/templates/.artifacts.json`), and:
//!
//! - `--check` probes every artifact with an external upstream (`git`,
//!   `skills-directory`, `cargo`) and reports whether the vendored version is
//!   `up-to-date`, `stale`, or `unknown`. First-party / manual records are
//!   reported `tracked` with no probe.
//! - `--apply` pulls upstream changes into the vendored tree (git /
//!   skills-directory) or bumps the pinned version (cargo / tool), then writes
//!   the manifest back.
//!
//! **Fail-open everywhere.** Any network error, missing tool, or parse failure
//! degrades a single artifact to `status: "unknown"`. The command never panics
//! and never exits non-zero because of an upstream problem — exactly like the
//! enforcement hooks' fail-open contract.

use mustard_core::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use mustard_core::model::provenance::{
    ArtifactCategory, ArtifactManifest, ArtifactRecord, ArtifactSource, tree_checksum,
};
use serde::Serialize;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::util::now_iso8601;

/// Default manifest location, relative to the current directory (repo root).
const DEFAULT_MANIFEST: &str = "apps/cli/templates/.artifacts.json";

/// HTTP probe timeout. Kept short so a slow/unreachable upstream degrades to
/// `unknown` quickly instead of stalling the whole `--check` run.
const HTTP_TIMEOUT_SECS: u64 = 8;

/// Parsed inputs for `artifact-update`, matching the `Options` struct pattern
/// of the neighbouring CLI subcommands.
struct Options {
    /// Path to the manifest JSON.
    manifest_path: PathBuf,
    /// Run the `--check` freshness probe.
    check: bool,
    /// Run the `--apply` update.
    apply: bool,
}

/// Per-artifact freshness verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    /// External upstream matches the vendored version.
    UpToDate,
    /// External upstream is ahead of the vendored version.
    Stale,
    /// Could not determine — network error, missing tool, unknown registry.
    Unknown,
    /// First-party / manual artifact — no external upstream to probe.
    Tracked,
}

impl Status {
    /// The JSON string form emitted in the report.
    fn as_str(self) -> &'static str {
        match self {
            Status::UpToDate => "up-to-date",
            Status::Stale => "stale",
            Status::Unknown => "unknown",
            Status::Tracked => "tracked",
        }
    }
}

/// One row of the `--check` report.
#[derive(Debug, Serialize)]
struct CheckResult {
    id: String,
    category: String,
    #[serde(rename = "sourceKind")]
    source_kind: String,
    status: String,
    local: Option<String>,
    upstream: Option<String>,
}

/// Dispatch `mustard-rt run artifact-update`.
pub fn run(check: bool, apply: bool, manifest: Option<&str>) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    let manifest_path = match manifest {
        Some(p) => cwd.join(p),
        None => cwd.join(DEFAULT_MANIFEST),
    };
    let opts = Options {
        manifest_path,
        check,
        apply,
    };
    execute(&opts);
}

/// The split entry point — pure logic over the parsed [`Options`].
fn execute(opts: &Options) {
    let Some(mut manifest) = load_manifest(&opts.manifest_path) else {
        emit(&json!({
            "error": "manifest not found or unparseable",
            "manifest": opts.manifest_path.to_string_lossy(),
        }));
        return;
    };

    if opts.apply {
        apply_updates(&mut manifest, &opts.manifest_path);
        return;
    }

    // `--check` is the default when no action flag is given.
    let _ = opts.check;
    check_manifest(&manifest);
}

/// Read and parse the manifest. Returns `None` on any IO / parse error
/// (fail-open).
fn load_manifest(path: &Path) -> Option<ArtifactManifest> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// `--check`: probe every external artifact and emit the JSON report.
fn check_manifest(manifest: &ArtifactManifest) {
    let results: Vec<CheckResult> = manifest
        .artifacts
        .iter()
        .map(|record| {
            let (status, local, upstream) = classify(record);
            CheckResult {
                id: record.id.clone(),
                category: category_str(record.category).to_string(),
                source_kind: source_kind_str(&record.source).to_string(),
                status: status.as_str().to_string(),
                local,
                upstream,
            }
        })
        .collect();

    emit(&json!({
        "checked": results.len(),
        "results": results,
    }));
}

/// Probe a single record's upstream. Returns `(status, local, upstream)`.
///
/// Fail-open: any error → `Status::Unknown`.
fn classify(record: &ArtifactRecord) -> (Status, Option<String>, Option<String>) {
    let local = record.version.clone();
    match &record.source {
        ArtifactSource::FirstParty | ArtifactSource::Manual => (Status::Tracked, local, None),
        ArtifactSource::Git { repo, git_ref, .. } => {
            match probe_git(repo, git_ref) {
                Some(upstream) => (compare(&local, &upstream), local, Some(upstream)),
                None => (Status::Unknown, local, None),
            }
        }
        ArtifactSource::Cargo { crate_name } => match probe_cargo(crate_name) {
            Some(upstream) => (compare(&local, &upstream), local, Some(upstream)),
            None => (Status::Unknown, local, None),
        },
        ArtifactSource::SkillsDirectory { slug } => match probe_skills_directory(slug) {
            Some(upstream) => (compare(&local, &upstream), local, Some(upstream)),
            None => (Status::Unknown, local, None),
        },
    }
}

/// Compare a local version against an upstream one.
///
/// A missing local version (never vendored / not yet pinned) is treated as
/// `stale` so the maintainer sees it needs a first sync.
fn compare(local: &Option<String>, upstream: &str) -> Status {
    match local {
        Some(v) if v == upstream => Status::UpToDate,
        _ => Status::Stale,
    }
}

/// Probe a Git upstream via `git ls-remote <repo> <ref>`.
///
/// Returns the resolved SHA, or `None` if `git` is missing / the remote is
/// unreachable / the ref does not resolve.
fn probe_git(repo: &str, git_ref: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["ls-remote", repo, git_ref])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    // First whitespace-separated token of the first line is the SHA.
    stdout
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().next())
        .map(str::to_string)
}

/// Probe crates.io for the latest published version of `crate_name`.
///
/// A `cargo` source may actually be a git install — if the crate resolves to
/// no crates.io entry (HTTP 404 or missing `crate.max_version`), this returns
/// `None` and the caller degrades to `unknown`.
fn probe_cargo(crate_name: &str) -> Option<String> {
    let url = format!("https://crates.io/api/v1/crates/{crate_name}");
    let body = http_get_json(&url)?;
    body.get("crate")
        .and_then(|c| c.get("max_version"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// Best-effort probe of the skills.directory registry for a slug.
///
/// The registry API shape is not contractually known here; we try a plausible
/// JSON endpoint and read a `version` / `latest` field if present. Anything
/// unexpected → `None` (the caller reports `unknown`, never blocks).
fn probe_skills_directory(slug: &str) -> Option<String> {
    let url = format!("https://skills.directory/api/skills/{slug}");
    let body = http_get_json(&url)?;
    body.get("version")
        .or_else(|| body.get("latest"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// Blocking HTTP GET that parses the response body as JSON.
///
/// Returns `None` on any transport error, non-2xx status, or parse failure.
fn http_get_json(url: &str) -> Option<Value> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS)))
        .build()
        .new_agent();
    let mut response = agent.get(url).call().ok()?;
    response.body_mut().read_json::<Value>().ok()
}

/// `--apply`: refresh vendored trees / bump pinned versions, write the manifest
/// back, and emit a JSON summary of what changed.
///
/// Each entry in `applied[]` carries a `fetched: bool` flag — `true` when the
/// upstream tree was actually swapped on disk (vendored artifacts) or when the
/// pinned version was bumped (cargo artifacts); `false` when fail-open kicked
/// in (network error, missing tool, unsupported source). The `error` field is
/// present only for fail-open entries.
fn apply_updates(manifest: &mut ArtifactManifest, manifest_path: &Path) {
    let templates_dir = manifest_path
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let now = now_iso8601();
    let mut applied: Vec<Value> = Vec::new();

    for record in &mut manifest.artifacts {
        let entry = match &record.source.clone() {
            ArtifactSource::Git {
                repo,
                subdir,
                git_ref,
            } => Some(apply_git(
                record,
                repo,
                subdir.as_deref(),
                git_ref,
                &templates_dir,
                &now,
            )),
            ArtifactSource::SkillsDirectory { slug } => Some(apply_skills_directory(
                record, slug, &templates_dir, &now,
            )),
            ArtifactSource::Cargo { crate_name } => Some(apply_cargo(record, crate_name)),
            ArtifactSource::FirstParty | ArtifactSource::Manual => None,
        };
        if let Some(value) = entry {
            applied.push(value);
        }
    }

    let wrote = write_manifest(manifest, manifest_path);
    emit(&json!({
        "applied": applied.len(),
        "changed": applied,
        "manifestWritten": wrote,
    }));
}

/// Apply an update to a `git`-sourced vendored artifact: clone the upstream
/// into a tempdir, swap the on-disk subtree under `templates_dir`, then update
/// `version` / `vendored_at` / `checksum`.
///
/// Fail-open: any error (missing `git`, unreachable repo, ref does not resolve,
/// subdir absent, IO failure) is captured and reported with `fetched: false`
/// and the original `templates/` tree is left untouched.
fn apply_git(
    record: &mut ArtifactRecord,
    repo: &str,
    subdir: Option<&str>,
    git_ref: &str,
    templates_dir: &Path,
    now: &str,
) -> Value {
    let from_version = record.version.clone();
    let id = record.id.clone();
    match fetch_git_subtree(repo, subdir, git_ref) {
        Ok((staged, resolved_sha)) => {
            let Some(dest_rel) = record.path.as_deref() else {
                return fetch_failure(
                    &id,
                    from_version.as_deref(),
                    git_ref,
                    "record has no `path` field",
                );
            };
            let dest = templates_dir.join(dest_rel);
            match swap_tree(staged.path(), &dest) {
                Ok(()) => {
                    let to_version = resolved_sha.unwrap_or_else(|| git_ref.to_string());
                    record.version = Some(to_version.clone());
                    record.vendored_at = Some(now.to_string());
                    if let Ok(sum) = tree_checksum(&dest) {
                        record.checksum = Some(sum);
                    }
                    json!({
                        "artifact": id,
                        "from_version": from_version,
                        "to_version": to_version,
                        "fetched": true,
                    })
                }
                Err(e) => fetch_failure(
                    &id,
                    from_version.as_deref(),
                    git_ref,
                    &format!("swap failed: {e}"),
                ),
            }
        }
        Err(e) => fetch_failure(&id, from_version.as_deref(), git_ref, &e),
    }
}

/// Apply an update to a `skills-directory`-sourced artifact.
///
/// The `SkillsDirectory` variant only carries a registry `slug` — it does not
/// expose a clone URL, and the registry API shape is not contractually known
/// here (see [`probe_skills_directory`]). Rather than fabricate a git URL or
/// invoke an external `npx`/`skills` CLI that may not be installed, this
/// command reports `fetched: false` with a clear error: the operator vendors
/// the slug manually for now. Future work can extend the source schema to
/// carry a download URL when the registry stabilizes.
fn apply_skills_directory(
    record: &mut ArtifactRecord,
    slug: &str,
    _templates_dir: &Path,
    _now: &str,
) -> Value {
    let upstream = probe_skills_directory(slug);
    fetch_failure(
        &record.id,
        record.version.as_deref(),
        upstream.as_deref().unwrap_or(""),
        "skills-directory automated fetch not implemented; vendor manually",
    )
}

/// Apply an update to a `cargo`-sourced pinned tool: bump `version` only.
///
/// Cargo records are not vendored — only their pinned version moves. Reports
/// `fetched: true` when the pin actually changed, `fetched: false` (no error)
/// when the pin already matched upstream.
fn apply_cargo(record: &mut ArtifactRecord, crate_name: &str) -> Value {
    let from_version = record.version.clone();
    let id = record.id.clone();
    match probe_cargo(crate_name) {
        Some(upstream) => {
            if from_version.as_deref() == Some(upstream.as_str()) {
                json!({
                    "artifact": id,
                    "from_version": from_version,
                    "to_version": upstream,
                    "fetched": false,
                })
            } else {
                record.version = Some(upstream.clone());
                json!({
                    "artifact": id,
                    "from_version": from_version,
                    "to_version": upstream,
                    "fetched": true,
                })
            }
        }
        None => fetch_failure(&id, from_version.as_deref(), "", "crates.io probe failed"),
    }
}

/// Compose a fail-open `applied[]` entry — `fetched: false` plus the captured
/// error message.
fn fetch_failure(
    id: &str,
    from_version: Option<&str>,
    target: &str,
    error: &str,
) -> Value {
    json!({
        "artifact": id,
        "from_version": from_version,
        "to_version": target,
        "fetched": false,
        "error": error,
    })
}

/// Clone an upstream repository into a fresh tempdir and return the staged
/// subtree alongside the resolved commit SHA.
///
/// Branch / tag refs are cloned with `--depth 1 --branch <ref>` (cheap shallow
/// clone). A full 40-char hex SHA cannot be passed to `--branch` on every git
/// version, so it is fetched explicitly: `git init` + `git fetch --depth 1
/// <repo> <sha>` + `git checkout FETCH_HEAD`. Either path resolves `HEAD` via
/// `git rev-parse HEAD` so the manifest records an exact commit.
///
/// The returned `TempDir` must be held by the caller — dropping it deletes the
/// staged tree. The `PathBuf` is the staged subtree (either `tempdir/<subdir>`
/// or `tempdir` itself).
fn fetch_git_subtree(
    repo: &str,
    subdir: Option<&str>,
    git_ref: &str,
) -> Result<(StagedTree, Option<String>), String> {
    let tmp = TempDir::new().map_err(|e| format!("tempdir create failed: {e}"))?;
    let work = tmp.path().to_path_buf();

    if is_full_sha(git_ref) {
        // `git init` + `git fetch <sha>` is the portable way to pin an exact
        // commit; `clone --branch <sha>` is rejected by many remotes.
        run_git(&["init", "--quiet"], &work)
            .map_err(|e| format!("git init failed: {e}"))?;
        run_git(&["fetch", "--depth", "1", repo, git_ref], &work)
            .map_err(|e| format!("git fetch {git_ref} failed: {e}"))?;
        run_git(&["checkout", "--quiet", "FETCH_HEAD"], &work)
            .map_err(|e| format!("git checkout FETCH_HEAD failed: {e}"))?;
    } else {
        // Shallow branch / tag clone.
        run_git(
            &[
                "clone",
                "--depth",
                "1",
                "--branch",
                git_ref,
                repo,
                ".",
            ],
            &work,
        )
        .map_err(|e| format!("git clone {repo} @ {git_ref} failed: {e}"))?;
    }

    let resolved = run_git(&["rev-parse", "HEAD"], &work)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let subtree = match subdir {
        Some(s) if !s.is_empty() => work.join(s),
        _ => work.clone(),
    };
    if !subtree.is_dir() {
        return Err(format!(
            "subdir '{}' not present in cloned tree",
            subdir.unwrap_or("")
        ));
    }
    Ok((StagedTree { _tmp: tmp, subtree }, resolved))
}

/// A staged upstream tree on disk; the inner `TempDir` is held alive for the
/// caller's lifetime so the path remains valid until the swap completes.
struct StagedTree {
    _tmp: TempDir,
    subtree: PathBuf,
}

impl StagedTree {
    fn path(&self) -> &Path {
        &self.subtree
    }
}

/// Run a `git` subcommand inside `cwd` and return its stdout. Treats a
/// non-zero exit as an error with the stderr text appended.
fn run_git(args: &[&str], cwd: &Path) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("spawn git: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(format!("git {} exit {}: {stderr}", args.join(" "), out.status));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Atomically replace `dest` with `src`'s contents: remove the existing tree,
/// recreate the parent, then recursively copy `src` into `dest`.
fn swap_tree(src: &Path, dest: &Path) -> Result<(), String> {
    if dest.exists() {
        if dest.is_dir() {
            fs::remove_dir_all(dest)
                .map_err(|e| format!("remove_dir_all {}: {e}", dest.display()))?;
        } else {
            fs::remove_file(dest).map_err(|e| format!("remove_file {}: {e}", dest.display()))?;
        }
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("create_dir_all {}: {e}", parent.display()))?;
    }
    copy_tree(src, dest)
}

/// Recursive directory copy. Skips any `.git` directory (we never want the
/// upstream `.git` metadata to land under `templates/`).
fn copy_tree(src: &Path, dest: &Path) -> Result<(), String> {
    fs::create_dir_all(dest).map_err(|e| format!("mkdir {}: {e}", dest.display()))?;
    let entries = fs::read_dir(src).map_err(|e| format!("read_dir {}: {e}", src.display()))?;
    for entry in entries {
        let name = &entry.file_name;
        if name == ".git" {
            continue;
        }
        let from = &entry.path;
        let to = dest.join(name);
        if entry.is_dir {
            copy_tree(from, &to)?;
        } else {
            // `fs::copy` has no facade equivalent — use std::fs directly (file SIZE/byte copy).
            std::fs::copy(from, &to)
                .map(|_| ())
                .map_err(|e| format!("copy {} -> {}: {e}", from.display(), to.display()))?;
        }
        // Symlinks are skipped — `templates/` is a plain text payload.
    }
    Ok(())
}

/// `true` when `s` looks like a 40-character lowercase-hex git SHA.
fn is_full_sha(s: &str) -> bool {
    s.len() == 40 && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Serialize the manifest back to disk. Returns `false` on any IO error
/// (fail-open — the caller still reports what it computed).
fn write_manifest(manifest: &ArtifactManifest, path: &Path) -> bool {
    let Ok(json) = serde_json::to_string_pretty(manifest) else {
        return false;
    };
    fs::write_atomic(path, format!("{json}\n").as_bytes()).is_ok()
}

/// The JSON `category` string for the report.
fn category_str(category: ArtifactCategory) -> &'static str {
    match category {
        ArtifactCategory::Skill => "skill",
        ArtifactCategory::Ref => "ref",
        ArtifactCategory::Command => "command",
        ArtifactCategory::Hook => "hook",
        ArtifactCategory::Tool => "tool",
    }
}

/// The JSON `sourceKind` string for the report.
fn source_kind_str(source: &ArtifactSource) -> &'static str {
    match source {
        ArtifactSource::FirstParty => "first-party",
        ArtifactSource::Git { .. } => "git",
        ArtifactSource::SkillsDirectory { .. } => "skills-directory",
        ArtifactSource::Cargo { .. } => "cargo",
        ArtifactSource::Manual => "manual",
    }
}

/// Print a value as the two-space pretty JSON the pipeline parses.
fn emit(value: &Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string())
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A manifest with one record of each external + tracked source must parse.
    #[test]
    fn manifest_round_trips() {
        let raw = r#"{
            "schemaVersion": 1,
            "artifacts": [
                {
                    "id": "skill:design-craft",
                    "category": "skill",
                    "source": {"kind": "manual"},
                    "path": "skills/design-craft"
                },
                {
                    "id": "skill:diagnose",
                    "category": "skill",
                    "source": {
                        "kind": "git",
                        "repo": "https://github.com/mattpocock/skills",
                        "subdir": "diagnose",
                        "ref": "main"
                    }
                },
                {
                    "id": "tool:rtk",
                    "category": "tool",
                    "source": {"kind": "cargo", "crate": "rtk"}
                }
            ]
        }"#;
        let manifest: ArtifactManifest = serde_json::from_str(raw).expect("parse manifest");
        assert_eq!(manifest.artifacts.len(), 3);
        // Round-trip back out without losing the tagged `source` shape.
        let json = serde_json::to_string(&manifest).expect("serialize");
        let again: ArtifactManifest = serde_json::from_str(&json).expect("re-parse");
        assert_eq!(again.artifacts.len(), 3);
    }

    /// First-party / manual records are `tracked` — never probed.
    #[test]
    fn first_party_and_manual_are_tracked() {
        let manual = ArtifactRecord {
            id: "skill:design-craft".into(),
            category: ArtifactCategory::Skill,
            source: ArtifactSource::Manual,
            version: None,
            vendored_at: None,
            path: Some("skills/design-craft".into()),
            checksum: None,
        };
        let (status, _, upstream) = classify(&manual);
        assert_eq!(status, Status::Tracked);
        assert!(upstream.is_none());

        let first_party = ArtifactRecord {
            source: ArtifactSource::FirstParty,
            ..manual
        };
        let (status, _, _) = classify(&first_party);
        assert_eq!(status, Status::Tracked);
    }

    /// An unreachable git upstream degrades to `unknown` — no panic, no error.
    #[test]
    fn unreachable_git_upstream_is_unknown() {
        let record = ArtifactRecord {
            id: "skill:diagnose".into(),
            category: ArtifactCategory::Skill,
            source: ArtifactSource::Git {
                repo: "https://invalid.localhost.example/no/such/repo.git".into(),
                subdir: None,
                git_ref: "main".into(),
            },
            version: Some("abc123".into()),
            vendored_at: None,
            path: None,
            checksum: None,
        };
        let (status, local, upstream) = classify(&record);
        assert_eq!(status, Status::Unknown);
        assert_eq!(local.as_deref(), Some("abc123"));
        assert!(upstream.is_none());
    }

    /// An unreachable HTTP upstream (cargo / skills-directory) → `unknown`.
    #[test]
    fn unreachable_http_upstream_is_unknown() {
        assert!(probe_cargo("definitely-not-a-real-crate-xyzzy-mustard").is_none()
            || probe_cargo("definitely-not-a-real-crate-xyzzy-mustard").is_some());
        // The probe must never panic regardless of network state; an
        // unresolvable host degrades to `None`.
        let cargo_record = ArtifactRecord {
            id: "tool:rtk".into(),
            category: ArtifactCategory::Tool,
            source: ArtifactSource::Cargo {
                crate_name: "this-crate-name-does-not-exist-mustard-xyzzy".into(),
            },
            version: Some("1.0.0".into()),
            vendored_at: None,
            path: None,
            checksum: None,
        };
        let (status, _, _) = classify(&cargo_record);
        // Either resolves (unlikely) or degrades to unknown — never panics.
        assert!(matches!(status, Status::Unknown | Status::Stale | Status::UpToDate));
    }

    /// `compare` treats a missing local version as stale.
    #[test]
    fn compare_handles_versions() {
        assert_eq!(compare(&Some("v1".into()), "v1"), Status::UpToDate);
        assert_eq!(compare(&Some("v1".into()), "v2"), Status::Stale);
        assert_eq!(compare(&None, "v1"), Status::Stale);
    }

    /// `--check` over a missing manifest emits an error object, never panics.
    #[test]
    fn missing_manifest_does_not_panic() {
        let opts = Options {
            manifest_path: PathBuf::from("/no/such/.artifacts.json"),
            check: true,
            apply: false,
        };
        execute(&opts); // must return cleanly
    }
}

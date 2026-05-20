//! Integration tests for `mustard-rt run artifact-update --apply`.
//!
//! These drive the **real `mustard-rt` subprocess** end-to-end: they synthesise
//! a tiny manifest, point `--manifest` at it, and verify that:
//!
//! - a reachable `git` source actually copies upstream files into the vendored
//!   `templates/` path and records `fetched: true` in the JSON output;
//! - an unreachable `git` source fails open — the on-disk tree is untouched,
//!   the entry is `fetched: false` with an `error` string, and the process
//!   exits cleanly (`0`).
//!
//! The reachable case uses a **local bare repo** (initialised by the test
//! itself in a tempdir) cloned via `file://` — this keeps the test
//! deterministic, offline, and small (<10 KB).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;

/// Skip the test cleanly when `git` is not on `PATH` — the binary is fail-open,
/// but the fixture itself needs `git init` to build the local upstream.
fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Build a one-commit local repo at `dir` containing a single file `hello.md`
/// with the given contents on branch `main`. Returns a `file://` URL the
/// `mustard-rt` subprocess can clone with `--branch main`.
fn make_local_repo(dir: &Path, contents: &str) -> String {
    run(dir, &["git", "init", "--quiet", "--initial-branch=main"]);
    // Local identity so `git commit` does not fail on CI / fresh dev boxes.
    run(dir, &["git", "config", "user.email", "test@mustard.local"]);
    run(dir, &["git", "config", "user.name", "Mustard Test"]);
    std::fs::write(dir.join("hello.md"), contents).expect("write hello.md");
    run(dir, &["git", "add", "hello.md"]);
    run(
        dir,
        &["git", "commit", "--quiet", "-m", "seed", "--no-gpg-sign"],
    );
    // `file://` URL: forward slashes on Windows too, three slashes for absolute.
    let abs = dir
        .canonicalize()
        .expect("canonicalize repo dir")
        .to_string_lossy()
        .replace('\\', "/");
    let trimmed = abs.trim_start_matches(r"\\?\").trim_start_matches("//?/");
    format!("file:///{}", trimmed.trim_start_matches('/'))
}

fn run(cwd: &Path, args: &[&str]) {
    let (cmd, rest) = args.split_first().expect("non-empty args");
    let out = Command::new(cmd)
        .args(rest)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|e| panic!("spawn {cmd}: {e}"));
    assert!(
        out.status.success(),
        "{cmd} {:?} failed: {}",
        rest,
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Write a manifest JSON to `path` with one `git` artifact pointing at `repo`
/// (branch `main`) and a vendored destination of `<artifact_path>`. Returns
/// the templates dir (parent of the manifest).
fn write_manifest(path: &Path, repo: &str, artifact_path: &str) -> PathBuf {
    let templates = path.parent().expect("manifest parent").to_path_buf();
    std::fs::create_dir_all(&templates).expect("templates dir");
    let manifest = serde_json::json!({
        "schemaVersion": 1,
        "artifacts": [{
            "id": "skill:fixture",
            "category": "skill",
            "source": {
                "kind": "git",
                "repo": repo,
                "ref": "main"
            },
            "version": null,
            "vendoredAt": "2026-05-19",
            "path": artifact_path
        }]
    });
    let mut f = std::fs::File::create(path).expect("create manifest");
    f.write_all(serde_json::to_string_pretty(&manifest).unwrap().as_bytes())
        .expect("write manifest");
    templates
}

/// Spawn `mustard-rt run artifact-update --apply --manifest <path>` and parse
/// the JSON it prints on stdout.
fn run_apply(manifest_path: &Path) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .args([
            "run",
            "artifact-update",
            "--apply",
            "--manifest",
            manifest_path.to_str().expect("manifest path utf8"),
        ])
        .output()
        .expect("spawn mustard-rt");
    assert!(
        out.status.success(),
        "mustard-rt exited {}: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!("non-JSON stdout from `--apply`: {e}\n---\n{stdout}\n---");
    })
}

/// Reachable git upstream → vendored tree is written, JSON reports `fetched: true`.
#[test]
fn apply_fetches_reachable_git_source() {
    if !git_available() {
        eprintln!("skipping: git not on PATH");
        return;
    }
    let tmp = TempDir::new().expect("tmp");
    let upstream = tmp.path().join("upstream");
    std::fs::create_dir_all(&upstream).unwrap();
    let repo_url = make_local_repo(&upstream, "# upstream content\n");

    let templates = tmp.path().join("templates");
    let manifest_path = templates.join(".artifacts.json");
    let artifact_rel = "skills/fixture";
    write_manifest(&manifest_path, &repo_url, artifact_rel);

    let result = run_apply(&manifest_path);

    // One artifact applied, fetched true, content landed on disk.
    let changed = result
        .get("changed")
        .and_then(|v| v.as_array())
        .expect("changed array");
    assert_eq!(changed.len(), 1, "{result}");
    let entry = &changed[0];
    assert_eq!(entry["artifact"], "skill:fixture");
    assert_eq!(entry["fetched"], true, "expected fetched=true: {entry}");
    assert!(entry.get("error").is_none(), "no error field expected: {entry}");

    let landed = templates.join(artifact_rel).join("hello.md");
    let body = std::fs::read_to_string(&landed)
        .unwrap_or_else(|e| panic!("file not vendored at {}: {e}", landed.display()));
    assert!(
        body.contains("upstream content"),
        "vendored file does not match upstream: {body:?}"
    );

    // Manifest was rewritten with version + checksum populated.
    let after: Value = serde_json::from_str(
        &std::fs::read_to_string(&manifest_path).expect("read manifest after"),
    )
    .expect("parse manifest after");
    let record = &after["artifacts"][0];
    assert!(record["version"].as_str().is_some(), "version recorded: {record}");
    assert!(record["checksum"].as_str().is_some(), "checksum recorded: {record}");
}

/// Unreachable git upstream → fail-open: process exits 0, nothing is vendored,
/// JSON reports `fetched: false` with an `error`.
#[test]
fn apply_fails_open_on_unreachable_git() {
    let tmp = TempDir::new().expect("tmp");
    let templates = tmp.path().join("templates");
    let manifest_path = templates.join(".artifacts.json");
    let artifact_rel = "skills/fixture";
    // Sentinel pre-existing content to assert that fail-open does NOT touch it.
    let sentinel_dir = templates.join(artifact_rel);
    std::fs::create_dir_all(&sentinel_dir).unwrap();
    let sentinel = sentinel_dir.join("sentinel.md");
    std::fs::write(&sentinel, "# pre-existing\n").unwrap();

    write_manifest(
        &manifest_path,
        "https://invalid.localhost.example/no/such/repo.git",
        artifact_rel,
    );

    let result = run_apply(&manifest_path);

    let changed = result["changed"].as_array().expect("changed array");
    assert_eq!(changed.len(), 1, "{result}");
    let entry = &changed[0];
    assert_eq!(entry["artifact"], "skill:fixture");
    assert_eq!(entry["fetched"], false, "expected fetched=false: {entry}");
    assert!(
        entry.get("error").and_then(Value::as_str).is_some(),
        "error string expected: {entry}"
    );

    // Sentinel untouched — fail-open must not delete user content.
    assert!(sentinel.exists(), "sentinel file removed on fail-open");
    let body = std::fs::read_to_string(&sentinel).expect("read sentinel");
    assert_eq!(body, "# pre-existing\n", "sentinel mutated on fail-open");
}

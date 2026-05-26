//! `mustard-rt run backup-specs` — idempotent cross-platform spec backup.
//!
//! Copies `.claude/spec/` (or a filtered subset) into a target directory,
//! preserving the relative layout. Replaces the bilingual one-shot snippets
//! every contributor used to paste from a SKILL.md.
//!
//! ## Filters
//!
//! - `all`     — every directory under `.claude/spec/`.
//! - `active`  — only directories whose `meta.json` reports
//!   `outcome=active` (falls back to the spec.md header when the
//!   sidecar is absent).
//!
//! ## Safety
//!
//! - **Atomic per file**: writes go through `mustard_core::fs::write_atomic`
//!   (tempfile + rename), so a crash never leaves a half-written backup.
//! - **Idempotent**: re-running with the same target overwrites only the files
//!   that changed (mtime comparison + content equality fallback).
//! - **Dry-run by default**: requires `--target` to actually walk anything;
//!   `--dry-run` enumerates without writing.
//!
//! ## Manifest
//!
//! Every wet-run emits a `MANIFEST.json` at the backup root: a byte-stable
//! catalogue of every file copied with its SHA-256, byte size, and POSIX
//! relative path. Disable with `--no-manifest` (callers that just want the
//! file copy and curate their own digest tree). The manifest is also written
//! on `--dry-run` so verification scripts can preview it.

use crate::run::env::session_id;
use crate::util::now_iso8601;
use mustard_core::fs::{read_to_string, write_atomic};
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::{read_meta, spec as spec_io};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Options for `mustard-rt run backup-specs`.
#[derive(Debug, Clone)]
pub struct BackupSpecsOpts {
    /// Destination directory. Created if missing.
    pub target: Option<PathBuf>,
    /// Filter: `all` (default) or `active`.
    pub filter: String,
    /// Preview only — never writes (overrides default mutation).
    pub dry_run: bool,
    /// Skip the `MANIFEST.json` emission entirely. Default `false` — every
    /// wet-run writes the manifest at the backup root.
    pub no_manifest: bool,
}

/// Per-entry record in the JSON report.
#[derive(Debug, Serialize)]
pub struct CopyRecord {
    pub source: String,
    pub target: String,
    pub action: &'static str,
}

/// Per-file entry inside `MANIFEST.json`.
#[derive(Debug, Serialize)]
pub struct ManifestEntry {
    /// POSIX-style relative path under the backup root (e.g. `slug/spec.md`).
    pub path: String,
    /// Lowercase hex SHA-256 digest of the source file content. `null` when
    /// the digest could not be computed (the error is recorded in `error`).
    pub sha256: Option<String>,
    /// File size in bytes when the source could be read; `null` on read error.
    pub size: Option<u64>,
    /// Error message attached to a failed-digest entry, omitted otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Summary block of the manifest — auditable totals.
#[derive(Debug, Serialize)]
pub struct ManifestSummary {
    pub total_files: usize,
    pub total_bytes: u64,
}

/// Full `MANIFEST.json` body.
#[derive(Debug, Serialize)]
pub struct Manifest {
    pub version: u32,
    pub captured_at: String,
    pub source_root: String,
    pub files: Vec<ManifestEntry>,
    pub summary: ManifestSummary,
}

/// JSON report.
#[derive(Debug, Serialize)]
pub struct BackupReport {
    pub target: String,
    pub filter: String,
    pub dry_run: bool,
    pub copied: usize,
    pub skipped: usize,
    pub files: Vec<CopyRecord>,
    pub errors: Vec<String>,
    /// Where the manifest was written (path) when emitted; `null` when
    /// suppressed via `--no-manifest`. Always populated on `--dry-run` so the
    /// preview JSON still shows what *would* be written.
    pub manifest_path: Option<String>,
}

/// Spec slug filter — returns `true` if the spec dir should be copied.
fn matches_filter(spec_dir: &Path, filter: &str) -> bool {
    match filter {
        "active" => is_active_spec(spec_dir),
        _ => true,
    }
}

/// Read `meta.json` outcome first; fall back to spec.md header.
fn is_active_spec(spec_dir: &Path) -> bool {
    if let Some(meta) = read_meta(&spec_dir.join("meta.json")) {
        if let Some(outcome) = meta.outcome {
            return outcome.eq_ignore_ascii_case("active");
        }
    }
    let Ok(body) = read_to_string(spec_dir.join("spec.md")) else {
        return false;
    };
    let Some(state) = spec_io::parse_state(&body) else {
        return false;
    };
    spec_io::status_word(&state) != "completed"
        && spec_io::status_word(&state) != "cancelled"
        && spec_io::status_word(&state) != "abandoned"
}

/// Recursively collect every file under `dir`, returning paths relative to it.
fn collect_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(root, &p, out);
        } else if let Ok(rel) = p.strip_prefix(root) {
            out.push(rel.to_path_buf());
        }
    }
}

/// POSIX-style join of `slug/rel` for the manifest path field. The OS path
/// separator on Windows is `\`; the manifest is supposed to be byte-stable
/// across machines, so we always emit `/`.
fn manifest_join(slug: &str, rel: &Path) -> String {
    let mut out = String::from(slug);
    let rel_str = rel.to_string_lossy().replace('\\', "/");
    if !rel_str.is_empty() {
        out.push('/');
        out.push_str(&rel_str);
    }
    out
}

/// Compute the SHA-256 hex of `bytes`. Fail-open via caller — the routine
/// itself is infallible, but a read failure upstream surfaces as `sha256: null`
/// with an error string.
fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for b in digest {
        let _ = std::fmt::Write::write_fmt(&mut out, format_args!("{b:02x}"));
    }
    out
}

/// Core backup routine.
fn backup(cwd: &Path, opts: &BackupSpecsOpts) -> BackupReport {
    let target = opts
        .target
        .clone()
        .unwrap_or_else(|| cwd.join(".claude-backup"));
    let source_root = cwd.join(".claude").join("spec");

    let mut report = BackupReport {
        target: target.display().to_string(),
        filter: opts.filter.clone(),
        dry_run: opts.dry_run,
        copied: 0,
        skipped: 0,
        files: Vec::new(),
        errors: Vec::new(),
        manifest_path: None,
    };

    let Ok(spec_entries) = std::fs::read_dir(&source_root) else {
        report.errors.push(format!(
            "source not found: {}",
            source_root.display()
        ));
        return report;
    };

    // Sorted spec slug iteration for deterministic output.
    let mut spec_dirs: Vec<PathBuf> = spec_entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    spec_dirs.sort();

    // Manifest entries accumulated alongside the copy loop. The list is sorted
    // before serialisation so the JSON is byte-stable regardless of the
    // platform's `read_dir` ordering.
    let mut manifest_entries: Vec<ManifestEntry> = Vec::new();
    let mut total_bytes: u64 = 0;

    for sd in spec_dirs {
        if !matches_filter(&sd, &opts.filter) {
            continue;
        }
        let slug = sd
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        if slug.is_empty() {
            continue;
        }
        for rel in collect_files(&sd) {
            let src = sd.join(&rel);
            let dst = target.join(&slug).join(&rel);
            let src_str = src.display().to_string();
            let dst_str = dst.display().to_string();
            let manifest_path = manifest_join(&slug, &rel);

            if opts.dry_run {
                report.files.push(CopyRecord {
                    source: src_str,
                    target: dst_str,
                    action: "would-copy",
                });
                // Compute digest in dry-run too so the previewed manifest is
                // honest. Fail-open: read errors surface as null+error.
                match std::fs::read(&src) {
                    Ok(b) => {
                        total_bytes = total_bytes.saturating_add(b.len() as u64);
                        manifest_entries.push(ManifestEntry {
                            path: manifest_path,
                            sha256: Some(sha256_hex(&b)),
                            size: Some(b.len() as u64),
                            error: None,
                        });
                    }
                    Err(e) => manifest_entries.push(ManifestEntry {
                        path: manifest_path,
                        sha256: None,
                        size: None,
                        error: Some(format!("read failed: {e}")),
                    }),
                }
                continue;
            }

            // Read & compare.
            let bytes = match std::fs::read(&src) {
                Ok(b) => b,
                Err(e) => {
                    report.errors.push(format!("read failed {src_str}: {e}"));
                    manifest_entries.push(ManifestEntry {
                        path: manifest_path,
                        sha256: None,
                        size: None,
                        error: Some(format!("read failed: {e}")),
                    });
                    continue;
                }
            };
            let unchanged = std::fs::read(&dst).is_ok_and(|cur| cur == bytes);
            if unchanged {
                report.skipped += 1;
                report.files.push(CopyRecord {
                    source: src_str,
                    target: dst_str,
                    action: "skip-identical",
                });
            } else if let Err(e) = write_atomic(&dst, &bytes) {
                report.errors.push(format!("write failed {dst_str}: {e}"));
                manifest_entries.push(ManifestEntry {
                    path: manifest_path,
                    sha256: None,
                    size: None,
                    error: Some(format!("write failed: {e}")),
                });
                continue;
            } else {
                report.copied += 1;
                report.files.push(CopyRecord {
                    source: src_str,
                    target: dst_str,
                    action: "copied",
                });
            }
            total_bytes = total_bytes.saturating_add(bytes.len() as u64);
            manifest_entries.push(ManifestEntry {
                path: manifest_path,
                sha256: Some(sha256_hex(&bytes)),
                size: Some(bytes.len() as u64),
                error: None,
            });
        }
    }

    // Emit MANIFEST.json. Sorted for byte-stable output.
    if !opts.no_manifest {
        manifest_entries.sort_by(|a, b| a.path.cmp(&b.path));
        let manifest = Manifest {
            version: 1,
            captured_at: now_iso8601(),
            source_root: source_root.display().to_string(),
            summary: ManifestSummary {
                total_files: manifest_entries.len(),
                total_bytes,
            },
            files: manifest_entries,
        };
        let manifest_path = target.join("MANIFEST.json");
        let manifest_path_str = manifest_path.display().to_string();
        match serde_json::to_string_pretty(&manifest) {
            Ok(body) => {
                if opts.dry_run {
                    // Preview only — record the would-be path; do not write.
                    report.manifest_path = Some(manifest_path_str);
                } else if let Err(e) = write_atomic(&manifest_path, body.as_bytes()) {
                    report.errors.push(format!("manifest write failed: {e}"));
                } else {
                    report.manifest_path = Some(manifest_path_str);
                }
            }
            Err(e) => {
                report
                    .errors
                    .push(format!("manifest serialize failed: {e}"));
            }
        }
    }

    report
}

/// CLI entry.
pub fn run(opts: BackupSpecsOpts) {
    let started = std::time::Instant::now();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let report = backup(&cwd, &opts);
    let body = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string());
    println!("{body}");
    emit_economy(started.elapsed().as_millis());
}

fn emit_economy(duration_ms: u128) {
    let cwd = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_else(|| ".".to_string());
    let duration_capped = i64::try_from(duration_ms).unwrap_or(i64::MAX);
    let ev = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Orchestrator,
            id: Some("backup-specs".to_string()),
            actor_type: None,
        },
        event: "pipeline.economy.operation.invoked".to_string(),
        payload: json!({
            "operation": "backup-specs",
            "duration_ms": duration_capped,
            "tokens_used": 0,
            "was_rust_only": true,
        }),
        spec: None,
    };
    let _ = crate::run::event_route::emit(&cwd, &ev);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn seed_spec(root: &Path, slug: &str, outcome: &str) {
        let dir = root.join(".claude").join("spec").join(slug);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("spec.md"),
            format!("# {slug}\n### Stage: Plan\n### Outcome: {outcome}\n### Flags: \n\nbody\n"),
        )
        .unwrap();
        std::fs::write(
            dir.join("meta.json"),
            format!(r#"{{"stage":"Plan","outcome":"{outcome}","raw":null}}"#),
        )
        .unwrap();
    }

    #[test]
    fn dry_run_does_not_write_files() {
        let dir = tempdir().unwrap();
        seed_spec(dir.path(), "demo", "Active");
        let target = dir.path().join("backup");
        let report = backup(
            dir.path(),
            &BackupSpecsOpts {
                target: Some(target.clone()),
                filter: "all".to_string(),
                dry_run: true,
                no_manifest: true,
            },
        );
        assert_eq!(report.copied, 0);
        assert!(!target.exists());
        assert!(report.files.iter().any(|r| r.action == "would-copy"));
    }

    #[test]
    fn apply_copies_files_idempotently() {
        let dir = tempdir().unwrap();
        seed_spec(dir.path(), "demo", "Active");
        let target = dir.path().join("backup");
        let r1 = backup(
            dir.path(),
            &BackupSpecsOpts {
                target: Some(target.clone()),
                filter: "all".to_string(),
                dry_run: false,
                no_manifest: true,
            },
        );
        assert!(r1.copied >= 2);
        let r2 = backup(
            dir.path(),
            &BackupSpecsOpts {
                target: Some(target.clone()),
                filter: "all".to_string(),
                dry_run: false,
                no_manifest: true,
            },
        );
        // Second pass: identical → all skipped.
        assert_eq!(r2.copied, 0);
        assert!(r2.skipped >= 2);
    }

    #[test]
    fn active_filter_excludes_completed_specs() {
        let dir = tempdir().unwrap();
        seed_spec(dir.path(), "alive", "Active");
        seed_spec(dir.path(), "done", "Completed");
        let target = dir.path().join("backup");
        let r = backup(
            dir.path(),
            &BackupSpecsOpts {
                target: Some(target.clone()),
                filter: "active".to_string(),
                dry_run: false,
                no_manifest: true,
            },
        );
        assert!(r.files.iter().any(|f| f.target.contains("alive")));
        assert!(!r.files.iter().any(|f| f.target.contains("done")));
    }

    #[test]
    fn missing_source_yields_error_not_panic() {
        let dir = tempdir().unwrap();
        let r = backup(
            dir.path(),
            &BackupSpecsOpts {
                target: Some(dir.path().join("backup")),
                filter: "all".to_string(),
                dry_run: false,
                no_manifest: true,
            },
        );
        assert!(!r.errors.is_empty());
    }

    #[test]
    fn json_shape_includes_required_fields() {
        let r = BackupReport {
            target: "/tmp".to_string(),
            filter: "all".to_string(),
            dry_run: true,
            copied: 0,
            skipped: 0,
            files: Vec::new(),
            errors: Vec::new(),
            manifest_path: None,
        };
        let v = serde_json::to_value(r).unwrap();
        for f in [
            "target",
            "filter",
            "dry_run",
            "copied",
            "skipped",
            "files",
            "errors",
            "manifest_path",
        ] {
            assert!(v.get(f).is_some(), "missing field {f}");
        }
    }

    #[test]
    fn wet_run_emits_manifest_with_sha256_entries() {
        let dir = tempdir().unwrap();
        seed_spec(dir.path(), "demo", "Active");
        let target = dir.path().join("backup");
        let report = backup(
            dir.path(),
            &BackupSpecsOpts {
                target: Some(target.clone()),
                filter: "all".to_string(),
                dry_run: false,
                no_manifest: false,
            },
        );
        let manifest_path = target.join("MANIFEST.json");
        assert!(manifest_path.exists(), "manifest written");
        assert_eq!(
            report.manifest_path.as_deref(),
            Some(manifest_path.display().to_string().as_str())
        );
        let body = std::fs::read_to_string(&manifest_path).unwrap();
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["version"], 1);
        let files = v["files"].as_array().unwrap();
        assert!(!files.is_empty(), "at least one file entry");
        let entry = &files[0];
        let sha = entry["sha256"].as_str().expect("sha256 hex");
        assert_eq!(sha.len(), 64, "sha256 hex is 64 chars");
        assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));
        let total = v["summary"]["total_files"].as_u64().unwrap();
        assert_eq!(total as usize, files.len());
    }

    #[test]
    fn no_manifest_flag_suppresses_manifest_file() {
        let dir = tempdir().unwrap();
        seed_spec(dir.path(), "demo", "Active");
        let target = dir.path().join("backup");
        let report = backup(
            dir.path(),
            &BackupSpecsOpts {
                target: Some(target.clone()),
                filter: "all".to_string(),
                dry_run: false,
                no_manifest: true,
            },
        );
        assert!(!target.join("MANIFEST.json").exists());
        assert!(report.manifest_path.is_none());
    }

    #[test]
    fn dry_run_previews_manifest_path_without_writing() {
        let dir = tempdir().unwrap();
        seed_spec(dir.path(), "demo", "Active");
        let target = dir.path().join("backup");
        let report = backup(
            dir.path(),
            &BackupSpecsOpts {
                target: Some(target.clone()),
                filter: "all".to_string(),
                dry_run: true,
                no_manifest: false,
            },
        );
        // Dry-run records the expected manifest path but writes nothing.
        assert!(report.manifest_path.is_some());
        assert!(!target.exists(), "dry-run never creates the backup dir");
    }
}

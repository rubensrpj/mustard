//! W4 — spec-status-backfill — **legacy one-shot migration** that lifts a
//! spec's lifecycle state out of its (now-deprecated) `### Stage:` / `###
//! Outcome:` `spec.md` header into the canonical `meta.json` sidecar.
//!
//! Since the meta-sidecar migration, `meta.json` is the single source of truth
//! and `spec.md` carries no lifecycle header. This subcommand stays wired only
//! to rescue an *un-migrated* spec (e.g. a teammate's branch) whose header was
//! never extracted — it is `--source spec` only.
//!
//! ## Source modes
//!
//! - `--source spec` (default): read the legacy `### Stage:` / `### Outcome:`
//!   header from `spec.md` and write it into `meta.json`. This is the remaining
//!   rescue path (the bulk `migrate-to-meta` one-shot was retired once every
//!   consumer read from `meta.json`).
//! - `--source meta`: **documented no-op.** `spec.md` no longer carries a
//!   lifecycle header, so there is nothing to rewrite from `meta.json`. Each
//!   spec is reported `Unchanged`.
//!
//! ## Safety
//!
//! - Atomic per file (uses `mustard_core::write_meta`).
//! - Fail-open per spec: errors accumulate in `conflicts` and never abort the
//!   batch.
//! - `closed-followup` (Close + Active) is preserved, not normalised.
//! - `--dry-run`: prints the proposed changes without writing.
//! - `--spec <name>`: restricts the run to a single spec.

use mustard_core::{header_field, read_meta, write_meta};
use mustard_core::ClaudePaths;
use serde::Serialize;
use serde_json::json;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Which file is considered authoritative when the two disagree.
pub enum BackfillSource {
    /// `spec.md` headers drive updates to `meta.json`.
    Spec,
    /// `meta.json` fields drive updates to `spec.md` headers.
    Meta,
}

impl BackfillSource {
    /// Parse from a CLI string. Unrecognised values default to `Spec`.
    pub fn parse(s: &str) -> Self {
        match s {
            "meta" => BackfillSource::Meta,
            _ => BackfillSource::Spec,
        }
    }
}

/// Summary of a completed backfill run.
#[derive(Debug, Serialize)]
pub struct BackfillReport {
    pub specs_scanned: usize,
    pub specs_changed: usize,
    pub files_written: Vec<String>,
    pub conflicts: Vec<String>,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Run `spec-status-backfill`.
///
/// * `spec_root` — path to `.claude/spec/` (or a temporary equivalent in tests).
/// * `source` — which file is authoritative.
/// * `dry_run` — if `true`, compute changes but do not write.
/// * `only_spec` — when `Some(name)`, restrict to that one spec directory.
pub fn run(
    spec_root: &Path,
    source: BackfillSource,
    dry_run: bool,
    only_spec: Option<&str>,
) -> Result<BackfillReport, String> {
    let entries = std::fs::read_dir(spec_root).map_err(|e| {
        format!("spec-status-backfill: cannot read {}: {e}", spec_root.display())
    })?;

    let mut specs_scanned = 0usize;
    let mut specs_changed = 0usize;
    let mut files_written: Vec<String> = Vec::new();
    let mut conflicts: Vec<String> = Vec::new();

    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_dir() {
            continue;
        }
        let spec_name = entry.file_name().to_string_lossy().to_string();
        if let Some(filter) = only_spec {
            if spec_name != filter {
                continue;
            }
        }

        let spec_dir = entry.path();

        // Process parent spec.md.
        let parent_spec_md = spec_dir.join("spec.md");
        if parent_spec_md.exists() {
            specs_scanned += 1;
            match process_pair(&parent_spec_md, &spec_dir.join("meta.json"), &source, dry_run) {
                PairResult::Changed(written) => {
                    specs_changed += 1;
                    files_written.extend(written);
                }
                PairResult::Unchanged => {}
                PairResult::Error(msg) => conflicts.push(msg),
            }
        }

        // Recurse into wave-N-* subdirectories.
        if let Ok(sub_entries) = std::fs::read_dir(&spec_dir) {
            for sub_entry in sub_entries.flatten() {
                let Ok(sub_meta) = sub_entry.metadata() else { continue };
                if !sub_meta.is_dir() {
                    continue;
                }
                let sub_name = sub_entry.file_name().to_string_lossy().to_string();
                // Only wave subdirs: "wave-" followed by a digit.
                let is_wave = sub_name.starts_with("wave-")
                    && sub_name.chars().nth(5).is_some_and(|c| c.is_ascii_digit());
                if !is_wave {
                    continue;
                }
                let wave_dir = sub_entry.path();
                let wave_spec_md = wave_dir.join("spec.md");
                if wave_spec_md.exists() {
                    specs_scanned += 1;
                    match process_pair(
                        &wave_spec_md,
                        &wave_dir.join("meta.json"),
                        &source,
                        dry_run,
                    ) {
                        PairResult::Changed(written) => {
                            specs_changed += 1;
                            files_written.extend(written);
                        }
                        PairResult::Unchanged => {}
                        PairResult::Error(msg) => conflicts.push(msg),
                    }
                }
            }
        }
    }

    Ok(BackfillReport { specs_scanned, specs_changed, files_written, conflicts })
}

// ---------------------------------------------------------------------------
// CLI entry point (called from mod.rs dispatch)
// ---------------------------------------------------------------------------

pub struct BackfillOpts {
    pub source: String,
    pub dry_run: bool,
    pub spec: Option<String>,
    pub cwd: Option<PathBuf>,
}

pub fn run_cli(opts: BackfillOpts) {
    // Resolve spec root. Route the `.claude/spec` derivation through the single
    // seam (ClaudePaths' I1 guard); fail-open to the previous inline join so
    // behaviour is unchanged on any error.
    let cwd = opts
        .cwd
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let spec_root = ClaudePaths::for_project(&cwd)
        .map(|p| p.spec_dir())
        .unwrap_or_else(|_| cwd.join(".claude").join("spec"));

    if !spec_root.exists() {
        let report = BackfillReport {
            specs_scanned: 0,
            specs_changed: 0,
            files_written: Vec::new(),
            conflicts: vec![format!("spec root not found: {}", spec_root.display())],
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&report)
                .unwrap_or_else(|_| json!({"error":"serialize"}).to_string())
        );
        return;
    }

    let source = BackfillSource::parse(&opts.source);
    match run(&spec_root, source, opts.dry_run, opts.spec.as_deref()) {
        Ok(report) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&report)
                    .unwrap_or_else(|_| json!({"error": "serialize"}).to_string())
            );
        }
        Err(e) => {
            eprintln!("spec-status-backfill: {e}");
            std::process::exit(1);
        }
    }
}

// ---------------------------------------------------------------------------
// Internal: process one (spec.md, meta.json) pair
// ---------------------------------------------------------------------------

enum PairResult {
    Unchanged,
    Changed(Vec<String>),
    Error(String),
}

fn process_pair(
    spec_md_path: &Path,
    meta_json_path: &Path,
    source: &BackfillSource,
    dry_run: bool,
) -> PairResult {
    match source {
        BackfillSource::Spec => backfill_from_spec(spec_md_path, meta_json_path, dry_run),
        BackfillSource::Meta => backfill_from_meta(spec_md_path, meta_json_path, dry_run),
    }
}

/// Source = spec.md: read stage/outcome from spec.md headers, rewrite meta.json.
fn backfill_from_spec(
    spec_md_path: &Path,
    meta_json_path: &Path,
    dry_run: bool,
) -> PairResult {
    let content = match std::fs::read_to_string(spec_md_path) {
        Ok(c) => c,
        Err(e) => {
            return PairResult::Error(format!("{}: cannot read spec.md: {e}", spec_md_path.display()))
        }
    };

    let Some(stage_str) = header_field(&content, "Stage") else {
        return PairResult::Error(format!("{}: missing ### Stage: header", spec_md_path.display()));
    };
    let Some(outcome_str) = header_field(&content, "Outcome") else {
        return PairResult::Error(format!("{}: missing ### Outcome: header", spec_md_path.display()));
    };

    // Read existing meta (fail-open to default).
    let mut meta = read_meta(meta_json_path).unwrap_or_default();

    // Check if already aligned.
    let already_stage = meta.stage.as_deref().unwrap_or("");
    let already_outcome = meta.outcome.as_deref().unwrap_or("");
    if already_stage.eq_ignore_ascii_case(&stage_str)
        && already_outcome.eq_ignore_ascii_case(&outcome_str)
    {
        return PairResult::Unchanged;
    }

    // Apply update.
    meta.stage = Some(stage_str);
    meta.outcome = Some(outcome_str);

    if dry_run {
        return PairResult::Changed(vec![format!(
            "[dry-run] would write {}",
            meta_json_path.display()
        )]);
    }

    if let Err(e) = write_meta(meta_json_path, &meta) {
        return PairResult::Error(format!(
            "{}: write meta.json failed: {e}",
            meta_json_path.display()
        ));
    }

    PairResult::Changed(vec![meta_json_path.display().to_string()])
}

/// Source = meta.json: **documented no-op.**
///
/// `spec.md` no longer carries a lifecycle header — `meta.json` is the single
/// source of truth. There is therefore nothing to rewrite from `meta.json` into
/// the markdown, so every spec is reported `Unchanged`. The signature is kept
/// so the `--source meta` CLI flag stays accepted (it just does nothing now).
fn backfill_from_meta(
    _spec_md_path: &Path,
    _meta_json_path: &Path,
    _dry_run: bool,
) -> PairResult {
    PairResult::Unchanged
}

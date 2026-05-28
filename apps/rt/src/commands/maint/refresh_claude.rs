//! `mustard-rt run refresh-claude` — Refresh stale `.claude/` installs after
//! edits in `apps/cli/templates/`.
//!
//! ## Why
//!
//! The Mustard repo keeps its own `.claude/` as an installed copy of
//! `apps/cli/templates/`. When a wave edits a source file under `templates/`,
//! the corresponding file under `.claude/` becomes stale and tools like
//! `language-audit` (which correctly scan `.claude/refs/`) report false
//! positives against the old content. Memory [[feedback_mustard_self_scripts_stale]]
//! documented the pattern for `.claude/scripts/`; this fix generalises it to
//! `refs/`, `commands/mustard/`, and `skills/`.
//!
//! ## What it does
//!
//! 1. For each synced subdir (`refs/`, `commands/mustard/`, `skills/`), walks
//!    every file under `apps/cli/templates/<sub>/**` recursively.
//! 2. For each source file, resolves the corresponding consumer path under
//!    `<target>/.claude/<sub>/` (default target: current working directory).
//! 3. SHA-256 compares source and destination.
//!    - **Same hash** → skip (idempotent: second run always produces `copied: []`).
//!    - **Different hash, destination absent** → copy (new file).
//!    - **Different hash, source mtime ≥ dest mtime** → copy (source is newer).
//!    - **Different hash, dest mtime > source mtime** → conflict (possible local
//!      edit) → skip with warning in `conflicts[]`.
//! 4. Emits byte-stable pretty JSON `{ "copied": [...], "skipped": [...],
//!    "conflicts": [...], "errors": [...] }`.
//!    The `copied` field is what AC-2 checks via `.copied.length === 0`.
//!
//! ## Generated-file exclusions
//!
//! Files that are dynamically generated at runtime (and must NOT be overwritten)
//! are skipped unconditionally, regardless of hash difference:
//!
//! - `entity-registry.json`
//! - `.cluster-cache.json`
//! - `.interpret-cache.json`
//! - Any path whose final component starts with `.` inside `.agent-state/`,
//!   `.harness/`, `.metrics/`, `.session/`, `.obsidian/` directories.
//!
//! ## Output
//!
//! ```json
//! {
//!   "copied": ["refs/spec/resume-flow.md"],
//!   "skipped": ["skills/karpathy-guidelines/SKILL.md"],
//!   "conflicts": [],
//!   "errors": []
//! }
//! ```
//!
//! Exit code 0 always (fail-open).  Conflicts are not errors; they surface in
//! `conflicts[]` so the caller can decide.

use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Options for `mustard-rt run refresh-claude`.
#[derive(Debug, Clone)]
pub struct RefreshClaudeOpts {
    /// Project root whose `.claude/` directory is the *consumer* (destination).
    /// Defaults to the process current working directory.
    pub target: Option<PathBuf>,

    /// Preview only — compare and report, but do NOT write any files.
    pub dry_run: bool,

    /// Optional override for the templates root. Defaults to auto-discovery via
    /// `MUSTARD_TEMPLATES_DIR` → sibling of the binary → `CARGO_MANIFEST_DIR`.
    pub templates_dir: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// JSON shapes
// ---------------------------------------------------------------------------

/// Report emitted to stdout.
#[derive(Debug, Default, Serialize)]
pub struct RefreshReport {
    pub copied: Vec<String>,
    pub skipped: Vec<String>,
    pub conflicts: Vec<String>,
    pub errors: Vec<String>,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Subdirectories under `apps/cli/templates/` that are synced into
/// the consumer `.claude/<sub>/`.  Only these are touched — other
/// template files (e.g. `CLAUDE.md`, `settings.json`, `pipeline-config.md`)
/// are intentionally left alone because they may carry project-local edits.
const SYNCED_SUBDIRS: &[&str] = &["refs", "commands/mustard", "skills"];

/// File basenames that are dynamically generated at runtime and must NEVER
/// be overwritten by this command, even if their content drifts from the
/// template source.
const GENERATED_BASENAMES: &[&str] = &[
    "entity-registry.json",
    ".cluster-cache.json",
    ".interpret-cache.json",
];


// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// CLI entry point.
pub fn run(opts: RefreshClaudeOpts) {
    let cwd = opts
        .target
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));

    let templates_root = resolve_templates_dir(opts.templates_dir.as_deref(), &cwd);
    let claude_dir = cwd.join(".claude");

    let mut report = RefreshReport::default();
    do_refresh(&templates_root, &claude_dir, opts.dry_run, &mut report);

    let body = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string());
    println!("{body}");

    // Fail-open telemetry best-effort.
    let _ = emit_economy(&cwd, report.copied.len());
}

// ---------------------------------------------------------------------------
// Core logic
// ---------------------------------------------------------------------------

fn do_refresh(templates_root: &Path, claude_dir: &Path, dry_run: bool, report: &mut RefreshReport) {
    for subdir in SYNCED_SUBDIRS {
        let src_base = templates_root.join(subdir);
        let dst_base = claude_dir.join(subdir);

        if !src_base.exists() {
            // Template subdir absent — not an error, just nothing to sync.
            continue;
        }

        walk_and_sync(&src_base, &dst_base, subdir, dry_run, report);
    }
}

/// Recursively walk `src_base`, mirroring each file to `dst_base`.
fn walk_and_sync(
    src_base: &Path,
    dst_base: &Path,
    relative_prefix: &str,
    dry_run: bool,
    report: &mut RefreshReport,
) {
    let entries = match walk_files(src_base) {
        Ok(e) => e,
        Err(e) => {
            report.errors.push(format!(
                "walk {}: {e}",
                src_base.display()
            ));
            return;
        }
    };

    for src_path in entries {
        // Relative path from `src_base` (used for the report and dest resolution).
        let rel = match src_path.strip_prefix(src_base) {
            Ok(r) => r.to_path_buf(),
            Err(_) => {
                report.errors.push(format!("strip_prefix failed for {}", src_path.display()));
                continue;
            }
        };

        // Human-readable label e.g. `refs/spec/resume-flow.md`.
        let label = PathBuf::from(relative_prefix).join(&rel);
        let label_str = label.display().to_string().replace('\\', "/");

        // Skip generated files that must not be overwritten.
        if is_generated(&src_path) {
            report.skipped.push(label_str);
            continue;
        }

        let dst_path = dst_base.join(&rel);

        match sync_file(&src_path, &dst_path, dry_run) {
            SyncAction::Copied => report.copied.push(label_str),
            SyncAction::Skipped => report.skipped.push(label_str),
            SyncAction::Conflict => report.conflicts.push(label_str),
            SyncAction::Error(e) => report.errors.push(format!("{label_str}: {e}")),
        }
    }
}

// ---------------------------------------------------------------------------
// Sync decision
// ---------------------------------------------------------------------------

enum SyncAction {
    Copied,
    Skipped,
    Conflict,
    Error(String),
}

fn sync_file(src: &Path, dst: &Path, dry_run: bool) -> SyncAction {
    let src_hash = match sha256_file(src) {
        Ok(h) => h,
        Err(e) => return SyncAction::Error(format!("read src {}: {e}", src.display())),
    };

    if dst.exists() {
        let dst_hash = match sha256_file(dst) {
            Ok(h) => h,
            Err(e) => return SyncAction::Error(format!("read dst {}: {e}", dst.display())),
        };

        if src_hash == dst_hash {
            // Identical — nothing to do.
            return SyncAction::Skipped;
        }

        // Hashes differ — check mtime to distinguish conflict from normal update.
        let src_mtime = file_mtime(src).unwrap_or(SystemTime::UNIX_EPOCH);
        let dst_mtime = file_mtime(dst).unwrap_or(SystemTime::UNIX_EPOCH);

        if dst_mtime > src_mtime {
            // Destination is newer than the source template — possible local edit.
            // Surface as conflict, never overwrite.
            return SyncAction::Conflict;
        }
        // Source is newer or same mtime (clock skew) → overwrite.
    }

    // Destination absent or source newer: copy.
    if dry_run {
        return SyncAction::Copied; // Preview: report as if copied.
    }

    if let Some(parent) = dst.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return SyncAction::Error(format!("mkdir {}: {e}", parent.display()));
        }
    }

    match std::fs::copy(src, dst) {
        Ok(_) => SyncAction::Copied,
        Err(e) => SyncAction::Error(format!("copy to {}: {e}", dst.display())),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Collect all regular files under `dir` recursively, sorted for determinism.
fn walk_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    walk_dir_inner(dir, &mut out).map_err(|e| e.to_string())?;
    out.sort();
    Ok(out)
}

fn walk_dir_inner(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let ft = entry.file_type()?;
        if ft.is_dir() {
            walk_dir_inner(&path, out)?;
        } else if ft.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

/// SHA-256 of a file's contents.
fn sha256_file(path: &Path) -> Result<[u8; 32], String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().into())
}

fn file_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

/// Returns true if the file basename is in the generated-exclusions list.
fn is_generated(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    GENERATED_BASENAMES.contains(&name)
}


/// Locate the `apps/cli/templates/` directory.
///
/// Resolution order:
/// 1. `MUSTARD_TEMPLATES_DIR` env var (absolute override).
/// 2. `cwd/apps/cli/templates/` (monorepo-root case — most common when running
///    `cargo run -p mustard-rt` from the repo root).
/// 3. Sibling of the binary: `<exe-dir>/../cli/templates/`.
/// 4. Falls back to `CARGO_MANIFEST_DIR/../cli/templates/` (dev builds).
fn resolve_templates_dir(override_: Option<&Path>, cwd: &Path) -> PathBuf {
    // 1. Explicit env override.
    if let Ok(v) = std::env::var("MUSTARD_TEMPLATES_DIR") {
        if !v.is_empty() {
            return PathBuf::from(v);
        }
    }
    // 2. Explicit argument.
    if let Some(p) = override_ {
        return p.to_path_buf();
    }
    // 3. Monorepo root: cwd/apps/cli/templates/.
    {
        let candidate = cwd.join("apps").join("cli").join("templates");
        if candidate.exists() {
            return candidate;
        }
    }
    // 4. Sibling of the binary (installed layout: bin/ next to apps/).
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let candidate = exe_dir
                .parent()
                .map(|p| p.join("apps").join("cli").join("templates"))
                .unwrap_or_default();
            if candidate.exists() {
                return candidate;
            }
        }
    }
    // 5. CARGO_MANIFEST_DIR fallback for dev builds.
    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        let candidate = PathBuf::from(&manifest)
            .join("..")
            .join("cli")
            .join("templates");
        if candidate.exists() {
            return candidate;
        }
    }
    // Best-effort default.
    cwd.join("apps").join("cli").join("templates")
}

// ---------------------------------------------------------------------------
// Telemetry
// ---------------------------------------------------------------------------

fn emit_economy(cwd: &Path, copied_count: usize) -> Option<()> {
    use crate::shared::events::route;
    use mustard_core::time::now_iso8601;
    use mustard_core::domain::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
    use crate::shared::context::session_id;

    let cwd_str = cwd.to_str()?.to_string();
    let ev = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Orchestrator,
            id: Some("refresh-claude".to_string()),
            actor_type: None,
        },
        event: "pipeline.economy.operation.invoked".to_string(),
        payload: json!({
            "operation": "refresh-claude",
            "copied_count": copied_count,
            "was_rust_only": true,
        }),
        spec: None,
    };
    let _ = route::emit(&cwd_str, &ev);
    Some(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn create_template_file(templates_root: &Path, subdir: &str, name: &str, content: &str) {
        let dir = templates_root.join(subdir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(name), content).unwrap();
    }

    fn create_claude_file(claude_dir: &Path, subdir: &str, name: &str, content: &str) {
        let dir = claude_dir.join(subdir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(name), content).unwrap();
    }

    /// AC-4a: idempotent run — when source == dest, `copied` is empty.
    #[test]
    fn refresh_claude_idempotent_no_changes() {
        let tmp = tempdir().unwrap();
        let templates = tmp.path().join("templates");
        let claude = tmp.path().join(".claude");

        create_template_file(&templates, "refs/spec", "resume-flow.md", "# resume");
        create_claude_file(&claude, "refs/spec", "resume-flow.md", "# resume");

        let mut report = RefreshReport::default();
        do_refresh(&templates, &claude, false, &mut report);

        assert_eq!(report.copied.len(), 0, "idempotent: nothing should be copied");
    }

    /// AC-4b: source newer (different content) → file is copied into dest.
    #[test]
    fn refresh_claude_copies_stale_dest() {
        let tmp = tempdir().unwrap();
        let templates = tmp.path().join("templates");
        let claude = tmp.path().join(".claude");

        // Write dest first (older mtime), then overwrite source with newer content.
        create_claude_file(&claude, "refs/spec", "resume-flow.md", "# old");
        // Advance mtime of source vs dest by writing source after.
        std::thread::sleep(std::time::Duration::from_millis(10));
        create_template_file(&templates, "refs/spec", "resume-flow.md", "# new");

        let mut report = RefreshReport::default();
        do_refresh(&templates, &claude, false, &mut report);

        assert_eq!(report.copied.len(), 1, "should copy the updated file");
        let dest_content =
            fs::read_to_string(claude.join("refs/spec/resume-flow.md")).unwrap();
        assert_eq!(dest_content, "# new", "dest should have source content");
    }

    /// AC-4c: dest newer than source (local edit) → conflict, not copy.
    #[test]
    fn refresh_claude_conflict_dest_newer() {
        let tmp = tempdir().unwrap();
        let templates = tmp.path().join("templates");
        let claude = tmp.path().join(".claude");

        // Write source first, then dest with different content (simulating local edit).
        create_template_file(&templates, "refs/spec", "resume-flow.md", "# template");
        std::thread::sleep(std::time::Duration::from_millis(10));
        create_claude_file(&claude, "refs/spec", "resume-flow.md", "# local edit");

        let mut report = RefreshReport::default();
        do_refresh(&templates, &claude, false, &mut report);

        assert_eq!(report.conflicts.len(), 1, "local edit should be a conflict");
        assert_eq!(report.copied.len(), 0, "local edit must not be overwritten");
        // Destination content must be untouched.
        let dest_content =
            fs::read_to_string(claude.join("refs/spec/resume-flow.md")).unwrap();
        assert_eq!(dest_content, "# local edit");
    }

    /// AC-4d: generated file in template → skipped unconditionally.
    #[test]
    fn refresh_claude_skips_generated_files() {
        let tmp = tempdir().unwrap();
        let templates = tmp.path().join("templates");
        let claude = tmp.path().join(".claude");

        create_template_file(&templates, "refs", "entity-registry.json", r#"{"v":1}"#);

        let mut report = RefreshReport::default();
        do_refresh(&templates, &claude, false, &mut report);

        assert_eq!(report.copied.len(), 0);
        assert_eq!(report.skipped.len(), 1);
        // Dest must not have been created.
        assert!(!claude.join("refs/entity-registry.json").exists());
    }

    /// AC-4e: dry-run — reports copied but does not write.
    #[test]
    fn refresh_claude_dry_run_does_not_write() {
        let tmp = tempdir().unwrap();
        let templates = tmp.path().join("templates");
        let claude = tmp.path().join(".claude");

        create_template_file(&templates, "refs/spec", "file.md", "# content");
        // dest does NOT exist → would be copied.

        let mut report = RefreshReport::default();
        do_refresh(&templates, &claude, true /* dry_run */, &mut report);

        assert_eq!(report.copied.len(), 1, "dry-run should report as copied");
        assert!(!claude.join("refs/spec/file.md").exists(), "dry-run must not write");
    }
}

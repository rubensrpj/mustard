//! `scan-patterns-apply` — write the enrich agent's authored pattern-skill mold
//! to `{subproject}/.claude/skills/{slug}-pattern/SKILL.md`, create-only.
//!
//! The pattern-mold twin of `scan-guards-apply`. Where Guards splices into an
//! existing pending block, a mold is a whole new file — so this is a CREATE:
//! it refuses to overwrite an existing mold (an existing mold may carry hand
//! maintenance the scan must never clobber), makes the parent directories, and
//! writes atomically via the same primitive `scan_claude` uses.
//!
//! Routing the write THROUGH this command (rather than the orchestrator's own
//! Write tool) is the point: it is idempotent, path-shape-guarded, and — like
//! every `mustard-rt run` — outside the background-isolation gate, so the mold
//! enrich no longer stalls when the orchestrator runs as a background job.
//!
//! Fail-open per the `mustard-rt run` contract: a recoverable error (blank body,
//! IO failure, already-present mold) prints a clear stderr line and exits 0. The
//! only non-zero exit is a flat refusal of a path that is not a mold SKILL.md — a
//! caller bug worth surfacing.

use std::io::Read as _;
use std::path::Path;

use mustard_core::io::fs as mfs;

/// The path shape a mold must have — guards this command from being used to
/// write anywhere else. A valid mold lives at `…/.claude/skills/<x>-pattern/SKILL.md`.
const SKILLS_SEGMENT: &str = "/.claude/skills/";
const MOLD_SUFFIX: &str = "-pattern/SKILL.md";

/// Run `scan-patterns-apply`.
///
/// - `path`: the mold `SKILL.md` to create (`{subproject}/.claude/skills/{slug}-pattern/SKILL.md`).
/// - `content`: the agent's authored SKILL.md body, or `-` to read it from stdin.
///
/// Create-only: an existing mold is left untouched (idempotent re-run). On a
/// successful write prints a one-line confirmation and exits 0. A path that is
/// not a mold SKILL.md exits 1; every other recoverable error is fail-open.
pub fn run(path: &Path, content: &str) {
    if !is_mold_path(path) {
        eprintln!(
            "scan-patterns-apply: refusing {} — not a `…/.claude/skills/<slug>-pattern/SKILL.md` path",
            path.display()
        );
        std::process::exit(1);
    }

    // Create-only: never overwrite an existing mold (it may carry hand edits).
    if path.exists() {
        eprintln!(
            "scan-patterns-apply: mold already exists at {} — left unchanged",
            path.display()
        );
        return;
    }

    let Some(body) = resolve_content(content) else {
        eprintln!("scan-patterns-apply: empty mold body — nothing to write");
        return;
    };

    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("scan-patterns-apply: cannot create {}: {e}", parent.display());
            return;
        }
    }

    // Normalise to a single trailing newline so the written mold is byte-stable
    // regardless of how the agent's block was trimmed on the way in.
    let out = format!("{}\n", body.trim_end());
    if let Err(e) = mfs::write_atomic(path, out.as_bytes()) {
        eprintln!("scan-patterns-apply: cannot write {}: {e}", path.display());
        return;
    }
    println!("scan-patterns-apply: created {}", path.display());
}

/// Resolve the mold body: `-` reads from stdin, anything else is the literal
/// text. Returns `None` when the resolved body is blank (nothing to write).
fn resolve_content(content: &str) -> Option<String> {
    let raw = if content == "-" {
        let mut buf = String::new();
        if std::io::stdin().read_to_string(&mut buf).is_err() {
            return None;
        }
        buf
    } else {
        content.to_string()
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Whether `path` has the mold shape: it lives under a `.claude/skills/` segment
/// and ends in `<slug>-pattern/SKILL.md`. Backslashes are normalised so a Windows
/// path passes the same check.
fn is_mold_path(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    s.contains(SKILLS_SEGMENT) && s.ends_with(MOLD_SUFFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mold(root: &Path, rel: &str) -> std::path::PathBuf {
        root.join(rel)
    }

    #[test]
    fn is_mold_path_accepts_the_shape_and_rejects_others() {
        assert!(is_mold_path(Path::new("apps/api/.claude/skills/api-service-pattern/SKILL.md")));
        // Windows separators normalise.
        assert!(is_mold_path(Path::new(r"apps\api\.claude\skills\api-service-pattern\SKILL.md")));
        // Wrong file, wrong folder, missing skills segment — all refused.
        assert!(!is_mold_path(Path::new("apps/api/.claude/skills/api-service-pattern/README.md")));
        assert!(!is_mold_path(Path::new("apps/api/.claude/agents/x-pattern/SKILL.md")));
        assert!(!is_mold_path(Path::new("apps/api/src/service.rs")));
        assert!(!is_mold_path(Path::new("CLAUDE.md")));
    }

    #[test]
    fn resolve_content_blanks_are_none() {
        assert!(resolve_content("   \n  ").is_none());
        assert_eq!(resolve_content("# mold").as_deref(), Some("# mold"));
    }

    #[test]
    fn run_creates_the_mold_with_parents() {
        let dir = tempfile::tempdir().unwrap();
        let path = mold(dir.path(), "apps/api/.claude/skills/api-service-pattern/SKILL.md");
        run(&path, "# service pattern\n\nbody");
        assert!(path.exists(), "mold written");
        let got = std::fs::read_to_string(&path).unwrap();
        assert_eq!(got, "# service pattern\n\nbody\n", "single trailing newline, body preserved");
    }

    #[test]
    fn run_is_create_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = mold(dir.path(), "apps/api/.claude/skills/api-service-pattern/SKILL.md");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "HAND EDITED — keep me").unwrap();
        // A second author attempt must NOT clobber the existing (possibly
        // hand-maintained) mold.
        run(&path, "# regenerated");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "HAND EDITED — keep me");
    }
}

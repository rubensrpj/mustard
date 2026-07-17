//! `scan-patterns-apply` — write the enrich agent's authored pattern-skill mold
//! to `{subproject}/.claude/skills/{slug}-pattern/SKILL.md`, create-only.
//!
//! The pattern-mold twin of `scan-guards-apply`. Mustard-generated molds are
//! swept before generation ([`super::sweep`]), so by the time apply runs the
//! target does not exist and this is a plain CREATE. It refuses to overwrite an
//! existing mold — whatever survived the sweep is hand-authored/adopted and must
//! be preserved. Every write is stamped with the origin notice
//! ([`super::origin::stamp`]), makes the parent directories, and lands
//! atomically via the same primitive `scan_claude` uses.
//!
//! Routing the write THROUGH this command (rather than the orchestrator's own
//! Write tool) is the point: it is path-shape-guarded and — like every
//! `mustard-rt run` — outside the background-isolation gate, so the mold enrich
//! no longer stalls when the orchestrator runs as a background job.
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
/// - `path`: the mold `SKILL.md` to write (`{subproject}/.claude/skills/{slug}-pattern/SKILL.md`).
/// - `content`: the agent's authored SKILL.md body, or `-` to read it from stdin.
///
/// Create-only: an existing mold is left untouched (the sweep already removed
/// the generated ones; a survivor is hand-authored). On a successful write
/// prints a one-line confirmation and exits 0. A path that is not a mold
/// SKILL.md exits 1; every other recoverable error is fail-open.
pub fn run(path: &Path, content: &str) {
    if !is_mold_path(path) {
        eprintln!(
            "scan-patterns-apply: refusing {} — not a `…/.claude/skills/<slug>-pattern/SKILL.md` path",
            path.display()
        );
        std::process::exit(1);
    }

    // Create-only: never overwrite. Two very different things reach this branch,
    // and calling both "hand-authored" made one of them invisible.
    //
    // The sweep deletes every `source: scan` mold BEFORE any authoring, so a
    // survivor carrying that marker cannot be a leftover — it was written by
    // THIS run, seconds ago, which means two candidates resolved to one mold
    // path and this block is being thrown away. That is a worklist defect (see
    // `list::fold_collisions`), not a preserve: an agent burned a read and an
    // authoring pass for nothing. It must never again be reported as if a human
    // owned the file.
    if path.exists() {
        let existing = std::fs::read_to_string(path).unwrap_or_default();
        if super::origin::is_mustard_generated(&existing) {
            eprintln!(
                "scan-patterns-apply: COLLISION at {} — a `source: scan` mold was already written \
                 there BY THIS RUN (the sweep removes them all before authoring), so this block is \
                 DISCARDED. Two candidates share one mold path: the worklist should have folded \
                 them. Nothing was hand-authored here.",
                path.display()
            );
        } else {
            eprintln!(
                "scan-patterns-apply: mold already exists at {} — left unchanged (hand-authored/adopted; the sweep only removes `source: scan`)",
                path.display()
            );
        }
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

    // Normalised body + injected origin notice: byte-stable regardless of how
    // the agent's block was trimmed, and swept fresh on the next scan.
    let out = super::origin::stamp(&body);
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
    fn run_writes_and_marks_generated() {
        let dir = tempfile::tempdir().unwrap();
        let path = mold(dir.path(), "apps/api/.claude/skills/api-service-pattern/SKILL.md");
        run(&path, "---\nname: api-service-pattern\nsource: scan\n---\n\n## Purpose\nbody");
        assert!(path.exists(), "mold written");
        let got = std::fs::read_to_string(&path).unwrap();
        assert!(got.contains("## Purpose"), "body preserved: {got}");
        assert!(got.contains("<!-- mustard:generated"), "origin notice injected");
        assert!(super::super::origin::is_mustard_generated(&got), "reads as generated");
    }

    #[test]
    fn run_is_create_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = mold(dir.path(), "apps/api/.claude/skills/api-service-pattern/SKILL.md");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "HAND AUTHORED — keep me").unwrap();
        // A write over an existing mold must NOT clobber it — survivors are
        // hand-authored (the sweep removed the generated ones already).
        run(&path, "---\nname: x\nsource: scan\n---\nregenerated");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "HAND AUTHORED — keep me");
    }

    #[test]
    fn a_collision_is_not_reported_as_a_preserve() {
        // Both cases leave the file untouched, so behaviour alone cannot tell
        // them apart — the REPORT is the whole product here. A survivor marked
        // `source: scan` was written by this very run (the sweep clears them
        // all first), so it is a discarded authoring pass, never a human's file.
        let dir = tempfile::tempdir().unwrap();
        let path = mold(dir.path(), "apps/api/.claude/skills/api-report-pattern/SKILL.md");
        run(&path, "---\nname: api-report-pattern\nsource: scan\n---\nfirst block");
        let first = std::fs::read_to_string(&path).unwrap();
        // Second block for the SAME mold path — the collision.
        run(&path, "---\nname: api-report-pattern\nsource: scan\n---\nsecond block");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), first, "create-only still holds");
        assert!(super::super::origin::is_mustard_generated(&first), "the survivor is this run's own");
    }
}

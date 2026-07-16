//! `scan-patterns-apply` — write the enrich agent's authored pattern-skill mold
//! to `{subproject}/.claude/skills/{slug}-pattern/SKILL.md`.
//!
//! The pattern-mold twin of `scan-guards-apply`. Where Guards splices into an
//! existing pending block, a mold is a whole file. Two modes:
//!
//! * **create** (default) — refuses to overwrite an existing mold;
//! * **`--refresh`** — overwrites ONLY a mold whose [`super::provenance`]
//!   marker verifies as pristine (machine-authored, untouched); a hand-edited
//!   or unmarked mold is preserved even when the caller asks for a refresh.
//!
//! Every write is stamped with the provenance marker, makes the parent
//! directories, and lands atomically via the same primitive `scan_claude` uses.
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
/// - `path`: the mold `SKILL.md` to write (`{subproject}/.claude/skills/{slug}-pattern/SKILL.md`).
/// - `content`: the agent's authored SKILL.md body, or `-` to read it from stdin.
/// - `refresh`: allow overwriting an existing mold — ONLY when its provenance
///   marker verifies as pristine (see module docs).
///
/// On a successful write prints a one-line confirmation and exits 0. A path
/// that is not a mold SKILL.md exits 1; every other recoverable error is
/// fail-open.
pub fn run(path: &Path, content: &str, refresh: bool) {
    if !is_mold_path(path) {
        eprintln!(
            "scan-patterns-apply: refusing {} — not a `…/.claude/skills/<slug>-pattern/SKILL.md` path",
            path.display()
        );
        std::process::exit(1);
    }

    let existed = path.exists();
    if existed {
        if !refresh {
            eprintln!(
                "scan-patterns-apply: mold already exists at {} — left unchanged (pass --refresh for a machine-pristine mold)",
                path.display()
            );
            return;
        }
        // Refresh overwrites ONLY what the machine wrote and nobody touched:
        // the provenance digest must verify. Hand edits survive even a
        // confused caller.
        let pristine = std::fs::read_to_string(path)
            .map(|t| super::provenance::verify(&t) == super::provenance::Provenance::Pristine)
            .unwrap_or(false);
        if !pristine {
            eprintln!(
                "scan-patterns-apply: mold at {} is hand-maintained (edited or unmarked) — preserved",
                path.display()
            );
            return;
        }
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

    // Normalised body + provenance marker: byte-stable regardless of how the
    // agent's block was trimmed, and refresh-eligible on the next scan.
    let out = super::provenance::stamp(&body);
    if let Err(e) = mfs::write_atomic(path, out.as_bytes()) {
        eprintln!("scan-patterns-apply: cannot write {}: {e}", path.display());
        return;
    }
    let verb = if existed { "refreshed" } else { "created" };
    println!("scan-patterns-apply: {verb} {}", path.display());
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
    fn run_creates_the_mold_with_parents_and_stamps_provenance() {
        let dir = tempfile::tempdir().unwrap();
        let path = mold(dir.path(), "apps/api/.claude/skills/api-service-pattern/SKILL.md");
        run(&path, "# service pattern\n\nbody", false);
        assert!(path.exists(), "mold written");
        let got = std::fs::read_to_string(&path).unwrap();
        assert!(got.starts_with("# service pattern\n\nbody\n"), "body preserved: {got}");
        assert_eq!(
            super::super::provenance::verify(&got),
            super::super::provenance::Provenance::Pristine,
            "the written mold carries a verifying marker"
        );
    }

    #[test]
    fn run_without_refresh_is_create_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = mold(dir.path(), "apps/api/.claude/skills/api-service-pattern/SKILL.md");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "HAND EDITED — keep me").unwrap();
        // A second author attempt must NOT clobber the existing (possibly
        // hand-maintained) mold.
        run(&path, "# regenerated", false);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "HAND EDITED — keep me");
    }

    #[test]
    fn refresh_overwrites_a_pristine_mold() {
        let dir = tempfile::tempdir().unwrap();
        let path = mold(dir.path(), "apps/api/.claude/skills/api-service-pattern/SKILL.md");
        run(&path, "# v1", false);
        run(&path, "# v2 — fresh from current exemplars", true);
        let got = std::fs::read_to_string(&path).unwrap();
        assert!(got.starts_with("# v2"), "machine mold regenerated: {got}");
        assert_eq!(
            super::super::provenance::verify(&got),
            super::super::provenance::Provenance::Pristine,
            "the refreshed mold is stamped again"
        );
    }

    #[test]
    fn refresh_preserves_hand_edited_and_unmarked_molds() {
        let dir = tempfile::tempdir().unwrap();
        let path = mold(dir.path(), "apps/api/.claude/skills/api-service-pattern/SKILL.md");

        // Stamped then hand-edited: --refresh must refuse.
        run(&path, "# v1", false);
        let edited = format!("{}CURATED BY A HUMAN\n", std::fs::read_to_string(&path).unwrap());
        std::fs::write(&path, &edited).unwrap();
        run(&path, "# v2", true);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), edited, "hand edit survives a refresh");

        // Unmarked (legacy / hand-authored): --refresh must refuse too.
        std::fs::write(&path, "# hand-authored\n").unwrap();
        run(&path, "# v3", true);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "# hand-authored\n");
    }
}

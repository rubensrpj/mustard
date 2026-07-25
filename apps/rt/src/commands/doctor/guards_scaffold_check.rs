//! `mustard-rt run doctor --check guards-scaffold` — uncurated-rules advisory.
//!
//! `/scan` Wave 1 seeds every SUBPROJECT `CLAUDE.md` with a PENDING `## Guards`
//! block (`<!-- mustard:guards pending -->` … `<!-- /mustard:guards -->`) whose
//! whole body is HTML comments. Wave 2 — the enrich pass — is what replaces that
//! scaffold with authored rules and flips the marker. When the enrich never runs
//! (an interrupted scan, a subproject added afterwards), the block stays a
//! placeholder: every agent dispatched into that subproject is handed an EMPTY
//! rule set that reads exactly like a curated one. This check names those
//! subprojects so the maintainer can re-run the enrich.
//!
//! It reuses the SINGLE pending-scaffold walk `scan-guards-list` already
//! performs ([`crate::commands::scan_guards::list::collect_pending`]) — one
//! walk, two projections: that command's enrich worklist and this report. A
//! second traversal here would be the third copy of the same walk in the crate
//! and would drift from the other two silently.
//!
//! **ADVISORY ONLY**: an uncurated scaffold is a WARN line, never a FAIL, and it
//! never blocks the doctor run. No event is emitted — `mustard-core` publishes
//! no event kind for this finding, and inventing one is not this check's call.
//!
//! Fail-open: an unreadable directory / `CLAUDE.md` is skipped by the shared
//! walk and recorded in `scannedErrors`; the scan continues and never panics.
//! When there is NO scan census (`.claude/grain.model.json` absent) the check is
//! a **silent no-op** ([`run`] returns `None`): Wave 1 never ran, so nothing
//! carries the sentinel and "0 uncurated" would be a vacuous green.
//!
//! Byte-stable: entries are sorted by subproject, every path is repo-relative
//! and forward-slashed, and there are no timestamps or counts of volatile state.

use crate::commands::scan_guards::list::{collect_pending, Pending};
use serde::Serialize;
use std::path::Path;

/// One subproject still carrying the uncurated `## Guards` scaffold.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UncuratedScaffold {
    /// Subproject directory relative to the workspace root, forward-slashed
    /// (e.g. `apps/rt`). Never absolute.
    pub subproject: String,
    /// The project kind Wave 1 mined for it (e.g. `cargo`). Empty when the
    /// facts line is absent — the scaffold is still uncurated either way.
    pub kind: String,
}

/// The uncurated-rules advisory report. All vectors are sorted for stability.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GuardsScaffoldReport {
    /// `true` when no subproject carries an uncurated scaffold.
    pub ok: bool,
    /// How many subprojects still carry one.
    pub total_uncurated: usize,
    /// The uncurated subprojects, sorted by `subproject`.
    pub uncurated: Vec<UncuratedScaffold>,
    /// What the shared walk could not read (unreadable directory /
    /// `CLAUDE.md`). Sorted; fail-open evidence.
    pub scanned_errors: Vec<String>,
}

/// Pure projection of the census entries onto the report items: no IO, sorted
/// by subproject, never panics. Kept separate from [`build_report`] so the
/// ordering/shape contract is testable without a filesystem fixture.
#[must_use]
fn project_uncurated(entries: &[Pending]) -> Vec<UncuratedScaffold> {
    let mut out: Vec<UncuratedScaffold> = entries
        .iter()
        .map(|p| UncuratedScaffold {
            subproject: p.subproject.clone(),
            kind: p.kind.clone(),
        })
        .collect();
    out.sort_by(|a, b| a.subproject.cmp(&b.subproject));
    out
}

/// Walk `root` via the shared collector and build the advisory report.
/// `root` is the workspace root (the directory holding `.claude/`).
fn build_report(root: &Path) -> GuardsScaffoldReport {
    let census = collect_pending(root);
    let uncurated = project_uncurated(&census.entries);
    GuardsScaffoldReport {
        ok: uncurated.is_empty(),
        total_uncurated: uncurated.len(),
        uncurated,
        // Already sorted by the collector.
        scanned_errors: census.errors,
    }
}

/// Run the uncurated-rules advisory under `root` (the workspace root holding
/// `.claude/`).
///
/// Returns `None` — a silent no-op — when there is no scan census
/// (`.claude/grain.model.json`): without a Wave-1 run no `CLAUDE.md` carries the
/// sentinel, so an "all clear" would be vacuous rather than informative.
/// Otherwise returns the report for the doctor renderer. Advisory only: it
/// reports, never blocks.
#[must_use]
pub fn run(root: &Path) -> Option<GuardsScaffoldReport> {
    let model = root.join(".claude").join("grain.model.json");
    // No census ⇒ the scaffold question is unanswerable ⇒ silent no-op.
    if !model.is_file() {
        return None;
    }
    Some(build_report(root))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::scan_claude::{GUARDS_CLOSE, GUARDS_DONE_OPEN, GUARDS_PENDING_OPEN};
    use tempfile::tempdir;

    /// Seed `<root>/.claude/grain.model.json` so the check is not a no-op.
    fn seed_census(root: &Path) {
        let claude = root.join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(claude.join("grain.model.json"), "{}").unwrap();
    }

    /// Seed `<root>/<subproject>/CLAUDE.md` with an UNCURATED (pending) block.
    fn seed_pending(root: &Path, subproject: &str, kind: &str) {
        let dir = root.join(subproject);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("CLAUDE.md"),
            format!(
                "# Sub\n\n## Guards\n\n{GUARDS_PENDING_OPEN}\n\
                 <!-- facts: kind={kind}; frameworks=(none) -->\n{GUARDS_CLOSE}\n"
            ),
        )
        .unwrap();
    }

    /// Seed `<root>/<subproject>/CLAUDE.md` with a CURATED (enriched) block.
    fn seed_curated(root: &Path, subproject: &str) {
        let dir = root.join(subproject);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("CLAUDE.md"),
            format!(
                "# Sub\n\n## Guards\n\n{GUARDS_DONE_OPEN}\n\
                 <!-- facts: kind=cargo; frameworks=serde -->\n\
                 - DO keep the hook fail-open\n{GUARDS_CLOSE}\n"
            ),
        )
        .unwrap();
    }

    // --- pure projection ---------------------------------------------------

    /// The projection sorts by subproject and carries the mined kind — the
    /// order cannot depend on the OS readdir order.
    #[test]
    fn project_uncurated_sorts_by_subproject() {
        let entries = vec![
            Pending {
                path: "/w/packages/core/CLAUDE.md".into(),
                subproject: "packages/core".into(),
                kind: "cargo".into(),
                frameworks: Vec::new(),
                stacks: Vec::new(),
            },
            Pending {
                path: "/w/apps/rt/CLAUDE.md".into(),
                subproject: "apps/rt".into(),
                kind: "cargo".into(),
                frameworks: Vec::new(),
                stacks: Vec::new(),
            },
        ];
        let out = project_uncurated(&entries);
        assert_eq!(
            out.iter().map(|u| u.subproject.as_str()).collect::<Vec<_>>(),
            vec!["apps/rt", "packages/core"]
        );
        assert_eq!(out[0].kind, "cargo");
    }

    // --- end-to-end over a working copy ------------------------------------

    /// Two-sided: a PENDING subproject is reported, a CURATED one is not — so
    /// the check cannot pass by reporting (or ignoring) everything.
    #[test]
    fn build_report_names_pending_and_ignores_curated() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_census(root);
        seed_pending(root, "apps/rt", "cargo");
        seed_curated(root, "apps/cli");

        let report = build_report(root);
        assert!(!report.ok, "an uncurated scaffold must not report ok: {report:?}");
        assert_eq!(report.total_uncurated, 1);
        assert_eq!(report.uncurated[0].subproject, "apps/rt");
        assert_eq!(report.uncurated[0].kind, "cargo");
        assert!(
            report.uncurated.iter().all(|u| u.subproject != "apps/cli"),
            "the curated subproject must NOT be reported: {report:?}"
        );
    }

    /// Only curated blocks ⇒ ok, and the shape is byte-stable across runs.
    #[test]
    fn build_report_all_curated_is_ok_and_stable() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_census(root);
        seed_curated(root, "apps/cli");

        let a = build_report(root);
        let b = build_report(root);
        assert!(a.ok, "every block curated ⇒ ok: {a:?}");
        assert_eq!(a.total_uncurated, 0);
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap(),
            "byte-stable"
        );
    }

    /// The workspace-root `CLAUDE.md` is never an enrich unit — Wave 1 does not
    /// seed a pending block there, and a stray one must not be reported.
    #[test]
    fn root_claude_md_is_not_a_subproject() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_census(root);
        std::fs::write(
            root.join("CLAUDE.md"),
            format!("# Root\n\n## Guards\n\n{GUARDS_PENDING_OPEN}\n{GUARDS_CLOSE}\n"),
        )
        .unwrap();

        let report = build_report(root);
        assert!(report.ok, "the root unit must not be reported: {report:?}");
    }

    /// No census ⇒ silent no-op, even with an uncurated scaffold on disk.
    #[test]
    fn run_without_census_is_silent_noop() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_pending(root, "apps/rt", "cargo");
        assert!(run(root).is_none(), "absent grain.model.json ⇒ silent no-op");
    }

    /// With a census, `run` returns the same report `build_report` builds.
    #[test]
    fn run_with_census_returns_the_report() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_census(root);
        seed_pending(root, "apps/rt", "cargo");
        let report = run(root).expect("a census makes the check judgeable");
        assert_eq!(report, build_report(root));
    }
}

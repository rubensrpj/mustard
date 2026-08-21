//! `enrichment_gap` — measure the AGENT-WRITTEN half of the census, and say
//! in ONE stderr line how much of it is still missing. One line per
//! pipeline-opening emit, for as long as the gap stands — not once per session:
//! a notice that fell silent while the gap persisted would read as "closed".
//!
//! The scan has two halves. The DETERMINISTIC half is `grain.model.json`, and
//! [`super::base_gate::refresh_census_if_stale`] already re-mines it from this
//! very gate whenever it goes stale. The ENRICHED half is the one only an agent
//! can write — each subproject's `## Guards` prose and the `{role}-pattern`
//! molds — and until now nothing measured it at all: a subproject installed
//! with a pending Guards scaffold kept that scaffold through any number of
//! fresh censuses, because the model's freshness says nothing about whether the
//! prose was ever authored.
//!
//! So the measurement is deliberately NOT conditioned on the refresh: it runs
//! every time the gate opens, not only when the census was actually re-mined.
//! Tying it to the re-mine would hide the most common case — a gap born at
//! install time, which no amount of fresh census ever moves.
//!
//! ## Decide, then print
//!
//! [`measure`] is pure: it reads, it never prints, and it returns an
//! [`EnrichmentGap`]. [`report_if_stale`] is the whole effect. The split mirrors
//! [`super::base_gate::census_refresh_due`] vs `refresh_census_if_stale` in the
//! sibling module, and it is what makes the judgement testable with no output to
//! capture.
//!
//! ## One line, on stderr
//!
//! Never stdout: this runs inside `emit-pipeline`, whose single JSON line is
//! byte-compared by gates — the same reason the census-refresh notice next door
//! writes to stderr.
//!
//! ## Reporting is where this module stops
//!
//! Authoring Guards and molds REWRITES versioned files, so closing the gap needs
//! a clean tree — the premise [`crate::hooks::write::scan_clean_gate`] refuses to
//! let a rewrite skip — and a commit it can keep to itself. It is therefore a
//! work unit of its OWN, dispatched by the flow once the current unit closes,
//! and not something a gate opening a different unit may start.
//!
//! Fail-open throughout: no census, an unreadable directory, an unparseable
//! model — each is an EMPTY gap and a silent return, never an error, never a
//! panic.

use std::path::Path;

use crate::commands::scan::default_model_path;

/// The literal every enrichment-stale line opens with.
///
/// Exported as ONE constant so the seeded prose that teaches the flow what to do
/// with this line and the code that emits it can be locked to the same text by a
/// single test. A literal typed twice in two files is precisely what lets the two
/// halves drift apart in silence.
pub const ENRICHMENT_STALE_TAG: &str = "base-gate: enrichment stale";

/// How many names the line spells out per half before it summarises the rest.
/// Enough to recognise WHICH gap this is; short enough that a workspace with
/// forty pending molds still emits one readable line.
const MAX_NAMED: usize = 3;

/// Where the enrichment would WRITE, per half — never printed, only measured.
/// Kept apart from [`EnrichmentGap`] because the two answer different
/// questions: that one is WHAT is missing, this one is WHERE closing it lands,
/// and only the second decides what the pass costs.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct EnrichmentGapPaths {
    /// Where the Guards half would write — EVERY pending subproject's
    /// instruction file, because the pass splices every one of them.
    pub(crate) guards: Vec<String>,
    /// Where the mold half would write. Every proposed mold, because the pass
    /// sweeps and re-authors all of them: one tracked mold makes the whole pass
    /// a rewrite of versioned files.
    pub(crate) molds: Vec<String>,
}

/// What the agent-written half of the census is still missing.
///
/// Two independent halves, kept apart rather than summed: a project can be
/// fully guarded with no mold authored, or the reverse, and the line names the
/// half that is actually behind.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct EnrichmentGap {
    /// Subproject directories (forward-slashed, root-relative) whose `## Guards`
    /// block is still the pending scaffold. Sorted and deduplicated.
    pub(crate) pending_guards: Vec<String>,
    /// Mold slugs the census proposes that no agent has authored — already
    /// excluding a mold present on disk and a slug the agent declined. Sorted
    /// and deduplicated.
    pub(crate) missing_molds: Vec<String>,
}

impl EnrichmentGap {
    /// `true` when there is nothing to report — both halves are complete, or
    /// nothing could be measured. The reporter's only question.
    pub(crate) fn is_empty(&self) -> bool {
        self.pending_guards.is_empty() && self.missing_molds.is_empty()
    }
}

/// Measure the enrichment gap under `project` (the state root holding
/// `mustard.json` and `.claude/`). PURE: reads only, prints nothing, and every
/// failure degrades to an empty gap.
///
/// Neither half opens a traversal of its own. Pending Guards come from the ONE
/// walk `scan-guards-list` performs and `doctor --check guards-scaffold` already
/// reuses; missing molds come from the worklist `scan-patterns-list` projects
/// off the model. A third copy of either would drift from the other two exactly
/// the way silent copies do — the reason those two are already shared.
pub(crate) fn measure(project: &Path) -> EnrichmentGap {
    measure_with_targets(project).0
}

/// [`measure`] plus WHERE closing the gap would land, from the SAME single pass
/// over both collectors.
///
/// One pass because both are real work — `scan-patterns-list` alone measured
/// 30-40 ms on this repository — and this runs on every pipeline-opening emit.
/// Reading the paths from the same entries that produced the gap also makes it
/// impossible for the two to disagree about which subprojects are pending.
pub(crate) fn measure_with_targets(project: &Path) -> (EnrichmentGap, EnrichmentGapPaths) {
    // No Wave-1 census ⇒ nothing ever seeded a Guards scaffold and no cluster
    // was ever proposed, so "the enrichment is behind" would be a claim about a
    // pass that never ran. Same silence `doctor --check guards-scaffold` keeps
    // for the same reason.
    if !default_model_path(project).is_file() {
        return (EnrichmentGap::default(), EnrichmentGapPaths::default());
    }
    // The Guards file name is resolved by the ONE helper `collect_pending` uses
    // for the same question — under a private install that is
    // `CLAUDE.local.md`, and asking about `CLAUDE.md` instead would measure a
    // file the pass never touches.
    let owned = crate::shared::context::guards_file_name(project);
    let pending = crate::commands::scan_guards::list::collect_pending(project).entries;
    // EVERY pending subproject, never just the first. The pass splices all of
    // them, so one of them being tracked makes the pass a rewrite of versioned
    // files — exactly the rule the mold half already followed. Probing only the
    // first made the advice depend on SORT ORDER: an ignored subproject that
    // happened to sort first flipped the whole prescription, and the line then
    // sent an agent to rewrite a tracked `CLAUDE.md` in place, with no clean
    // tree and no commit.
    let guards: Vec<String> =
        pending.iter().map(|p| format!("{}/{owned}", p.subproject)).collect();
    let mut pending_guards: Vec<String> =
        pending.into_iter().map(|pending| pending.subproject).collect();
    pending_guards.sort();
    pending_guards.dedup();

    let candidates = crate::commands::scan_patterns::list::collect(project);
    let molds: Vec<String> = candidates.iter().map(|c| c.mold_path.clone()).collect();
    let mut missing_molds: Vec<String> = candidates.into_iter().map(|c| c.slug).collect();
    missing_molds.sort();
    missing_molds.dedup();

    (EnrichmentGap { pending_guards, missing_molds }, EnrichmentGapPaths { guards, molds })
}

/// Print the one-line notice when [`measure`] finds a gap, and nothing at all
/// when it does not. The whole effect of this module.
///
/// stderr, never stdout — see the module doc.
pub(crate) fn report_if_stale(project: &Path) {
    let (gap, targets) = measure_with_targets(project);
    if gap.is_empty() {
        return;
    }
    eprintln!("{}", gap_line(&gap, enrichment_is_versioned(project, &targets)));
}

/// Whether the pass would rewrite anything the host repository VERSIONS.
///
/// **Asked of every half, and ANY versioned target is enough.** The pass has
/// two, and they can differ: under this project's own install the Guards go to
/// an excluded `CLAUDE.local.md` while 37 `{role}-pattern` molds are tracked —
/// and the mold sweep DELETES and re-authors every one of them. Measuring only
/// the Guards half announced "rewrites nothing versioned" while prescribing a
/// pass that rewrites 37 tracked files, which is the more dangerous direction
/// of the two errors.
///
/// Unmeasured counts as VERSIONED — the stricter advice costs a needless unit
/// at worst, where the opposite would send someone to rewrite versioned files
/// in place.
fn enrichment_is_versioned(project: &Path, targets: &EnrichmentGapPaths) -> bool {
    let visible = |rel: &String| !super::base_gate::path_is_ignored(project, rel);
    targets.guards.iter().any(visible) || targets.molds.iter().any(visible)
}

/// Render the notice for a NON-empty gap. Split from [`report_if_stale`] so the
/// wording is testable without capturing a stream, and deterministic: both
/// halves arrive sorted, so the same gap always renders the same bytes.
///
/// `versioned` chooses the PRESCRIPTION, never the gap: the missing prose is
/// missing either way, and only what it costs to write differs.
fn gap_line(gap: &EnrichmentGap, versioned: bool) -> String {
    let mut clauses: Vec<String> = Vec::new();
    if !gap.pending_guards.is_empty() {
        let count = gap.pending_guards.len();
        let noun = if count == 1 { "subproject" } else { "subprojects" };
        clauses.push(format!(
            "{count} {noun} on the pending ## Guards scaffold ({})",
            named(&gap.pending_guards)
        ));
    }
    if !gap.missing_molds.is_empty() {
        let count = gap.missing_molds.len();
        let noun = if count == 1 { "mold" } else { "molds" };
        clauses.push(format!(
            "{count} {noun} with no author ({})",
            named(&gap.missing_molds)
        ));
    }
    // The prescription, and ONLY the prescription, follows the measurement.
    // Saying "own unit, clean tree, dispatch it later" where the pass writes
    // nothing git tracks asked the operator to REMEMBER to schedule something
    // that needs no scheduling — a notice that reappears forever and can only
    // be answered by a chore that was never owed.
    let prescription = if versioned {
        "the enrich pass rewrites versioned files, so it is a work unit of its OWN on a clean \
         tree — dispatch it once the current unit closes"
    } else {
        "this install hides the enrich pass's output from git, so it rewrites nothing versioned \
         — no unit, no clean tree, no commit: dispatch it right here, now"
    };
    format!("{ENRICHMENT_STALE_TAG} — {}; {prescription}", clauses.join(" and "))
}

/// The first [`MAX_NAMED`] names, then how many were left unsaid. Never the
/// whole list: the point is to identify the gap, not to reproduce the worklist
/// the enrich unit will read for itself.
fn named(names: &[String]) -> String {
    let head = names.iter().take(MAX_NAMED).cloned().collect::<Vec<_>>().join(", ");
    match names.len().saturating_sub(MAX_NAMED) {
        0 => head,
        rest => format!("{head}, +{rest} more"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::scan_claude::{GUARDS_CLOSE, GUARDS_PENDING_OPEN};

    /// Write `<root>/.claude/grain.model.json` — the census whose presence makes
    /// the gap a judgeable question at all.
    fn write_model(root: &Path, json: &str) {
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        std::fs::write(root.join(".claude").join("grain.model.json"), json).unwrap();
    }

    /// Seed `<root>/<subproject>/CLAUDE.md` with an UNCURATED (pending) block —
    /// exactly what Wave 1 writes and Wave 2 is supposed to replace.
    fn seed_pending_guards(root: &Path, subproject: &str) {
        let dir = root.join(subproject);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("CLAUDE.md"),
            format!(
                "# Sub\n\n## Guards\n\n{GUARDS_PENDING_OPEN}\n\
                 <!-- facts: kind=cargo; frameworks=(none) -->\n{GUARDS_CLOSE}\n"
            ),
        )
        .unwrap();
    }

    /// A mold candidate the census proposes and no agent has authored is part of
    /// the gap — the half that lives entirely in the model, with no `CLAUDE.md`
    /// anywhere on disk.
    #[test]
    fn counts_molds_with_no_author() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_model(
            root,
            r#"{
              "projects": [{"name":"api","dir":"apps/api"}],
              "roles": [{"affix":"Service","kind":"suffix","count":5,"common_dir":"apps/api/services","decl_kind":"class"}],
              "modules": [
                {"path":"apps/api/services/UserService.ts"},
                {"path":"apps/api/services/OrderService.ts"}
              ]
            }"#,
        );

        let gap = measure(root);
        assert_eq!(gap.missing_molds, vec!["api-service".to_string()], "{gap:?}");
        assert!(gap.pending_guards.is_empty(), "no CLAUDE.md on disk to be pending: {gap:?}");
        assert!(!gap.is_empty(), "an unauthored mold IS the gap");
        let line = gap_line(&gap, true);
        assert!(line.starts_with(ENRICHMENT_STALE_TAG), "the line carries the tag: {line}");
        assert!(line.contains("api-service"), "and names what is missing: {line}");
    }

    /// A subproject whose `## Guards` is still the scaffold is NAMED, and a
    /// census that proposes no cluster contributes no mold — so the two halves
    /// are measured apart, not summed.
    #[test]
    fn names_a_subproject_whose_guards_are_still_a_scaffold() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_model(root, "{}"); // a census with no role to propose a mold for
        seed_pending_guards(root, "apps/rt");

        let gap = measure(root);
        assert_eq!(gap.pending_guards, vec!["apps/rt".to_string()], "{gap:?}");
        assert!(gap.missing_molds.is_empty(), "an empty model proposes no mold: {gap:?}");
        let line = gap_line(&gap, true);
        assert!(line.contains("apps/rt"), "the line names the subproject: {line}");
        assert!(
            line.contains("work unit of its OWN"),
            "and says closing it is a unit of its own: {line}"
        );
        assert!(!line.contains('\n'), "exactly one line — gates read stderr too: {line}");
    }

    /// AC-4 — the MEASUREMENT, driven against a real repository instead of a
    /// hardcoded bool. One tracked target among several ignored ones must keep
    /// the strict prescription.
    ///
    /// This is the case that shipped twice. First the Guards half was not
    /// measured at all; then it was measured by `.first()` alone, so an ignored
    /// subproject that happened to SORT FIRST flipped the whole advice and the
    /// line sent an agent to rewrite a tracked instruction file in place — the
    /// one thing `scan_clean_gate` exists to refuse. Both slipped through
    /// because every test passed the deciding bool in by hand.
    #[test]
    fn one_tracked_target_among_ignored_ones_keeps_the_strict_advice() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "--initial-branch=dev"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        // `aignored/` sorts BEFORE `apps/` — the ordering that produced the bug.
        std::fs::write(root.join(".gitignore"), "aignored/\n").unwrap();
        write_model(root, "{}");
        seed_pending_guards(root, "aignored/sub");
        seed_pending_guards(root, "apps/rt");
        git(root, &["add", "-A"]);
        git(root, &["commit", "-m", "seed"]);

        let (gap, targets) = measure_with_targets(root);
        assert_eq!(
            gap.pending_guards,
            vec!["aignored/sub".to_string(), "apps/rt".to_string()],
            "both subprojects are pending: {gap:?}",
        );
        assert_eq!(targets.guards.len(), 2, "every pending target is measured: {targets:?}");
        assert!(
            enrichment_is_versioned(root, &targets),
            "apps/rt's instruction file is tracked, so the pass DOES rewrite versioned \
             files — whatever sorts first: {targets:?}",
        );
        assert!(
            gap_line(&gap, enrichment_is_versioned(root, &targets))
                .contains("work unit of its OWN"),
            "and the line must keep asking for a unit",
        );
    }

    /// The other side of the same measurement: when every target really is
    /// hidden, the relaxed advice is the honest one.
    #[test]
    fn every_target_ignored_relaxes_the_advice() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "--initial-branch=dev"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        std::fs::write(root.join(".gitignore"), "**/CLAUDE.md\n").unwrap();
        write_model(root, "{}");
        seed_pending_guards(root, "apps/rt");
        git(root, &["add", "-A"]);
        git(root, &["commit", "-m", "seed"]);

        let (gap, targets) = measure_with_targets(root);
        assert!(!gap.is_empty(), "the gap is real: {gap:?}");
        assert!(
            !enrichment_is_versioned(root, &targets),
            "every target is hidden, so nothing versioned is rewritten: {targets:?}",
        );
    }

    fn git(root: &Path, args: &[&str]) {
        let _ = std::process::Command::new("git").args(args).current_dir(root).output();
    }

    /// AC-1 — where the enrichment's output is hidden from git, the notice must
    /// not ask for a unit, a clean tree or a commit. None of the three is real
    /// there: the pass rewrites nothing versioned, so it can run on the spot.
    ///
    /// This is the failure the line had in the field. It reappeared at every
    /// unit opening, forever, asking the operator to REMEMBER to schedule work
    /// that needed no scheduling — and the Guards it named were written inline
    /// in one pass, with no branch and no commit, exactly as this wording now
    /// says they can be.
    #[test]
    fn a_hidden_enrichment_asks_for_no_ceremony() {
        let gap = EnrichmentGap {
            pending_guards: vec!["apps/rt".to_string()],
            missing_molds: Vec::new(),
        };
        let line = gap_line(&gap, false);

        assert!(line.contains("apps/rt"), "the gap is still named: {line}");
        assert!(
            line.ends_with(
                "this install hides the enrich pass's output from git, so it rewrites nothing \
                 versioned — no unit, no clean tree, no commit: dispatch it right here, now"
            ),
            "the prescription must be to run it on the spot: {line}",
        );
        // The demanding phrases, matched whole. A bare `contains("clean tree")`
        // would trip on this very sentence's own "no clean tree" — the negative
        // has to name what the line would DEMAND, not a word it may mention.
        for ceremony in ["work unit of its OWN", "once the current unit closes"] {
            assert!(
                !line.contains(ceremony),
                "{ceremony:?} is false when nothing versioned is rewritten: {line}",
            );
        }
        assert!(!line.contains('\n'), "exactly one line — gates read stderr too: {line}");
    }

    /// AC-2 — and where the output IS versioned nothing moves: the pass really
    /// does rewrite files the repository tracks, so every requirement in the
    /// original wording stands, word for word.
    #[test]
    fn a_versioned_enrichment_still_asks_for_its_own_unit() {
        let gap = EnrichmentGap {
            pending_guards: Vec::new(),
            missing_molds: vec!["api-service".to_string()],
        };
        let line = gap_line(&gap, true);

        assert!(
            line.ends_with(
                "the enrich pass rewrites versioned files, so it is a work unit of its OWN on \
                 a clean tree — dispatch it once the current unit closes"
            ),
            "the versioned wording must not have drifted: {line}",
        );
        assert!(line.contains("api-service"), "and it still names the gap: {line}");
    }

    /// Without a census the pass that seeds scaffolds never ran, so the gap is
    /// EMPTY and silent — a pending block on disk does not make it otherwise,
    /// and an absent `.claude/` never panics.
    #[test]
    fn no_census_means_an_empty_gap() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        seed_pending_guards(root, "apps/rt");

        let gap = measure(root);
        assert!(gap.is_empty(), "no grain.model.json ⇒ nothing measurable: {gap:?}");
        // And the reporter prints nothing for it — fail-open all the way out.
        report_if_stale(root);
    }
}

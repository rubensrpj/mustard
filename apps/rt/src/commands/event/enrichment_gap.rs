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
    // No Wave-1 census ⇒ nothing ever seeded a Guards scaffold and no cluster
    // was ever proposed, so "the enrichment is behind" would be a claim about a
    // pass that never ran. Same silence `doctor --check guards-scaffold` keeps
    // for the same reason.
    if !default_model_path(project).is_file() {
        return EnrichmentGap::default();
    }
    let mut pending_guards: Vec<String> =
        crate::commands::scan_guards::list::collect_pending(project)
            .entries
            .into_iter()
            .map(|pending| pending.subproject)
            .collect();
    pending_guards.sort();
    pending_guards.dedup();

    let mut missing_molds: Vec<String> = crate::commands::scan_patterns::list::collect(project)
        .into_iter()
        .map(|candidate| candidate.slug)
        .collect();
    missing_molds.sort();
    missing_molds.dedup();

    EnrichmentGap { pending_guards, missing_molds }
}

/// Print the one-line notice when [`measure`] finds a gap, and nothing at all
/// when it does not. The whole effect of this module.
///
/// stderr, never stdout — see the module doc.
pub(crate) fn report_if_stale(project: &Path) {
    let gap = measure(project);
    if gap.is_empty() {
        return;
    }
    eprintln!("{}", gap_line(&gap));
}

/// Render the notice for a NON-empty gap. Split from [`report_if_stale`] so the
/// wording is testable without capturing a stream, and deterministic: both
/// halves arrive sorted, so the same gap always renders the same bytes.
fn gap_line(gap: &EnrichmentGap) -> String {
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
    format!(
        "{ENRICHMENT_STALE_TAG} — {}; the enrich pass rewrites versioned files, so it is a \
         work unit of its OWN on a clean tree — dispatch it once the current unit closes",
        clauses.join(" and "),
    )
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
        let line = gap_line(&gap);
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
        let line = gap_line(&gap);
        assert!(line.contains("apps/rt"), "the line names the subproject: {line}");
        assert!(
            line.contains("work unit of its OWN"),
            "and says closing it is a unit of its own: {line}"
        );
        assert!(!line.contains('\n'), "exactly one line — gates read stderr too: {line}");
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

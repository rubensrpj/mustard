//! `base-candidates` — the branches a unit could be cut from, as git has them.
//!
//! ## Scope (ONE behavior)
//!
//! Answers the question the dispatch asks the operator: *"sai de qual?"* It
//! fetches, lists every branch on `origin` newest-commit-first, and marks the
//! two things a picker needs to render a row without asking anything else —
//! which branch refuses a direct commit, and which one the cursor opens on.
//!
//! It exists because that question used to have a hardcoded answer. The two
//! options came from `mustard.json#git.flow`, written once at `mustard init`,
//! so a branch cut after the install was not offered and was refused if typed.
//! In a client repository — where the operator does not own the naming and new
//! integration lines appear weekly — the list was wrong by the second week.
//!
//! ## What it does NOT do
//!
//! It does not choose, and it does not refuse. Validation of an explicit
//! `--base` belongs to [`super::work_branch::resolve_kind_base`], which the
//! opening door already calls; duplicating it here would give the project two
//! places to disagree about what a base is. This command only reports.
//!
//! ## Contracts honoured
//!
//! - Output is deterministic and byte-stable: no timestamps of its own, no
//!   absolute paths, ordering fixed by git's `--sort=-committerdate` with the
//!   branch name breaking ties. The `committedAt` field IS a clock reading, but
//!   it is the repository's, not this run's, so the same repository renders the
//!   same bytes twice.
//! - Never panics and never blocks: no git, no repository, or no remote answers
//!   an EMPTY list with `measured:false`, and the caller reads that as "ask the
//!   operator without a menu", never as "this repository has no branches".

use serde::Serialize;

use crate::shared::context::project_dir;

/// One row of the picker.
///
/// Mirrors [`mustard_core::BranchEntry`] rather than re-deriving it: the
/// catalogue is mined in `core` (where `protected_branches` also lives, so the
/// two marks cannot drift), and this type exists only to fix the JSON field
/// names the orchestrator reads.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseCandidateEntry {
    /// Branch name with no `origin/` prefix.
    pub name: String,
    /// Unix seconds of the branch tip's commit — what the ordering follows, and
    /// what lets a picker show a stale line as stale.
    pub committed_at: i64,
    /// `true` when a direct commit here is refused.
    pub protected: bool,
    /// `true` when `git.flow` names it: where the cursor opens. A hint about
    /// the default, never a restriction on the rest.
    pub preselected: bool,
}

/// The command's JSON document.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseCandidatesReport {
    /// `false` when git could not be asked at all. The distinction matters:
    /// an unmeasured repository and one with no branches are the same empty
    /// list, and only one of them means "do not trust this menu".
    pub measured: bool,
    /// Whether the remote was refreshed before listing.
    pub fetched: bool,
    /// The branch the picker opens on — `git.flow`'s primary base. Present even
    /// when it is not in `branches`, because a project may declare a base it
    /// has not pushed yet.
    pub primary: String,
    /// The rows, newest commit first.
    pub branches: Vec<BaseCandidateEntry>,
}

/// Mine the catalogue for `root`, refreshing the remote when `fetch`.
#[must_use]
pub fn report_at(root: &std::path::Path, fetch: bool) -> BaseCandidatesReport {
    let config = mustard_core::ProjectConfig::load(root);
    let branches: Vec<BaseCandidateEntry> = mustard_core::branch_catalog(root, &config.git, fetch)
        .into_iter()
        .map(|b| BaseCandidateEntry {
            name: b.name,
            committed_at: b.committed_at,
            protected: b.protected,
            preselected: b.preselected,
        })
        .collect();
    BaseCandidatesReport {
        measured: !branches.is_empty(),
        fetched: fetch,
        primary: config.git.primary_base(),
        branches,
    }
}

/// The `run base-candidates` face.
pub fn run(no_fetch: bool) {
    let root = project_dir();
    let report = report_at(std::path::Path::new(&root), !no_fetch);
    println!(
        "{}",
        serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into())
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::process::Command;

    fn git(root: &Path, args: &[&str]) -> bool {
        Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// A repository whose remote default is `producao` and which also carries a
    /// line no configuration ever mentioned.
    fn fixture(root: &Path) -> bool {
        if !git(root, &["init", "-q", "-b", "producao", "."]) {
            return false;
        }
        let _ = git(root, &["config", "user.email", "t@t.t"]);
        let _ = git(root, &["config", "user.name", "t"]);
        if !git(root, &["commit", "-q", "--allow-empty", "-m", "seed"]) {
            return false;
        }
        for branch in ["producao", "release/2026-Q3"] {
            let _ = git(root, &["update-ref", &format!("refs/remotes/origin/{branch}"), "HEAD"]);
        }
        git(
            root,
            &["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/producao"],
        )
    }

    #[test]
    fn the_menu_offers_branches_no_configuration_declared() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        if !fixture(root) {
            return; // no usable git here
        }
        std::fs::write(root.join("mustard.json"), r#"{"git":{"flow":{"*":"producao"}}}"#).unwrap();

        // `fetch:false` — the fixture has no reachable remote, and the listing
        // must not depend on one.
        let report = report_at(root, false);
        assert!(report.measured, "git answered, so the menu is trustworthy");
        assert_eq!(report.primary, "producao", "the cursor opens on the declared base");

        let names: Vec<&str> = report.branches.iter().map(|b| b.name.as_str()).collect();
        assert!(
            names.contains(&"release/2026-Q3"),
            "a line no configuration mentions is still offered: {names:?}",
        );
        assert!(!names.contains(&"HEAD"), "origin/HEAD is a pointer, not a row: {names:?}");

        let producao = report.branches.iter().find(|b| b.name == "producao").expect("listed");
        assert!(producao.protected, "the remote's default refuses a direct commit");
        assert!(producao.preselected, "and git.flow names it");
        let release =
            report.branches.iter().find(|b| b.name == "release/2026-Q3").expect("listed");
        assert!(!release.protected, "an ordinary line is writable");
        assert!(!release.preselected, "and being undeclared does not exclude it from the menu");
    }

    /// An unmeasurable directory answers an empty menu that SAYS it is empty
    /// for lack of an answer — never a menu that looks complete.
    #[test]
    fn an_unmeasurable_repository_says_so_instead_of_offering_nothing() {
        let dir = tempfile::tempdir().unwrap(); // never a repository
        let report = report_at(dir.path(), false);
        assert!(!report.measured, "the caller must not read this as 'no branches'");
        assert!(report.branches.is_empty());
    }
}

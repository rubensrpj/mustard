//! `pr_qa_gate` — the QA ↔ integration coupling advisory.
//!
//! ## Scope (ONE behavior)
//!
//! A `PreToolUse(Bash)` stage that warns when a `gh pr create` / `gh pr merge`
//! is issued for a spec whose acceptance criteria have not passed.
//!
//! ## Why it exists
//!
//! The pipeline (`ANALYZE→…→QA→CLOSE`) and the `/git` flow are two independent
//! subsystems: `git.md` never mentions QA or the spec, and the close/resume
//! docs never mention the PR or the merge. The canonical order does run QA
//! BEFORE integration — `close-pipeline` fires while the unit is still live on
//! its work branch — but **no gate ever tied the two together**, so nothing
//! stopped an operator from merging first and only meeting QA at close, which
//! integrates unverified work. This stage is the missing coupling, raised at
//! the exact moment integration is requested.
//!
//! ## Advisory, never blocking
//!
//! It answers `Warn`, never `Deny`. The ledger invariant is already
//! hard-enforced downstream — `emit-pipeline` refuses `pipeline.complete`
//! without a passing `qa.result`, and the CLOSE gate refuses the transition —
//! so a veto here would only add a second, redundant block over a legitimately
//! early PR (work stays live on the branch and the SAME PR is updated until
//! `pr close`).
//!
//! ## Fail-open invariant
//!
//! A non-PR command, no spec bound to the session, or an unreadable event log
//! all answer `None` (pass through). The stage only speaks on a positive
//! observation: a PR command + a known spec + no passing QA.
//!
//! ## Shape (which mold applies)
//!
//! Modelled on [`super::review_gate`], the Bash family's other state-reading
//! stage — NOT on the `*_redirect` stages (`rt-redirect-pattern` mandates a
//! pure string function that never probes the filesystem, and this stage must
//! read the spec's event log) and NOT on `rt-gate-pattern` (that mold describes
//! `*Gate` structs implementing `Check` under `hooks/write/`; a Bash-chain
//! stage is a `pub(super) fn … -> Option<Verdict>` called by
//! `bash_command_gate`, which owns the single `Check` impl for the family).

use std::path::Path;

use mustard_core::domain::model::contract::Verdict;

use super::pr_detect::classify_pr;

/// Warn when a PR is opened/merged for a spec with no passing `qa.result`.
///
/// `None` = pass through. Reuses [`classify_pr`] (the same conservative
/// `gh pr` classifier the DORA telemetry uses, `rtk`-wrapper tolerant) and
/// [`crate::commands::event::emit_pipeline::qa_result_passed`] (the same
/// single source of truth the `pipeline.complete` hard gate consults), so the
/// advisory can never disagree with the gate that actually blocks.
pub(super) fn pr_qa_gate(command: &str, cwd: &str) -> Option<Verdict> {
    let kind = classify_pr(command)?;
    let spec = crate::shared::context::current_spec(cwd)?;
    if crate::commands::event::emit_pipeline::qa_result_passed(Path::new(cwd), &spec) {
        return None;
    }
    let moment = if kind == "pr.merged" {
        "Merging"
    } else {
        "Opening a PR for"
    };
    Some(Verdict::Warn {
        message: format!(
            "[qa-coupling] {moment} `{spec}` — no `qa.result` with overall=pass exists yet. \
             The canonical order runs QA BEFORE integration (close-pipeline fires while the unit \
             is still live on its work branch). Run `/mustard:qa --spec {spec}` first, or accept \
             that this integrates unverified work — CLOSE will refuse the spec until QA passes."
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn non_pr_command_passes_through() {
        let dir = tempdir().unwrap();
        let cwd = dir.path().to_str().unwrap();
        assert!(pr_qa_gate("cargo test --workspace", cwd).is_none());
        assert!(pr_qa_gate("rtk git push", cwd).is_none());
        // `gh` without the `pr <verb>` shape is not an integration moment.
        assert!(pr_qa_gate("gh repo view", cwd).is_none());
    }

    #[test]
    fn pr_command_without_an_active_spec_passes_through() {
        // Fail-open: nothing to verify when no spec is bound to the session
        // (a docs-only or spec-less PR must never be nagged).
        let dir = tempdir().unwrap();
        let cwd = dir.path().to_str().unwrap();
        assert!(pr_qa_gate("rtk gh pr create --base dev --fill", cwd).is_none());
        assert!(pr_qa_gate("rtk gh pr merge 80 --squash", cwd).is_none());
    }

    #[test]
    fn classifier_is_shared_with_the_dora_telemetry() {
        // The advisory must fire on exactly what `pr_detect` calls a PR event —
        // including the `rtk` wrapper — so the two can never drift apart.
        assert_eq!(classify_pr("rtk gh pr create --fill"), Some("pr.opened"));
        assert_eq!(classify_pr("gh pr merge 80"), Some("pr.merged"));
        assert_eq!(classify_pr("gh pr view 80"), None);
    }
}

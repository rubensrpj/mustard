// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::uninlined_format_args)]

//! Two instructions that could not both be obeyed, and one decision taken off a
//! filtered log.
//!
//! The `/git` ritual ordered the parent to stage a submodule's moved gitlink
//! unconditionally and MANDATORILY — while its own PR section, twenty lines
//! down, explained why that pointer is wrong until the submodule PR merges. An
//! operator following the first sentence recorded a work-branch SHA that exists
//! nowhere on the submodule's base; an operator following the second left the
//! parent dirty and read that dirt as a missed step. The ritual could not be
//! followed correctly by anyone.
//!
//! The second rule here comes from a measurement: `rtk` filters `git log` and
//! DROPS merge commits, so a range holding only merges renders as a lone
//! newline through the wrapper — byte-indistinguishable from "nothing here".
//! A sweep that asks "any commits left?" before offering a deletion therefore
//! reads an empty answer over commits that exist.
//!
//! Both tests read BOTH halves and require them to agree:
//!
//! 1. the new instruction is PRESENT, at the place a reader actually arrives at
//!    it — inside the step or the rules block, not merely somewhere in the file;
//!    and
//! 2. the superseded instruction is GONE (test 1), or the code that takes the
//!    destructive decision really reads the plumbing the rule names (test 2).
//!
//! Half 2 is what keeps these from being spell-checks. A presence assertion
//! alone passes forever once the sentence is written — including over a file
//! that still carries the contradicting order right beside it, which is exactly
//! the state this spec found.
//!
//! They live in `tests/` rather than in-file because each acceptance criterion
//! runs `cargo test -p mustard-rt --test git_prose_rules <fn>` and libtest
//! matches the FULL test path — which equals the bare function name only at the
//! root of an integration-test binary.

use std::path::{Path, PathBuf};

/// The repository root — two levels up from this crate's manifest.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Read a repo-relative file, failing with the path when it is missing.
fn read(rel: &str) -> String {
    let path = repo_root().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{rel} unreadable at {}: {e}", path.display()))
}

/// The first line of `body` containing `needle`, or `None`.
fn line_with<'a>(body: &'a str, needle: &str) -> Option<&'a str> {
    body.lines().find(|l| l.contains(needle))
}

/// The slice of `body` under the markdown `heading`, up to the next same-or-
/// higher heading. Anchoring matters: a rule that drifted into an unrelated
/// section is a rule the reader of THAT step never meets.
fn section<'a>(body: &'a str, heading: &str) -> &'a str {
    let start = body
        .find(heading)
        .unwrap_or_else(|| panic!("the document no longer carries the heading `{heading}`"));
    let rest = &body[start + heading.len()..];
    let end = rest.find("\n## ").unwrap_or(rest.len());
    &rest[..end]
}

/// AC-6 — the gitlink stage is conditioned on reachability, the pending state
/// is named, the bump step exists, and the parent PR opens as a draft while a
/// submodule PR is open.
///
/// The four are one criterion because they are one mechanism: the parent stops
/// recording an unreachable pointer (condition), says so out loud instead of
/// looking dirty (`[pending-bump]`), records it later when it becomes reachable
/// (bump), and cannot merge past that gap in the meantime (draft). Assert any
/// three and the fourth's absence turns the ritual back into two instructions
/// that contradict each other.
#[test]
fn git_prose_conditions_gitlink_on_reachability() {
    let git_md = read("plugin/commands/git.md");
    let refs = read("plugin/refs/git/submodule-rules.md");

    // --- 1a. The commit step asks git, instead of ordering the stage --------
    let commit_step = line_with(&git_md, "- **commit** —")
        .expect("the procedure no longer spells out the `commit` action");
    assert!(
        commit_step.contains("merge-base --is-ancestor"),
        "the commit step stages the gitlink without asking whether that SHA is \
         reachable from the submodule's base: {commit_step}",
    );
    assert!(
        commit_step.contains("origin/$SUB_BASE"),
        "the reachability question must be asked against the SUBMODULE's base, \
         which is the ref the submodule PR lands on: {commit_step}",
    );
    // Naming the condition is not enough — the operator has to be told what the
    // leftover line MEANS, or an unstaged pointer reads as forgotten work.
    assert!(
        commit_step.contains("[pending-bump]"),
        "the not-reachable branch leaves ` M <sub>` behind without naming it: {commit_step}",
    );

    // --- 1b. The superseded order is GONE ----------------------------------
    // Without this half every assertion above passes over a file that still
    // carries the contradicting sentence twenty lines away — the exact state
    // the field report found.
    assert!(
        !git_md.contains("moved gitlink in the parent**"),
        "`/git` still carries the unconditional order to stage the moved gitlink",
    );
    assert!(
        !refs.contains("gitlink step (MANDATORY)"),
        "the submodule ref still titles the gitlink step MANDATORY, so the \
         conditioned version is advice sitting under an order",
    );
    // The reference's own code block must ask before it stages: the `git add`
    // may only appear AFTER the reachability test, never before it.
    let commit_section = section(&refs, "## Commit: submodule steps");
    let asks_at = commit_section.find("merge-base --is-ancestor").unwrap_or_else(|| {
        panic!("the reference's commit section never tests reachability")
    });
    let stages_at = commit_section
        .find("rtk git add -- \"<SUB_PATH>\"")
        .expect("the reference's commit section no longer shows the gitlink stage");
    assert!(
        stages_at > asks_at,
        "the reference stages the gitlink before testing reachability \
         (stage at {stages_at}, test at {asks_at})",
    );

    // --- 1c. The bump step exists, and the close ritual runs it -------------
    // A conditioned stage without a later bump would simply never record the
    // pointer: the condition would trade a wrong pointer for no pointer.
    assert!(
        refs.contains("### The bump step"),
        "nothing tells the operator how the deferred pointer is ever recorded",
    );
    let close_step = line_with(&git_md, "- **finish** —")
        .expect("the procedure no longer spells out `finish` (the exit ritual, once `pr close`)");
    assert!(
        close_step.contains("bump"),
        "the close ritual never runs the bump, so `[pending-bump]` is permanent: {close_step}",
    );
    let close_section = section(&refs, "## Close per repo");
    let bump_at = close_section
        .find("bump")
        .expect("the close section never names the bump");
    let parent_at = close_section
        .find("**Then the parent**")
        .expect("the close section no longer hands off to the parent's procedure");
    assert!(
        bump_at < parent_at,
        "the bump must land BEFORE the parent settles — after it, the submodule \
         branch is already deleted (bump at {bump_at}, parent at {parent_at})",
    );

    // --- 1d. The parent PR cannot merge past an open submodule PR ----------
    // Read from `pr.md`, not `git.md`: publishing moved to `/mustard:pr open`
    // when the doors were split by what they touch (the provider vs. your tree).
    // The rule is about the PR, so it followed the PR.
    let pr_md = read("plugin/commands/pr.md");
    let pr_step = section(&pr_md, "### 0. `open [<target>]` — publish it");
    assert!(
        pr_step.contains("--draft"),
        "the parent PR still opens mergeable while a submodule PR is open — the \
         ordering rule stays a sentence: {pr_step}",
    );
    assert!(
        pr_step.contains("Blocked by"),
        "the draft never says WHAT blocks it, so no reader can find the sibling PR: {pr_step}",
    );
    assert!(
        pr_step.contains("pr-ready"),
        "the prose opens a draft without naming what clears it: {pr_step}",
    );
    // GitHub does not request code owners on a draft. Unsaid, the operator reads
    // the missing reviewers as a broken CODEOWNERS file.
    assert!(
        pr_step.contains("CODEOWNERS"),
        "the draft step must warn that reviewer requests only fire at `ready`: {pr_step}",
    );

    // --- 1e. The status report reads the leftover line BOTH ways ------------
    // One reading is what made the ritual unfollowable: a lone ` M <sub>` was
    // declared a missed step unconditionally, so obeying the condition produced
    // a report that accused the operator of forgetting.
    let report_section = section(&refs, "## Final status report");
    assert!(
        report_section.contains("[pending-bump]"),
        "the status report has no name for a pointer deliberately not staged",
    );
    assert!(
        !refs.contains("as a MISSED gitlink step**"),
        "the report still reads every lone ` M <sub>` as a missed step, which is \
         what the conditioned stage deliberately produces",
    );
    for reading in ["OPEN", "MERGED"] {
        assert!(
            report_section.contains(reading),
            "the report names `[pending-bump]` without saying that the submodule \
             PR's state (`{reading}`) is what separates the two readings",
        );
    }
}

/// AC-7 — a decision that authorises deletion reads `rev-list`, never `git log`,
/// and the rule states why.
///
/// Measured on this repository: `rtk git log --oneline dd095023 --not
/// dd095023^1 dd095023^2` — a range whose only commit is a merge — comes back as
/// a single newline, while plain git prints the merge. The wrapper drops merge
/// commits. So "no commits left" and "the only commits left are merges" reach
/// the reader spelled identically, and the second one authorises a deletion that
/// destroys work. `rev-list` passes through byte-identical (1014 = 1014 bytes on
/// a real range), which is why the rule names a plumbing command rather than
/// telling anyone to drop the `rtk` prefix.
#[test]
fn git_prose_routes_destructive_decisions_through_rev_list() {
    let git_md = read("plugin/commands/git.md");

    // --- 1. The rule sits among the Iron rules, where a reader meets it -----
    let rules = section(&git_md, "## Iron rules");
    let rule = line_with(rules, "rev-list")
        .expect("the iron rules never mention `rev-list`, so no decision is routed through it");
    assert!(
        rule.contains("`git log`"),
        "the rule names the replacement without naming what it replaces: {rule}",
    );
    assert!(
        rule.contains("NEVER") || rule.contains("never"),
        "the rule reads as a preference, not a prohibition: {rule}",
    );
    assert!(
        rule.contains("delet"),
        "the rule must say WHICH decisions it governs — the ones that authorise \
         a deletion, not every range read: {rule}",
    );

    // Naming the commands is not teaching the rule: a reader who does not know
    // the wrapper drops merges will drop the `rtk` prefix instead, breaking the
    // Golden Rule to fix a symptom.
    assert!(
        rule.contains("merge commits"),
        "the rule states no reason, so the next author will revert it: {rule}",
    );
    assert!(
        rule.contains("nothing here") || rule.contains("lone newline"),
        "the reason must say the filtered range is INDISTINGUISHABLE from an \
         empty one — that ambiguity is the whole defect: {rule}",
    );
    assert!(
        rule.contains("byte-identical"),
        "the rule must say why `rev-list` is safe through the wrapper, or it \
         reads as superstition: {rule}",
    );
    assert!(
        rule.contains("rtk git rev-list"),
        "the rule must keep the Golden Rule intact — `rtk` still prefixes the \
         replacement: {rule}",
    );

    // --- 2. The code that authorises deletion really reads that plumbing ----
    // Without this half the sentence outlives the practice it describes: the
    // rule would ship while the sweep that offers the deletion quietly went
    // back to a filtered log.
    let branch_state = read("apps/rt/src/shared/branch_state.rs");
    assert!(
        branch_state.contains("\"rev-list\""),
        "the module that decides a branch is prunable no longer reads ranges \
         with `rev-list`",
    );
    for (module, body) in [
        ("apps/rt/src/shared/branch_state.rs", &branch_state),
        ("apps/rt/src/commands/git_settle.rs", &read("apps/rt/src/commands/git_settle.rs")),
    ] {
        assert!(
            !body.contains("\"log\","),
            "{module} authorises or performs a deletion off a `git log` argv — \
             the exact reading the iron rule forbids",
        );
    }
}

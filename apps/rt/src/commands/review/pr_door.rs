//! `mustard-rt run pr-list` / `pr-review` / `pr-merge` — the engine of the
//! `/mustard:pr` door.
//!
//! ONE module for three commands, because they are one ritual over one seam:
//! the link between a pull request and the work unit behind it. A PR's head
//! branch is `{base}_{slug}` and that slug IS the spec — `pr-review` records a
//! verdict under it and `pr-merge` reads that verdict back. Written once here,
//! the link cannot drift into three spellings across three files.
//!
//! ## What each command answers
//!
//! - **`pr-list`** — the base gate first: it runs ONLY from a `git.flow`
//!   integration base, because "which PRs are open" is a question about the
//!   BASE, not about one unit. From a work branch it REFUSES and names the base
//!   to switch to, touching nothing. On a base it answers one row per open PR:
//!   number, title, whether the provider calls it mergeable, whether it is a
//!   draft, and the head branch its unit lives on.
//! - **`pr-review`** — resolves the PR to its unit and prints the review brief:
//!   the spec the unit belongs to, the subproject its `## Files` name, and the
//!   SAME skill shelf the implementer was dispatched with — so "reviewed
//!   against the project patterns" means the very molds the work was written
//!   to, never a second list that can drift. With `--verdict` it RECORDS
//!   through [`review_result::record_review`], the one recorder `review-result`
//!   already uses, so the merge step reads what the REVIEW phase has always
//!   written.
//!
//! ## The spec is read out of the PR's OWN branch
//!
//! A review runs from an integration base — that is the door's design — and the
//! spec no longer lives there: this unit's whole layout (`spec.md`, the waves,
//! the ceremony) is materialized INSIDE `{base}_{slug}`. Reading
//! `.claude/spec/{slug}/spec.md` off the checkout therefore finds NOTHING from a
//! base, and the brief would come back with `spec_path`, `subproject` and
//! `patterns` all null while `pr.md` promises them. So the text is read from the
//! head ref itself — `git show {head}:.claude/spec/{slug}/spec.md` — with the
//! remote-tracking ref and then the working tree as fallbacks
//! ([`read_spec_text`]); `spec_source` reports which one answered, because "the
//! spec is not in this checkout" and "the unit has no spec" are different facts
//! and must not print the same.
//!
//! The recorded verdict is the mirror image and needs no such hop: `.claude/` is
//! redirected state, resolved to the MAIN checkout from inside any linked
//! worktree, and `.claude/spec/*/.events/` is gitignored — so the ONE store the
//! unit reads its own `review.result` back from is the main checkout's, whatever
//! branch happens to be out. Recording from the base writes exactly the file the
//! unit reads, and it adds nothing tracked to the base's tree.
//! - **`pr-merge`** — merges, then hands the pruning to
//!   [`git_settle::settle_at`]: returning to the base, pulling it, removing the
//!   worktree and deleting the local + remote branch IS the exit ritual, already
//!   written and already covering the in-place unit and the per-repo report.
//!   Reimplementing it here would be a second exit ritual to keep in step.
//!
//! ## The unreviewed merge WARNS and ASKS — it never refuses
//!
//! A merge requested without an `approved` verdict answers `action:"confirm"`
//! with `ok:true` and touches NOTHING: not a refusal (the operator decides case
//! by case) and not a silent merge. `--confirm` is that answer coming back —
//! the same hand-back shape `git-settle` uses for `exit-and-rerun`. The rule is
//! deliberately one rule, not two: an absent verdict and a recorded rejection
//! are both "not approved", and both are ASKED about rather than forked into
//! separate behaviours.
//!
//! Fail-open everywhere `gh` is involved: an absent CLI or an unreachable
//! provider degrades to an honest `gh_error` field and exit 0. The consent rule
//! above is the one thing that never degrades — no evidence means ASK.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;
use serde_json::Value;

use crate::commands::agent::render::reference::files_section_paths;
use crate::commands::agent::render::skills::build_skills_list;
use crate::commands::git_settle::{git_out, main_checkout_root, settle_at};
use crate::commands::review::dependency_precheck::detect_subproject;
use crate::commands::review::review_result;
use crate::commands::work_unit_open::checkout_holding_branch;
use crate::shared::work_kind::BaseFlow;

// ---------------------------------------------------------------------------
// Shared plumbing — the provider, the bases, and the PR↔unit link
// ---------------------------------------------------------------------------

/// Run `gh` in `root` and return its trimmed stdout, or the reason it did not
/// answer.
///
/// Same shape [`crate::commands::review::review_prefetch`] uses (the `cmd /C`
/// hop is how a `gh.cmd` shim is found on Windows) plus one addition that
/// matters here: the working directory. `gh` resolves the repository from the
/// cwd, and every command in this module asks about THIS project's pull
/// requests — inheriting the process cwd would ask about whichever repository
/// the session happens to sit in.
pub(crate) fn gh_out(root: &Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", "gh"]);
        c
    } else {
        Command::new("gh")
    };
    let Ok(out) = cmd.args(args).current_dir(root).output() else {
        return Err("gh-not-found".to_string());
    };
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        if stderr.contains("command not found") || out.status.code() == Some(127) {
            return Err("gh-not-found".to_string());
        }
        return Err(if stderr.is_empty() { "gh-failed".to_string() } else { stderr });
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// [`gh_out`] plus a JSON parse — an unparseable body is `parse-error`, never a
/// panic.
pub(crate) fn gh_json(root: &Path, args: &[&str]) -> Result<Value, String> {
    let text = gh_out(root, args)?;
    serde_json::from_str(&text).map_err(|_| "parse-error".to_string())
}

/// The repository root every command here works from: the MAIN checkout when
/// `root` sits inside a linked worktree, `root` itself otherwise. `mustard.json`
/// and `.claude/` live there, and so does the repository `gh` must resolve.
fn project_root(root: &Path) -> PathBuf {
    main_checkout_root(root).unwrap_or_else(|| root.to_path_buf())
}

/// The project's base model (derived from `git.flow`) and the branch the
/// checkout is standing on. No branch name is ever hardcoded — the core owns
/// that derivation so this door and the work-branch gate agree.
fn bases_and_branch(root: &Path) -> (BaseFlow, String) {
    let cfg = mustard_core::ProjectConfig::load(root);
    let flow = BaseFlow::of(&cfg.git);
    let branch = git_out(root, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
    (flow, branch)
}

/// The spec slug a work branch names — [`BaseFlow::slug_of`], the crate's ONE
/// spelling of the question, shared with the per-branch notebook.
///
/// `None` when the branch is nobody's work unit — a PR opened by hand or a
/// base→base promotion has no unit, and therefore no spec to review against or
/// verdict to read.
fn spec_of_branch(branch: &str, flow: &BaseFlow) -> Option<String> {
    flow.slug_of(branch)
}

/// Where a unit's spec lives, relative to the repository root.
fn spec_rel_path(slug: &str) -> String {
    format!(".claude/spec/{slug}/spec.md")
}

/// The spec text of `slug` as the PR's OWN branch carries it.
///
/// `git show <head>:.claude/spec/<slug>/spec.md`, never the working tree. This
/// spec moved the spec directory ONTO the work branch, and `pr-review` runs from
/// an integration base by design — so the file is simply not in the checkout,
/// and reading from disk answered `null` for `spec_path`, `subproject` AND
/// `patterns` on every single review. The local ref is tried first (the author
/// reviewing their own unit) and the remote-tracking ref second (the reviewer
/// who only ever fetched it).
///
/// The on-disk read stays as the last fallback: for a unit checked out IN PLACE
/// the tree and the branch are the same thing, and for a spec that was never
/// committed it is the only copy there is.
fn spec_text_of_unit(root: &Path, head: &str, slug: &str) -> Option<String> {
    let rel = spec_rel_path(slug);
    for reference in [format!("{head}:{rel}"), format!("origin/{head}:{rel}")] {
        if let Some(text) = git_out(root, &["show", &reference]).filter(|t| !t.trim().is_empty()) {
            return Some(text);
        }
    }
    let on_disk = mustard_core::ClaudePaths::for_project(root)
        .ok()
        .and_then(|p| p.for_spec(slug).ok())
        .map(|p| p.dir().join("spec.md"))?;
    std::fs::read_to_string(on_disk).ok().filter(|t| !t.trim().is_empty())
}

/// The pull request a command was pointed at, reduced to the two facts every
/// step here needs: which PR, and which branch carries its unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrFacts {
    pub number: u64,
    /// The PR's head branch — the work unit's branch.
    pub head: String,
}

/// Ask the provider which PR is meant. `None` = the one for the current branch,
/// which is what the door uses from inside a unit.
fn resolve_pr(root: &Path, pr: Option<u64>) -> Result<PrFacts, String> {
    let number = pr.map(|n| n.to_string());
    let mut args: Vec<&str> = vec!["pr", "view"];
    if let Some(n) = number.as_deref() {
        args.push(n);
    }
    args.extend_from_slice(&["--json", "number,headRefName"]);
    let value = gh_json(root, &args)?;
    let Some(number) = value.get("number").and_then(Value::as_u64) else {
        return Err("no-pr-for-branch".to_string());
    };
    Ok(PrFacts {
        number,
        head: value
            .get("headRefName")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

/// The subproject a spec's `## Files` section names, relative to the repo root
/// (`apps/rt`, `packages/core`, …). `None` when the paths disagree or name no
/// `apps/<x>` / `packages/<x>` segment.
///
/// Derived through [`detect_subproject`], the ONE discovery the dispatch plan
/// already uses — joined onto an empty root so the answer comes back relative,
/// which is the form both the skill shelf and `review.result` want.
fn spec_subproject(spec_text: &str) -> Option<String> {
    let files = files_section_paths(spec_text);
    detect_subproject(&files, Path::new(""))
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .filter(|s| !s.is_empty())
}

// ---------------------------------------------------------------------------
// `pr-list`
// ---------------------------------------------------------------------------

/// One open pull request, as the door lists it.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub(crate) struct PrEntry {
    pub number: u64,
    pub title: String,
    /// The provider's own word, verbatim (`MERGEABLE` / `CONFLICTING` /
    /// `UNKNOWN`). Never re-spelled into a bool: `UNKNOWN` means the provider
    /// has not finished computing it, which is not the same answer as "no".
    pub mergeable: String,
    /// A draft cannot be merged even when it is mergeable — the parent of a
    /// monorepo unit opens as a draft while any submodule PR is still open, so
    /// omitting this would show a row the merge step will refuse.
    pub draft: bool,
    /// The head branch — the work unit `pr-review` and `pr-merge` act on.
    pub head: String,
}

/// The `pr-list` document.
#[derive(Debug, Serialize)]
pub(crate) struct PrListReport {
    /// False only when the base gate refused. An unreachable provider is
    /// `ok: true` with `gh_error` set — the checkout was fine, the network was
    /// not.
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
    pub branch: String,
    pub bases: Vec<String>,
    /// Sorted by number, so two runs over the same state print the same bytes.
    pub prs: Vec<PrEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gh_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// Read one `gh pr list` row into a [`PrEntry`]. A row without a number is not
/// a pull request and is dropped rather than reported as number 0.
fn pr_entry(row: &Value) -> Option<PrEntry> {
    Some(PrEntry {
        number: row.get("number").and_then(Value::as_u64)?,
        title: row.get("title").and_then(Value::as_str).unwrap_or_default().to_string(),
        mergeable: row
            .get("mergeable")
            .and_then(Value::as_str)
            .unwrap_or("UNKNOWN")
            .to_string(),
        draft: row.get("isDraft").and_then(Value::as_bool).unwrap_or(false),
        head: row
            .get("headRefName")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

/// List the open pull requests of the base `root` is standing on.
#[must_use]
pub(crate) fn list_at(root: &Path) -> PrListReport {
    let repo = project_root(root);
    let (flow, branch) = bases_and_branch(&repo);
    let bases: Vec<String> = flow.bases().to_vec();
    if !bases.iter().any(|b| b == &branch) {
        // Name the base rather than the rule. The branch's own `{base}_` prefix
        // says which one it belongs to; a branch with no prefix (or none at all)
        // falls back to the project's primary base, so the refusal always ends
        // with something the operator can type.
        let target = flow.base_of(&branch).into_known()
            .unwrap_or_else(|| mustard_core::ProjectConfig::load(&repo).git.primary_base());
        return PrListReport {
            ok: false,
            reason: Some("not-on-integration-base"),
            hint: Some(format!(
                "`pr list` asks about a BASE, not about one unit — switch to `{target}` \
                 (`git checkout {target}`) and run it again"
            )),
            branch,
            bases,
            prs: Vec::new(),
            gh_error: None,
        };
    }

    let (prs, gh_error) = match gh_json(
        &repo,
        &["pr", "list", "--state", "open", "--json", "number,title,mergeable,isDraft,headRefName"],
    ) {
        Ok(value) => {
            let mut rows: Vec<PrEntry> = value
                .as_array()
                .map(|arr| arr.iter().filter_map(pr_entry).collect())
                .unwrap_or_default();
            rows.sort_by_key(|p| p.number);
            (rows, None)
        }
        Err(e) => (Vec::new(), Some(e)),
    };
    PrListReport { ok: true, reason: None, branch, bases, prs, gh_error, hint: None }
}

// ---------------------------------------------------------------------------
// `pr-review`
// ---------------------------------------------------------------------------

/// The `pr-review` document — the review brief, plus what was recorded when a
/// verdict was supplied.
#[derive(Debug, Serialize)]
pub(crate) struct PrReviewReport {
    /// False only when the PR could not be resolved. A unit with no spec is a
    /// legitimate answer (`spec: null`), not a failure.
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
    pub pr: u64,
    pub head: String,
    /// The spec slug the head branch names — the unit under review.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec: Option<String>,
    /// Where that spec lives, when it is on disk in this checkout. The spec is
    /// materialized ON the work branch, so a base checkout legitimately reports
    /// `null` here while the branch itself reports the path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subproject: Option<String>,
    /// The subproject's skill shelf, verbatim — the same block the implementer
    /// was dispatched with.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patterns: Option<String>,
    /// True when `--verdict` was supplied and the verdict was recorded.
    pub recorded: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
}

/// Build the review brief for a resolved PR, recording `verdict` when one is
/// supplied.
///
/// Recording goes through [`review_result::record_review`] — the same path the
/// `review-result` CLI and the `SubagentStop` verdict capture already take, so
/// a verdict recorded from this door is indistinguishable from one recorded by
/// the REVIEW phase, which is exactly what lets `pr-merge` read it.
#[must_use]
fn review_brief(
    root: &Path,
    facts: &PrFacts,
    flow: &BaseFlow,
    verdict: Option<&str>,
    critical: i64,
) -> PrReviewReport {
    let spec = spec_of_branch(&facts.head, flow);
    let spec_text = spec
        .as_deref()
        .and_then(|slug| spec_text_of_unit(root, &facts.head, slug));
    // Named only when the spec was really found — a path on an `ok:true` report
    // is a promise that something is there.
    let spec_path = spec
        .as_deref()
        .filter(|_| spec_text.is_some())
        .map(|slug| spec_rel_path(slug));
    let subproject = spec_subproject(spec_text.as_deref().unwrap_or_default());
    let patterns = subproject
        .as_deref()
        .map(|sub| build_skills_list(root, sub))
        .filter(|shelf| !shelf.is_empty());

    // Record where the UNIT can see it. The spec directory rides the work
    // branch now, so a base checkout does not track it and a verdict written
    // there would land in a tree the unit never reads. The checkout that HOLDS
    // the head branch is that tree — the main checkout after an in-place cut,
    // or the unit's own worktree. With none (the branch exists only on the
    // server) the main checkout's shared `.claude/` is the only home there is.
    let unit_root = checkout_holding_branch(root, &facts.head)
        .map(PathBuf::from)
        .unwrap_or_else(|| root.to_path_buf());
    let recorded = match (spec.as_deref(), verdict) {
        (Some(slug), Some(v)) => {
            review_result::record_review(
                &unit_root,
                slug,
                v,
                critical,
                subproject.as_deref(),
                None,
            );
            true
        }
        _ => false,
    };

    PrReviewReport {
        ok: true,
        reason: None,
        pr: facts.number,
        head: facts.head.clone(),
        spec,
        // Repo-relative, forward slashes: one report reads the same on every
        // platform and carries no machine path.
        spec_path,
        subproject,
        patterns,
        recorded,
        verdict: verdict.map(str::to_string),
    }
}

// ---------------------------------------------------------------------------
// `pr-merge`
// ---------------------------------------------------------------------------

/// What the merge step is allowed to do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MergeConsent {
    /// Merge now.
    Proceed,
    /// WARN and ASK — nothing is touched. Never a refusal: the operator's
    /// answer decides, case by case.
    Ask { reason: &'static str },
}

/// The ONE rule the merge step applies before it merges: a unit whose review
/// did not come back `approved` is ASKED about — never refused, never merged
/// silently.
///
/// An absent verdict and a recorded rejection take the SAME branch on purpose.
/// They are one fact ("not approved") and the answer to both is the same
/// question; forking them into two behaviours would add a decision the operator
/// never asked for. `confirmed` is that operator's answer coming back.
#[must_use]
pub(crate) fn merge_consent(verdict: Option<&str>, confirmed: bool) -> MergeConsent {
    if confirmed || verdict == Some("approved") {
        return MergeConsent::Proceed;
    }
    MergeConsent::Ask {
        reason: if verdict.is_none() { "no-review-verdict" } else { "review-not-approved" },
    }
}

/// The review verdict recorded for `spec` — `approved` only when EVERY
/// subproject that recorded one says so, otherwise the first dissenting verdict
/// verbatim. `None` = the unit carries no `review.result` at all.
///
/// Grouped per subproject (an absent one buckets as `"."`), because a later
/// approval of subproject B must not bury an earlier rejection of A. No group
/// is filtered out here: the merge step's answer to a dissent is a QUESTION, so
/// an over-cautious group costs one confirmation, while dropping it would cost
/// a silent merge over a rejection.
fn recorded_verdict(root: &Path, spec: &str) -> Option<String> {
    let spec_paths = mustard_core::ClaudePaths::for_project(root).ok()?.for_spec(spec).ok()?;
    let mut events = mustard_core::view::projection::read_harness_events_from_ndjson_dir(
        &spec_paths.dir().join(".events"),
    );
    events.sort_by(|a, b| a.ts.cmp(&b.ts));

    let mut latest: BTreeMap<String, String> = BTreeMap::new();
    for event in &events {
        if event.event != "review.result" {
            continue;
        }
        let Some(verdict) = event.payload.get("verdict").and_then(Value::as_str) else {
            continue;
        };
        let subproject = event
            .payload
            .get("subproject")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(".")
            .to_string();
        latest.insert(subproject, verdict.to_string());
    }
    if latest.is_empty() {
        return None;
    }
    latest
        .values()
        .find(|v| v.as_str() != "approved")
        .cloned()
        .or_else(|| Some("approved".to_string()))
}

/// The `pr-merge` document.
#[derive(Debug, Serialize)]
pub(crate) struct PrMergeReport {
    /// True for `merged` (as far as the exit ritual got) AND for `confirm` —
    /// an ASK is an instruction, not a failure, exactly like `git-settle`'s
    /// `exit-and-rerun`. Downgrading it would teach the caller to stop where it
    /// must continue.
    pub ok: bool,
    /// `confirm` (asked, nothing touched) · `merged` (merged, then settled) ·
    /// `merge-failed` (the provider refused; nothing was pruned).
    pub action: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
    pub pr: u64,
    /// The head branch — the unit this merge retires.
    pub head: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    /// The `git-settle` report, folded verbatim — the pruning half of this
    /// command IS that ritual, so its answer is not re-spelled here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settle: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// Merge a resolved PR, with both external effects injected: `merge` asks the
/// provider, `settle` runs the exit ritual. Injected so the consent rule can be
/// exercised without a provider, a network or a repository — and so a test can
/// prove that an unreviewed merge calls NEITHER.
#[must_use]
fn merge_core(
    root: &Path,
    facts: &PrFacts,
    flow: &BaseFlow,
    confirmed: bool,
    merge: &dyn Fn(&Path, u64) -> Result<(), String>,
    settle: &dyn Fn(&Path, &str) -> Value,
) -> PrMergeReport {
    let spec = spec_of_branch(&facts.head, flow);
    let verdict = spec.as_deref().and_then(|slug| recorded_verdict(root, slug));

    if let MergeConsent::Ask { reason } = merge_consent(verdict.as_deref(), confirmed) {
        let unit = spec.as_deref().unwrap_or(&facts.head);
        return PrMergeReport {
            ok: true,
            action: "confirm",
            reason: Some(reason),
            pr: facts.number,
            head: facts.head.clone(),
            warning: Some(match verdict.as_deref() {
                None => format!("`{unit}` carries no recorded review verdict — nothing was merged."),
                Some(v) => format!("the last review of `{unit}` came back `{v}` — nothing was merged."),
            }),
            hint: Some(format!(
                "ask the operator, then re-run with `--confirm` to merge anyway, or record a \
                 verdict first with `mustard-rt run pr-review --pr {} --verdict approved`",
                facts.number
            )),
            spec,
            verdict,
            settle: None,
        };
    }

    if let Err(e) = merge(root, facts.number) {
        return PrMergeReport {
            ok: false,
            action: "merge-failed",
            reason: Some("provider-refused"),
            pr: facts.number,
            head: facts.head.clone(),
            spec,
            verdict,
            warning: Some(e),
            settle: None,
            hint: Some(
                "nothing was pruned — the unit is untouched; resolve the refusal (conflicts, \
                 draft state, required checks) and run `pr merge` again"
                    .to_string(),
            ),
        };
    }

    // Merged. The rest — back to the base, pull it, remove the worktree, delete
    // the local and remote branch — IS `git-settle`, called rather than
    // rewritten: it already verifies the merge landed, already advances every
    // base and already handles the in-place unit that has no worktree to leave.
    let settled = settle(root, &facts.head);
    PrMergeReport {
        ok: settled.get("ok") == Some(&Value::Bool(true)),
        action: "merged",
        reason: None,
        pr: facts.number,
        head: facts.head.clone(),
        spec,
        verdict,
        warning: None,
        settle: Some(settled),
        hint: None,
    }
}

/// Ask the provider to merge. The strategy is explicit because it has to be: a
/// bare `gh pr merge` opens an interactive picker, and a `run`-face command has
/// no stdin to answer it with. A merge commit is what this project's history is
/// made of, so that is what is asked for.
fn gh_merge(root: &Path, pr: u64) -> Result<(), String> {
    gh_out(root, &["pr", "merge", &pr.to_string(), "--merge"]).map(|_| ())
}

// ---------------------------------------------------------------------------
// CLI faces — resolve the root, build the report, print it
// ---------------------------------------------------------------------------

/// Print one report as the single JSON document the command answers with.
fn emit<T: Serialize>(report: &T) {
    println!("{}", serde_json::to_string_pretty(report).unwrap_or_else(|_| "{}".to_string()));
}

/// Dispatch `mustard-rt run pr-list`.
pub fn run_list(root: &Path) {
    emit(&list_at(root));
}

/// Dispatch `mustard-rt run pr-review`.
pub fn run_review(root: &Path, pr: Option<u64>, verdict: Option<&str>, critical: i64) {
    if let Some(v) = verdict {
        if v != "approved" && v != "rejected" {
            eprintln!("[pr-review] Invalid --verdict \"{v}\" — expected approved|rejected");
            return;
        }
    }
    let repo = project_root(root);
    match resolve_pr(&repo, pr) {
        Ok(facts) => {
            let (flow, _) = bases_and_branch(&repo);
            emit(&review_brief(&repo, &facts, &flow, verdict, critical));
        }
        Err(e) => emit(&serde_json::json!({ "ok": false, "reason": e, "pr": pr })),
    }
}

/// Dispatch `mustard-rt run pr-merge`.
pub fn run_merge(root: &Path, pr: Option<u64>, confirm: bool) {
    let repo = project_root(root);
    match resolve_pr(&repo, pr) {
        Ok(facts) => {
            let (flow, _) = bases_and_branch(&repo);
            let settle = |r: &Path, branch: &str| settle_at(r, Some(branch));
            emit(&merge_core(&repo, &facts, &flow, confirm, &gh_merge, &settle));
        }
        Err(e) => emit(&serde_json::json!({ "ok": false, "reason": e, "pr": pr })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use serde_json::json;
    use tempfile::tempdir;

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git").args(args).current_dir(dir).output().expect("spawn git");
        assert!(out.status.success(), "git {args:?} failed");
    }

    /// A repo with `git.flow` declaring `dev` (primary) and `main`, sitting on
    /// `dev`. No remote, no `gh` — the base gate is answered from local state
    /// alone, which is the whole point of testing it here.
    fn repo() -> tempfile::TempDir {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        git(root, &["init", "."]);
        git(root, &["config", "user.email", "t@t"]);
        git(root, &["config", "user.name", "t"]);
        git(root, &["checkout", "-b", "dev"]);
        std::fs::write(root.join("mustard.json"), r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#)
            .expect("cfg");
        git(root, &["add", "-A"]);
        git(root, &["commit", "-m", "seed"]);
        dir
    }

    /// AC-4 — `pr list` from a work branch REFUSES and names the base to switch
    /// to; from the base it does not refuse (whatever the provider answers).
    #[test]
    fn pr_list_refuses_off_an_integration_base_and_names_it() {
        let dir = repo();
        let root = dir.path();

        // On the base: the gate passes. `gh` is absent/unauthenticated in the
        // test environment, which is reported as `gh_error` — never as a
        // refusal, because the checkout was fine.
        let on_base = list_at(root);
        assert!(on_base.ok, "the base itself is never refused: {:?}", on_base.reason);
        assert_eq!(on_base.reason, None);
        assert_eq!(on_base.branch, "dev");
        assert!(on_base.bases.contains(&"dev".to_string()), "bases: {:?}", on_base.bases);

        // On a work branch: refused, and the refusal NAMES the base.
        git(root, &["checkout", "-b", "dev_some-unit"]);
        let off_base = list_at(root);
        assert!(!off_base.ok, "a work branch must be refused");
        assert_eq!(off_base.reason, Some("not-on-integration-base"));
        assert_eq!(off_base.branch, "dev_some-unit");
        assert!(off_base.prs.is_empty(), "a refusal lists nothing");
        let hint = off_base.hint.unwrap_or_default();
        assert!(hint.contains("dev"), "the refusal must name the base: {hint}");

        // A branch with no declared prefix still gets a base named — the
        // project's primary one — so the message is never a dead end.
        git(root, &["checkout", "-b", "loose-branch"]);
        let loose = list_at(root);
        assert_eq!(loose.reason, Some("not-on-integration-base"));
        assert!(loose.hint.unwrap_or_default().contains("dev"), "primary base named");
    }

    /// The rows survive the trip from `gh` shape to report shape, sorted, with
    /// the provider's own mergeable word kept verbatim.
    #[test]
    fn pr_list_rows_keep_the_provider_word_and_sort_by_number() {
        let rows = json!([
            {"number": 9, "title": "later", "mergeable": "UNKNOWN", "isDraft": true, "headRefName": "dev_b"},
            {"number": 2, "title": "earlier", "mergeable": "MERGEABLE", "isDraft": false, "headRefName": "dev_a"},
            {"number": null, "title": "not a pr"},
        ]);
        let mut got: Vec<PrEntry> =
            rows.as_array().map(|a| a.iter().filter_map(pr_entry).collect()).unwrap_or_default();
        got.sort_by_key(|p| p.number);
        assert_eq!(
            got,
            vec![
                PrEntry {
                    number: 2,
                    title: "earlier".into(),
                    mergeable: "MERGEABLE".into(),
                    draft: false,
                    head: "dev_a".into(),
                },
                PrEntry {
                    number: 9,
                    title: "later".into(),
                    mergeable: "UNKNOWN".into(),
                    draft: true,
                    head: "dev_b".into(),
                },
            ],
            "numbered rows only, sorted, provider word verbatim"
        );
    }

    /// AC-5 — a merge requested with NO recorded review verdict warns and asks:
    /// it does not refuse (`ok` stays true) and it does not merge (neither
    /// injected effect runs). `--confirm` is the answer coming back.
    #[test]
    fn pr_merge_without_verdict_warns_and_asks_instead_of_merging() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        let bases = door_flow();
        let facts = PrFacts { number: 42, head: "dev_unreviewed".to_string() };

        let merges = Cell::new(0u32);
        let settles = Cell::new(0u32);
        let merge = |_: &Path, _: u64| {
            merges.set(merges.get() + 1);
            Ok(())
        };
        let settle = |_: &Path, _: &str| {
            settles.set(settles.get() + 1);
            json!({ "ok": true })
        };

        let asked = merge_core(root, &facts, &bases, false, &merge, &settle);
        assert!(asked.ok, "an ASK is an instruction, never a failure");
        assert_eq!(asked.action, "confirm");
        assert_eq!(asked.reason, Some("no-review-verdict"));
        assert_eq!(asked.verdict, None);
        assert!(asked.settle.is_none(), "nothing was pruned");
        assert!(
            asked.warning.unwrap_or_default().contains("unreviewed"),
            "the warning names the unit"
        );
        assert_eq!(merges.get(), 0, "nothing may be merged without an answer");
        assert_eq!(settles.get(), 0, "nothing may be pruned without an answer");

        // The operator's answer comes back as `--confirm`: now it merges and
        // settles. Still not a refusal at any point.
        let confirmed = merge_core(root, &facts, &bases, true, &merge, &settle);
        assert!(confirmed.ok);
        assert_eq!(confirmed.action, "merged");
        assert_eq!(merges.get(), 1);
        assert_eq!(settles.get(), 1);
        assert_eq!(confirmed.settle, Some(json!({ "ok": true })));
    }

    /// The consent rule itself: only an `approved` verdict (or the operator's
    /// answer) proceeds; absent and rejected both ASK, and neither ever
    /// produces a refusal.
    #[test]
    fn pr_merge_consent_asks_for_anything_but_approved() {
        assert_eq!(merge_consent(Some("approved"), false), MergeConsent::Proceed);
        assert_eq!(merge_consent(None, true), MergeConsent::Proceed);
        assert_eq!(merge_consent(Some("rejected"), true), MergeConsent::Proceed);
        assert_eq!(
            merge_consent(None, false),
            MergeConsent::Ask { reason: "no-review-verdict" }
        );
        assert_eq!(
            merge_consent(Some("rejected"), false),
            MergeConsent::Ask { reason: "review-not-approved" }
        );
    }

    /// A recorded verdict is read back through the same store `review-result`
    /// writes — and a rejection in ANY subproject wins over a later approval of
    /// another, so the merge step still asks.
    #[test]
    fn pr_merge_reads_the_verdict_review_result_recorded() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        assert_eq!(recorded_verdict(root, "unit-a"), None, "nothing recorded yet");

        review_result::record_review(root, "unit-a", "approved", 0, Some("apps/rt"), None);
        assert_eq!(recorded_verdict(root, "unit-a").as_deref(), Some("approved"));

        review_result::record_review(root, "unit-a", "rejected", 1, Some("packages/core"), None);
        assert_eq!(
            recorded_verdict(root, "unit-a").as_deref(),
            Some("rejected"),
            "one subproject's rejection is not buried by another's approval"
        );
    }

    /// The base model of a project declaring the ordinary two-tier flow.
    fn door_flow() -> BaseFlow {
        let mut git = mustard_core::domain::config::GitConfig::default();
        git.flow.insert("*".to_string(), "dev".to_string());
        git.flow.insert("dev".to_string(), "main".to_string());
        BaseFlow::of(&git)
    }

    /// The PR↔unit link: a head named by its kind names its spec, so does one
    /// still in the `{base}_{slug}` shape, and a bare base names none.
    #[test]
    fn spec_of_branch_reads_the_unit_out_of_the_head_ref() {
        let bases = door_flow();
        assert_eq!(spec_of_branch("feature/my-spec", &bases).as_deref(), Some("my-spec"));
        assert_eq!(spec_of_branch("hotfix/login", &bases).as_deref(), Some("login"));
        assert_eq!(spec_of_branch("dev_my-spec", &bases).as_deref(), Some("my-spec"));
        assert_eq!(spec_of_branch("worktree-dev_my-spec", &bases).as_deref(), Some("my-spec"));
        assert_eq!(spec_of_branch("main_hotfix", &bases).as_deref(), Some("hotfix"));
        assert_eq!(spec_of_branch("dev", &bases), None, "a bare base carries no unit");
        assert_eq!(spec_of_branch("feature_x", &bases), None, "a name of neither shape");
    }

    /// The brief points at the spec and hands back the SAME shelf the
    /// implementer got; with a verdict it records through `review-result`'s own
    /// path, which is what `pr-merge` then reads.
    #[test]
    fn pr_review_brief_names_the_spec_and_records_the_verdict() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        let spec_dir = root.join(".claude").join("spec").join("my-unit");
        std::fs::create_dir_all(&spec_dir).expect("spec dir");
        std::fs::write(
            spec_dir.join("spec.md"),
            "# demo\n\n## Files\n\n- `apps/rt/src/lib.rs`\n- `apps/rt/src/main.rs`\n",
        )
        .expect("spec");
        let shelf = root.join("apps/rt/.claude/skills/rt-demo-pattern");
        std::fs::create_dir_all(&shelf).expect("shelf");
        std::fs::write(
            shelf.join("SKILL.md"),
            "---\nname: rt-demo-pattern\ndescription: Use when demoing.\n---\n\nbody\n",
        )
        .expect("skill");

        let bases = door_flow();
        let facts = PrFacts { number: 7, head: "dev_my-unit".to_string() };

        let brief = review_brief(root, &facts, &bases, None, 0);
        assert!(brief.ok);
        assert_eq!(brief.spec.as_deref(), Some("my-unit"));
        assert_eq!(brief.subproject.as_deref(), Some("apps/rt"));
        assert!(
            brief.spec_path.unwrap_or_default().ends_with("my-unit/spec.md"),
            "forward slashes on every platform"
        );
        assert!(
            brief.patterns.unwrap_or_default().contains("rt-demo-pattern"),
            "the review reads the implementer's own shelf"
        );
        assert!(!brief.recorded, "no --verdict → nothing recorded");
        assert_eq!(recorded_verdict(root, "my-unit"), None);

        let recorded = review_brief(root, &facts, &bases, Some("approved"), 0);
        assert!(recorded.recorded);
        assert_eq!(recorded.verdict.as_deref(), Some("approved"));
        assert_eq!(
            recorded_verdict(root, "my-unit").as_deref(),
            Some("approved"),
            "the merge step reads exactly what the review step wrote"
        );
    }
}

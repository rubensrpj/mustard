//! `pr_provider` — the pull-request ACTIONS (open / edit / ready / view) as a
//! port, mirroring what [`crate::shared::branch_state`] already does for the
//! pull-request STATUS query ([`crate::shared::branch_state::PrLookup`]).
//!
//! The trait is what every caller depends on; no consumer ever names a provider
//! or its CLI. The adapters — [`GithubPrCli`] below, and the REST-speaking
//! [`crate::shared::pr_azure::AzurePrRest`] in its sibling module — are the
//! ONLY places a provider and its CLI/API are spelled, which is what an
//! adapter IS. A provider with no adapter answers the stable token
//! [`PR_UNSUPPORTED`], never a fabricated "absent": an unimplemented operation
//! is an unmeasured state, not a measured result.
//!
//! ## What the port normalises — and what it refuses to
//!
//! - **Refs are ALWAYS short.** Azure DevOps returns `sourceRefName` as a full
//!   ref (`refs/heads/x`) while GitHub's `headRefName` is already short (`x`).
//!   The port speaks the short name on both sides ([`short_ref`]); without
//!   this, every caller reimplements the conversion and gets it wrong on one
//!   of the two sides. Verified against the `GitPullRequest` contract of the
//!   Azure DevOps REST reference (7.1) and the `gh pr view --json` fields.
//! - **States map onto [`PrStatus`]** — the crate's ONE canonical vocabulary,
//!   owned by `branch_state`. Azure has four storable states and GitHub three;
//!   the port absorbs the difference ([`status_from_azure`],
//!   [`status_from_github`]) so no consumer ever re-reduces provider words.
//! - **Azure's `mergeStatus` travels VERBATIM** in its own
//!   [`PrView::merge_status`] field, unmapped. The REST contract
//!   (`PullRequestAsyncStatus`) has six values — `notSet`, `queued`,
//!   `conflicts`, `succeeded`, `rejectedByPolicy`, `failure` — against
//!   GitHub's three-word `mergeable`; a common vocabulary here would throw
//!   away information only Azure gives. GitHub answers `None`.
//! - **Diff and file lists are NOT offered.** After a fetch the local git has
//!   the commits, the answer is identical on both providers, and reading it
//!   locally spends no API quota — so that read stays with git, and a port
//!   method for it would only invite a second, slower spelling.
//!
//! **Never depends back on a face.** Per [`super`], `shared` is the leaf both
//! `hooks` and `commands` may depend on. The `gh` runner here therefore cannot
//! be imported from `commands::review::pr_door` — it is the same shape written
//! at the level both faces can reach, and the commands-face copies fold into
//! this one as their callers move behind the port.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::shared::branch_state::{PrStatus, PR_UNREADABLE, PR_UNSUPPORTED};

/// The provider token [`GithubPrCli`] adapts, as `resolve_provider` spells it.
const PROVIDER_GITHUB: &str = "github";
/// The provider token [`crate::shared::pr_azure::AzurePrRest`] adapts, as
/// `resolve_provider` spells it (`dev.azure.com` / `visualstudio.com`
/// remotes, or declared).
pub(crate) const PROVIDER_AZURE: &str = "azure";
/// The full-ref namespace Azure prefixes branch names with.
pub(crate) const HEADS: &str = "refs/heads/";

// ---------------------------------------------------------------------------
// Normalisation — the port's own vocabulary
// ---------------------------------------------------------------------------

/// The short branch name of a provider ref — the ONE spelling the port speaks.
///
/// Azure DevOps returns `sourceRefName` / `targetRefName` as full refs
/// (`refs/heads/x`, per the `GitPullRequest` REST contract); GitHub's
/// `headRefName` / `baseRefName` are already short. Applied on BOTH sides so
/// the answer cannot depend on which adapter produced it. Anything outside
/// `refs/heads/` (a tag, an already-short name) passes through unchanged.
pub(crate) fn short_ref(name: &str) -> &str {
    name.strip_prefix(HEADS).unwrap_or(name)
}

/// Map Azure's `PullRequestStatus` word onto the canonical [`PrStatus`].
///
/// The REST contract (verified against the Azure DevOps REST reference, 7.1)
/// stores four values: `active`, `completed`, `abandoned`, `notSet`. The map
/// is `active → Open`, `completed → Merged` (an Azure PR completes by
/// merging), `abandoned → Closed` (closed WITHOUT merging), and
/// `notSet → Open` — the contract calls it "status not set, default state",
/// which is a PR that has not left flight, never one that landed. The fifth
/// word of the enum, `all`, exists only as a SEARCH criterion and is never
/// stored on a pull request, so it — like any word outside the contract —
/// answers [`PrStatus::Unknown`]`(`[`PR_UNREADABLE`]`)`: an answer this
/// adapter could not read, never a guessed state.
pub(crate) fn status_from_azure(status: &str) -> PrStatus {
    match status {
        "active" | "notSet" => PrStatus::Open,
        "completed" => PrStatus::Merged,
        "abandoned" => PrStatus::Closed,
        _ => PrStatus::Unknown(PR_UNREADABLE),
    }
}

/// Map GitHub's `state` word onto the canonical [`PrStatus`].
///
/// `gh` answers UPPERCASE (`OPEN` / `MERGED` / `CLOSED`) — the same contract
/// [`crate::shared::branch_state::ProviderPrCli`] already reduces over, and
/// matched case-insensitively for the same reason it upper-cases there.
pub(crate) fn status_from_github(state: &str) -> PrStatus {
    match state.to_ascii_uppercase().as_str() {
        "OPEN" => PrStatus::Open,
        "MERGED" => PrStatus::Merged,
        "CLOSED" => PrStatus::Closed,
        _ => PrStatus::Unknown(PR_UNREADABLE),
    }
}

/// What the PROVIDER'S OWN checks say about one pull request.
///
/// A different fact from the review verdict Mustard records: that one is an
/// opinion somebody wrote down, this one is a result the provider OBSERVED —
/// the workflow runs GitHub starts on its own, the statuses a pipeline posts on
/// an Azure pull request. The merge door reads both, and neither substitutes
/// for the other.
///
/// The vocabulary is closed and the same on both sides, so no caller ever
/// re-reduces provider words. What it deliberately does NOT carry is which run
/// failed or how many are left: the door's question is whether it may merge,
/// and a list of run names would only invite a caller to re-decide from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrChecks {
    /// The provider attached no check at all — a MEASURED absence (a project
    /// with no CI), never "could not ask", which is an `Err`.
    Absent,
    /// At least one run has not decided yet.
    Running,
    /// Every run decided, and none of them failed.
    Passed,
    /// At least one run decided in failure.
    Failed,
    /// Rows arrived but no state word inside them could be read — the same
    /// answer [`status_from_azure`] gives a word outside a provider's
    /// contract, and for the same reason: an unreadable answer is not a green
    /// one.
    Unknown(&'static str),
}

impl PrChecks {
    /// The stable token a report prints. `Unknown` prints its own reason, so
    /// the operator reads WHY it could not be reduced.
    pub(crate) fn word(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Running => "running",
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Unknown(reason) => reason,
        }
    }
}

/// Reduce every run of one pull request into the ONE answer the door reads.
///
/// The precedence is `Failed` > `Running` > `Unknown` > `Passed`, and each step
/// of it is a decision:
///
/// - A decided failure beats a run still in flight: waiting for the rest cannot
///   un-fail it, and "failed" tells the operator to fix while "running" would
///   tell them to wait.
/// - An unreadable row beats a green one for the reason the whole port exists:
///   a word we could not read is not evidence that a run passed.
/// - No rows at all is [`PrChecks::Absent`] — a measurement, not an absence of
///   one. This is what lets a project with no CI merge without an argument.
///
/// One spelling, shared by both adapters, so the two can never drift.
pub(crate) fn checks_from_rows(rows: &[PrChecks]) -> PrChecks {
    if rows.is_empty() {
        return PrChecks::Absent;
    }
    if rows.contains(&PrChecks::Failed) {
        return PrChecks::Failed;
    }
    if rows.contains(&PrChecks::Running) {
        return PrChecks::Running;
    }
    rows.iter().find(|row| matches!(row, PrChecks::Unknown(_))).copied().unwrap_or(PrChecks::Passed)
}

/// Reduce ONE row of GitHub's `statusCheckRollup` array.
///
/// The array mixes two node types and each spells its state differently — a
/// `CheckRun` carries `status` plus a `conclusion` that is empty until it
/// completes, a `StatusContext` carries a single `state`. Keyed on which field
/// is present rather than on `__typename`, because the field IS the contract
/// and a third node type would then still reduce by whichever word it speaks.
fn check_row_from_github(row: &Value) -> PrChecks {
    let text = |key: &str| row.get(key).and_then(Value::as_str).unwrap_or_default().to_ascii_uppercase();
    if row.get("status").is_some() {
        // CheckRun: anything that is not COMPLETED is still in flight
        // (QUEUED, IN_PROGRESS, WAITING, PENDING, REQUESTED).
        if text("status") != "COMPLETED" {
            return PrChecks::Running;
        }
        return match text("conclusion").as_str() {
            "SUCCESS" | "NEUTRAL" | "SKIPPED" => PrChecks::Passed,
            "FAILURE" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED" | "STARTUP_FAILURE"
            | "STALE" => PrChecks::Failed,
            _ => PrChecks::Unknown(PR_UNREADABLE),
        };
    }
    match text("state").as_str() {
        "SUCCESS" => PrChecks::Passed,
        "FAILURE" | "ERROR" => PrChecks::Failed,
        "PENDING" | "EXPECTED" => PrChecks::Running,
        _ => PrChecks::Unknown(PR_UNREADABLE),
    }
}

/// Read one `gh pr view --json statusCheckRollup` document into [`PrChecks`].
///
/// Pure, so the whole reduction is provable without a network. A document
/// whose `statusCheckRollup` is not an array could not be read — never an
/// empty one, because an empty one authorises a merge.
fn checks_from_github(row: &Value) -> Result<PrChecks, String> {
    let rollup = row
        .get("statusCheckRollup")
        .and_then(Value::as_array)
        .ok_or_else(|| "parse-error".to_string())?;
    let rows: Vec<PrChecks> = rollup.iter().map(check_row_from_github).collect();
    Ok(checks_from_rows(&rows))
}

/// One pull request as the port answers it — normalised, provider-free data.
///
/// Plain data on purpose, like `branch_state`'s read view: no path, no process
/// handle. `head` and `base` are ALWAYS short names, `status` is the canonical
/// vocabulary, and the one provider-specific fact rides in its own clearly
/// labelled field instead of leaking into the shared ones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrView {
    /// The provider's PR number (Azure: `pullRequestId`).
    pub(crate) number: u64,
    pub(crate) title: String,
    /// The head branch, SHORT — [`short_ref`] applied whatever the provider.
    pub(crate) head: String,
    /// The base (target) branch, SHORT.
    pub(crate) base: String,
    /// The canonical status — the same vocabulary `branch_state` reduces over.
    pub(crate) status: PrStatus,
    /// Azure's `mergeStatus` (`PullRequestAsyncStatus`), VERBATIM: `notSet`,
    /// `queued`, `conflicts`, `succeeded`, `rejectedByPolicy` or `failure`.
    /// `None` on providers that do not speak it (GitHub). Deliberately
    /// unmapped — six values collapsed into GitHub's three would lose exactly
    /// the information only Azure gives.
    pub(crate) merge_status: Option<String>,
    pub(crate) draft: bool,
    /// The PR's web URL — what a door prints for the operator to click.
    pub(crate) url: String,
}

/// What a caller asks [`PrProvider::open`] to create. Branch names are SHORT —
/// the adapter that needs a full ref builds it, never the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrToOpen {
    pub(crate) title: String,
    pub(crate) body: String,
    /// The work-unit branch the PR is opened FROM, short.
    pub(crate) head: String,
    /// The integration base it targets, short.
    pub(crate) base: String,
    pub(crate) draft: bool,
}

/// What [`PrProvider::open`] answers: the two facts the create call itself
/// proves. Everything else (status, mergeability) is a [`PrProvider::view`]
/// away, and folding a second round-trip into `open` would let the view's
/// failure mask a create that SUCCEEDED.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrOpened {
    pub(crate) number: u64,
    pub(crate) url: String,
}

// ---------------------------------------------------------------------------
// The port
// ---------------------------------------------------------------------------

/// The pull-request actions as a PORT.
///
/// Callers depend on this trait and never on a provider's CLI or REST API, so
/// a new provider is a new adapter and not one line of new caller logic —
/// the same inversion [`crate::shared::branch_state::PrLookup`] already made
/// for the status query. Every operation degrades to `Err(String)` (a stable
/// token where this module decides the words, the CLI's own stderr where it
/// does not), never a panic — `clippy::unwrap_used` is `deny` crate-wide and
/// these run on command paths that must keep answering JSON.
pub(crate) trait PrProvider {
    /// The provider token this adapter speaks for — what a report prints so
    /// the operator knows WHO was asked. `resolve_provider`'s vocabulary.
    fn provider(&self) -> &str;

    /// Open a pull request. Answers the facts the create itself proved.
    fn open(&self, pr: &PrToOpen) -> Result<PrOpened, String>;

    /// Replace the body of pull request `number`.
    fn edit_body(&self, number: u64, body: &str) -> Result<(), String>;

    /// Mark draft pull request `number` ready for review.
    fn ready(&self, number: u64) -> Result<(), String>;

    /// One pull request, normalised. `None` = the PR whose head is the branch
    /// the checkout is standing on — the shape the doors use from inside a
    /// unit.
    fn view(&self, number: Option<u64>) -> Result<PrView, String>;

    /// What the PROVIDER'S own checks say about pull request `number`.
    ///
    /// Its own query and not a field of [`PrView`], because the two have
    /// different lifetimes: a view is a description that keeps, while this is a
    /// reading that is stale the moment a run finishes — the merge door asks it
    /// immediately before it merges, and nothing else should be tempted to
    /// cache it alongside a title. An unreachable provider answers `Err`, never
    /// [`PrChecks::Absent`]: "nobody ran anything" and "nobody could be asked"
    /// are different facts and the door treats them differently.
    fn checks(&self, number: u64) -> Result<PrChecks, String>;
}

// ---------------------------------------------------------------------------
// GitHub — the `gh` adapter
// ---------------------------------------------------------------------------

/// Run `gh` in `root` and return its trimmed stdout, or the reason it did not
/// answer — the SAME shape as `commands::review::pr_door::gh_out`, which this
/// module cannot import without inverting the `shared` ← `commands` DAG.
///
/// The `cmd /C` hop is how a `gh.cmd` shim is found on Windows; the explicit
/// cwd matters because `gh` resolves the repository from it, and every call
/// here asks about THIS project's pull requests — inheriting the process cwd
/// would ask about whichever repository the session happens to sit in.
fn gh_out(root: &Path, args: &[&str]) -> Result<String, String> {
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

/// [`gh_out`] plus a JSON parse — an unparseable body is `parse-error`, never
/// a panic.
fn gh_json(root: &Path, args: &[&str]) -> Result<Value, String> {
    let text = gh_out(root, args)?;
    serde_json::from_str(&text).map_err(|_| "parse-error".to_string())
}

/// Read one `gh pr view --json` document into the port's [`PrView`].
///
/// Pure, so every field mapping is testable without a network: the short-ref
/// normalisation, the state reduction, and the absence of `merge_status` —
/// GitHub does not speak Azure's word, and inventing one here would be the
/// exact information-erasing map the port refuses.
fn view_from_github(row: &Value) -> Result<PrView, String> {
    let Some(number) = row.get("number").and_then(Value::as_u64) else {
        // A document without a number is not a pull request — `parse-error`,
        // never a PR "number 0" a caller might act on.
        return Err("parse-error".to_string());
    };
    let text = |key: &str| row.get(key).and_then(Value::as_str).unwrap_or_default();
    Ok(PrView {
        number,
        title: text("title").to_string(),
        head: short_ref(text("headRefName")).to_string(),
        base: short_ref(text("baseRefName")).to_string(),
        status: status_from_github(text("state")),
        merge_status: None,
        draft: row.get("isDraft").and_then(Value::as_bool).unwrap_or(false),
        url: text("url").to_string(),
    })
}

/// The PR number a `gh pr create` answer names — the URL's last path segment.
/// `gh` prints the created PR's web URL as its stdout.
fn number_from_pr_url(url: &str) -> Option<u64> {
    url.trim().rsplit('/').next()?.parse().ok()
}

/// The GitHub adapter — the ONE place `gh`'s pr-action argv is spelled.
///
/// Everything network-shaped degrades to `Err(String)`: an absent CLI, an
/// unauthenticated session, a refused create. The caller decides what a
/// failure means; this adapter only reports it honestly.
pub(crate) struct GithubPrCli {
    repo: PathBuf,
}

impl GithubPrCli {
    /// Bind the adapter to one repository root — the cwd every `gh` call runs
    /// in.
    pub(crate) fn new(repo: &Path) -> Self {
        Self { repo: repo.to_path_buf() }
    }
}

impl PrProvider for GithubPrCli {
    fn provider(&self) -> &str {
        PROVIDER_GITHUB
    }

    fn open(&self, pr: &PrToOpen) -> Result<PrOpened, String> {
        let mut args: Vec<&str> = vec![
            "pr", "create", "--title", &pr.title, "--body", &pr.body, "--head", &pr.head,
            "--base", &pr.base,
        ];
        if pr.draft {
            args.push("--draft");
        }
        let answer = gh_out(&self.repo, &args)?;
        // `gh pr create` prints the new PR's URL. When that shape ever drifts,
        // the number is re-asked from the head branch rather than reported as
        // a failure — the create DID succeed, and an `Err` here would send
        // the caller into opening a duplicate.
        if let Some(number) = number_from_pr_url(&answer) {
            return Ok(PrOpened { number, url: answer });
        }
        let row = gh_json(&self.repo, &["pr", "view", &pr.head, "--json", "number,url"])?;
        let view = view_from_github(&row)?;
        Ok(PrOpened { number: view.number, url: view.url })
    }

    fn edit_body(&self, number: u64, body: &str) -> Result<(), String> {
        gh_out(&self.repo, &["pr", "edit", &number.to_string(), "--body", body]).map(|_| ())
    }

    fn ready(&self, number: u64) -> Result<(), String> {
        gh_out(&self.repo, &["pr", "ready", &number.to_string()]).map(|_| ())
    }

    fn view(&self, number: Option<u64>) -> Result<PrView, String> {
        let number = number.map(|n| n.to_string());
        let mut args: Vec<&str> = vec!["pr", "view"];
        if let Some(n) = number.as_deref() {
            args.push(n);
        }
        args.extend_from_slice(&[
            "--json",
            "number,title,state,headRefName,baseRefName,isDraft,url",
        ]);
        view_from_github(&gh_json(&self.repo, &args)?)
    }

    fn checks(&self, number: u64) -> Result<PrChecks, String> {
        // `gh pr view --json statusCheckRollup`, NOT `gh pr checks`: the latter
        // encodes the answer in its EXIT CODE (8 while runs are pending, 1 on
        // failure), which `gh_out` reads as a failed command — the two states
        // this query exists to distinguish would both arrive as `Err`.
        let row = gh_json(
            &self.repo,
            &["pr", "view", &number.to_string(), "--json", "statusCheckRollup"],
        )?;
        checks_from_github(&row)
    }
}

// ---------------------------------------------------------------------------
// The answer for every unadapted provider
// ---------------------------------------------------------------------------

/// The one answer of an operation nobody implemented: the stable token
/// [`PR_UNSUPPORTED`] — the same word `branch_state`'s status port uses for a
/// provider without an adapter, and for the same reason. Never a fabricated
/// success and never a measured-looking absence.
fn unsupported<T>() -> Result<T, String> {
    Err(PR_UNSUPPORTED.to_string())
}

/// The adapter for a provider this module has no adapter FOR (`gitlab`,
/// `bitbucket`, anything the resolver may learn later): every operation is
/// [`PR_UNSUPPORTED`]. Separate from [`AzurePrRest`] because the two are
/// different facts — Azure is an adapter that is not written YET; this is the
/// honest answer for providers that have none at all.
pub(crate) struct UnsupportedPr {
    /// The resolved provider token, kept so a report can still NAME who was
    /// asked-for even though nothing could be asked.
    provider: String,
}

impl PrProvider for UnsupportedPr {
    fn provider(&self) -> &str {
        &self.provider
    }

    fn open(&self, _pr: &PrToOpen) -> Result<PrOpened, String> {
        unsupported()
    }

    fn edit_body(&self, _number: u64, _body: &str) -> Result<(), String> {
        unsupported()
    }

    fn ready(&self, _number: u64) -> Result<(), String> {
        unsupported()
    }

    fn view(&self, _number: Option<u64>) -> Result<PrView, String> {
        unsupported()
    }

    fn checks(&self, _number: u64) -> Result<PrChecks, String> {
        unsupported()
    }
}

// ---------------------------------------------------------------------------
// The factory — the provider in force picks the adapter
// ---------------------------------------------------------------------------

/// The adapter for the provider IN FORCE at `root`: what
/// `mustard_core::resolve_provider` answers from `mustard.json#git.provider`
/// (declared wins), the `origin` remote, or the fallback — the ONE spelling
/// of that precedence, reused rather than re-derived.
///
/// A `Box<dyn PrProvider>` so no caller ever writes a provider name: the
/// commands that open/edit/ready a PR ask for "the provider", and WHICH one
/// answers stays an internal detail of this module.
pub(crate) fn provider_for(root: &Path) -> Box<dyn PrProvider> {
    let cfg = mustard_core::ProjectConfig::load(root);
    let provider = mustard_core::resolve_provider(root, &cfg.git.provider);
    match provider.as_str() {
        PROVIDER_GITHUB => Box::new(GithubPrCli::new(root)),
        PROVIDER_AZURE => Box::new(crate::shared::pr_azure::AzurePrRest::new(root)),
        _ => Box::new(UnsupportedPr { provider }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::pr_azure::{
        do_open, do_view_number, pat_from,
        test_support::{remote, FakeTransport},
        AzureRemote, PAT_ENV,
    };
    use serde_json::json;

    /// The port speaks the short name whatever a provider handed it: Azure's
    /// full ref is stripped, GitHub's already-short name passes unchanged,
    /// and a name outside `refs/heads/` is not guessed at.
    #[test]
    fn a_full_ref_and_a_short_name_answer_the_same_branch() {
        assert_eq!(short_ref("refs/heads/feature/my-unit"), "feature/my-unit");
        assert_eq!(short_ref("feature/my-unit"), "feature/my-unit");
        assert_eq!(short_ref("refs/tags/v1.0"), "refs/tags/v1.0");
        assert_eq!(short_ref(""), "");
    }

    /// The Azure state map, exactly as decided against the REST contract:
    /// `active`/`notSet` are in flight, `completed` merged, `abandoned`
    /// closed-without-merging — and a word outside the contract (including
    /// `all`, which is search-criteria-only) is an unreadable answer, never a
    /// guessed state.
    #[test]
    fn azure_states_map_to_the_canonical_vocabulary() {
        assert_eq!(status_from_azure("active"), PrStatus::Open);
        assert_eq!(status_from_azure("notSet"), PrStatus::Open);
        assert_eq!(status_from_azure("completed"), PrStatus::Merged);
        assert_eq!(status_from_azure("abandoned"), PrStatus::Closed);
        assert_eq!(status_from_azure("all"), PrStatus::Unknown(PR_UNREADABLE));
        assert_eq!(status_from_azure(""), PrStatus::Unknown(PR_UNREADABLE));
    }

    /// GitHub's three words, upper-case per `gh`'s contract but matched
    /// case-insensitively; anything else is unreadable, never guessed.
    #[test]
    fn github_states_map_onto_the_canonical_vocabulary() {
        assert_eq!(status_from_github("OPEN"), PrStatus::Open);
        assert_eq!(status_from_github("MERGED"), PrStatus::Merged);
        assert_eq!(status_from_github("CLOSED"), PrStatus::Closed);
        assert_eq!(status_from_github("open"), PrStatus::Open);
        assert_eq!(status_from_github("DRAFT"), PrStatus::Unknown(PR_UNREADABLE));
    }

    /// The GitHub view mapping is pure and testable: refs come out short,
    /// the state is reduced, `merge_status` stays `None` (GitHub does not
    /// speak Azure's word), and a document without a number is refused
    /// rather than answered as PR 0.
    #[test]
    fn github_view_normalises_refs_and_never_invents_a_merge_status() {
        let row = json!({
            "number": 42,
            "title": "the unit",
            "state": "OPEN",
            "headRefName": "feature/my-unit",
            "baseRefName": "dev",
            "isDraft": true,
            "url": "https://github.com/org/repo/pull/42",
        });
        let view = view_from_github(&row).expect("a full row parses");
        assert_eq!(
            view,
            PrView {
                number: 42,
                title: "the unit".into(),
                head: "feature/my-unit".into(),
                base: "dev".into(),
                status: PrStatus::Open,
                merge_status: None,
                draft: true,
                url: "https://github.com/org/repo/pull/42".into(),
            }
        );
        assert_eq!(
            view_from_github(&json!({ "title": "not a pr" })),
            Err("parse-error".to_string()),
            "no number, no pull request",
        );
    }

    /// `gh pr create` answers the new PR's URL; the number is its last path
    /// segment, and a shape that is not that yields `None` (the adapter then
    /// re-asks) rather than a fabricated number.
    #[test]
    fn the_created_pr_number_is_read_out_of_the_url() {
        assert_eq!(number_from_pr_url("https://github.com/org/repo/pull/123"), Some(123));
        assert_eq!(number_from_pr_url("https://github.com/org/repo/pull/7\n"), Some(7));
        assert_eq!(number_from_pr_url("Creating pull request..."), None);
        assert_eq!(number_from_pr_url(""), None);
    }

    /// Every operation of an adapter-less provider answers the stable token,
    /// honest about not having measured or done anything. The same word
    /// `branch_state` uses, on purpose.
    #[test]
    fn a_provider_without_an_adapter_refuses_honestly() {
        let to_open = PrToOpen {
            title: "t".into(),
            body: "b".into(),
            head: "feature/x".into(),
            base: "dev".into(),
            draft: false,
        };
        let token = || PR_UNSUPPORTED.to_string();
        let provider = UnsupportedPr { provider: "gitlab".into() };
        assert_eq!(provider.open(&to_open), Err(token()));
        assert_eq!(provider.edit_body(1, "body"), Err(token()));
        assert_eq!(provider.ready(1), Err(token()));
        assert_eq!(provider.view(Some(1)), Err(token()));
        assert_eq!(
            provider.checks(1),
            Err(token()),
            "an unasked provider never answers a green check",
        );
        assert_eq!(provider.provider(), "gitlab");
    }

    /// The reduction the merge door stands on: a decided failure outranks a
    /// run still in flight, an unreadable row outranks a green one, and NO row
    /// at all is a measured absence — the answer that lets a project with no
    /// CI merge without an argument.
    #[test]
    fn the_checks_reduction_ranks_failure_over_flight_and_never_guesses_green() {
        let unknown = PrChecks::Unknown(PR_UNREADABLE);
        assert_eq!(checks_from_rows(&[]), PrChecks::Absent);
        assert_eq!(checks_from_rows(&[PrChecks::Passed, PrChecks::Passed]), PrChecks::Passed);
        assert_eq!(
            checks_from_rows(&[PrChecks::Passed, PrChecks::Running]),
            PrChecks::Running,
            "one run still in flight is not a finished answer",
        );
        assert_eq!(
            checks_from_rows(&[PrChecks::Running, PrChecks::Failed, PrChecks::Passed]),
            PrChecks::Failed,
            "waiting for the rest cannot un-fail a decided failure",
        );
        assert_eq!(
            checks_from_rows(&[PrChecks::Passed, unknown]),
            unknown,
            "a word we could not read is not evidence that a run passed",
        );
        assert_eq!(
            checks_from_rows(&[unknown, PrChecks::Running]),
            PrChecks::Running,
            "an undecided run is a stronger fact than an unreadable one",
        );
    }

    /// GitHub's rollup mixes two node types: `CheckRun` (a `status` plus a
    /// `conclusion` that is empty until it completes) and `StatusContext` (one
    /// `state`). Both reduce, and a row speaking neither vocabulary is
    /// unreadable rather than green.
    #[test]
    fn the_github_rollup_reduces_both_of_its_node_types() {
        let rollup = |rows: Value| json!({ "statusCheckRollup": rows });

        assert_eq!(checks_from_github(&rollup(json!([]))), Ok(PrChecks::Absent));
        assert_eq!(
            checks_from_github(&rollup(json!([
                { "status": "COMPLETED", "conclusion": "SUCCESS" },
                { "status": "COMPLETED", "conclusion": "SKIPPED" },
                { "state": "SUCCESS" },
            ]))),
            Ok(PrChecks::Passed),
        );
        assert_eq!(
            checks_from_github(&rollup(json!([
                { "status": "COMPLETED", "conclusion": "SUCCESS" },
                { "status": "IN_PROGRESS", "conclusion": "" },
            ]))),
            Ok(PrChecks::Running),
            "the run that measured PR 237: one green OS, the others still going",
        );
        assert_eq!(
            checks_from_github(&rollup(json!([
                { "status": "IN_PROGRESS", "conclusion": "" },
                { "status": "COMPLETED", "conclusion": "FAILURE" },
            ]))),
            Ok(PrChecks::Failed),
        );
        assert_eq!(
            checks_from_github(&rollup(json!([{ "state": "PENDING" }]))),
            Ok(PrChecks::Running),
        );
        assert_eq!(
            checks_from_github(&rollup(json!([{ "state": "ERROR" }]))),
            Ok(PrChecks::Failed),
        );
        assert_eq!(
            checks_from_github(&rollup(json!([{ "name": "neither vocabulary" }]))),
            Ok(PrChecks::Unknown(PR_UNREADABLE)),
        );
        assert_eq!(
            checks_from_github(&json!({ "number": 7 })),
            Err("parse-error".to_string()),
            "no rollup array could be read — never an empty one, which would authorise a merge",
        );
    }

    /// The factory picks by the provider IN FORCE — the declared setting wins
    /// (resolve_provider's own precedence), so no git remote is needed to
    /// prove the routing, and no caller ever names a provider itself.
    #[test]
    fn the_factory_routes_by_the_provider_in_force() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let declare = |provider: &str| {
            std::fs::write(
                root.join("mustard.json"),
                format!(r#"{{"git":{{"provider":"{provider}"}}}}"#),
            )
            .expect("mustard.json");
        };

        declare("github");
        assert_eq!(provider_for(root).provider(), "github");

        declare("azure");
        let azure = provider_for(root);
        assert_eq!(azure.provider(), "azure");
        // The tempdir has no `origin`, so the REAL Azure adapter refuses at
        // remote derivation — deterministically, before any network.
        let refusal = azure.view(Some(1)).expect_err("no origin remote to derive from");
        assert!(refusal.starts_with("azure-remote-"), "stable token: {refusal}");

        declare("gitlab");
        let other = provider_for(root);
        assert_eq!(other.provider(), "gitlab", "the report can still name who was asked-for");
        assert_eq!(other.ready(1), Err(PR_UNSUPPORTED.to_string()));
    }

    // -----------------------------------------------------------------------
    // The Azure adapter's AC-pinned tests. The adapter lives in
    // `crate::shared::pr_azure`; these four run against it through the fake
    // transport its `test_support` exposes — the spec's acceptance criteria
    // name THESE exact paths.
    // -----------------------------------------------------------------------

    /// Open POSTs the `GitPullRequest` create contract — FULL refs built from
    /// the port's short names, title/description/isDraft — and the answered
    /// URL is DERIVED from the remote: the response's own `url` field (a REST
    /// API address, not the web page) is ignored on purpose.
    #[test]
    fn azure_open_posts_the_pullrequest_contract() {
        let remote = remote();
        let create_url = format!("{}?api-version=7.1", remote.api_pulls());
        let fake = FakeTransport::of(&[(
            "POST",
            &create_url,
            json!({
                "pullRequestId": 42,
                "url": "https://dev.azure.com/suzano/_apis/git/NOT-THE-WEB-URL",
            }),
        )]);
        let pr = PrToOpen {
            title: "the unit".into(),
            body: "why and what".into(),
            head: "feature/my-unit".into(),
            base: "dev".into(),
            draft: true,
        };

        let opened = do_open(&remote, &fake, "Basic Zzo=", &pr).expect("create succeeds");
        assert_eq!(opened.number, 42);
        assert_eq!(
            opened.url, "https://dev.azure.com/suzano/florestal/_git/portal/pullrequest/42",
            "derived from the remote, never read from the response",
        );

        let calls = fake.calls.borrow();
        let call = calls.first().expect("one request");
        assert_eq!(call.auth, "Basic Zzo=");
        assert_eq!(
            call.body,
            Some(json!({
                "sourceRefName": "refs/heads/feature/my-unit",
                "targetRefName": "refs/heads/dev",
                "title": "the unit",
                "description": "why and what",
                "isDraft": true,
            })),
            "short names became full refs; the draft flag travelled",
        );
        drop(calls);

        // A response without an id is a parse error, never PR 0.
        let broken = FakeTransport::of(&[("POST", &create_url, json!({ "status": "active" }))]);
        assert_eq!(do_open(&remote, &broken, "a", &pr), Err("parse-error".to_string()));
    }

    /// The credential precedence: the env override wins when non-blank, the
    /// git vault answers otherwise, and the refusal NAMES both sources so the
    /// operator knows the two ways to fix it.
    #[test]
    fn azure_without_credential_refuses_naming_both_sources() {
        let url = "https://dev.azure.com/suzano/florestal/_git/portal";
        assert_eq!(
            pat_from(Some("env-pat".into()), || Some("vault-pat".into()), url),
            Ok("env-pat".to_string()),
            "the env var is the deliberate override",
        );
        assert_eq!(
            pat_from(Some("  ".into()), || Some("vault-pat".into()), url),
            Ok("vault-pat".to_string()),
            "a blank env var is not a credential",
        );
        assert_eq!(pat_from(None, || Some("vault-pat".into()), url), Ok("vault-pat".to_string()));

        let refusal = pat_from(None, || None, url).expect_err("no source, no credential");
        assert!(refusal.starts_with("azure-credential-missing"), "stable token: {refusal}");
        assert!(refusal.contains(PAT_ENV), "names the env source: {refusal}");
        assert!(refusal.contains("git credential"), "names the vault source: {refusal}");
        assert!(refusal.contains(url), "names the remote the vault was asked for: {refusal}");
    }

    /// An Azure `GitPullRequest` document folds into the port's normalised
    /// [`PrView`]: full refs come out short, the status word is reduced to
    /// the canonical vocabulary, `mergeStatus` travels verbatim, and the URL
    /// is derived.
    #[test]
    fn an_azure_response_folds_into_the_normalized_view() {
        let remote = remote();
        let view_url = format!("{}/9?api-version=7.1", remote.api_pulls());
        let fake = FakeTransport::of(&[(
            "GET",
            &view_url,
            json!({
                "pullRequestId": 9,
                "title": "the unit",
                "status": "active",
                "mergeStatus": "conflicts",
                "sourceRefName": "refs/heads/feature/my-unit",
                "targetRefName": "refs/heads/dev",
                "isDraft": false,
            }),
        )]);
        let view = do_view_number(&remote, &fake, "a", 9).expect("view succeeds");
        assert_eq!(
            view,
            PrView {
                number: 9,
                title: "the unit".into(),
                head: "feature/my-unit".into(),
                base: "dev".into(),
                status: PrStatus::Open,
                merge_status: Some("conflicts".into()),
                draft: false,
                url: "https://dev.azure.com/suzano/florestal/_git/portal/pullrequest/9".into(),
            }
        );
    }

    /// The three spellings of an Azure remote all derive the SAME facts: the
    /// REST base, the https remote the credential is asked for, and the PR
    /// web URL. The host family (dev.azure.com vs visualstudio.com) stays
    /// whatever the remote itself spoke.
    #[test]
    fn every_azure_remote_spelling_yields_the_rest_base() {
        let modern = [
            "https://dev.azure.com/suzano/florestal/_git/portal",
            "https://suzano@dev.azure.com/suzano/florestal/_git/portal",
            "git@ssh.dev.azure.com:v3/suzano/florestal/portal",
            "ssh://git@ssh.dev.azure.com/v3/suzano/florestal/portal",
        ];
        for url in modern {
            let remote = AzureRemote::parse(url).unwrap_or_else(|| panic!("{url:?} parses"));
            assert_eq!(
                remote.api_pulls(),
                "https://dev.azure.com/suzano/florestal/_apis/git/repositories/portal/pullrequests",
                "for {url:?}",
            );
            assert_eq!(
                remote.https_remote(),
                "https://dev.azure.com/suzano/florestal/_git/portal",
                "for {url:?}",
            );
            assert_eq!(
                remote.pr_url(7),
                "https://dev.azure.com/suzano/florestal/_git/portal/pullrequest/7",
                "for {url:?}",
            );
        }

        let legacy = [
            "https://suzano.visualstudio.com/florestal/_git/portal",
            "https://suzano.visualstudio.com/DefaultCollection/florestal/_git/portal",
            "suzano@vs-ssh.visualstudio.com:v3/suzano/florestal/portal",
        ];
        for url in legacy {
            let remote = AzureRemote::parse(url).unwrap_or_else(|| panic!("{url:?} parses"));
            assert_eq!(
                remote.https_remote(),
                "https://suzano.visualstudio.com/florestal/_git/portal",
                "for {url:?}",
            );
        }
    }
}

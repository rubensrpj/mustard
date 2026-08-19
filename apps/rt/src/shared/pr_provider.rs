//! `pr_provider` — the pull-request ACTIONS (open / edit / ready / view) as a
//! port, mirroring what [`crate::shared::branch_state`] already does for the
//! pull-request STATUS query ([`crate::shared::branch_state::PrLookup`]).
//!
//! The trait is what every caller depends on; no consumer ever names a provider
//! or its CLI. The adapters below are the ONLY place a provider and its
//! CLI/API are spelled — which is what an adapter IS. A provider with no
//! adapter answers the stable token [`PR_UNSUPPORTED`], never a fabricated
//! "absent": an unimplemented operation is an unmeasured state, not a measured
//! result.
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
/// The provider token [`AzurePrRest`] will adapt, as `resolve_provider`
/// spells it (`dev.azure.com` / `visualstudio.com` remotes, or declared).
const PROVIDER_AZURE: &str = "azure";
/// The full-ref namespace Azure prefixes branch names with.
const HEADS: &str = "refs/heads/";

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
}

// ---------------------------------------------------------------------------
// Azure — an honest skeleton, and the answer for every unadapted provider
// ---------------------------------------------------------------------------

/// The one answer of an operation nobody implemented: the stable token
/// [`PR_UNSUPPORTED`] — the same word `branch_state`'s status port uses for a
/// provider without an adapter, and for the same reason. Never a fabricated
/// success and never a measured-looking absence.
fn unsupported<T>() -> Result<T, String> {
    Err(PR_UNSUPPORTED.to_string())
}

/// The Azure DevOps adapter — a SKELETON that answers [`PR_UNSUPPORTED`] on
/// every operation until the REST client is implemented (next unit, with a
/// real credential to measure against).
///
/// What the implementation will hold to, verified against the official
/// `GitPullRequest` contract of the Azure DevOps REST reference (7.1):
///
/// - `sourceRefName` / `targetRefName` are FULL refs (`refs/heads/x`) —
///   normalised through [`short_ref`] before they leave the adapter.
/// - `status` (`PullRequestStatus`) stores `active`, `completed`, `abandoned`
///   or `notSet` (`all` is a search criterion only) — mapped through
///   [`status_from_azure`].
/// - `mergeStatus` (`PullRequestAsyncStatus`) has six values — `notSet`,
///   `queued`, `conflicts`, `succeeded`, `rejectedByPolicy`, `failure` — and
///   travels VERBATIM in [`PrView::merge_status`].
pub(crate) struct AzurePrRest;

impl PrProvider for AzurePrRest {
    fn provider(&self) -> &str {
        PROVIDER_AZURE
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
        PROVIDER_AZURE => Box::new(AzurePrRest),
        _ => Box::new(UnsupportedPr { provider }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    /// Every operation of the Azure skeleton — and of the adapter-less
    /// provider — answers the stable token, honest about not having measured
    /// or done anything. The same word `branch_state` uses, on purpose.
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
        for provider in [&AzurePrRest as &dyn PrProvider, &UnsupportedPr { provider: "gitlab".into() }] {
            assert_eq!(provider.open(&to_open), Err(token()));
            assert_eq!(provider.edit_body(1, "body"), Err(token()));
            assert_eq!(provider.ready(1), Err(token()));
            assert_eq!(provider.view(Some(1)), Err(token()));
        }
        assert_eq!(AzurePrRest.provider(), "azure");
        assert_eq!(UnsupportedPr { provider: "gitlab".into() }.provider(), "gitlab");
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
        assert_eq!(azure.view(Some(1)), Err(PR_UNSUPPORTED.to_string()), "skeleton, honestly");

        declare("gitlab");
        let other = provider_for(root);
        assert_eq!(other.provider(), "gitlab", "the report can still name who was asked-for");
        assert_eq!(other.ready(1), Err(PR_UNSUPPORTED.to_string()));
    }
}

//! `branch_state` — the ONE answer to "which work-unit branches exist, and what
//! state is each of them in".
//!
//! Two questions, deliberately split into two types that cannot answer each
//! other's:
//!
//! - [`BranchEnumerator`] answers **which refs exist**. It sweeps `refs/heads/`
//!   AND `refs/remotes/`, keeping only names whose `{base}_` prefix names a base
//!   the project itself declares (`mustard.json#git.flow`). It knows nothing
//!   about state. Sweeping BOTH namespaces is the whole point: the two sweeps
//!   this module replaces each looked at one half — a branch that lives only on
//!   the server was invisible to one, and an IN-PLACE unit (cut on the main
//!   checkout, no worktree — the default shape) was invisible to the other.
//! - [`StateClassifier`] answers **what state each branch is in**, crossing the
//!   enumerator with TWO local measurements — ancestry ([`merged_refs`]) and
//!   commits of the branch's own ([`refs_ahead_of_base`]) — and the [`PrLookup`]
//!   port. It never enumerates and it never acts.
//!
//! Why the state needs TWO local measurements and not ancestry alone: a branch
//! the work-branch gate cut seconds ago is reachable from its base in exactly
//! the way a merged one is, because it IS its base. Read on ancestry alone, the
//! unit the user is still editing answers "delivered, prune me". Cutting a
//! branch is not delivering work, so delivery requires the second, independent
//! fact — that the branch carries a commit of its own — and only the conjunction
//! of the two authorises a pruning verdict.
//!
//! Three properties are structural here, not disciplinary:
//!
//! 1. **The module cannot delete anything.** Its own source names none of the
//!    deleting argv — not the force-delete of a local branch, not the removal of
//!    a remote one, not the removal of a worktree. That capability lives in the
//!    exit ritual (`crate::commands::git_settle`) and nowhere else;
//!    `report_module_cannot_reach_deletion` reads both sources and requires
//!    exactly that split.
//! 2. **The read view carries no handle to git.** [`BranchState`] is plain data
//!    — no `Path`, no process, no callback. A consumer handed a slice of them
//!    (the report, the statusline) is provably unable to act on the repository,
//!    because the type it received exposes no way to.
//! 3. **An unmeasured PR is never reported as a negative.** [`PrStatus::Unknown`]
//!    carries the REASON and classifies as [`UnitState::Unmeasured`], never as
//!    "pushed without PR". Reporting a state nobody measured as if it had been
//!    measured negative is the exact defect class this module exists to end.
//!
//! **Never depends back on a face.** Per [`super`], `shared` is the leaf both
//! `hooks` and `commands` may depend on. The git primitive (`git_out`) lives in
//! the `commands` face, so this module does not import it — it takes the read as
//! [`GitOut`], a callback the caller supplies. That inverts the dependency the
//! same way [`PrLookup`] does for the network, and it makes every sweep testable
//! against a fixed listing instead of a real repository.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use serde_json::{json, Value};

/// The `refs/heads/` namespace, as `for-each-ref` prints it with `%(refname)`.
const HEADS: &str = "refs/heads/";
/// The `refs/remotes/` namespace. The remote NAME is read out of the ref itself
/// (its first path segment), never hardcoded — a project may call its remote
/// anything, and this module names no remote, base or provider literally.
const REMOTES: &str = "refs/remotes/";

/// How this module reads git: one call, argv in, stdout out, `None` when git
/// could not answer.
///
/// A callback rather than an import, so `shared` never depends back on the
/// `commands` face that owns the git primitive — and so every sweep here is
/// testable against a fixed listing.
pub(crate) type GitOut<'a> = &'a dyn Fn(&[&str]) -> Option<String>;

/// The base a work branch integrates into, read from its `{base}_` prefix
/// (tolerating the harness's `worktree-` prefix). `None` when the prefix names
/// no known base — such a branch is never a work unit, and the `None` propagates
/// out of `split_once`, so a name with no `_` at all (an integration base, a
/// stray ref, `HEAD`) is refused by construction.
///
/// The ONE predicate for this question in the crate: the exit ritual, the
/// enumerator and the spec inventory all ask it here. A second copy is how the
/// two sweeps this module replaces drifted apart in the first place.
pub(crate) fn base_of_branch(branch: &str, bases: &[String]) -> Option<String> {
    let name = branch.strip_prefix("worktree-").unwrap_or(branch);
    let (prefix, _) = name.split_once('_')?;
    bases.iter().find(|b| b.as_str() == prefix).cloned()
}

/// Split a full ref name into `(remote, branch)` — `remote` is `None` for a
/// local head. Any other namespace (tags, notes, stash) answers `None`.
fn split_ref(refname: &str) -> Option<(Option<&str>, &str)> {
    if let Some(local) = refname.strip_prefix(HEADS) {
        return Some((None, local));
    }
    let rest = refname.strip_prefix(REMOTES)?;
    let (remote, branch) = rest.split_once('/')?;
    Some((Some(remote), branch))
}

/// One work-unit branch as the ENUMERATOR sees it: its identity and WHERE its
/// refs live. Deliberately free of state — merged-ness, PRs and verdicts are the
/// classifier's answer, and keeping them out of this type is what stops a
/// consumer from mistaking "I found the ref" for "I know what it means".
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BranchRefs {
    /// The work-branch name, without any namespace or remote prefix.
    pub(crate) branch: String,
    /// The integration base its `{base}_` prefix names.
    pub(crate) base: String,
    /// Whether `refs/heads/<branch>` exists.
    pub(crate) local: bool,
    /// The remotes carrying it, sorted. Empty means no remote has it — which is
    /// NOT evidence of a merge (see [`UnitState::Danger`]).
    pub(crate) remotes: Vec<String>,
    /// The commit its readable ref ([`read_ref`](Self::read_ref)) points at, or
    /// empty when the sweep carried no object name.
    ///
    /// Still identity, not state: WHERE the ref points is part of what the ref
    /// IS. What that position means is [`refs_ahead_of_base`]'s answer, and the
    /// verdict is the classifier's.
    pub(crate) tip: String,
}

impl BranchRefs {
    /// The ref a READER should read this unit's tree from: the local head when
    /// there is one, else the first remote-tracking ref (`<remote>/<branch>`).
    ///
    /// The ONE place `<remote>/<branch>` is spelled. A consumer assembling it
    /// itself would have to name a remote, and no remote name is written in
    /// this crate — the name comes out of the ref that was swept.
    pub(crate) fn read_ref(&self) -> String {
        if self.local {
            return self.branch.clone();
        }
        match self.remotes.first() {
            Some(remote) => format!("{remote}/{branch}", branch = self.branch),
            None => self.branch.clone(),
        }
    }

    /// `true` when NO local ref carries this unit — only a remote does. Such a
    /// unit is invisible to any sweep of `refs/heads/` alone, which is the
    /// blind spot this module was built to close.
    pub(crate) fn is_remote_only(&self) -> bool {
        !self.local && !self.remotes.is_empty()
    }
}

/// Every work-unit branch of one repository, local and remote, sorted by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BranchEnumerator {
    units: Vec<BranchRefs>,
}

impl BranchEnumerator {
    /// Sweep both ref namespaces of the repository `git` reads.
    ///
    /// ONE `for-each-ref` covers both patterns, so the answer is a single
    /// consistent snapshot rather than two reads that can disagree. Fail-open:
    /// a git that cannot answer yields an empty sweep, never a panic.
    pub(crate) fn sweep(git: GitOut<'_>, bases: &[String]) -> Self {
        Self::try_sweep(git, bases).unwrap_or_else(|| Self::from_refs("", bases))
    }

    /// [`sweep`](Self::sweep), keeping apart "git could not answer" (`None`)
    /// and "this repository has no work branch" (an empty sweep).
    ///
    /// A consumer that REPORTS an absence needs that difference: an unanswered
    /// read printed as a verified "nothing in flight" is the same lie as an
    /// unmeasured PR printed as "no PR". A consumer that merely counts degrades
    /// through [`sweep`] and shows one fewer nudge.
    pub(crate) fn try_sweep(git: GitOut<'_>, bases: &[String]) -> Option<Self> {
        let listing =
            git(&["for-each-ref", "--format=%(refname) %(objectname)", HEADS, REMOTES])?;
        Some(Self::from_refs(&listing, bases))
    }

    /// The pure half of [`sweep`](Self::sweep): parse a `for-each-ref` listing.
    pub(crate) fn from_refs(listing: &str, bases: &[String]) -> Self {
        // Keyed by branch name so a unit with both a local head and one or more
        // remote-tracking refs is ONE entry, and so the output order is the
        // name order (the crate's determinism Guard admits no arbitrary order).
        let mut by_branch: BTreeMap<String, BranchRefs> = BTreeMap::new();
        for line in listing.lines() {
            // `%(refname) %(objectname)`. The object name is optional, so a
            // listing of bare names still parses — it simply carries no tip.
            let line = line.trim();
            let (refname, tip) = line.split_once(' ').unwrap_or((line, ""));
            let Some((remote, name)) = split_ref(refname) else { continue };
            let Some(base) = base_of_branch(name, bases) else { continue };
            let entry = by_branch.entry(name.to_string()).or_insert_with(|| BranchRefs {
                branch: name.to_string(),
                base,
                local: false,
                remotes: Vec::new(),
                tip: String::new(),
            });
            match remote {
                None => {
                    entry.local = true;
                    // The local head is the ref `read_ref` reads, so its tip is
                    // the unit's tip — whichever order the listing arrived in.
                    entry.tip = tip.to_string();
                }
                Some(r) => {
                    if !entry.local && entry.tip.is_empty() {
                        entry.tip = tip.to_string();
                    }
                    if !entry.remotes.iter().any(|known| known == r) {
                        entry.remotes.push(r.to_string());
                    }
                }
            }
        }
        let mut units: Vec<BranchRefs> = by_branch.into_values().collect();
        for unit in &mut units {
            unit.remotes.sort();
        }
        Self { units }
    }

    /// The swept units, sorted by branch name.
    pub(crate) fn units(&self) -> &[BranchRefs] {
        &self.units
    }
}

/// The work-branch names already reachable from their base — measured LOCALLY,
/// with no network at all.
///
/// `for-each-ref --merged <base>` answers over both namespaces, so a branch that
/// exists only on the server is measured by the same call as a local one. The
/// network only ever CONFIRMS this (via [`PrLookup`]); it is never required to
/// reach an answer, which is what keeps the sweep honest offline.
///
/// Fail-open per base: a base with no local ref simply contributes nothing.
pub(crate) fn merged_refs(git: GitOut<'_>, bases: &[String]) -> BTreeSet<String> {
    let mut merged: BTreeSet<String> = BTreeSet::new();
    for base in bases {
        let Some(listing) =
            git(&["for-each-ref", "--format=%(refname)", "--merged", base, HEADS, REMOTES])
        else {
            continue;
        };
        for line in listing.lines() {
            let Some((_, name)) = split_ref(line.trim()) else { continue };
            if base_of_branch(name, bases).is_some() {
                merged.insert(name.to_string());
            }
        }
    }
    merged
}

/// The work-branch names carrying at least ONE commit of their own — the units
/// that actually delivered something, as opposed to the ones that were merely
/// cut.
///
/// **Why not `rev-list --count <base>..<branch>`.** Measured, not assumed: once
/// a unit is merged its commits are reachable from the base, so that range
/// answers `0` — the SAME answer a branch cut a second ago and never committed
/// on gives. The range cannot tell the two apart, and the work-branch gate cuts
/// every new unit in precisely the second shape (`checkout -b <unit> <base>`,
/// no commit), so a verdict built on it announces live work as delivered.
///
/// The base's MAINLINE can tell them apart. A fresh cut's tip IS a commit of the
/// base's first-parent line; a unit merged with a merge commit hangs its tip off
/// that line as the second parent, where it never appears. Verified in a scratch
/// repository and against this one.
///
/// Fail-open, and always toward silence: a base whose mainline git will not read
/// contributes nothing, so its units carry no own commits and can never reach a
/// pruning verdict. Under-reporting costs a nudge; over-reporting offers to
/// delete a branch that delivered nothing.
pub(crate) fn refs_ahead_of_base(
    git: GitOut<'_>,
    units: &[BranchRefs],
    bases: &[String],
) -> BTreeSet<String> {
    let mut ahead: BTreeSet<String> = BTreeSet::new();
    for base in bases {
        let mine: Vec<&BranchRefs> = units.iter().filter(|u| &u.base == base).collect();
        if mine.is_empty() {
            continue;
        }
        let Some(listing) = git(&["rev-list", "--first-parent", base]) else { continue };
        let mainline: BTreeSet<&str> =
            listing.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        for unit in mine {
            if !unit.tip.is_empty() && !mainline.contains(unit.tip.as_str()) {
                ahead.insert(unit.branch.clone());
            }
        }
    }
    ahead
}

// ---------------------------------------------------------------------------
// The PR query, as a port
// ---------------------------------------------------------------------------

/// What the PR query answered for one branch.
///
/// [`Absent`](PrStatus::Absent) is a MEASUREMENT — the query ran and found
/// nothing. [`Unknown`](PrStatus::Unknown) is the absence of a measurement, and
/// carries why. Collapsing the two is the defect this enum exists to prevent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrStatus {
    /// Measured: no pull request was ever opened from this branch.
    Absent,
    /// A pull request from this branch is open.
    Open,
    /// A pull request from this branch was merged.
    Merged,
    /// A pull request from this branch was closed WITHOUT merging.
    Closed,
    /// Not measured. The payload is a stable reason token, never free prose.
    Unknown(&'static str),
}

impl PrStatus {
    /// The stable token this status prints as in a report.
    pub(crate) fn token(self) -> &'static str {
        match self {
            PrStatus::Absent => "absent",
            PrStatus::Open => "open",
            PrStatus::Merged => "merged",
            PrStatus::Closed => "closed",
            PrStatus::Unknown(_) => "unknown",
        }
    }
}

/// Reason: the configured provider has no adapter here, so nothing was asked.
pub(crate) const PR_UNSUPPORTED: &str = "provider-unsupported";
/// Reason: the provider's CLI could not be launched (absent from `PATH`).
pub(crate) const PR_CLI_ABSENT: &str = "provider-cli-absent";
/// Reason: the CLI ran and failed — unauthenticated, offline, or not a repo of
/// that provider. All three are "we did not measure", never "there is no PR".
pub(crate) const PR_CLI_FAILED: &str = "provider-cli-failed";
/// Reason: the CLI answered something this adapter could not read.
pub(crate) const PR_UNREADABLE: &str = "provider-answer-unreadable";

/// Reason: the consumer deliberately did not ask (see [`LocalOnlyPr`]).
pub(crate) const PR_NOT_QUERIED: &str = "pr-not-queried";

/// The PR query as a PORT.
///
/// The classifier depends on this trait and never on a provider's CLI, so a new
/// provider is a new adapter and not one line of new logic in the classifier.
pub(crate) trait PrLookup {
    /// The strongest known status of any pull request whose HEAD is `branch`.
    fn status_of(&self, branch: &str) -> PrStatus;
}

/// The port for a consumer that asks NOTHING — every branch answers
/// [`PrStatus::Unknown`]`(`[`PR_NOT_QUERIED`]`)`.
///
/// A surface redrawn on every keystroke (the status bar) or blocking the start
/// of a session cannot afford a network round-trip per branch, so it measures
/// LOCAL ancestry only. The classifier then reaches a pruning verdict only
/// where ancestry already proved the merge; everything else stays
/// [`UnitState::Unmeasured`]. Such a count can only UNDER-report — a merge the
/// provider squashed leaves no ancestry — and never invent a prunable branch.
/// A missed nudge is a nuisance; an invented one offers to delete work nobody
/// verified.
pub(crate) struct LocalOnlyPr;

impl PrLookup for LocalOnlyPr {
    fn status_of(&self, _branch: &str) -> PrStatus {
        PrStatus::Unknown(PR_NOT_QUERIED)
    }
}

/// The adapter for the provider declared in `mustard.json#git.provider`.
///
/// This is the ONE place in the module where a provider and its CLI are named —
/// which is what an adapter IS. A provider without an adapter here answers
/// [`PrStatus::Unknown`], never [`PrStatus::Absent`]: an unimplemented query is
/// an unmeasured state, not a measured "no PR".
pub(crate) struct ProviderPrCli<'a> {
    repo: &'a Path,
    provider: &'a str,
}

/// The one provider this module can currently ask, and the CLI that asks it.
///
/// Query and JSON shape verified against the official `gh pr list` manual and
/// confirmed live (gh 2.96.0): `--head`, `--state open|closed|merged|all`,
/// `--limit`, `--json` with the fields `number,state,mergedAt,headRefName`;
/// `state` comes back UPPERCASE (`OPEN` / `CLOSED` / `MERGED`), and a query that
/// matches nothing prints `[]` and exits 0 — an empty array is therefore a real
/// measurement of absence, not a failure.
const PROVIDER_GITHUB: &str = "github";
const GITHUB_CLI: &str = "gh";
/// How many PRs to reduce over. A branch can carry several (a closed attempt
/// then a merged one); taking only the newest would let ordering decide the
/// verdict, so the strongest status among a handful wins instead.
const PR_SCAN_LIMIT: &str = "10";

impl<'a> ProviderPrCli<'a> {
    /// Bind the adapter to one repository and the project's declared provider.
    pub(crate) fn new(repo: &'a Path, provider: &'a str) -> Self {
        Self { repo, provider }
    }

    /// Reduce the CLI's rows to one status: merged beats open beats closed, and
    /// an empty array is a measured absence.
    fn reduce(rows: &[Value]) -> PrStatus {
        let states: Vec<String> = rows
            .iter()
            .filter_map(|row| row["state"].as_str())
            .map(str::to_ascii_uppercase)
            .collect();
        if states.iter().any(|s| s == "MERGED") {
            PrStatus::Merged
        } else if states.iter().any(|s| s == "OPEN") {
            PrStatus::Open
        } else if states.is_empty() {
            PrStatus::Absent
        } else {
            PrStatus::Closed
        }
    }
}

impl PrLookup for ProviderPrCli<'_> {
    fn status_of(&self, branch: &str) -> PrStatus {
        if !self.provider.eq_ignore_ascii_case(PROVIDER_GITHUB) {
            return PrStatus::Unknown(PR_UNSUPPORTED);
        }
        let Ok(out) = Command::new(GITHUB_CLI)
            .args([
                "pr",
                "list",
                "--head",
                branch,
                "--state",
                "all",
                "--limit",
                PR_SCAN_LIMIT,
                "--json",
                "state",
            ])
            .current_dir(self.repo)
            .output()
        else {
            return PrStatus::Unknown(PR_CLI_ABSENT);
        };
        if !out.status.success() {
            return PrStatus::Unknown(PR_CLI_FAILED);
        }
        let body = String::from_utf8_lossy(&out.stdout);
        let Ok(Value::Array(rows)) = serde_json::from_str::<Value>(body.trim()) else {
            return PrStatus::Unknown(PR_UNREADABLE);
        };
        Self::reduce(&rows)
    }
}

// ---------------------------------------------------------------------------
// The classifier
// ---------------------------------------------------------------------------

/// The state ONE work-unit branch is in — exactly one per branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnitState {
    /// Local, no remote, no PR: a plan that was never approved. Never merged, so
    /// a sweep may never delete it.
    DraftAbandoned,
    /// Pushed, but no pull request is open for it.
    PushedWithoutPr,
    /// A pull request from it is open.
    InReview,
    /// Merged AND the remote branch is still there: both sides can be pruned —
    /// the remote alone where the local ref is already gone.
    AwaitingPrune,
    /// Merged and the remote branch is already gone: only the local one remains.
    AwaitingPruneLocal,
    /// The remote is gone and the merge is NOT verified. A branch deleted
    /// WITHOUT merging looks exactly like a merged one whose remote was
    /// auto-deleted, so this state exists to keep the two apart: it is the one
    /// that must never be offered for deletion.
    Danger,
    /// Only the server has it — there is no local ref to prune.
    RemoteOnly,
    /// The PR query could not answer, so no verdict is claimed. It exists so an
    /// unmeasured branch is never dressed up as [`PushedWithoutPr`](Self::PushedWithoutPr).
    Unmeasured,
}

impl UnitState {
    /// The stable token this state prints as in a report.
    pub(crate) fn token(self) -> &'static str {
        match self {
            UnitState::DraftAbandoned => "draft-abandoned",
            UnitState::PushedWithoutPr => "pushed-without-pr",
            UnitState::InReview => "in-review",
            UnitState::AwaitingPrune => "awaiting-prune",
            UnitState::AwaitingPruneLocal => "awaiting-prune-local",
            UnitState::Danger => "danger",
            UnitState::RemoteOnly => "remote-only",
            UnitState::Unmeasured => "unmeasured",
        }
    }

    /// Whether this state means "the work landed; the branch may go".
    pub(crate) fn is_awaiting_prune(self) -> bool {
        matches!(self, UnitState::AwaitingPrune | UnitState::AwaitingPruneLocal)
    }
}

/// One branch, classified — the READ view.
///
/// Plain data by design: no path, no process handle, no callback. A consumer
/// handed these (the report, the statusline) is structurally unable to act on
/// the repository, because the type it received offers no way to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BranchState {
    /// The work-branch name.
    pub(crate) branch: String,
    /// The integration base it belongs to.
    pub(crate) base: String,
    /// Whether a local ref exists.
    pub(crate) local: bool,
    /// The remotes carrying it, sorted.
    pub(crate) remotes: Vec<String>,
    /// Whether it is already reachable from its base — measured locally.
    pub(crate) ancestry: bool,
    /// Whether it carries a commit of its OWN — measured locally, and the fact
    /// that separates a delivered unit from one that was only cut.
    pub(crate) ahead: bool,
    /// What the PR port answered.
    pub(crate) pr: PrStatus,
    /// The single verdict.
    pub(crate) state: UnitState,
}

/// Crosses the enumerator with local ancestry and the PR port.
///
/// It does not enumerate and it does not act: everything it needs arrives as an
/// argument, and everything it produces is data.
pub(crate) struct StateClassifier<'a> {
    pr: &'a dyn PrLookup,
}

impl<'a> StateClassifier<'a> {
    /// Bind a classifier to a PR port.
    pub(crate) fn new(pr: &'a dyn PrLookup) -> Self {
        Self { pr }
    }

    /// One verdict per enumerated branch, in the enumerator's order.
    ///
    /// `merged` is the locally measured ancestry set ([`merged_refs`]) and
    /// `ahead` the locally measured set of units carrying commits of their own
    /// ([`refs_ahead_of_base`]); the PR port only ever CONFIRMS a merge the
    /// local measurement missed (a portal that squashes produces no ancestry),
    /// and can never turn a verified merge back into a doubt.
    pub(crate) fn classify(
        &self,
        units: &[BranchRefs],
        merged: &BTreeSet<String>,
        ahead: &BTreeSet<String>,
    ) -> Vec<BranchState> {
        units
            .iter()
            .map(|unit| {
                let ancestry = merged.contains(&unit.branch);
                let carries_own = ahead.contains(&unit.branch);
                let pr = self.pr.status_of(&unit.branch);
                let state = verdict(unit, ancestry, carries_own, pr);
                BranchState {
                    branch: unit.branch.clone(),
                    base: unit.base.clone(),
                    local: unit.local,
                    remotes: unit.remotes.clone(),
                    ancestry,
                    ahead: carries_own,
                    pr,
                    state,
                }
            })
            .collect()
    }
}

/// The verdict table, isolated so the seven-plus-one situations read as one
/// piece.
///
/// Two load-bearing rules:
///
/// 1. An absent remote is NEVER evidence of a merge. Git marks the upstream of
///    any deleted remote branch `gone`, merged or not.
/// 2. Ancestry is never evidence of DELIVERY on its own. A branch that was cut
///    and never committed on is reachable from its base because it is a copy of
///    it, so a verdict that stopped at ancestry announced live work as landed.
///
/// Only `merged_verified` — local ancestry over a branch that carries commits of
/// its own, or a merge the provider confirms — authorises the pruning states.
fn verdict(unit: &BranchRefs, ancestry: bool, ahead: bool, pr: PrStatus) -> UnitState {
    let merged_verified = (ancestry && ahead) || pr == PrStatus::Merged;
    let remote_alive = !unit.remotes.is_empty();
    // The merge is weighed BEFORE the absence of a local ref: a unit whose local
    // branch is already gone but whose REMOTE outlived the merge still owes a
    // prune — of the remote. Answering `RemoteOnly` first hid exactly the remote
    // branches the field report counted alongside the local ones.
    if merged_verified {
        return if remote_alive {
            UnitState::AwaitingPrune
        } else {
            UnitState::AwaitingPruneLocal
        };
    }
    if !unit.local {
        return UnitState::RemoteOnly;
    }
    if remote_alive {
        match pr {
            PrStatus::Open => UnitState::InReview,
            // A PR that was closed unmerged leaves the branch exactly where one
            // that never had a PR sits: pushed and unintegrated.
            PrStatus::Absent | PrStatus::Closed => UnitState::PushedWithoutPr,
            PrStatus::Unknown(_) => UnitState::Unmeasured,
            // Unreachable: a merged PR is caught by `merged_verified` above.
            PrStatus::Merged => UnitState::AwaitingPrune,
        }
    } else {
        // No remote and no verified merge. Only a MEASURED absence of a PR tells
        // an abandoned draft from a branch whose remote vanished under it.
        match pr {
            PrStatus::Absent => UnitState::DraftAbandoned,
            _ => UnitState::Danger,
        }
    }
}

/// The units whose merge is VERIFIED and whose branch is still around — the
/// ONE definition of "the exit ritual is still owed here", shared by every
/// surface that shows it.
///
/// A second count somewhere else is exactly how the two sweeps this module
/// replaced drifted apart, so the status bar, the session-start advisory and
/// any later consumer all fold through this function: same enumeration, same
/// ancestry measurement, same verdict table.
pub(crate) fn awaiting_prune(
    git: GitOut<'_>,
    pr: &dyn PrLookup,
    bases: &[String],
) -> Vec<BranchState> {
    let units = BranchEnumerator::sweep(git, bases);
    let merged = merged_refs(git, bases);
    let ahead = refs_ahead_of_base(git, units.units(), bases);
    StateClassifier::new(pr)
        .classify(units.units(), &merged, &ahead)
        .into_iter()
        .filter(|state| state.state.is_awaiting_prune())
        .collect()
}

// ---------------------------------------------------------------------------
// The report — read-only by construction
// ---------------------------------------------------------------------------

/// One repository's inventory as JSON: sorted, tokenised, no timestamps and no
/// machine paths, so the output is byte-stable per the crate's Guard.
///
/// It takes measured states and nothing else — no repository, no git handle —
/// which is what makes the reading phase provably incapable of deleting
/// anything rather than merely disciplined about it.
pub(crate) fn report_value(repo: &str, states: &[BranchState]) -> Value {
    let units: Vec<Value> = states
        .iter()
        .map(|s| {
            json!({
                "branch": s.branch,
                "base": s.base,
                "state": s.state.token(),
                "local": s.local,
                "remotes": s.remotes,
                "ancestry": s.ancestry,
                "ahead": s.ahead,
                "pr": pr_value(s.pr),
            })
        })
        .collect();
    let awaiting: Vec<String> = states
        .iter()
        .filter(|s| s.state.is_awaiting_prune())
        .map(|s| s.branch.clone())
        .collect();
    json!({
        "repo": repo,
        // Stated, because the sibling shape a consumer prints when the refs
        // would not read carries `ok:false`: an empty `units` then means
        // "measured, nothing in flight" rather than "nobody looked".
        "ok": true,
        "units": units,
        "awaitingPrune": awaiting,
    })
}

/// The PR column: the status token, plus the REASON whenever nothing was
/// measured.
fn pr_value(pr: PrStatus) -> Value {
    match pr {
        PrStatus::Unknown(reason) => json!({ "status": pr.token(), "reason": reason }),
        _ => json!({ "status": pr.token() }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `PrLookup` that answers from a table — the port's payoff: the seven
    /// situations are testable without a network, a token or a provider.
    struct FakePr(BTreeMap<String, PrStatus>);

    impl FakePr {
        fn of(pairs: &[(&str, PrStatus)]) -> Self {
            Self(pairs.iter().map(|(b, s)| ((*b).to_string(), *s)).collect())
        }
    }

    impl PrLookup for FakePr {
        fn status_of(&self, branch: &str) -> PrStatus {
            self.0.get(branch).copied().unwrap_or(PrStatus::Absent)
        }
    }

    fn bases() -> Vec<String> {
        vec!["dev".to_string(), "main".to_string()]
    }

    /// Run git in `root`, failing the test with git's own words.
    fn run(root: &Path, args: &[&str]) {
        let out = Command::new("git").args(args).current_dir(root).output().expect("spawn git");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr),
        );
    }

    /// Read git in `root` — the test twin of the callback production injects.
    ///
    /// Local on purpose: borrowing the command face's reader would invert the
    /// DAG `shared/mod.rs` declares impossible, and a test that breaks the
    /// layering still breaks it (found in review, 2026-07-30).
    fn read(root: &Path, args: &[&str]) -> Option<String> {
        let out = Command::new("git").args(args).current_dir(root).output().ok()?;
        if !out.status.success() {
            return None;
        }
        let s = String::from_utf8(out.stdout).ok()?;
        let s = s.trim().to_string();
        if s.is_empty() { None } else { Some(s) }
    }

    /// A throwaway repository with one commit on a base the project would
    /// declare, and the repository's own root.
    ///
    /// The two criteria below run against REAL git on purpose. Both defects they
    /// pin survived every hand-written listing in this file, because a fixture
    /// author writes down the shape they already have in mind — and neither of
    /// these shapes was in anyone's mind.
    fn scratch_repo() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().to_path_buf();
        run(&root, &["init", "."]);
        run(&root, &["config", "user.email", "t@t"]);
        run(&root, &["config", "user.name", "t"]);
        run(&root, &["config", "commit.gpgsign", "false"]);
        run(&root, &["checkout", "-b", "dev"]);
        std::fs::write(root.join("seed.txt"), "seed").expect("seed file");
        run(&root, &["add", "-A"]);
        run(&root, &["commit", "-m", "seed"]);
        (dir, root)
    }

    /// AC-1 — the enumerator returns BOTH families (local heads and refs that
    /// exist only on a remote), filtered by base prefix, and a ref with no `_`
    /// after the prefix — an integration base, `HEAD`, a stray name — never
    /// enters. One sweep, both halves: each of the two sweeps this module
    /// replaces saw only one of them.
    #[test]
    fn branch_enumerator_sees_local_and_remote_refs() {
        let listing = "\
refs/heads/dev aaa0
refs/heads/dev_local-only aaa1
refs/heads/dev_both aaa2
refs/heads/nounderscore aaa3
refs/heads/feature_x aaa4
refs/remotes/origin/HEAD aaa5
refs/remotes/origin/dev aaa0
refs/remotes/origin/dev_both bbb2
refs/remotes/origin/dev_remote-only bbb6
refs/remotes/upstream/dev_both ccc2
refs/tags/v1.0_dev aaa7
";
        let found = BranchEnumerator::from_refs(listing, &bases());
        let names: Vec<&str> = found.units().iter().map(|u| u.branch.as_str()).collect();
        assert_eq!(
            names,
            vec!["dev_both", "dev_local-only", "dev_remote-only"],
            "both families, sorted; a base, a bare name and a foreign prefix never enter",
        );

        let both = &found.units()[0];
        assert!(both.local, "the local head of dev_both was seen");
        assert_eq!(both.remotes, vec!["origin", "upstream"], "every remote carrying it, sorted");
        assert_eq!(both.base, "dev", "the base comes from the prefix, never from a literal");
        assert_eq!(both.tip, "aaa2", "the tip of the ref `read_ref` reads — the local head");

        let local_only = &found.units()[1];
        assert!(local_only.local);
        assert!(local_only.remotes.is_empty(), "no remote carries it");

        let remote_only = &found.units()[2];
        assert!(!remote_only.local, "a branch that exists ONLY on the server: no local ref");
        assert_eq!(remote_only.remotes, vec!["origin"]);
        assert_eq!(remote_only.tip, "bbb6", "with no local head, the remote ref gives the tip");

        // The base itself is excluded by the same predicate the exit ritual uses
        // — `split_once('_')` propagating `None` — so the sweep can never offer
        // an integration base for anything.
        assert_eq!(base_of_branch("dev", &bases()), None);
        assert!(
            !found.units().iter().any(|u| u.branch == "dev"),
            "an integration base is never a work unit",
        );
    }

    /// A ref that only a remote carries is READ from `<remote>/<branch>`, and
    /// a read that git could not answer stays telling apart from a repository
    /// with no work branch — the two properties the spec inventory needs to
    /// stop enumerating on its own.
    #[test]
    fn remote_only_units_carry_their_read_ref_and_a_failed_sweep_is_not_an_empty_one() {
        let found = BranchEnumerator::from_refs(
            "refs/heads/dev_local\nrefs/remotes/origin/dev_remote\n",
            &bases(),
        );
        let local = &found.units()[0];
        assert!(!local.is_remote_only());
        assert_eq!(local.read_ref(), "dev_local", "a local head is read by its own name");

        let remote = &found.units()[1];
        assert!(remote.is_remote_only(), "no local ref carries it");
        assert_eq!(
            remote.read_ref(),
            "origin/dev_remote",
            "the remote name comes out of the swept ref, never from a literal",
        );

        // Fail to answer vs. answer nothing: the reporting consumer needs both.
        let silent = |_: &[&str]| None;
        assert!(BranchEnumerator::try_sweep(&silent, &bases()).is_none(), "git said nothing");
        let empty = |_: &[&str]| Some(String::new());
        let swept = BranchEnumerator::try_sweep(&empty, &bases()).expect("git answered");
        assert!(swept.units().is_empty(), "an answer of no branches is a measurement");
        // The degrading face keeps the old contract for consumers that count.
        assert!(BranchEnumerator::sweep(&silent, &bases()).units().is_empty());
    }

    /// The read-only consumers' composition: a lookup that asks NOTHING still
    /// counts what LOCAL ancestry proved, and can never invent a prunable
    /// branch out of a merge nobody measured.
    #[test]
    fn local_only_lookup_counts_verified_merges_and_invents_none() {
        const SWEPT: &str = "refs/heads/dev_landed 1a\nrefs/remotes/origin/dev_landed 1a\n\
                             refs/heads/dev_live 2a\nrefs/remotes/origin/dev_live 2a\n\
                             refs/heads/dev_gone 3a\n";
        let git = |args: &[&str]| -> Option<String> {
            // Only `dev_landed` is reachable from its base.
            if args.contains(&"--merged") {
                return Some("refs/heads/dev_landed\n".to_string());
            }
            // The base's mainline: every unit here carries commits of its own,
            // so none of their tips sits on it.
            if args.contains(&"rev-list") {
                return Some("d0\nd1\n".to_string());
            }
            Some(SWEPT.to_string())
        };
        let pending = awaiting_prune(&git, &LocalOnlyPr, &bases());
        let names: Vec<&str> = pending.iter().map(|s| s.branch.as_str()).collect();
        assert_eq!(names, vec!["dev_landed"], "only the verified merge is owed a prune");
        assert_eq!(pending[0].state, UnitState::AwaitingPrune);
        assert_eq!(
            pending[0].pr,
            PrStatus::Unknown(PR_NOT_QUERIED),
            "the count says out loud that it never asked the provider",
        );
        assert_ne!(
            pending[0].pr,
            PrStatus::Absent,
            "not asking is never the same as measuring that there is no PR",
        );
        // `dev_gone` has no remote and an unmeasured PR: dangerous, never
        // offered for pruning — the whole reason this lookup under-reports.
        assert!(!names.contains(&"dev_gone"));
    }

    /// AC-4 — `gone` (no remote) alone NEVER authorises deletion. A branch
    /// deleted without merging and one merged whose remote was auto-deleted look
    /// identical from the local side; only a VERIFIED merge separates them.
    #[test]
    fn gone_alone_never_authorises_deletion() {
        let units = vec![
            // Remote vanished, merge NOT verified — the dangerous one.
            BranchRefs {
                branch: "dev_gone-unmerged".into(),
                base: "dev".into(),
                local: true,
                remotes: Vec::new(),
                tip: "1a".into(),
            },
            // Remote vanished AND the merge is verified locally — prunable.
            BranchRefs {
                branch: "dev_gone-merged".into(),
                base: "dev".into(),
                local: true,
                remotes: Vec::new(),
                tip: "2a".into(),
            },
        ];
        // The unmerged one even has a PR on record, so the only difference that
        // can produce the two verdicts is the ancestry measurement — both carry
        // commits of their own.
        let pr = FakePr::of(&[
            ("dev_gone-unmerged", PrStatus::Open),
            ("dev_gone-merged", PrStatus::Absent),
        ]);
        let merged: BTreeSet<String> = ["dev_gone-merged".to_string()].into_iter().collect();
        let ahead: BTreeSet<String> = units.iter().map(|u| u.branch.clone()).collect();
        let states = StateClassifier::new(&pr).classify(&units, &merged, &ahead);

        assert_eq!(states[0].state, UnitState::Danger, "gone + unverified merge = danger");
        assert!(
            !states[0].state.is_awaiting_prune(),
            "the dangerous branch must never be offered for pruning",
        );
        assert_eq!(
            states[1].state,
            UnitState::AwaitingPruneLocal,
            "only a verified merge turns a gone remote into a prune",
        );
        assert!(states[1].state.is_awaiting_prune());

        // And the report agrees: exactly one branch is listed as prunable.
        let value = report_value(".", &states);
        assert_eq!(value["awaitingPrune"], json!(["dev_gone-merged"]));
    }

    /// AC-5 — an absent or unauthenticated provider CLI answers UNKNOWN with a
    /// reason, never "no PR". Both halves are asserted: the adapter refuses to
    /// invent an answer it did not measure, and the classifier refuses to turn
    /// that non-answer into the negative verdict `pushed-without-pr`.
    #[test]
    fn absent_provider_answers_unknown_never_absent() {
        // --- the adapter half: a provider with no adapter is UNMEASURED ------
        let cli = ProviderPrCli::new(Path::new("."), "a-provider-with-no-adapter");
        let answer = cli.status_of("dev_anything");
        assert_eq!(answer, PrStatus::Unknown(PR_UNSUPPORTED), "unimplemented ≠ measured absence");
        assert_ne!(answer, PrStatus::Absent, "an unmeasured query is never reported as no-PR");

        // An empty array IS a measurement, though — that distinction is the
        // whole point of keeping the two apart.
        assert_eq!(ProviderPrCli::reduce(&[]), PrStatus::Absent);
        assert_eq!(
            ProviderPrCli::reduce(&[json!({"state": "CLOSED"}), json!({"state": "MERGED"})]),
            PrStatus::Merged,
            "the strongest status wins, so row order never decides the verdict",
        );

        // --- the classifier half: unknown never becomes a negative verdict ---
        let units = vec![BranchRefs {
            branch: "dev_pushed".into(),
            base: "dev".into(),
            local: true,
            remotes: vec!["origin".into()],
            tip: "1a".into(),
        }];
        let pr = FakePr::of(&[("dev_pushed", PrStatus::Unknown(PR_CLI_FAILED))]);
        let ahead: BTreeSet<String> = ["dev_pushed".to_string()].into_iter().collect();
        let states = StateClassifier::new(&pr).classify(&units, &BTreeSet::new(), &ahead);
        assert_eq!(states[0].state, UnitState::Unmeasured);
        assert_ne!(
            states[0].state,
            UnitState::PushedWithoutPr,
            "reporting an unmeasured state as a negative is the defect this module ends",
        );

        // --- and the report carries the REASON, not just the non-answer ------
        let value = report_value(".", &states);
        assert_eq!(value["units"][0]["pr"]["status"], json!("unknown"));
        assert_eq!(value["units"][0]["pr"]["reason"], json!(PR_CLI_FAILED));
        assert_eq!(value["awaitingPrune"], json!([]), "nothing unmeasured is ever prunable");
    }

    /// The remaining situations of the table, so all eight are pinned by a test
    /// and not only the two an acceptance criterion names.
    #[test]
    fn classifier_answers_one_state_per_situation() {
        let unit = |branch: &str, local: bool, remote: bool| BranchRefs {
            branch: branch.to_string(),
            base: "dev".to_string(),
            local,
            remotes: if remote { vec!["origin".to_string()] } else { Vec::new() },
            tip: format!("{branch}-tip"),
        };
        let units = vec![
            unit("dev_draft", true, false),
            unit("dev_pushed", true, true),
            unit("dev_review", true, true),
            unit("dev_landed", true, true),
            unit("dev_remote", false, true),
        ];
        let pr = FakePr::of(&[
            ("dev_draft", PrStatus::Absent),
            ("dev_pushed", PrStatus::Absent),
            ("dev_review", PrStatus::Open),
            ("dev_landed", PrStatus::Merged),
        ]);
        let ahead: BTreeSet<String> = units.iter().map(|u| u.branch.clone()).collect();
        let states = StateClassifier::new(&pr).classify(&units, &BTreeSet::new(), &ahead);
        let tokens: Vec<&str> = states.iter().map(|s| s.state.token()).collect();
        assert_eq!(
            tokens,
            vec!["draft-abandoned", "pushed-without-pr", "in-review", "awaiting-prune", "remote-only"],
        );
        // The provider CONFIRMS a merge local ancestry could not see (a portal
        // that squashes leaves no ancestry) — the network adds evidence, never
        // removes it.
        assert!(!states[3].ancestry, "no local ancestry for the squashed one");
        assert!(states[3].state.is_awaiting_prune());
    }

    /// AC-6 — the reading phase is structurally unable to delete a branch.
    ///
    /// Read like the plugin-prose tests: BOTH halves, so the assertion can
    /// actually fail. Half one — this module's own source names no deleting
    /// argv. Half two — the exit ritual's source DOES, which proves the needles
    /// are the real spellings and that the capability simply lives elsewhere.
    /// The needles are assembled at runtime so writing the test does not put the
    /// forbidden spellings into the file under assertion.
    #[test]
    fn report_module_cannot_reach_deletion() {
        let here = include_str!("branch_state.rs");
        let ritual = include_str!("../commands/git_settle.rs");

        let delete_branch = ["\"-", "D\""].concat(); // the force-delete argv
        let delete_remote = ["--", "delete"].concat(); // the remote-delete argv
        let remove_worktree = ["\"worktree\", \"", "remove\""].concat();
        for needle in [&delete_branch, &delete_remote, &remove_worktree] {
            assert!(
                !here.contains(needle.as_str()),
                "the read module must not name the deleting argv {needle}",
            );
            assert!(
                ritual.contains(needle.as_str()),
                "{needle} must still exist in the exit ritual — otherwise this test asserts \
                 nothing about where the capability lives",
            );
        }

        // The READ view is plain data: a consumer handed these cannot reach the
        // repository, because the type carries no path, no process and no
        // callback to reach it with.
        let view = here
            .split_once("pub(crate) struct BranchState {")
            .and_then(|(_, rest)| rest.split_once("\n}"))
            .map(|(body, _)| body)
            .unwrap_or_default();
        assert!(!view.is_empty(), "the read view must still be a struct this test can read");
        for forbidden in ["Path", "Command", "Fn(", "GitOut"] {
            assert!(
                !view.contains(forbidden),
                "the read view must carry no {forbidden} — it is data, not a capability",
            );
        }
    }

    /// AC-11 — a work branch with NO commit of its own is NEVER offered for
    /// pruning.
    ///
    /// The work-branch gate opens every unit with `checkout -b <unit> <base>`
    /// and nothing else, so from the cut until the first commit the branch is
    /// reachable from its base in exactly the way a merged one is. Read on
    /// ancestry alone, the unit the user is EDITING answered "delivered, prune
    /// me" — and the advisory built on that answer hands out the command that
    /// deletes it. Cutting a branch is not delivering work.
    ///
    /// Both halves, so the assertions can fail: the same branch, one commit and
    /// one merge later, IS owed its prune.
    #[test]
    fn a_branch_with_no_commits_ahead_is_never_awaiting_prune() {
        let (_dir, root) = scratch_repo();
        run(&root, &["checkout", "-b", "dev_fresh"]);
        let git_read = |args: &[&str]| read(&root, args);

        // Ancestry answers YES — the branch IS its base — and that used to be
        // the whole verdict.
        let merged = merged_refs(&git_read, &bases());
        assert!(
            merged.contains("dev_fresh"),
            "git reports a freshly cut branch as merged into its base",
        );
        let swept = BranchEnumerator::sweep(&git_read, &bases());
        let ahead = refs_ahead_of_base(&git_read, swept.units(), &bases());
        assert!(!ahead.contains("dev_fresh"), "a branch just cut carries no commit of its own");

        assert!(
            awaiting_prune(&git_read, &LocalOnlyPr, &bases()).is_empty(),
            "the unit being edited must never be announced as delivered",
        );
        let states = StateClassifier::new(&LocalOnlyPr).classify(swept.units(), &merged, &ahead);
        assert!(!states[0].state.is_awaiting_prune(), "not prunable: {states:?}");
        assert!(
            states[0].ancestry,
            "the veto is the absent commits, not an absent ancestry — otherwise this test \
             would pass on a sweep that simply saw nothing",
        );

        // The other half: deliver, merge, and the same branch is owed a prune.
        std::fs::write(root.join("work.txt"), "w").expect("work file");
        run(&root, &["add", "-A"]);
        run(&root, &["commit", "-m", "work"]);
        run(&root, &["checkout", "dev"]);
        run(&root, &["merge", "--no-ff", "dev_fresh", "-m", "merge dev_fresh"]);
        let pending = awaiting_prune(&git_read, &LocalOnlyPr, &bases());
        let names: Vec<&str> = pending.iter().map(|s| s.branch.as_str()).collect();
        assert_eq!(names, vec!["dev_fresh"], "a unit that delivered commits IS owed its prune");
    }

    /// AC-12 — a unit whose merge is verified, whose local ref is already gone
    /// and whose REMOTE branch is still alive is owed a prune (of the remote),
    /// instead of being filed away as "only on the server".
    ///
    /// The verdict table answered `remote-only` before it ever weighed the
    /// merge, so the REMOTE branches the field report counted alongside the
    /// local ones could not enter the pending list at all — half the motivating
    /// measurement stayed invisible to the report meant to surface it.
    #[test]
    fn merged_unit_alive_only_on_the_remote_is_awaiting_prune() {
        let (_dir, root) = scratch_repo();
        run(&root, &["checkout", "-b", "dev_landed"]);
        std::fs::write(root.join("work.txt"), "w").expect("work file");
        run(&root, &["add", "-A"]);
        run(&root, &["commit", "-m", "work"]);
        run(&root, &["checkout", "dev"]);
        run(&root, &["merge", "--no-ff", "dev_landed", "-m", "merge dev_landed"]);
        // The remote outlives the local ref — the shape a machine that already
        // pruned locally, or never had the branch, is left in.
        run(&root, &["update-ref", "refs/remotes/origin/dev_landed", "refs/heads/dev_landed"]);
        run(&root, &["update-ref", "-d", "refs/heads/dev_landed"]);

        let git_read = |args: &[&str]| read(&root, args);
        let swept = BranchEnumerator::sweep(&git_read, &bases());
        let unit = swept.units().iter().find(|u| u.branch == "dev_landed").expect("unit swept");
        assert!(!unit.local, "no local ref carries it any more");
        assert_eq!(unit.remotes, vec!["origin"], "…but the remote is still alive");

        let pending = awaiting_prune(&git_read, &LocalOnlyPr, &bases());
        let names: Vec<&str> = pending.iter().map(|s| s.branch.as_str()).collect();
        assert_eq!(names, vec!["dev_landed"], "the remote of a merged unit is owed its prune");
        assert_eq!(pending[0].state, UnitState::AwaitingPrune);

        let merged = merged_refs(&git_read, &bases());
        let ahead = refs_ahead_of_base(&git_read, swept.units(), &bases());
        let states = StateClassifier::new(&LocalOnlyPr).classify(swept.units(), &merged, &ahead);
        assert_eq!(report_value(".", &states)["awaitingPrune"], json!(["dev_landed"]));

        // And the reordering swallowed nothing: a remote-only unit that did NOT
        // land is still remote-only.
        let stranger = vec![BranchRefs {
            branch: "dev_elsewhere".into(),
            base: "dev".into(),
            local: false,
            remotes: vec!["origin".into()],
            tip: "9a".into(),
        }];
        let unlanded = StateClassifier::new(&LocalOnlyPr).classify(
            &stranger,
            &BTreeSet::new(),
            &BTreeSet::new(),
        );
        assert_eq!(unlanded[0].state, UnitState::RemoteOnly);
    }
}

//! `branch_state` — the ONE answer to "which work-unit branches exist, and what
//! state is each of them in".
//!
//! Two questions, deliberately split into two types that cannot answer each
//! other's:
//!
//! - [`BranchEnumerator`] answers **which refs exist**. It sweeps `refs/heads/`
//!   AND `refs/remotes/`, keeping only names [`BaseFlow`] recognises as a work
//!   unit's of this project (`mustard.json#git.flow`). It knows nothing
//!   about state. Sweeping BOTH namespaces is the whole point: the two sweeps
//!   this module replaces each looked at one half — a branch that lives only on
//!   the server was invisible to one, and an IN-PLACE unit (cut on the main
//!   checkout, no worktree — the default shape) was invisible to the other.
//! - [`classify`] answers **what state each branch is in**, crossing the
//!   enumerator with TWO local measurements — ancestry ([`try_merged_refs`]) and
//!   commits of the branch's own ([`refs_ahead_of_base`]) — and whatever the
//!   provider answered ([`PrQuery`]). It never enumerates and it never acts.
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
//! 4. **The evidence is per REF, never per branch NAME.** A unit is a set of
//!    refs that happen to share a name, and each one carries its own answer
//!    ([`RefVerdict`]): contained in the base NOW, or covered by the frozen head
//!    of a merged pull request. Collapsing them to the name is how a local ref
//!    that landed vouched for a remote ref that had since moved ahead — and the
//!    sweep then offered to delete a remote carrying unintegrated commits. Every
//!    existing ref must be accounted for before any pruning state is reached;
//!    a merged pull request whose refs moved answers
//!    [`UnitState::MovedAfterMerge`], because PR history says what HAPPENED
//!    while pruning asks what EXISTS, and the first answer ages while the branch
//!    keeps moving.
//!
//! **It reads git directly, and it says which repository.** This module used to
//! take its read as a callback: the crate's only git primitive lived in the
//! `commands` face, and per [`super`] `shared` may not depend back on a face, so
//! the read was inverted into a parameter. The primitive moved to the shared
//! library ([`mustard_core::platform::git`]), which `shared` may import like any
//! other leaf — and from that moment the callback inverted nothing. What was
//! left of it was a seam for a test to pretend to be git, and a sweep proved
//! against a listing somebody typed proves the typing. Every read below is that
//! one executor, given the repository root; the tests build a real repository in
//! a temporary directory.
//!
//! The same applies to the two PORTS this module used to declare — the pull
//! request query and the reachability query — and to the three implementations
//! and two doubles they carried. Reachability is a git read like every other one
//! here, so it is now one call. The pull request query has exactly two shapes a
//! consumer ever chooses between, and two closed cases are [`PrQuery`], not a
//! trait: a surface that cannot afford a round trip asks nothing, and everybody
//! else asks the declared provider.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use mustard_core::platform::git;
use serde_json::{json, Value};

use crate::shared::work_kind::BaseFlow;

/// The `refs/heads/` namespace, as `for-each-ref` prints it with `%(refname)`.
const HEADS: &str = "refs/heads/";
/// The `refs/remotes/` namespace. The remote NAME is read out of the ref itself
/// (its first path segment), never hardcoded — a project may call its remote
/// anything, and this module names no remote, base or provider literally.
const REMOTES: &str = "refs/remotes/";

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

    /// Every ref that CURRENTLY carries this unit, spelled the way
    /// `for-each-ref` spells it — the local head first, then one per remote.
    ///
    /// The unit's IDENTITY is the branch name; its EVIDENCE is not. A local head
    /// already on the base while `<remote>/<branch>` sits three commits ahead is
    /// two facts, and a set keyed on the name can only hold one of them. This is
    /// the list every per-ref measurement iterates.
    pub(crate) fn refnames(&self) -> Vec<String> {
        let mut names = Vec::new();
        if self.local {
            names.push(format!("{HEADS}{branch}", branch = self.branch));
        }
        for remote in &self.remotes {
            names.push(format!("{REMOTES}{remote}/{branch}", branch = self.branch));
        }
        names
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
    pub(crate) fn sweep(root: &Path, flow: &BaseFlow) -> Self {
        Self::try_sweep(root, flow).unwrap_or_else(|| Self::from_refs("", flow))
    }

    /// [`sweep`](Self::sweep), keeping apart "git could not answer" (`None`)
    /// and "this repository has no work branch" (an empty sweep).
    ///
    /// A consumer that REPORTS an absence needs that difference: an unanswered
    /// read printed as a verified "nothing in flight" is the same lie as an
    /// unmeasured PR printed as "no PR". A consumer that merely counts degrades
    /// through [`sweep`] and shows one fewer nudge.
    pub(crate) fn try_sweep(root: &Path, flow: &BaseFlow) -> Option<Self> {
        let listing =
            git::run(root, &["for-each-ref", "--format=%(refname) %(objectname)", HEADS, REMOTES])
                .out()?;
        Some(Self::from_refs(&listing, flow))
    }

    /// The pure half of [`sweep`](Self::sweep): parse a `for-each-ref` listing.
    pub(crate) fn from_refs(listing: &str, flow: &BaseFlow) -> Self {
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
            // Enumerated by IDENTITY, not by the base: a unit whose base nothing
            // established is still this project's unit, and dropping it here
            // would make the sweep blind to exactly the branches whose base a
            // reader most needs told. `base` is then the empty string — a unit
            // that belongs to no base group can never reach a pruning verdict,
            // which is the safe direction for every consumer of this sweep.
            let answer = flow.base_of(name);
            if !answer.is_unit() {
                continue;
            }
            let base = answer.into_known().unwrap_or_default();
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

    /// Fill in the base of every swept unit whose base nothing RECORDED, by
    /// MEASURING which declared base already contains it.
    ///
    /// [`from_refs`](Self::from_refs) is pure, so it can only answer from the
    /// unit's own directory; with several declared bases a unit nobody recorded
    /// comes back `Ambiguous` and is filed under the empty base. Every
    /// per-base measurement below then skips it —
    /// [`refs_ahead_of_base`] iterates `flow.bases()` and matches on equality —
    /// so the unit is invisible to every consumer of this sweep. Measured
    /// 2026-08-28: a branch cut by hand (no `work-unit-open`) merged into `dev`
    /// and left alive was reported by nothing at all, which is the exact debt
    /// the prune advisory exists to name.
    ///
    /// Containment IS the answer this sweep needs, and it needs no record: a
    /// unit reachable from exactly ONE declared base was demonstrably merged
    /// there. `git-settle` already reaches for that measurement by hand when a
    /// record is missing; this puts it where every consumer benefits.
    ///
    /// Reachable from NO base — the ordinary unmerged unit — keeps the empty
    /// base: there is nothing to prune and nothing to claim.
    ///
    /// **Reachable from SEVERAL is the ordinary state, not the odd one, and
    /// refusing to answer there was this function's own first bug.** The moment
    /// `dev` is promoted into `main`, every unit merged into `dev` becomes
    /// reachable from `main` as well — so a rule of "several candidates, say
    /// nothing" goes blind on exactly the repositories that ship regularly, and
    /// goes blind the day after a release. The tie is broken by where work
    /// LANDS ([`BaseFlow::work_base`]): a unit reachable from the work base was
    /// delivered there, whatever else has since absorbed it. Only when the work
    /// base is not among the holders is the answer withheld.
    pub(crate) fn resolve_unrecorded_bases(&mut self, root: &Path, flow: &BaseFlow) {
        if !self.units.iter().any(|u| u.base.is_empty()) {
            return;
        }
        // One reachability read per declared base, reusing the module's ONE
        // primitive — never one read per unrecorded unit.
        let per_base: Vec<(&String, BTreeSet<String>)> =
            flow.bases().iter().map(|base| (base, refs_merged_into(root, base))).collect();
        let work_base = flow.work_base().unwrap_or_default();
        for unit in self.units.iter_mut().filter(|u| u.base.is_empty()) {
            let refnames = unit.refnames();
            let holders: Vec<&str> = per_base
                .iter()
                .filter(|(_, merged)| refnames.iter().any(|r| merged.contains(r)))
                .map(|(base, _)| base.as_str())
                .collect();
            unit.base = match holders.as_slice() {
                [] => continue,
                [only] => (*only).to_string(),
                many if many.contains(&work_base) => work_base.to_string(),
                _ => continue,
            };
        }
    }
}

/// The FULL refnames git reports as reachable from `commit` — the ONE
/// reachability read of this module.
///
/// Two questions fold through it, and they must never drift apart: "is this ref
/// on its base now" ([`try_merged_refs`], asked of a base) and "does this merge
/// account for that ref" ([`PrEvidence::covered_refs`], asked of a merged pull
/// request's frozen head). One `for-each-ref` covers both ref namespaces, so a branch that
/// exists only on the server is measured by the same call as a local one.
///
/// Fail-open, and always toward silence: a commit git cannot resolve — a squash
/// head this clone never fetched — yields an EMPTY set, so absent evidence can
/// only withhold a prune, never authorise one.
pub(crate) fn refs_merged_into(root: &Path, commit: &str) -> BTreeSet<String> {
    if commit.is_empty() {
        return BTreeSet::new();
    }
    let Some(listing) = git::run(
        root,
        &["for-each-ref", "--format=%(refname)", "--merged", commit, HEADS, REMOTES],
    )
    .out() else {
        return BTreeSet::new();
    };
    listing.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect()
}

/// The work-unit REFS already reachable from their base — measured LOCALLY, with
/// no network at all — plus whether git ANSWERED at all.
///
/// Keyed on the full refname, not on the branch name. The name-keyed set this
/// replaces inserted `dev_x` as soon as ANY of its refs was contained, so a
/// local head that had landed vouched for a remote ref that had since moved
/// ahead — and the deleting side, trusting that name-level evidence, removed a
/// remote carrying commits nobody had integrated.
///
/// The network only ever CONFIRMS this (via [`PrQuery`]); it is never required
/// to reach an answer, which is what keeps the sweep honest offline. Fail-open
/// per base: a base with no local ref simply contributes nothing.
///
/// The second half of the pair keeps apart "git ANSWERED, nothing is contained"
/// from "git never answered" — the same split [`BranchEnumerator::try_sweep`]
/// draws, and for the same reason.
///
/// The two are indistinguishable in the set alone, and they ask for opposite
/// verdicts: the first says a merged branch really moved past its merge, the
/// second says nobody looked. Handing the flag to
/// [`classify`] is what keeps the second from being printed as the first.
///
/// **How the answer is recognised.** A `--merged <base>` read that answered
/// always carries at least the base's own ref — a commit is reachable from
/// itself. An EMPTY listing is therefore git declining (absent base, unreadable
/// repository), never a repository in which nothing is contained. One base
/// answering is enough: a base with no local ref legitimately contributes
/// nothing, so demanding all of them would report a healthy read as unmeasured.
pub(crate) fn try_merged_refs(root: &Path, flow: &BaseFlow) -> (BTreeSet<String>, bool) {
    let mut merged: BTreeSet<String> = BTreeSet::new();
    let mut measured = false;
    for base in flow.bases() {
        let listing = refs_merged_into(root, base);
        if listing.is_empty() {
            continue;
        }
        measured = true;
        for refname in listing {
            let is_unit =
                split_ref(&refname).is_some_and(|(_, name)| flow.base_of(name).is_unit());
            if is_unit {
                merged.insert(refname);
            }
        }
    }
    (merged, measured)
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
    root: &Path,
    units: &[BranchRefs],
    flow: &BaseFlow,
) -> BTreeSet<String> {
    let mut ahead: BTreeSet<String> = BTreeSet::new();
    for base in flow.bases() {
        let mine: Vec<&BranchRefs> = units.iter().filter(|u| &u.base == base).collect();
        if mine.is_empty() {
            continue;
        }
        let Some(listing) = git::run(root, &["rev-list", "--first-parent", base]).out() else {
            continue;
        };
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

/// Reason: the consumer deliberately did not ask (see [`PrQuery::Skip`]).
pub(crate) const PR_NOT_QUERIED: &str = "pr-not-queried";

/// What the PR query answered, WITH the evidence a pruning decision needs.
///
/// The status alone was never enough. `Merged` says a pull request from this
/// branch landed ONCE; it says nothing about where the branch sits today, and a
/// verdict built on it stayed "prunable" for as long as the record existed —
/// including after somebody pushed new commits to the same branch. The frozen
/// heads are what turn history into a measurement: each one accounts for exactly
/// the refs it contains, and a ref beyond them is not accounted for by anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrEvidence {
    /// The strongest status measured.
    pub(crate) status: PrStatus,
    /// The head commit of every MERGED pull request of this branch.
    ///
    /// A SET, not the newest one: a branch can carry several merged pull
    /// requests, and picking one would let ROW ORDER decide which merge counts —
    /// the same trap [`PrQuery::reduce`] already refuses for the status.
    pub(crate) merged_heads: BTreeSet<String>,
}

impl PrEvidence {
    /// The evidence of a query nobody ran — the honest zero value.
    pub(crate) fn unqueried() -> Self {
        Self { status: PrStatus::Unknown(PR_NOT_QUERIED), merged_heads: BTreeSet::new() }
    }

    /// The refs these merges account for: every ref contained in some frozen
    /// head, read in `root`. Empty when nothing merged, and empty is never
    /// evidence.
    fn covered_refs(&self, root: &Path) -> BTreeSet<String> {
        self.merged_heads.iter().flat_map(|head| refs_merged_into(root, head)).collect()
    }
}

/// WHETHER to ask about pull requests at all — the only decision a consumer of
/// this module ever takes about the network, and a closed one.
///
/// This was a trait with two adapters and a test double. The double went with
/// the injected git read; what remained were two cases nobody adds a third to
/// from outside the crate, which is an enum. A consumer chooses:
///
/// - [`Skip`](PrQuery::Skip) — every branch answers
///   [`PrStatus::Unknown`]`(`[`PR_NOT_QUERIED`]`)`. A surface redrawn on every
///   keystroke (the status bar) or blocking the start of a session cannot
///   afford a round trip per branch, so it measures LOCAL ancestry only. The
///   classification then reaches a pruning verdict only where ancestry already
///   proved the merge; everything else stays [`UnitState::Unmeasured`]. Such a
///   count can only UNDER-report — a merge the provider squashed leaves no
///   ancestry — and never invent a prunable branch. A missed nudge is a
///   nuisance; an invented one offers to delete work nobody verified.
/// - [`Ask`](PrQuery::Ask) — the provider declared in
///   `mustard.json#git.provider` is asked.
#[derive(Debug, Clone, Copy)]
pub(crate) enum PrQuery<'a> {
    /// Ask nothing, and say so in the reason.
    Skip,
    /// Ask this provider.
    Ask(&'a str),
}

/// The one provider this module can currently ask, and the CLI that asks it.
///
/// Query and JSON shape verified against the official `gh pr list` manual and
/// confirmed live (gh 2.96.0): `--head`, `--state open|closed|merged|all`,
/// `--limit`, `--json` with the fields `number,state,mergedAt,headRefName`;
/// `state` comes back UPPERCASE (`OPEN` / `CLOSED` / `MERGED`), and a query that
/// matches nothing prints `[]` and exits 0 — an empty array is therefore a real
/// measurement of absence, not a failure.
///
/// `headRefOid` comes back in the SAME call, one round-trip for both facts.
/// GitHub's own GraphQL reference defines it as "the oid of the head ref
/// associated with the pull request, **even if the ref has been deleted**" — a
/// value RECORDED on the pull request, not a live lookup of where the branch
/// points; a head only advances through the `synchronize` event, which an
/// already-merged (hence closed) pull request no longer receives. Measured
/// against that contract on this repository: five merged pull requests all
/// opened from the branch `dev` report five DIFFERENT heads, while `dev` itself
/// sits on a sixth commit. A live pointer would have answered the same sha six
/// times.
const PROVIDER_GITHUB: &str = "github";
const GITHUB_CLI: &str = "gh";
/// How many PRs to reduce over. A branch can carry several (a closed attempt
/// then a merged one); taking only the newest would let ordering decide the
/// verdict, so the strongest status among a handful wins instead.
const PR_SCAN_LIMIT: &str = "10";

impl PrQuery<'_> {
    /// Reduce the CLI's rows to one piece of evidence: merged beats open beats
    /// closed, an empty array is a measured absence, and every merged row
    /// contributes its frozen head.
    fn reduce(rows: &[Value]) -> PrEvidence {
        let state_of = |row: &Value| row["state"].as_str().map(str::to_ascii_uppercase);
        let states: Vec<String> = rows.iter().filter_map(state_of).collect();
        let status = if states.iter().any(|s| s == "MERGED") {
            PrStatus::Merged
        } else if states.iter().any(|s| s == "OPEN") {
            PrStatus::Open
        } else if states.is_empty() {
            PrStatus::Absent
        } else {
            PrStatus::Closed
        };
        let merged_heads: BTreeSet<String> = rows
            .iter()
            .filter(|row| state_of(row).as_deref() == Some("MERGED"))
            .filter_map(|row| row["headRefOid"].as_str())
            .filter(|head| !head.is_empty())
            .map(str::to_string)
            .collect();
        PrEvidence { status, merged_heads }
    }

    /// What is known about the pull requests whose HEAD is `branch`, in `repo`.
    ///
    /// This is the ONE place in the module where a provider and its CLI are
    /// named. GitHub is asked through `gh` below; Azure is routed to the
    /// REST-speaking [`crate::shared::pr_azure::evidence_of`], the same search
    /// and reduction over the injectable transport that module already proves.
    /// A provider with an adapter on neither side answers
    /// [`PrStatus::Unknown`], never [`PrStatus::Absent`]: an unimplemented
    /// query is an unmeasured state, not a measured "no PR".
    pub(crate) fn evidence_of(&self, repo: &Path, branch: &str) -> PrEvidence {
        let unknown = |reason| PrEvidence { status: PrStatus::Unknown(reason), merged_heads: BTreeSet::new() };
        let Self::Ask(provider) = *self else {
            return PrEvidence::unqueried();
        };
        if provider.eq_ignore_ascii_case(crate::shared::pr_provider::PROVIDER_AZURE) {
            // Azure speaks REST, not a CLI — same answer shape, reduced in
            // `pr_azure` where its fake transport can prove it.
            return crate::shared::pr_azure::evidence_of(repo, branch);
        }
        if !provider.eq_ignore_ascii_case(PROVIDER_GITHUB) {
            return unknown(PR_UNSUPPORTED);
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
                // ONE call for both facts: what happened, and what the merge
                // actually accounted for. Two calls could disagree.
                "state,headRefOid",
            ])
            .current_dir(repo)
            .output()
        else {
            return unknown(PR_CLI_ABSENT);
        };
        if !out.status.success() {
            return unknown(PR_CLI_FAILED);
        }
        let body = String::from_utf8_lossy(&out.stdout);
        let Ok(Value::Array(rows)) = serde_json::from_str::<Value>(body.trim()) else {
            return unknown(PR_UNREADABLE);
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
    /// A pull request from it MERGED, and then one of its refs moved beyond what
    /// that merge accounts for. The work landed; the branch did not stop. It is
    /// not prunable, because deleting it would take the commits added after the
    /// merge with it.
    MovedAfterMerge,
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
            UnitState::MovedAfterMerge => "moved-after-merge",
            UnitState::Unmeasured => "unmeasured",
        }
    }

    /// Whether this state means "the work landed; the branch may go".
    pub(crate) fn is_awaiting_prune(self) -> bool {
        matches!(self, UnitState::AwaitingPrune | UnitState::AwaitingPruneLocal)
    }
}

/// One REF of a unit with its OWN evidence — the branch name only groups them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RefVerdict {
    /// The full refname, as `for-each-ref` spells it.
    pub(crate) refname: String,
    /// Contained NOW: reachable from the base at measurement time.
    pub(crate) contained: bool,
    /// Covered by a merge: reachable from the frozen head of a merged pull
    /// request. This is the squash case, where containment never holds.
    pub(crate) covered: bool,
}

impl RefVerdict {
    /// Whether SOMETHING accounts for this ref. Nothing else may.
    pub(crate) fn accounted(&self) -> bool {
        self.contained || self.covered
    }
}

/// The per-REF evidence of one unit, in [`BranchRefs::refnames`] order.
///
/// `contained` is measured against whatever reference point the CALLER chose —
/// the local base for the report, the server's base for the exit ritual, which
/// fetches first. The predicate is one; the vantage point is the caller's.
pub(crate) fn ref_verdicts(
    unit: &BranchRefs,
    contained: &BTreeSet<String>,
    evidence: &PrEvidence,
    root: &Path,
) -> Vec<RefVerdict> {
    let covered = evidence.covered_refs(root);
    unit.refnames()
        .into_iter()
        .map(|refname| RefVerdict {
            contained: contained.contains(&refname),
            covered: covered.contains(&refname),
            refname,
        })
        .collect()
}

/// Whether EVERY existing ref of a unit is accounted for — the ONE predicate
/// that authorises pruning, asked here by the classifier and by the exit ritual.
///
/// A unit with no ref at all answers `false`: there is nothing to prove and
/// nothing to prune, and "vacuously merged" is the shape that let an empty
/// measurement pass for a positive one.
pub(crate) fn all_refs_accounted(verdicts: &[RefVerdict]) -> bool {
    !verdicts.is_empty() && verdicts.iter().all(RefVerdict::accounted)
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
    /// The per-ref evidence this verdict was reached on.
    pub(crate) refs: Vec<RefVerdict>,
    /// Whether EVERY existing ref is already reachable from its base — measured
    /// locally, per ref.
    pub(crate) ancestry: bool,
    /// Whether it carries a commit of its OWN — measured locally, and the fact
    /// that separates a delivered unit from one that was only cut.
    pub(crate) ahead: bool,
    /// What the PR query answered.
    pub(crate) pr: PrStatus,
    /// The single verdict.
    pub(crate) state: UnitState,
}

/// One verdict per enumerated branch, in the enumerator's order — the crossing
/// of the enumerator with local ancestry and the pull request query.
///
/// It does not enumerate and it does not act: everything it needs arrives as an
/// argument, and everything it produces is data. It used to be a struct bound to
/// two ports and carrying one flag through a builder; with the ports gone the
/// struct held nothing a call could not say, and a builder for a single boolean
/// is a second way to spell an argument.
///
/// `merged` is the locally measured ancestry set ([`try_merged_refs`]) and
/// `ahead` the locally measured set of units carrying commits of their own
/// ([`refs_ahead_of_base`]); the pull request query only ever CONFIRMS a merge
/// the local measurement missed (a portal that squashes produces no ancestry),
/// and can never turn a verified merge back into a doubt.
///
/// `reach_measured` says whether the containment read behind `merged` actually
/// ANSWERED. [`try_merged_refs`] is fail-open: a git that will not answer yields
/// an empty set, indistinguishable from "nothing is contained". Told apart, the
/// two ask for opposite verdicts — the second means the branch really moved past
/// its merge, the first means nobody looked — and printing the unmeasured one as
/// the measured one is principle 3 of this module broken from the other side
/// (found in review, 2026-07-30).
pub(crate) fn classify(
    root: &Path,
    pr_query: PrQuery<'_>,
    units: &[BranchRefs],
    merged: &BTreeSet<String>,
    ahead: &BTreeSet<String>,
    reach_measured: bool,
) -> Vec<BranchState> {
    units
        .iter()
        .map(|unit| {
            let evidence = pr_query.evidence_of(root, &unit.branch);
            state_of(root, unit, &evidence, merged, ahead, reach_measured)
        })
        .collect()
}

/// ONE branch's state, given the evidence the provider returned FOR IT.
///
/// Split out of [`classify`] because the evidence is a VALUE: everything the
/// provider contributes arrives here as [`PrEvidence`], so every situation of
/// the verdict table can be measured against a real repository by handing this
/// the evidence a provider would have returned — with no double standing in for
/// the provider, and no second copy of this body in a test.
fn state_of(
    root: &Path,
    unit: &BranchRefs,
    evidence: &PrEvidence,
    merged: &BTreeSet<String>,
    ahead: &BTreeSet<String>,
    reach_measured: bool,
) -> BranchState {
    let refs = ref_verdicts(unit, merged, evidence, root);
    // Reported `ancestry` is the LOCAL half alone — every ref on the base right
    // now — so a reader can still tell "git proved it" from "the provider
    // vouched for it".
    let ancestry = !refs.is_empty() && refs.iter().all(|r| r.contained);
    let accounted = all_refs_accounted(&refs);
    let carries_own = ahead.contains(&unit.branch);
    let pr = evidence.status;
    let state = verdict(unit, accounted, carries_own, pr, reach_measured);
    BranchState {
        branch: unit.branch.clone(),
        base: unit.base.clone(),
        local: unit.local,
        remotes: unit.remotes.clone(),
        refs,
        ancestry,
        ahead: carries_own,
        pr,
        state,
    }
}

/// The verdict table, isolated so the eight-plus-one situations read as one
/// piece.
///
/// Three load-bearing rules:
///
/// 1. An absent remote is NEVER evidence of a merge. Git marks the upstream of
///    any deleted remote branch `gone`, merged or not.
/// 2. Reachability is never evidence of DELIVERY on its own. A branch that was
///    cut and never committed on is reachable from its base because it is a copy
///    of it, so a verdict that stopped there announced live work as landed.
/// 3. A merged pull request is never evidence of the PRESENT. It proves the work
///    landed once; `accounted` — every ref of the unit contained now or covered
///    by that merge's frozen head — is the only thing that proves nothing has
///    moved since.
///
/// Only the conjunction of `accounted` and `delivered` authorises the pruning
/// states; a merge whose refs moved is [`UnitState::MovedAfterMerge`].
fn verdict(
    unit: &BranchRefs,
    accounted: bool,
    ahead: bool,
    pr: PrStatus,
    reach_measured: bool,
) -> UnitState {
    // Delivery: a commit of its own, or a merge the provider confirms. A freshly
    // cut branch has neither, which is what keeps live work off the prune list.
    let delivered = ahead || pr == PrStatus::Merged;
    let remote_alive = !unit.remotes.is_empty();
    // The merge is weighed BEFORE the absence of a local ref: a unit whose local
    // branch is already gone but whose REMOTE outlived the merge still owes a
    // prune — of the remote. Answering `RemoteOnly` first hid exactly the remote
    // branches the field report counted alongside the local ones.
    if accounted && delivered {
        return if remote_alive {
            UnitState::AwaitingPrune
        } else {
            UnitState::AwaitingPruneLocal
        };
    }
    // Merged, but some ref reaches past what the merge accounts for. Weighed
    // before `RemoteOnly` for the same reason as above: the ref that moved is
    // usually the remote one, and filing it as "only on the server" would hide
    // precisely the unintegrated commits this state exists to name.
    //
    // …unless the containment read never ANSWERED. Then nothing was measured,
    // and "the branch moved past its merge" would be a claim nobody checked —
    // the same lie as an unmeasured PR printed as "no PR", which principle 3
    // of this module exists to refuse. Neither state is prunable, so the
    // correction costs nothing but the truth of the label.
    if pr == PrStatus::Merged {
        return if reach_measured { UnitState::MovedAfterMerge } else { UnitState::Unmeasured };
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
            // Unreachable: every merged PR is caught by one of the two returns
            // above, and the arm keeps the match exhaustive without a wildcard.
            PrStatus::Merged => UnitState::MovedAfterMerge,
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
    root: &Path,
    pr_query: PrQuery<'_>,
    flow: &BaseFlow,
) -> Vec<BranchState> {
    let mut units = BranchEnumerator::sweep(root, flow);
    // Before any per-base measurement: a unit filed under the empty base is
    // skipped by every one of them, so resolving here is what makes a
    // hand-cut unit visible at all.
    units.resolve_unrecorded_bases(root, flow);
    let (merged, measured) = try_merged_refs(root, flow);
    let ahead = refs_ahead_of_base(root, units.units(), flow);
    classify(root, pr_query, units.units(), &merged, &ahead, measured)
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
                // The evidence the verdict was reached on, per REF. Without it a
                // `moved-after-merge` is an assertion the reader cannot check —
                // and WHICH ref moved is the only actionable part of it.
                "refs": s.refs.iter().map(ref_value).collect::<Vec<Value>>(),
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

/// One ref's own row: what it is, and what accounts for it.
fn ref_value(verdict: &RefVerdict) -> Value {
    json!({
        "ref": verdict.refname,
        "contained": verdict.contained,
        "coveredByPr": verdict.covered,
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

    /// O fluxo de duas camadas que todo teste daqui lê: o trabalho sobe para
    /// `dev` e `dev` sobe para `main`.
    fn bases() -> BaseFlow {
        let mut git = mustard_core::domain::config::GitConfig::default();
        git.flow.insert("*".to_string(), "dev".to_string());
        git.flow.insert("dev".to_string(), "main".to_string());
        BaseFlow::of(&git)
    }

    /// Roda o git em `root`, e falha o teste com as palavras do próprio git.
    fn run(root: &Path, args: &[&str]) {
        let out = git::run(root, args);
        assert!(out.ok, "git {args:?} falhou: {}", out.stderr);
    }

    /// O commit a que `rev` aponta em `root`.
    fn sha(root: &Path, rev: &str) -> String {
        git::run(root, &["rev-parse", rev]).out().expect("o git resolve a referência")
    }

    /// Um repositório de mentira nenhuma, em pasta temporária: um commit numa
    /// base que o projeto declararia, e a raiz dele.
    ///
    /// Tudo que lê o git neste arquivo é medido aqui. A listagem escrita à mão
    /// que ficava no lugar deste repositório só provava a listagem: quem a
    /// escreve anota a forma que já tem em mente, e os dois defeitos que este
    /// arquivo existe para pegar não estavam na cabeça de ninguém.
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

    /// Corta `branch` de `dev`, entrega um commit nela, e volta para `dev`.
    /// Devolve o commit entregue.
    fn deliver(root: &Path, branch: &str) -> String {
        run(root, &["checkout", "-q", "-b", branch, "dev"]);
        std::fs::write(root.join(format!("{branch}.txt")), branch).expect("arquivo da unidade");
        run(root, &["add", "-A"]);
        run(root, &["commit", "-q", "-m", branch]);
        let tip = sha(root, branch);
        run(root, &["checkout", "-q", "dev"]);
        tip
    }

    /// A unidade `branch` como o varredor a enxerga no repositório `root`.
    fn swept_unit(root: &Path, branch: &str) -> BranchRefs {
        BranchEnumerator::sweep(root, &bases())
            .units()
            .iter()
            .find(|u| u.branch == branch)
            .cloned()
            .unwrap_or_else(|| panic!("a unidade {branch} tinha de ser varrida"))
    }

    /// Uma unidade de mão, para as situações da tabela que não dependem de
    /// nenhuma leitura do repositório.
    fn unit(branch: &str, local: bool, remote: bool) -> BranchRefs {
        BranchRefs {
            branch: branch.to_string(),
            base: "dev".to_string(),
            local,
            remotes: if remote { vec!["origin".to_string()] } else { Vec::new() },
            tip: format!("{branch}-tip"),
        }
    }

    /// O que o provedor teria respondido: só um valor, nunca um dublê.
    fn evidence(status: PrStatus, heads: &[&str]) -> PrEvidence {
        PrEvidence {
            status,
            merged_heads: heads.iter().map(|h| (*h).to_string()).collect(),
        }
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
        // — a bare base carries neither a kind nor a `{base}_` prefix — so the
        // sweep can never offer an integration base for anything.
        assert!(!bases().base_of("dev").is_unit());
        assert!(
            !found.units().iter().any(|u| u.branch == "dev"),
            "an integration base is never a work unit",
        );
    }

    /// A ref que só o remoto carrega é lida por `<remoto>/<branch>`, e a
    /// varredura que o git não respondeu continua separada do repositório que
    /// respondeu "nenhuma branch de trabalho".
    ///
    /// As duas respostas são medidas em pastas de verdade: um lugar que não é
    /// repositório é onde o git não responde, e um repositório recém-criado é
    /// onde ele responde nada.
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

        // Não respondeu × respondeu nada: quem RELATA precisa das duas.
        let nowhere = tempfile::tempdir().expect("tempdir");
        assert!(
            BranchEnumerator::try_sweep(nowhere.path(), &bases()).is_none(),
            "numa pasta que não é repositório o git não responde",
        );
        let (_dir, root) = scratch_repo();
        run(&root, &["checkout", "-q", "-b", "outra"]);
        run(&root, &["update-ref", "-d", "refs/heads/dev"]);
        let swept = BranchEnumerator::try_sweep(&root, &bases()).expect("o git respondeu");
        assert!(swept.units().is_empty(), "nenhuma branch de trabalho é uma medição");
        // A face que degrada mantém o contrato de quem só conta.
        assert!(BranchEnumerator::sweep(nowhere.path(), &bases()).units().is_empty());
    }

    /// Quem não pergunta ao provedor ainda conta o que a ancestralidade LOCAL
    /// provou, e nunca inventa uma branch podável a partir de um merge que
    /// ninguém mediu.
    #[test]
    fn local_only_lookup_counts_verified_merges_and_invents_none() {
        let (_dir, root) = scratch_repo();
        // Entregue e mergeada, viva nos dois lados: a única que deve uma poda.
        deliver(&root, "dev_landed");
        run(&root, &["merge", "-q", "--no-ff", "dev_landed", "-m", "merge dev_landed"]);
        run(&root, &["update-ref", "refs/remotes/origin/dev_landed", "refs/heads/dev_landed"]);
        // Entregue e não mergeada, viva nos dois lados.
        deliver(&root, "dev_live");
        run(&root, &["update-ref", "refs/remotes/origin/dev_live", "refs/heads/dev_live"]);
        // Entregue, não mergeada e sem remoto nenhum.
        deliver(&root, "dev_gone");

        let pending = awaiting_prune(&root, PrQuery::Skip, &bases());
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
        // `dev_gone` não tem remoto e tem o pull request por medir: perigosa,
        // nunca oferecida para poda — o motivo de quem não pergunta contar
        // menos do que existe.
        assert!(!names.contains(&"dev_gone"));
    }

    /// AC-4 — `gone` (sem remoto) sozinho NUNCA autoriza deleção. Uma branch
    /// apagada sem merge e uma mergeada cujo remoto foi apagado sozinho são
    /// idênticas vistas do lado local; só o merge VERIFICADO as separa.
    #[test]
    fn gone_alone_never_authorises_deletion() {
        let unmerged = unit("dev_gone-unmerged", true, false);
        let landed = unit("dev_gone-merged", true, false);
        // A não mergeada tem até um pull request aberto no registro, então a
        // única diferença capaz de produzir os dois veredictos é a medição da
        // ancestralidade — as duas carregam commits próprios.
        assert_eq!(
            verdict(&unmerged, false, true, PrStatus::Open, true),
            UnitState::Danger,
            "gone + unverified merge = danger",
        );
        assert!(
            !verdict(&unmerged, false, true, PrStatus::Open, true).is_awaiting_prune(),
            "the dangerous branch must never be offered for pruning",
        );
        assert_eq!(
            verdict(&landed, true, true, PrStatus::Absent, true),
            UnitState::AwaitingPruneLocal,
            "only a verified merge turns a gone remote into a prune",
        );

        // E o relatório concorda: exatamente uma branch é listada como podável.
        let (_dir, root) = scratch_repo();
        let states = vec![
            state_of(&root, &unmerged, &evidence(PrStatus::Open, &[]), &BTreeSet::new(), &ahead_of(&[&unmerged, &landed]), true),
            state_of(
                &root,
                &landed,
                &evidence(PrStatus::Absent, &[]),
                &["refs/heads/dev_gone-merged".to_string()].into_iter().collect(),
                &ahead_of(&[&unmerged, &landed]),
                true,
            ),
        ];
        assert_eq!(states[0].state, UnitState::Danger);
        assert_eq!(states[1].state, UnitState::AwaitingPruneLocal);
        let value = report_value(".", &states);
        assert_eq!(value["awaitingPrune"], json!(["dev_gone-merged"]));
    }

    /// Todas as unidades citadas carregam commit próprio.
    fn ahead_of(units: &[&BranchRefs]) -> BTreeSet<String> {
        units.iter().map(|u| u.branch.clone()).collect()
    }

    /// AC-5 — um CLI de provedor ausente ou não autenticado responde
    /// DESCONHECIDO com um motivo, nunca "não tem pull request". As duas
    /// metades: a pergunta se recusa a inventar o que não mediu, e a
    /// classificação se recusa a transformar essa não-resposta no veredicto
    /// negativo "empurrada sem pull request".
    #[test]
    fn absent_provider_answers_unknown_never_absent() {
        // --- a metade da pergunta: provedor sem adaptador é NÃO MEDIDO -------
        let answer =
            PrQuery::Ask("a-provider-with-no-adapter").evidence_of(Path::new("."), "dev_anything").status;
        assert_eq!(answer, PrStatus::Unknown(PR_UNSUPPORTED), "unimplemented ≠ measured absence");
        assert_ne!(answer, PrStatus::Absent, "an unmeasured query is never reported as no-PR");

        // E quem decide não perguntar diz isso, em vez de medir uma ausência.
        let skipped = PrQuery::Skip.evidence_of(Path::new("."), "dev_anything");
        assert_eq!(skipped.status, PrStatus::Unknown(PR_NOT_QUERIED));
        assert!(skipped.merged_heads.is_empty());

        // Um array vazio É uma medição, e essa diferença é o ponto de manter
        // as duas separadas.
        assert_eq!(PrQuery::reduce(&[]).status, PrStatus::Absent);
        let both = PrQuery::reduce(&[
            json!({"state": "CLOSED", "headRefOid": "c1"}),
            json!({"state": "MERGED", "headRefOid": "m1"}),
        ]);
        assert_eq!(
            both.status,
            PrStatus::Merged,
            "the strongest status wins, so row order never decides the verdict",
        );
        assert_eq!(
            both.merged_heads,
            ["m1".to_string()].into_iter().collect::<BTreeSet<String>>(),
            "only the MERGED row's frozen head is evidence of a merge",
        );
        // Dois pull requests mergeados da mesma branch contribuem os DOIS
        // commits — a mesma recusa a deixar a ordem das linhas decidir.
        assert_eq!(
            PrQuery::reduce(&[
                json!({"state": "MERGED", "headRefOid": "m1"}),
                json!({"state": "MERGED", "headRefOid": "m2"}),
            ])
            .merged_heads,
            ["m1".to_string(), "m2".to_string()].into_iter().collect::<BTreeSet<String>>(),
        );

        // --- a metade da classificação: desconhecido nunca vira negativo -----
        let (_dir, root) = scratch_repo();
        let pushed = unit("dev_pushed", true, true);
        let states = vec![state_of(
            &root,
            &pushed,
            &evidence(PrStatus::Unknown(PR_CLI_FAILED), &[]),
            &BTreeSet::new(),
            &ahead_of(&[&pushed]),
            true,
        )];
        assert_eq!(states[0].state, UnitState::Unmeasured);
        assert_ne!(
            states[0].state,
            UnitState::PushedWithoutPr,
            "reporting an unmeasured state as a negative is the defect this module ends",
        );

        // --- e o relatório carrega o MOTIVO, não só a não-resposta -----------
        let value = report_value(".", &states);
        assert_eq!(value["units"][0]["pr"]["status"], json!("unknown"));
        assert_eq!(value["units"][0]["pr"]["reason"], json!(PR_CLI_FAILED));
        assert_eq!(value["awaitingPrune"], json!([]), "nothing unmeasured is ever prunable");
    }

    /// O provedor azure não é mais "sem adaptador": a pergunta o encaminha para
    /// o adaptador REST em `pr_azure`. Sem um `origin` de onde tirar o remoto,
    /// o adaptador recusa na resolução do contexto — antes de qualquer rede — e
    /// a recusa é um estado NÃO MEDIDO, nunca uma ausência medida nem o motivo
    /// "sem adaptador", e sem nenhuma evidência inventada junto.
    #[test]
    fn an_azure_provider_is_asked_through_the_adapter() {
        let dir = tempfile::tempdir().expect("tempdir");
        let azure = PrQuery::Ask("azure").evidence_of(dir.path(), "dev_anything");
        assert_eq!(azure.status, PrStatus::Unknown(PR_CLI_FAILED), "asked, could not answer");
        assert_ne!(azure.status, PrStatus::Unknown(PR_UNSUPPORTED), "azure IS adapted now");
        assert_ne!(azure.status, PrStatus::Absent, "a refusal is never a measured no-PR");
        assert!(azure.merged_heads.is_empty(), "no evidence was fabricated either");
    }

    /// A tabela inteira, uma situação por linha, para que todas fiquem presas
    /// por um teste e não só as duas que um critério nomeia.
    #[test]
    fn classifier_answers_one_state_per_situation() {
        let local_only = unit("dev_draft", true, false);
        let pushed = unit("dev_pushed", true, true);
        let remote_only = unit("dev_remote", false, true);

        // Nada explica as refs, a unidade entregou, e o pull request diz o que
        // diz: o rodapé da tabela.
        assert_eq!(
            verdict(&local_only, false, true, PrStatus::Absent, true),
            UnitState::DraftAbandoned,
        );
        assert_eq!(
            verdict(&pushed, false, true, PrStatus::Absent, true),
            UnitState::PushedWithoutPr,
        );
        assert_eq!(
            verdict(&pushed, false, true, PrStatus::Closed, true),
            UnitState::PushedWithoutPr,
            "um pull request fechado sem merge deixa a branch onde uma sem nenhum está",
        );
        assert_eq!(verdict(&pushed, false, true, PrStatus::Open, true), UnitState::InReview);
        assert_eq!(
            verdict(&pushed, false, true, PrStatus::Unknown(PR_CLI_FAILED), true),
            UnitState::Unmeasured,
        );
        assert_eq!(
            verdict(&local_only, false, true, PrStatus::Unknown(PR_CLI_FAILED), true),
            UnitState::Danger,
            "sem remoto e sem ausência MEDIDA de pull request, não dá para dizer que foi abandonada",
        );
        assert_eq!(
            verdict(&remote_only, false, true, PrStatus::Absent, true),
            UnitState::RemoteOnly,
        );

        // Tudo explicado e entregue: as duas podas, pela existência do remoto.
        assert_eq!(
            verdict(&pushed, true, true, PrStatus::Absent, true),
            UnitState::AwaitingPrune,
        );
        assert_eq!(
            verdict(&local_only, true, true, PrStatus::Absent, true),
            UnitState::AwaitingPruneLocal,
        );
        // Cortada e nunca commitada: alcançável da base porque É a base, e é
        // isso que mantém o trabalho vivo fora da lista de poda.
        assert_eq!(
            verdict(&pushed, true, false, PrStatus::Absent, true),
            UnitState::PushedWithoutPr,
            "alcançar a base sem commit próprio não é ter entregado nada",
        );

        // Mergeada, mas alguma ref passa do que o merge explica — e a mesma
        // situação sem a leitura de alcance ter RESPONDIDO.
        assert_eq!(
            verdict(&pushed, false, true, PrStatus::Merged, true),
            UnitState::MovedAfterMerge,
        );
        assert_eq!(
            verdict(&pushed, false, true, PrStatus::Merged, false),
            UnitState::Unmeasured,
            "dizer que a branch passou do merge sem ninguém ter olhado é a mesma mentira",
        );
    }

    /// O defeito que esta unidade existe para pegar: um pull request MERGEADO
    /// cuja branch andou depois não é podável, e a prova é por REF.
    ///
    /// O veredicto lia `pr == Merged` como suficiente, então o registro de um
    /// merge autorizava a deleção para sempre — inclusive de uma ref remota
    /// empurrada depois dele. O commit que o merge congela explica exatamente
    /// as refs contidas nele, e nada além.
    ///
    /// As duas metades num repositório de verdade, para as asserções poderem
    /// falhar: a MESMA unidade, com o remoto parado no commit congelado, deve
    /// sim a sua poda.
    #[test]
    fn moved_after_merge() {
        let (_dir, root) = scratch_repo();
        let frozen = deliver(&root, "dev_shipped");
        run(&root, &["merge", "-q", "--no-ff", "dev_shipped", "-m", "merge dev_shipped"]);
        // O remoto andou DEPOIS do merge; o local ficou onde o merge o pegou.
        run(&root, &["checkout", "-q", "dev_shipped"]);
        std::fs::write(root.join("depois.txt"), "depois").expect("arquivo de depois");
        run(&root, &["add", "-A"]);
        run(&root, &["commit", "-q", "-m", "depois do merge"]);
        let moved = sha(&root, "dev_shipped");
        run(&root, &["checkout", "-q", "dev"]);
        run(&root, &["update-ref", "refs/remotes/origin/dev_shipped", &moved]);
        run(&root, &["update-ref", "refs/heads/dev_shipped", &frozen]);

        let unit = swept_unit(&root, "dev_shipped");
        let (contained, measured) = try_merged_refs(&root, &bases());
        assert!(measured, "a leitura de contenção respondeu");
        let ahead = refs_ahead_of_base(&root, std::slice::from_ref(&unit), &bases());
        let merged_pr = evidence(PrStatus::Merged, &[&frozen]);
        let states = vec![state_of(&root, &unit, &merged_pr, &contained, &ahead, measured)];

        assert_eq!(states[0].state, UnitState::MovedAfterMerge, "{:?}", states[0]);
        assert_eq!(states[0].state.token(), "moved-after-merge");
        assert!(
            !states[0].state.is_awaiting_prune(),
            "a ref carrying commits the merge never saw must never be offered for deletion",
        );
        // O relatório diz QUAL ref andou — sem isso o veredicto é uma
        // afirmação que o leitor não tem como conferir.
        let value = report_value(".", &states);
        assert_eq!(value["awaitingPrune"], json!([]));
        assert_eq!(value["units"][0]["state"], json!("moved-after-merge"));
        assert_eq!(
            value["units"][0]["refs"],
            json!([
                { "ref": "refs/heads/dev_shipped", "contained": true, "coveredByPr": true },
                { "ref": "refs/remotes/origin/dev_shipped", "contained": false, "coveredByPr": false },
            ]),
            "the local head is accounted for TWICE over and the remote not at all — the name-keyed \
             set could only hold the first of those two answers: {value}",
        );

        // A outra metade: o remoto nunca andou, então o merge explica as duas
        // refs e a unidade é podável. Mesmo pull request, mesma unidade — só o
        // alcance das refs muda, que é justamente o ponto.
        run(&root, &["update-ref", "refs/remotes/origin/dev_shipped", &frozen]);
        let still = swept_unit(&root, "dev_shipped");
        let (contained, measured) = try_merged_refs(&root, &bases());
        let landed = state_of(&root, &still, &merged_pr, &contained, &ahead, measured);
        assert_eq!(landed.state, UnitState::AwaitingPrune, "{landed:?}");
    }

    /// AC-6 — a fase de leitura é estruturalmente incapaz de apagar uma branch.
    ///
    /// Lido como as provas de prosa: as DUAS metades, para a asserção poder
    /// falhar de verdade. Metade um — o código deste módulo não nomeia nenhum
    /// argumento de deleção. Metade dois — o do ritual de saída nomeia, o que
    /// prova que as agulhas são as grafias reais e que a capacidade apenas mora
    /// noutro lugar. As agulhas são montadas em tempo de execução para escrever
    /// o teste não pôr as grafias proibidas no arquivo sob asserção.
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

        // A visão de LEITURA é dado puro: quem recebe uma destas não alcança o
        // repositório, porque o tipo não carrega caminho, processo nem função
        // com que alcançá-lo. As três grafias existem neste arquivo fora da
        // visão, então a asserção pode falhar.
        let view = here
            .split_once("pub(crate) struct BranchState {")
            .and_then(|(_, rest)| rest.split_once("\n}"))
            .map(|(body, _)| body)
            .unwrap_or_default();
        assert!(!view.is_empty(), "the read view must still be a struct this test can read");
        for forbidden in ["Path", "Command", "Fn("] {
            assert!(
                here.contains(forbidden),
                "{forbidden} tem de existir no arquivo, ou a asserção de baixo não prova nada",
            );
            assert!(
                !view.contains(forbidden),
                "the read view must carry no {forbidden} — it is data, not a capability",
            );
        }
    }

    /// AC-11 — uma branch de trabalho SEM commit próprio nunca é oferecida para
    /// poda.
    ///
    /// O portão abre toda unidade com `checkout -b <unidade> <base>` e mais
    /// nada, então do corte até o primeiro commit ela é alcançável da base
    /// exatamente como uma mergeada. Lida só pela ancestralidade, a unidade que
    /// a pessoa está EDITANDO respondia "entregue, me pode" — e o aviso
    /// construído sobre essa resposta entrega o comando que a apaga.
    ///
    /// As duas metades, para as asserções poderem falhar: a mesma branch, um
    /// commit e um merge depois, DEVE a sua poda.
    #[test]
    fn a_branch_with_no_commits_ahead_is_never_awaiting_prune() {
        let (_dir, root) = scratch_repo();
        run(&root, &["checkout", "-q", "-b", "dev_fresh"]);

        // A ancestralidade responde SIM — a branch É a base — e isso já foi o
        // veredicto inteiro.
        let merged = try_merged_refs(&root, &bases()).0;
        assert!(
            merged.contains("refs/heads/dev_fresh"),
            "git reports a freshly cut branch as merged into its base — per REF",
        );
        let swept = BranchEnumerator::sweep(&root, &bases());
        let ahead = refs_ahead_of_base(&root, swept.units(), &bases());
        assert!(!ahead.contains("dev_fresh"), "a branch just cut carries no commit of its own");

        assert!(
            awaiting_prune(&root, PrQuery::Skip, &bases()).is_empty(),
            "the unit being edited must never be announced as delivered",
        );
        let states = classify(&root, PrQuery::Skip, swept.units(), &merged, &ahead, true);
        assert!(!states[0].state.is_awaiting_prune(), "not prunable: {states:?}");
        assert!(
            states[0].ancestry,
            "the veto is the absent commits, not an absent ancestry — otherwise this test \
             would pass on a sweep that simply saw nothing",
        );

        // A outra metade: entregue, mergeie, e a mesma branch deve a poda.
        std::fs::write(root.join("work.txt"), "w").expect("work file");
        run(&root, &["add", "-A"]);
        run(&root, &["commit", "-q", "-m", "work"]);
        run(&root, &["checkout", "-q", "dev"]);
        run(&root, &["merge", "-q", "--no-ff", "dev_fresh", "-m", "merge dev_fresh"]);
        let pending = awaiting_prune(&root, PrQuery::Skip, &bases());
        let names: Vec<&str> = pending.iter().map(|s| s.branch.as_str()).collect();
        assert_eq!(names, vec!["dev_fresh"], "a unit that delivered commits IS owed its prune");
    }

    /// AC-12 — uma unidade cujo merge está verificado, cuja ref local já sumiu
    /// e cuja branch REMOTA continua viva deve uma poda (a do remoto), em vez
    /// de ser arquivada como "só no servidor".
    ///
    /// A tabela respondia `remote-only` antes de pesar o merge, então as
    /// branches REMOTAS que o relato de campo contou junto com as locais não
    /// entravam na lista — metade da medição que motivou o relatório ficava
    /// invisível para ele.
    #[test]
    fn merged_unit_alive_only_on_the_remote_is_awaiting_prune() {
        let (_dir, root) = scratch_repo();
        deliver(&root, "dev_landed");
        run(&root, &["merge", "-q", "--no-ff", "dev_landed", "-m", "merge dev_landed"]);
        // O remoto sobrevive à ref local — a forma em que fica a máquina que já
        // podou localmente, ou que nunca teve a branch.
        run(&root, &["update-ref", "refs/remotes/origin/dev_landed", "refs/heads/dev_landed"]);
        run(&root, &["update-ref", "-d", "refs/heads/dev_landed"]);

        let swept = BranchEnumerator::sweep(&root, &bases());
        let landed = swept.units().iter().find(|u| u.branch == "dev_landed").expect("unit swept");
        assert!(!landed.local, "no local ref carries it any more");
        assert_eq!(landed.remotes, vec!["origin"], "…but the remote is still alive");

        let pending = awaiting_prune(&root, PrQuery::Skip, &bases());
        let names: Vec<&str> = pending.iter().map(|s| s.branch.as_str()).collect();
        assert_eq!(names, vec!["dev_landed"], "the remote of a merged unit is owed its prune");
        assert_eq!(pending[0].state, UnitState::AwaitingPrune);

        let merged = try_merged_refs(&root, &bases()).0;
        let ahead = refs_ahead_of_base(&root, swept.units(), &bases());
        let states = classify(&root, PrQuery::Skip, swept.units(), &merged, &ahead, true);
        assert_eq!(report_value(".", &states)["awaitingPrune"], json!(["dev_landed"]));

        // E a reordenação não engoliu nada: uma unidade só no remoto que NÃO
        // aterrissou continua sendo só do remoto.
        let stranger = unit("dev_elsewhere", false, true);
        let unlanded = classify(
            &root,
            PrQuery::Skip,
            std::slice::from_ref(&stranger),
            &BTreeSet::new(),
            &BTreeSet::new(),
            true,
        );
        assert_eq!(unlanded[0].state, UnitState::RemoteOnly);
    }
}

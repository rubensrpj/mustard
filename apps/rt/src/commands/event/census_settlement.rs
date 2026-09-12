//! `census_settlement` — THE question every door that is about to move, or
//! write into, a dirty tree has to answer: asked once, answered in one place,
//! and ACTED ON here rather than by the door.
//!
//! ## The question, stated whole
//!
//! It has three inputs and exactly one answer.
//!
//! 1. **What is dirty** — harness scratch (dropped from the measurement),
//!    census artefacts (the tool's own output), the operator's work. Measured by
//!    [`checkout_work`], EXACTLY ONCE per settlement, and carried from there.
//! 2. **Where the checkout stands** ([`CheckoutPosition`]) — the resolved base,
//!    another unit's branch, a protected branch, or a position this decision
//!    cannot attribute at all.
//! 3. **What is about to happen** ([`CensusDoor`]) — an explicit open the
//!    operator typed, a branch cut, or a write-hook pass.
//!
//! One answer, of three shapes ([`CensusSettlement`]):
//!
//! - **Refuse**, naming what is in the way and why this door cannot settle it
//!   here ([`RefusalCause`]).
//! - **Refresh the base from origin, record the census on it, then proceed.**
//! - **Proceed**, nothing owed — or nothing that COULD be recorded, which the
//!   recording says on stderr.
//!
//! ## The table
//!
//! Every combination of (what is dirty × the state of the index × the shape
//! of the root a door passed × where the checkout stands × which door) has a
//! row; a hole in the table is a defect of the table, never a condition to add
//! at a door. Two of the five axes collapse at the entrance, before any row is
//! taken, and the collapse is stated here so nobody re-derives it at a door:
//!
//! - **root shape** — a door may pass the toplevel, a subdirectory (the write
//!   hook passes the edited file's directory), a linked worktree or a
//!   submodule. `settle` resolves `git rev-parse --show-toplevel` ONCE at
//!   entry and every git call below uses that root. Every row is therefore
//!   written for ONE shape; a door cannot get it wrong because a door no longer
//!   chooses. (The submodule keeps its own toplevel: it is a repository of its
//!   own, and its position is what [`CheckoutPosition::attributable`] declines
//!   to attribute.)
//! - **index state** — a dirty path may be unstaged (` M`), staged (`M `,
//!   `A `), both (`MM`) or untracked (`??`). [`checkout_work`] accepts all four
//!   into the same reading, and the one step that touches those paths — the
//!   set-aside — is a `stash push --include-untracked` restricted to them,
//!   which holds all four states alike. No row below reads the index state
//!   again: there is no state a row handles differently.
//!
//! The rows, in the order the body takes them:
//!
//! | dirty         | position                              | door        | row |
//! |---------------|---------------------------------------|-------------|-----|
//! | any           | (vcs opted out)                       | any         | Proceed, nothing measured |
//! | `Holds`/`Unproven` | another unit's branch, cutting   | cut / hook  | **Refuse** — work would travel |
//! | `Holds`/`Unproven` | the base, protected, cutting     | cut / hook  | fall through: their work rides into the unit, by design (the first unit cuts off the base in place) |
//! | `Holds`/`Unproven` | any, not cutting                 | explicit    | fall through: nothing moves |
//! | `CensusOnly`  | off the base (any branch, `HEAD`), cutting, base known | cut / hook | **Refuse** — the census cannot land here, so it must not travel |
//! | `CensusOnly`  | the base, protected, cutting          | cut / hook  | **Refuse** — this door may not commit on a protected base; the explicit open can |
//! | `CensusOnly`  | any, base unknown                     | cut / hook  | fall through: nothing moves (`BaseUnknown` follows at the door) |
//! | `Holds`       | the base, the advance overwrites THEIR paths | any  | **Refuse** — names their files, prescribes the stash; nothing touched |
//! | `CensusOnly`/`Holds` | the base, the advance overwrites census paths | any | set those paths aside — stashed, never discarded — then advance |
//! | any (fell through) | base known, `origin` answers, base cannot be advanced | any | **Refuse** — stale base, git's words; what was set aside is put back first, index state included |
//! | (set aside)   | the base, advanced                    | any         | account for each path: miner output → `origin`'s stands; authored mold → kept BESIDE `origin`'s; then drop the stash |
//! | `ProvenClean`/`CensusOnly` | the base (protected only at the explicit door) | any | **Record** what is dirty plus what the re-mine wrote |
//! | `Holds`/`Unproven` | the base                         | explicit    | Proceed — theirs to commit, beside ours |
//! | any           | not the base                          | any         | Proceed — nowhere to record |
//!
//! Every row that touches the tree has a rollback: the set-aside is the only
//! one, and its rollback is the put-back on the refusal row that follows it.
//! The recording touches the tree too, but it is the last step and answers
//! `Proceed` when git declines — nothing after it needs undoing.
//!
//! ## Why this is ONE function and not a condition at each door
//!
//! Six review rounds found nine instances of the same defect, and every round
//! answered it the same way: it added a condition at a call site.
//! `DirtyPathKind` and `CheckoutWork::CensusOnly`; then a guard before the cut;
//! then a second guard at the recording; then the positional predicate
//! `census_commit_belongs_here`; then the refresh/record ORDER — in two of the
//! three doors. Each round guarded the writers it could see, and the next round
//! found the writer it had missed. There were FOUR writers of this decision:
//! the explicit `emit-pipeline` door, the census re-mine's own recording (never
//! guarded at all), `spec-draft`'s cut, and the write hook.
//!
//! So the doors stopped deciding AND stopped acting. A door states the three
//! inputs and obeys the answer; it performs no step, so there is no step left
//! for it to get wrong. The base refresh, the re-mine and the census commit all
//! happen HERE, in that order, once — the ordering that used to be a comment
//! repeated in each door is now the shape of a single function body.
//!
//! ## The one difference between doors
//!
//! [`CensusDoor::ExplicitOpen`] is the command the operator typed, and it is
//! the one place in the flow where a clean tree on the base is a premise of the
//! command itself: it records onto a PROTECTED base, and it always did. The two
//! cutting doors do not — a hook must not create a commit on a protected branch
//! behind the operator's back. That difference is a NAMED INPUT here, never an
//! `if` in a door.
//!
//! The explicit door also owns the deterministic RE-MINE, for the reason
//! [`super::base_gate`] gives: a freshly updated base, before the first edit, is
//! the one moment in the flow where a clean tree holds by construction. A cut
//! does not re-mine — it happens moments after that open, and re-mining inside a
//! `PreToolUse` hook would put the grain sidecar in front of every first edit.
//!
//! ## Fail-open where nothing was measured, loud where something was
//!
//! A census git cannot see, a git that declines the commit, an unreachable
//! remote: every one of them leaves the write where it fell and answers
//! `Proceed` — and the declined commit says so on stderr. Only a POSITIVE
//! observation refuses: somebody's work that would travel, a census that
//! cannot land where the move would carry it, a base that `origin` proved
//! stale and git could not advance.

use std::path::Path;

use mustard_core::ProjectConfig;

use super::base_gate;
use super::work_branch::{
    checkout_work, fast_forward_base, fetch_origin, holds_other_work, is_protected,
    paths_the_advance_overwrites, toplevel_of, BaseRefresh, BusyCheckout, CensusSetAside,
    CheckoutWork, RefusalCause,
};

/// WHAT IS ABOUT TO HAPPEN — the third input, and the only thing the doors
/// disagree about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CensusDoor {
    /// `emit-pipeline --kind pipeline.kind` — the operator typed the command
    /// that opens the unit. Nothing is checked out here, so nothing can ride
    /// along and the tree is never refused; the one refusal it can receive is
    /// for the BASE, when `origin` proved it stale and git could not advance
    /// it. The tree is on the base by the premise of the command, and a
    /// PROTECTED base records like any other.
    ExplicitOpen,
    /// `spec-draft`'s cut of the pending work branch
    /// ([`super::work_branch::cut_pending_work_branch`]).
    BranchCut,
    /// The write hook's first in-repo mutation
    /// ([`crate::hooks::write::work_branch_gate`]).
    WriteHookPass,
}

impl CensusDoor {
    /// May the census commit land on a PROTECTED base at this door?
    ///
    /// The ONE legitimate difference between the doors. `true` only where the
    /// operator typed the command: a hook that committed on a protected branch
    /// would be writing history nobody asked for.
    fn may_record_on_a_protected_base(self) -> bool {
        matches!(self, Self::ExplicitOpen)
    }

    /// Does this door re-mine the deterministic census before recording it?
    ///
    /// Only the explicit open. See the module doc.
    fn remines_the_census(self) -> bool {
        matches!(self, Self::ExplicitOpen)
    }
}

/// WHERE THE CHECKOUT STANDS — the second input, stated by the door and never
/// re-derived here.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CheckoutPosition<'a> {
    /// The branch the tree really sits on — `None` on a detached HEAD or a
    /// probe that did not answer. Used BOTH to judge attribution and to drive
    /// the git steps, which is why it is never masked: see
    /// [`Self::attributable`].
    current: Option<&'a str>,
    /// The branch about to be cut, when one is. `None` at a door that cuts
    /// nothing — the explicit open — where no work can ride anywhere and so
    /// nothing in the TREE is ever refused.
    target: Option<&'a str>,
    /// The base this open or cut resolved to. `None` when the door could not
    /// establish it: a base nobody knows authorises no commit, and nothing is
    /// going to move either, so the census is not what the operator needs to
    /// read about.
    base: Option<&'a str>,
    /// Whether the position can be ATTRIBUTED at all — see
    /// [`Self::attributable`].
    attributable: bool,
}

impl<'a> CheckoutPosition<'a> {
    /// The ordinary position: where the tree sits, what is about to be cut (if
    /// anything), and the base that was resolved for it.
    pub(crate) fn at(
        current: Option<&'a str>,
        target: Option<&'a str>,
        base: Option<&'a str>,
    ) -> Self {
        Self {
            current,
            target,
            base,
            attributable: true,
        }
    }

    /// Declare whether "whose work is this?" has an answer in this tree.
    ///
    /// `false` for a SUBMODULE reached through the write hook: its HEAD is
    /// judged against the SUPERproject's bases, which misreads its position
    /// outright — so it refuses nothing and records nothing, exactly as the
    /// `if !in_submodule` at that door did before the decision collapsed here.
    ///
    /// The branch name itself is NOT dropped: the base refresh is a git step
    /// that never depended on attribution, and it needs to know whether the
    /// tree is standing on the base it is about to fast-forward.
    pub(crate) fn attributable(mut self, attributable: bool) -> Self {
        self.attributable = attributable;
        self
    }

    /// `true` when the census commit BELONGS on this checkout: the position was
    /// measured, the base is a fact, the position IS that base, and — at every
    /// door but the explicit one — that base is not protected.
    ///
    /// Positional and not a list of exclusions, because the list is what kept
    /// missing a position: two earlier iterations excluded "protected", "HEAD"
    /// and "unmeasured", and both let through the one nobody had named — ANOTHER
    /// unit's branch, which is none of those three. The census describes the
    /// whole project, so its commit belongs to the base and to nothing else;
    /// anywhere else it enters some unit's diff and some unit's pull request as
    /// if that unit had rewritten the repository's map.
    fn is_the_base(&self, root: &Path, config: &ProjectConfig, door: CensusDoor) -> bool {
        if !self.attributable {
            return false;
        }
        let Some(branch) = self.current.filter(|b| *b != "HEAD") else {
            return false;
        };
        let Some(base) = self.base.map(str::trim).filter(|b| !b.is_empty()) else {
            return false;
        };
        branch == base
            && (door.may_record_on_a_protected_base() || !is_protected(root, branch, config))
    }

    /// `true` when taking this checkout would carry work that is not this
    /// unit's onto the branch about to be cut — the plain `git checkout -b`
    /// this settlement stands in front of moves everything uncommitted with it.
    ///
    /// `false` wherever nothing is going to be checked out (no target), and
    /// wherever the position cannot be attributed.
    fn would_carry_work_off(&self, root: &Path, config: &ProjectConfig) -> bool {
        self.attributable
            && self
                .target
                .is_some_and(|target| holds_other_work(root, self.current, target, config))
    }

    /// `true` when this move is going to CHECK SOMETHING OUT from a known base
    /// — the only situation in which anything dirty can travel at all.
    ///
    /// The question the census row asks, and deliberately NOT
    /// [`Self::would_carry_work_off`]: that one exempts a protected position
    /// and an unmeasured one because the operator's work riding off the base
    /// into the first unit is by design, and an unmeasured HEAD must not
    /// trigger a refusal nobody asked for. Neither exemption transfers to the
    /// census — it is nobody's work and belongs to no unit, so ANY position
    /// where it cannot be recorded is one it must not travel from.
    ///
    /// A base that is not a fact (`None`) is excluded: nothing is going to move
    /// at all — the cut answers `BaseUnknown` and the hook refuses or warns,
    /// both before any `git checkout -b` — so the sentence the operator needs is
    /// about the base, not about the census.
    fn moves_from_a_known_base(&self) -> bool {
        self.attributable
            && self.target.is_some()
            && self.base.map(str::trim).is_some_and(|b| !b.is_empty())
    }
}

/// THE ANSWER — three shapes and no more. Everything the answer describes has
/// ALREADY HAPPENED when it is returned; the caller only obeys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CensusSettlement {
    /// Do NOT proceed. Something is in the way of this move, and the refusal
    /// carries the measured paths and the cause so each door can say the same
    /// sentence in its own shape ([`BusyCheckout::reason`]).
    ///
    /// A refusal for the TREE (work or census that would travel) happens before
    /// any fetch, mine or commit: the tree is left exactly as it was found. A
    /// refusal for the BASE necessarily comes after the fetch that proved it
    /// stale, and it leaves the tree exactly as it was found too: the
    /// operator's files in the advance's way
    /// ([`RefusalCause::BaseBlockedByWork`]) are refused before any path is
    /// set aside, and a fast-forward that still fails
    /// ([`RefusalCause::BaseStale`]) puts back — index state included — the
    /// census paths that had been set aside for it. Nothing was mined or
    /// committed in either case.
    Refuse(BusyCheckout),
    /// The base was refreshed from `origin`, the census was re-mined where that
    /// door does so, and the artefacts named here were recorded on the base —
    /// in that order. Proceed.
    Recorded(Vec<String>),
    /// Proceed. Either nothing was owed — the base was refreshed when there was
    /// a base to refresh, and there was nothing of the tool's to record here or
    /// nowhere for it to go — or the recording was attempted and git declined
    /// it, in which case the recording said so on stderr and the census stays
    /// where it fell.
    Proceed,
}

/// Settle the census question for one door, one position and one tree.
///
/// The whole decision AND the whole effect. In order, and the order is the
/// point:
///
/// 0. **Resolve the root, once** ([`toplevel_of`]). Whatever a door passed —
///    the project root, the edited file's directory, a worktree — every git
///    call below runs from the repository's toplevel, because the paths git
///    ANSWERS (`status`, `diff --name-only`) are toplevel-relative and the
///    pathspecs it is HANDED (`stash push -- p`, `add -- p`) are CWD-relative.
///    From any other directory the two disagree in silence. Resolving the root
///    is not a measurement of the tree: step 1 is still the only one.
/// 1. **Measure the tree, once** ([`checkout_work`]). Every later question
///    reads this one value; nothing measures again.
/// 2. **Refuse for the tree**: work that is not this unit's would ride along,
///    or a census that cannot be recorded where the tree stands would. Refusing
///    FIRST is what keeps a refused move from leaving a fetch, a re-mine or a
///    commit behind it.
/// 3. **Fetch `origin`** — the measurement the next two steps read. Offline,
///    nothing below happens and the cut takes the local base, as it always did.
/// 4. **Set the census aside where it stands in the way of the advance** —
///    and refuse, before touching anything, where the OPERATOR's files do.
///    Standing on the base with dirty paths that `origin`'s advance
///    overwrites, the fast-forward refuses on them ("local changes would be
///    overwritten"), and the old answer to that was to swallow the refusal
///    and commit the census onto the STALE base — which then carried a commit
///    of its own AND trailed its remote, so the `git pull --ff-only origin
///    {base}` the gate prescribes could never succeed again. Two kinds of path
///    can stand in the way, and they get different sentences:
///    - THEIRS (a [`CheckoutWork::Holds`] tree on the base — the first unit
///      cutting off the base in place, by design): the move is refused naming
///      those files and prescribing the stash that unblocks the advance
///      ([`RefusalCause::BaseBlockedByWork`]). Not a `git pull`, which fails
///      on the same files; and nothing was touched, so there is nothing to
///      undo.
///    - THE CENSUS (in a `CensusOnly` tree, or beside their work in a `Holds`
///      one — it is the tool's whatever else is dirty): those paths, and only
///      those, are SET ASIDE with a stash restricted to them
///      ([`CensusSetAside::push`]) — staged, unstaged and untracked alike.
///      Never discarded: an authored mold is not regenerable, and a base that
///      then fails to advance owes the tree back exactly as it was.
/// 5. **Fast-forward the base** — THE base this settlement is about, and no
///    other ref ([`fast_forward_base`]). If it still trails `origin` afterwards,
///    **put back what step 4 set aside, then refuse, loudly**, with git's
///    words: never swallowed, never a commit on a stale base. It is the
///    invariant the root `CLAUDE.md` states in its own words: `--ff-only` only
///    passes while the integration base carries no commit of its own. When it
///    advanced, what was set aside is accounted for path by path
///    ([`CensusSetAside::settle_after_advance`]): miner output yields to
///    `origin`'s version (the re-mine wins), an authored mold is kept beside
///    `origin`'s and said so — nobody's text is deleted.
/// 6. **Re-mine**, at the door that does, from the fresh base.
/// 7. **Record** what the tool wrote — what was already dirty (minus what step
///    4 set aside) plus what the re-mine just produced — as ONE commit on the
///    base. The recording says on stderr what it did, on every outcome.
///
/// The caller does none of those steps and cannot reorder them.
pub(crate) fn settle(
    root: &Path,
    position: CheckoutPosition<'_>,
    config: &ProjectConfig,
    door: CensusDoor,
) -> CensusSettlement {
    // An explicit `vcs: ""` opt-out (or a tree git does not manage) has no
    // base to refresh and no commit to write.
    let Some(vcs) = config.vcs() else {
        return CensusSettlement::Proceed;
    };
    // 0. THE ROOT — the repository's toplevel, whatever the door passed. A
    //    tree git cannot place keeps the door's root: the measurement below
    //    then answers `Unproven`, which authorises nothing.
    let toplevel = toplevel_of(&vcs, root);
    let root: &Path = toplevel.as_deref().unwrap_or(root);
    let root_s = root.to_string_lossy().into_owned();
    let base = position.base.map(str::trim).filter(|b| !b.is_empty());

    // 1. WHAT IS DIRTY — measured here and NOWHERE else in this settlement.
    let work = checkout_work(root);
    let on_the_base = position.is_the_base(root, config, door);

    // 2. REFUSE FOR THE TREE. Two rows, one per kind of dirt, because the two
    //    kinds are exempt from different things:
    //
    //    - the OPERATOR's work (or a tree that could not be measured — an
    //      unmeasured tree is not an empty one, and reading it as empty is how
    //      another unit's work rides off in silence) refuses only when it is
    //      another unit's, i.e. where `holds_other_work` says so. Riding off a
    //      protected base into the first unit is by design.
    //    - the CENSUS is nobody's work and has exactly one place to land, the
    //      base. Anywhere else it would ride into the new branch exactly as
    //      work would, and there is no design that wants it there: another
    //      unit's branch, a detached HEAD, a protected branch that is not the
    //      base — and the base itself when it is protected and this door may
    //      not commit on it. Each of those is a refusal, and the last one names
    //      the door that can.
    let refusal = match &work {
        CheckoutWork::ProvenClean => None,
        CheckoutWork::Holds { .. } | CheckoutWork::Unproven => position
            .would_carry_work_off(root, config)
            .then_some(RefusalCause::WorkWouldTravel),
        CheckoutWork::CensusOnly(_) if !position.moves_from_a_known_base() || on_the_base => {
            None
        }
        CheckoutWork::CensusOnly(_) => {
            let standing_on_a_protected_base = position
                .current
                .zip(base)
                .is_some_and(|(current, base)| current == base && is_protected(root, base, config));
            Some(if standing_on_a_protected_base {
                RefusalCause::CensusOnProtectedBase
            } else {
                RefusalCause::CensusOffBase {
                    base: base.unwrap_or_default().to_string(),
                }
            })
        }
    };
    if let Some(cause) = refusal {
        return CensusSettlement::Refuse(BusyCheckout {
            current: position.current.unwrap_or("HEAD").to_string(),
            target: position.target.unwrap_or_default().to_string(),
            work,
            cause,
        });
    }

    // 3–5. THE BASE — fetched, with the tool's own output moved out of the
    //      advance's way where it stands in it, and fast-forwarded. A base
    //      nobody established cannot be refreshed; offline, nothing can be
    //      measured and the local base is taken as before.
    let mut set_aside: Vec<String> = Vec::new();
    if let Some(base) = base
        && fetch_origin(&vcs, &root_s) {
            let mut aside = CensusSetAside::none();
            // 4. SET ASIDE — only on the base, only the paths the advance
            //    overwrites, and only when the advance IS a fast-forward (a
            //    diverged base refuses below without a single path touched).
            //    Their files first, because those refuse before anything is
            //    set aside; then the census, in whichever reading it came.
            if position.current == Some(base) {
                let (theirs, census): (&[String], &[String]) = match &work {
                    CheckoutWork::Holds { theirs, census } => (theirs, census),
                    CheckoutWork::CensusOnly(census) => (&[], census),
                    CheckoutWork::ProvenClean | CheckoutWork::Unproven => (&[], &[]),
                };
                let blocking = paths_the_advance_overwrites(&vcs, &root_s, base, theirs);
                if !blocking.is_empty() {
                    return CensusSettlement::Refuse(BusyCheckout {
                        current: position.current.unwrap_or("HEAD").to_string(),
                        target: position.target.unwrap_or_default().to_string(),
                        work,
                        cause: RefusalCause::BaseBlockedByWork {
                            base: base.to_string(),
                            paths: blocking,
                        },
                    });
                }
                let overwritten = paths_the_advance_overwrites(&vcs, &root_s, base, census);
                aside = CensusSetAside::push(&vcs, root, base, &overwritten);
                set_aside = aside.paths().to_vec();
                if !set_aside.is_empty() {
                    eprintln!(
                        "base-gate: census output set aside (stashed) so '{base}' can advance \
                         to origin/{base} — {}",
                        set_aside.join(", ")
                    );
                }
            }
            // 5. FAST-FORWARD, and READ the answer. A refusal puts back what
            //    was set aside FIRST: the tree is returned exactly as found.
            match fast_forward_base(&vcs, &root_s, position.current, base) {
                BaseRefresh::Stale { base, error } => {
                    aside.put_back(&vcs, &root_s);
                    return CensusSettlement::Refuse(BusyCheckout {
                        current: position.current.unwrap_or("HEAD").to_string(),
                        target: position.target.unwrap_or_default().to_string(),
                        work,
                        cause: RefusalCause::BaseStale { base, error },
                    });
                }
                BaseRefresh::Current => {
                    let report = aside.settle_after_advance(&vcs, root);
                    if !report.origin_kept.is_empty() {
                        eprintln!(
                            "base-gate: miner output rewritten on origin/{base} — origin's \
                             version stands, the local one was regenerable: {}",
                            report.origin_kept.join(", ")
                        );
                    }
                    for (path, sibling) in &report.kept_both {
                        eprintln!(
                            "base-gate: authored mold {path} was rewritten on origin/{base} \
                             too — origin's text is at {path}, the text found here is kept \
                             at {sibling}; both survive, reconcile them by hand"
                        );
                    }
                }
            }
        }

    // 6. RE-MINE, at the door that owns it, reading the tree measured at step 1.
    //    Whether the miner RAN is read: it decides below whether the recorder
    //    is owed a visit at all.
    let mined = door.remines_the_census()
        && base_gate::mine_census_if_stale(root, &work, on_the_base);

    // 7. RECORD. Only on the base, and only when nothing of the operator's is
    //    in the tree — `ProvenClean` and `CensusOnly` are the two readings that
    //    say the index holds nothing of theirs for a commit to sweep up, which
    //    is exactly the fact `record_written_path` needs to be told.
    if !on_the_base {
        return CensusSettlement::Proceed;
    }
    let mut paths: Vec<String> = match &work {
        // What step 4 set aside is origin's now, not dirt of ours.
        CheckoutWork::CensusOnly(dirty) => {
            dirty.iter().filter(|p| !set_aside.contains(p)).cloned().collect()
        }
        CheckoutWork::ProvenClean => Vec::new(),
        // Their work is in the tree: it is theirs to commit, beside ours.
        CheckoutWork::Holds { .. } | CheckoutWork::Unproven => return CensusSettlement::Proceed,
    };
    // What the re-mine writes is DERIVED, not measured again: the model and its
    // sidecar are written at a known path, and a pathspec for a file that did
    // not change is a no-op for the recorder. Only when the miner ran, though:
    // a clean tree where nothing was mined owes nothing, and the recorder
    // would only announce that it found nothing — noise at every open.
    if mined {
        for path in base_gate::mined_census_paths(root) {
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    if paths.is_empty() {
        return CensusSettlement::Proceed;
    }
    if base_gate::commit_census(root, &paths) {
        CensusSettlement::Recorded(paths)
    } else {
        // Ignored by git, invisible to it, or a commit git refused: the write
        // stays where it fell, exactly as the deterministic mine already
        // degrades — and `commit_census` has just said which on stderr.
        CensusSettlement::Proceed
    }
}

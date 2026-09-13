//! `census_settlement` — THE question every door that is about to move a dirty
//! tree has to answer: asked once, answered in one place, and ACTED ON here
//! rather than by the door.
//!
//! ## The question, stated whole
//!
//! It has two inputs and exactly one answer.
//!
//! 1. **What is dirty** — harness scratch (dropped from the measurement),
//!    census artefacts (the tool's own output), the operator's work. Measured by
//!    [`checkout_work`], EXACTLY ONCE per settlement, and carried from there.
//! 2. **Where the checkout stands** ([`CheckoutPosition`]) — the branch the tree
//!    sits on, the branch about to be cut (none at the explicit open), the
//!    resolved base, and whether the position can be attributed at all.
//!
//! One answer, of two shapes ([`CensusSettlement`]):
//!
//! - **Refuse**, naming what is in the way and what unblocks it
//!   ([`RefusalCause`]).
//! - **Proceed**, with the base already refreshed from `origin` when there was
//!   a base to refresh.
//!
//! Nothing here writes a commit. The scan never writes to git, so the census is
//! nobody's work and there is nowhere to record it: a tree dirty only with it
//! ([`CheckoutWork::CensusOnly`]) never refuses anything. The census is still
//! told apart from the operator's work, so that their files are the only ones
//! that can refuse a move and the only ones a refusal names.
//!
//! ## The table
//!
//! Every combination of (what is dirty × the state of the index × the shape
//! of the root a door passed × where the checkout stands) has a row; a hole in
//! the table is a defect of the table, never a condition to add at a door. Two
//! of the four axes collapse at the entrance, before any row is taken, and the
//! collapse is stated here so nobody re-derives it at a door:
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
//! | dirty         | position                              | row |
//! |---------------|---------------------------------------|-----|
//! | any           | (vcs opted out)                       | Proceed, nothing measured |
//! | `Holds`/`Unproven` | another unit's branch, cutting   | **Refuse** — work would travel |
//! | `Holds`/`Unproven` | the base, protected, cutting     | fall through: their work rides into the unit, by design (the first unit cuts off the base in place) |
//! | `Holds`/`Unproven` | any, not cutting                 | fall through: nothing moves |
//! | `ProvenClean`/`CensusOnly` | any                      | fall through: nothing of anyone's can travel |
//! | `Holds`       | the base, the advance overwrites THEIR paths | **Refuse** — names their files, prescribes the stash; nothing touched |
//! | `CensusOnly`/`Holds` | the base, the advance overwrites census paths | set those paths aside — stashed, never discarded — then advance |
//! | any (fell through) | base known, `origin` answers, base cannot be advanced | **Refuse** — stale base, git's words; what was set aside is put back first, index state included |
//! | (set aside)   | the base, advanced                    | account for each path: miner output → `origin`'s stands; authored mold → kept BESIDE `origin`'s; then drop the stash |
//! | any           | anything else                         | Proceed |
//!
//! Every row that touches the tree has a rollback: the set-aside is the only
//! one, and its rollback is the put-back on the refusal row that follows it.
//!
//! ## Why this is ONE function and not a condition at each door
//!
//! Three doors take this decision: the explicit `emit-pipeline` open,
//! `spec-draft`'s cut and the write hook. While each carried a condition of its
//! own, the next review always found the door that had missed one, or that
//! took the steps in another order. So the doors stopped deciding AND stopped
//! acting: a door states where the checkout stands and obeys the answer, and
//! the base refresh happens HERE, once, in the order this body states. The
//! doors differ only in the position they state — the explicit open cuts
//! nothing (no target), and the write hook marks a submodule as a position it
//! cannot attribute.
//!
//! ## Fail-open where nothing was measured
//!
//! An unreachable remote, a tree git cannot place: each answers `Proceed`.
//! Only a POSITIVE observation refuses: somebody's work that would travel, or a
//! base that `origin` proved stale and git could not advance.

use std::path::Path;

use mustard_core::ProjectConfig;

use super::work_branch::{
    checkout_work, fast_forward_base, fetch_origin, holds_other_work,
    paths_the_advance_overwrites, toplevel_of, BaseRefresh, BusyCheckout, CensusSetAside,
    CheckoutWork, RefusalCause,
};

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
    /// establish it: a base nobody knows cannot be refreshed, and nothing is
    /// going to move from it either.
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
    /// outright — so it refuses nothing for the tree.
    ///
    /// The branch name itself is NOT dropped: the base refresh is a git step
    /// that never depended on attribution, and it needs to know whether the
    /// tree is standing on the base it is about to fast-forward.
    pub(crate) fn attributable(mut self, attributable: bool) -> Self {
        self.attributable = attributable;
        self
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
}

/// THE ANSWER — two shapes and no more. Everything the answer describes has
/// ALREADY HAPPENED when it is returned; the caller only obeys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CensusSettlement {
    /// Do NOT proceed. Something is in the way of this move, and the refusal
    /// carries the measured paths and the cause so each door can say the same
    /// sentence in its own shape ([`BusyCheckout::reason`]).
    ///
    /// A refusal for the TREE (another unit's work that would travel) happens
    /// before any fetch: the tree is left exactly as it was found. A refusal
    /// for the BASE necessarily comes after the fetch that proved it stale, and
    /// it leaves the tree exactly as it was found too: the operator's files in
    /// the advance's way ([`RefusalCause::BaseBlockedByWork`]) are refused
    /// before any path is set aside, and a fast-forward that still fails
    /// ([`RefusalCause::BaseStale`]) puts back — index state included — the
    /// census paths that had been set aside for it.
    Refuse(BusyCheckout),
    /// Proceed. The base was refreshed from `origin` when there was a base to
    /// refresh and a remote that answered; nothing else was owed.
    Proceed,
}

/// Settle the question for one position and one tree.
///
/// The whole decision AND the whole effect. In order, and the order is the
/// point:
///
/// 0. **Resolve the root, once** ([`toplevel_of`]). Whatever a door passed —
///    the project root, the edited file's directory, a worktree — every git
///    call below runs from the repository's toplevel, because the paths git
///    ANSWERS (`status`, `diff --name-only`) are toplevel-relative and the
///    pathspecs it is HANDED (`stash push -- p`) are CWD-relative. From any
///    other directory the two disagree in silence. Resolving the root is not a
///    measurement of the tree: step 1 is still the only one.
/// 1. **Measure the tree, once** ([`checkout_work`]). Every later question
///    reads this one value; nothing measures again.
/// 2. **Refuse for the tree**: work that is not this unit's would ride along.
///    Refusing FIRST is what keeps a refused move from leaving a fetch or an
///    advance behind it.
/// 3. **Fetch `origin`** — the measurement the next two steps read. Offline,
///    nothing below happens and the cut takes the local base, as it always did.
/// 4. **Set the census aside where it stands in the way of the advance** —
///    and refuse, before touching anything, where the OPERATOR's files do.
///    Standing on the base with dirty paths that `origin`'s advance
///    overwrites, the fast-forward refuses on them ("local changes would be
///    overwritten"). Two kinds of path can stand in the way, and they get
///    different sentences:
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
///    words: never swallowed, never a cut from a stale base. It is the
///    invariant the root `CLAUDE.md` states in its own words: `--ff-only` only
///    passes while the integration base carries no commit of its own. When it
///    advanced, what was set aside is accounted for path by path
///    ([`CensusSetAside::settle_after_advance`]): miner output yields to
///    `origin`'s version, an authored mold is kept beside `origin`'s and said
///    so — nobody's text is deleted.
///
/// The caller does none of those steps and cannot reorder them.
pub(crate) fn settle(
    root: &Path,
    position: CheckoutPosition<'_>,
    config: &ProjectConfig,
) -> CensusSettlement {
    // An explicit `vcs: ""` opt-out (or a tree git does not manage) has no
    // base to refresh and nothing to refuse.
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

    // 2. REFUSE FOR THE TREE. Only the OPERATOR's work refuses (or a tree that
    //    could not be measured — an unmeasured tree is not an empty one, and
    //    reading it as empty is how another unit's work rides off in silence),
    //    and only where it is another unit's, i.e. where `holds_other_work`
    //    says so. Riding off a protected base into the first unit is by
    //    design. The census is nobody's work and never refuses.
    let refusal = match &work {
        CheckoutWork::ProvenClean | CheckoutWork::CensusOnly(_) => None,
        CheckoutWork::Holds { .. } | CheckoutWork::Unproven => position
            .would_carry_work_off(root, config)
            .then_some(RefusalCause::WorkWouldTravel),
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
                if !aside.paths().is_empty() {
                    eprintln!(
                        "base-gate: census output set aside (stashed) so '{base}' can advance \
                         to origin/{base} — {}",
                        aside.paths().join(", ")
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

    CensusSettlement::Proceed
}

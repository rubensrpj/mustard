//! `work_kind` — WHAT a work unit is, and WHICH base that makes it come from.
//!
//! Two types, one subject, deliberately apart:
//!
//! - [`WorkKind`] is the closed set of things a unit can BE (a feature, a fix,
//!   an emergency fix) and the ONE spelling of the `{kind}/{slug}` branch name
//!   built out of it. Nothing here reads configuration: a kind is an answer the
//!   operator gives, not a fact the repository holds.
//! - [`BaseFlow`] is the project's base model, derived ONCE from
//!   `mustard.json#git.flow`: which branches are integration bases, which of
//!   them ordinary work is cut from, and which are candidates for an emergency.
//!   It is also the crate's ONE parser of a work-branch NAME — both the current
//!   `{kind}/{slug}` shape and the `{base}_{slug}` shape units already in
//!   flight carry.
//!
//! **Why the base is not in the name any more.** It used to be: a unit was
//! `dev_my-thing`, and every consumer that needed the base recovered it by
//! reading the prefix back. The prefix now records what the unit IS — which is
//! what an operator reading a branch list actually wants — so the base is
//! derived from the declared flow instead of parsed out of a string. Both
//! shapes stay readable, because a unit in flight must not be orphaned by the
//! change: its pull request target, its merged-ancestry check and the
//! second-unit refusal all resolve a unit through its branch name.
//!
//! **Where the answer the flow cannot derive is kept.** An emergency in a
//! project declaring several candidate bases is a CHOICE, and the name does not
//! carry it. The open writes it into the spec's own event record (`base`), where
//! the pull-request step reads it; this model never does, so for such a unit
//! [`BaseFlow::base_of`] answers [`UnitBase::Ambiguous`] and the doors that must
//! settle it measure by containment.
//!
//! **Why this lives in `shared`.** Both faces ask these questions — the hook
//! gate cutting the branch and the commands settling, deleting, reporting and
//! resuming it — so per [`super`] the answer lives in the leaf both may depend
//! on. A second spelling in either face is how two consumers that must agree
//! about a branch stop agreeing.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use mustard_core::domain::config::GitConfig;
use mustard_core::platform::git;
use mustard_core::io::claude_paths::ClaudePaths;

/// The harness's own worktree-name prefix, tolerated wherever a branch NAME is
/// read: a worktree may be registered as `worktree-<branch>`, and the unit it
/// belongs to is the same either way.
const WORKTREE_PREFIX: &str = "worktree-";

/// Strip the tolerated [`WORKTREE_PREFIX`] — the one place it is spelled.
fn branch_of_name(name: &str) -> &str {
    name.strip_prefix(WORKTREE_PREFIX).unwrap_or(name)
}

/// The unit's own directory — `<project>/.claude/spec/{slug}/`. `None` when the
/// project root fails the `ClaudePaths` guard.
fn unit_dir(project: &Path, slug: &str) -> Option<PathBuf> {
    Some(ClaudePaths::for_project(project).ok()?.spec_dir().join(slug))
}

/// `true` when `rev` carries `path` in its tree — `git cat-file -e <rev>:<path>`.
///
/// The question a working-tree `stat` cannot answer: a unit's record is
/// committed ON the unit's branch, so from the base, or from a linked worktree
/// whose main checkout is elsewhere, the directory is simply not on disk while
/// the ref carries it perfectly well.
///
/// `false` on any failure — an unknown ref, a path the tree does not carry, git
/// missing entirely. The callers that matter here are asking whether they may
/// destroy something, so an unanswerable probe must not read as a yes.
fn ref_carries(project: &Path, rev: &str, path: &str) -> bool {
    git::run(project, &["cat-file", "-e", &format!("{rev}:{path}")]).ok
}

/// Process-wide memo of [`mustard_core::remote_branch_names`], keyed by the
/// root it was measured in. `None` inside the entry is the probe's own "could
/// not measure", memoised like any other answer.
///
/// One `git for-each-ref` is cheap; asking it once per BRANCH is not, and that
/// is what the repository-wide sweeps do — [`crate::shared::branch_state`]
/// resolves EVERY ref through [`BaseFlow::base_of`], so an unmemoised probe
/// turns one spawn into one per branch with an underscore in its name.
///
/// Same shape and same lifetime as the config memo in
/// [`crate::shared::context`]: `mustard-rt` is a one-shot process, so
/// "process-wide" is "for this dispatch", and a repository does not gain or
/// lose branches inside one.
fn remote_names_memo() -> &'static Mutex<HashMap<PathBuf, Option<BTreeSet<String>>>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Option<BTreeSet<String>>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Drop the memoised remote branch names for `root`.
///
/// The memo answers "which branches does `origin` have?" from the refs on disk,
/// and a `git fetch` is precisely the thing that changes that answer mid-run.
/// Without this the first probe of a dispatch freezes the pre-fetch picture, so
/// a branch that only MATERIALISES during the fetch reads as absent for the rest
/// of the same dispatch — and the old-shape reader that consults it
/// ([`BaseFlow::base_of`]) takes a unit already on the remote for one about to
/// be cut. Call it right after any fetch that can add or prune remote-tracking
/// refs.
pub(crate) fn forget_remote_names(root: &Path) {
    if let Ok(mut memo) = remote_names_memo().lock() {
        memo.remove(root);
    }
}

/// `true` when `base` is a branch the remote STILL has — and `true` as well
/// when its existence could NOT be measured.
///
/// The question the pending marker's recorded base is asked
/// ([`crate::commands::event::work_branch::recorded_or_derived_base`]).
///
/// **What it measures, and what it used to.** The test used to be membership in
/// `git.flow`'s declared set, which refuses a base the operator really picked
/// out of the real catalogue for the sole reason that a file written at install
/// time does not list it. Existence is the fact the protection was always
/// after: a base that no longer exists cannot be cut from, and one that exists
/// can — whoever declared it.
///
/// **Why unmeasured obeys.** A recorded base is a MEASUREMENT of a person's
/// answer, taken against the real catalogue when the marker was written.
/// Dropping it because the probe stayed silent — no git, no remote, a clone
/// whose refs were never fetched — refuses a real choice on the strength of a
/// source that said nothing, which is the very defect the membership test was.
/// An empty answer
/// counts as silence too: a repository with no remote-tracking refs cannot
/// testify about the remote, the same reading
/// [`crate::commands::event::work_branch::resolve_kind_base`] takes of an empty
/// catalogue and the candidate listing reports as `measured: false`.
/// **Local heads count too, and leaving them out re-created the defect.** The
/// cut that accepts the pick
/// ([`crate::commands::event::work_branch::checkout_work_branch`]) reads it
/// as `refs/heads/<b>` OR `refs/remotes/origin/<b>` — a base that was never
/// pushed is a real branch someone can cut from. This probe read only the
/// remote-tracking side, so such a pick was accepted by the cut and then
/// DISCARDED here. Two halves of one question measuring different things is the
/// shape this whole unit exists to remove, so they ask the same thing: does this
/// branch still exist, anywhere this repository can see?
// Sem chamador na produção: só o portão de base, guardado por decisão do
// usuário até ele decidir se o portão volta, pergunta se a base gravada no
// marcador ainda existe.
#[cfg(test)]
pub(crate) fn base_still_on_remote(root: &Path, base: &str) -> bool {
    if with_remote_names(root, |names| names_obey(names, base)) {
        return true;
    }
    // **A local head counts only if it was NEVER pushed.** Two very different
    // branches look identical in the remote-tracking catalogue — both absent:
    //
    //   never pushed          a real branch, living only on this machine
    //   deleted upstream      merged and retired; cutting from it is the
    //                         "base that no longer exists" this probe exists
    //                         to refuse
    //
    // `git fetch --prune` prunes remote-tracking refs, never local heads, so a
    // plain "does refs/heads/<base> exist?" obeys the retired branch and
    // reopens exactly the retired base this guard forbids. The upstream
    // configuration separates
    // them and is measured, not guessed: a branch that was pushed carries
    // `branch.<name>.remote`, and one that never left this machine does not.
    // Absent upstream ⇒ never pushed ⇒ a real local base, obey. Upstream set
    // but gone from the catalogue ⇒ retired upstream ⇒ ignore.
    local_head_exists(root, base) && !has_upstream(root, base)
}

/// `true` when `refs/heads/<branch>` resolves in `root`.
#[cfg(test)]
fn local_head_exists(root: &Path, branch: &str) -> bool {
    git::run(root, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")]).ok
}

/// `true` when `branch` has an upstream configured — the durable mark that it
/// was pushed at least once, and therefore that its absence from the
/// remote-tracking catalogue means RETIRED rather than never-published.
///
/// `false` on any failure, which is the safe reading for the only caller: an
/// unanswerable probe must not turn a local base into a retired one and drop a
/// base the operator really picked.
#[cfg(test)]
fn has_upstream(root: &Path, branch: &str) -> bool {
    let probe = git::run(root, &["config", "--get", &format!("branch.{branch}.remote")]);
    probe.ok && !probe.stdout.is_empty()
}

/// Hand `read` the MEMOISED remote branch names of `root`, measuring them on
/// the first call. The inner `None` is the probe's own "could not measure" and
/// is passed through untouched — folding it into an empty set is what turns an
/// offline machine into a repository with no branches.
///
/// The memo is never held across `read`: the value is cloned out first, so a
/// reader is free to ask anything it likes without the non-reentrant lock
/// deciding whether it deadlocks.
fn with_remote_names<T>(root: &Path, read: impl FnOnce(Option<&BTreeSet<String>>) -> T) -> T {
    let key = root.to_path_buf();
    let hit = remote_names_memo().lock().ok().and_then(|memo| memo.get(&key).cloned());
    if let Some(names) = hit {
        return read(names.as_ref());
    }
    let names = mustard_core::remote_branch_names(root);
    let answer = read(names.as_ref());
    if let Ok(mut memo) = remote_names_memo().lock() {
        memo.insert(key, names);
    }
    answer
}

/// The reading of one probe result: measured and naming `base` → obey;
/// measured and NOT naming it → drop; unmeasured (`None`, or an empty listing)
/// → obey. See [`base_still_on_remote`], which is where the reasoning lives.
#[cfg(test)]
fn names_obey(names: Option<&BTreeSet<String>>, base: &str) -> bool {
    match names {
        Some(names) if !names.is_empty() => names.contains(base),
        _ => true,
    }
}

/// What a work unit IS — the closed set the branch prefix names.
///
/// [`Hotfix`](WorkKind::parse("hotfix").expect("suggested token parses")) is NOT a third kind of work: the same code
/// change is a fix or a hotfix depending only on where it goes, next release or
/// straight to production. Nothing in a request's text separates them, which is
/// why this is never inferred from prose — it is asked, and this type is what
/// the answer parses into.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct WorkKind(String);

impl WorkKind {
    /// The tokens a chooser OFFERS — suggestions, not the permitted set.
    ///
    /// The first three are the git-flow words anyone arriving at a repository
    /// already reads; the rest are the conventional-commit types teams reach
    /// for next. A project that spells its work differently types its own and
    /// is not corrected.
    pub(crate) const SUGGESTED: [&'static str; 6] =
        ["feature", "fix", "hotfix", "chore", "refactor", "docs"];

    /// The stable token this kind is spelled with — in a branch name, on the
    /// command line, and in a report.
    pub(crate) fn token(&self) -> &str {
        &self.0
    }

    /// The kind an answer names, or `None` when the answer cannot be one.
    ///
    /// It no longer checks MEMBERSHIP of a closed set — that is the change.
    /// What it checks is whether the answer can be the first segment of a git
    /// ref: lower-cased, ASCII letters/digits/`-`/`_`, non-empty, and short
    /// enough to read. A branch name is the only thing this token becomes, so
    /// "can it be one" is the whole question; anything stricter was a taste
    /// about vocabulary dressed up as a validation.
    ///
    /// Case- and whitespace-insensitive: the value arrives from a person.
    pub(crate) fn parse(answer: &str) -> Option<Self> {
        let token = answer.trim().to_ascii_lowercase();
        if token.is_empty() || token.len() > 32 {
            return None;
        }
        if !token.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return None;
        }
        Some(Self(token))
    }

    /// The branch name for one unit — `{kind}/{slug}`.
    ///
    /// The ONE spelling of the join, so the builder
    /// ([`crate::commands::event::work_branch::compute_work_branch`]) and every
    /// parser here cannot drift into two shapes of the same name. It does NOT
    /// sanitise: making a valid git ref out of a slug is the builder's job, and
    /// doing it twice would let one caller's name differ from another's.
    pub(crate) fn branch_name(&self, slug: &str) -> String {
        format!("{}/{slug}", self.0)
    }

    /// The kind `branch` carries, or `None` when its name is not of this shape
    /// (an integration base, a unit still in the `{base}_{slug}` shape, a
    /// hand-cut branch). Tolerates the harness's `worktree-` prefix.
    pub(crate) fn of_branch(branch: &str) -> Option<Self> {
        let name = branch_of_name(branch);
        let (head, tail) = name.split_once('/')?;
        if tail.is_empty() {
            return None;
        }
        Self::parse(head)
    }
}

/// What the flow can say about the integration base one work branch belongs to.
///
/// THREE answers, never two, because two of them used to be told apart by
/// nothing: a name nobody owns and a name whose base cannot be chosen both
/// answered "here is a base" once the derivation was allowed to guess. They ask
/// for opposite things from a caller — the first is not this project's unit at
/// all, the second IS a unit whose base only the operator ever knew — and
/// [`Ambiguous`](UnitBase::Ambiguous) exists so the second is never handed over
/// dressed as a fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UnitBase {
    /// Not a work unit's name at all — a bare base, a stray ref, `HEAD`.
    NotAUnit,
    /// The base, KNOWN: carried by an old-shape prefix, or derived where the
    /// flow leaves no choice.
    Known(String),
    /// A work unit whose base nothing established: any `{kind}/{slug}` unit in a
    /// project declaring SEVERAL bases. Carries the candidates, so a caller that
    /// must refuse can name what it could not choose between.
    Ambiguous(Vec<String>),
}

impl UnitBase {
    /// The base when it is known, `None` when the answer does not exist
    /// (`NotAUnit`) or was never established ([`Ambiguous`](Self::Ambiguous)).
    pub(crate) fn known(&self) -> Option<&str> {
        match self {
            UnitBase::Known(base) => Some(base.as_str()),
            _ => None,
        }
    }

    /// [`known`](Self::known), by value — for the callers that store the answer.
    pub(crate) fn into_known(self) -> Option<String> {
        match self {
            UnitBase::Known(base) => Some(base),
            _ => None,
        }
    }

    /// `true` when the NAME is a work unit's of this project, whether or not its
    /// base could be answered.
    ///
    /// Deliberately apart from [`known`](Self::known): a collector deciding what
    /// it may delete, and a sweep deciding what to enumerate, are asking whether
    /// something is somebody's unit — and answering that with the base would
    /// make an unanswerable hotfix look like nobody's worktree.
    pub(crate) fn is_unit(&self) -> bool {
        !matches!(self, UnitBase::NotAUnit)
    }

    /// The bases the answer could not be chosen between — empty unless
    /// [`Ambiguous`](Self::Ambiguous).
    pub(crate) fn candidates(&self) -> &[String] {
        match self {
            UnitBase::Ambiguous(bases) => bases,
            _ => &[],
        }
    }
}

/// The project's integration bases, and what each work kind is cut from.
///
/// Built ONCE from [`GitConfig`] and passed around, rather than re-derived at
/// every question: the derivation allocates, and two consumers deriving it
/// separately is how they come to disagree about which base a unit belongs to.
///
/// Agnostic by construction — every name in here comes out of `git.flow`. This
/// type spells no branch literally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BaseFlow {
    /// Every base `git.flow` declares, in [`GitConfig::declared_bases`] order.
    /// A hint about where a picker opens — it refuses nothing.
    bases: Vec<String>,
    /// The base ordinary work is cut from — `flow["*"]`. `None` for a project
    /// that declares no flow: it has no ordinary base, and writing one down
    /// here would be this type inventing the project's own answer.
    work: Option<String>,
    /// The project root whose UNIT DIRECTORIES and remote branches this model
    /// may consult ([`BaseFlow::of_at`]), `None` for the pure derivation
    /// ([`BaseFlow::of`]).
    ///
    /// It is the only reason this type touches the repository, and it touches it
    /// for two questions the flow alone cannot answer: whether a name in the
    /// old `{base}_{slug}` shape is a unit of this project, and whether a
    /// branch has a unit record at all ([`BaseFlow::has_unit_record`]).
    project: Option<PathBuf>,
}

impl BaseFlow {
    /// Derive the model from one project's declared flow.
    ///
    /// It used to also walk the promotion chain outward from the work base, so a
    /// separate `emergency` list could name the outermost base as a hotfix's
    /// default. Nothing asks for that default any more —
    /// [`base_of`](Self::base_of) answers `Known` only when the DECLARED set
    /// lands on one base or the name carries it, and `Ambiguous` otherwise — so
    /// the walk and the list it fed are gone. Ordering candidates
    /// only mattered while something picked one off the list unasked, which is
    /// exactly the behaviour that was removed.
    pub(crate) fn of(git: &GitConfig) -> Self {
        Self::build(git, None)
    }

    /// [`of`](Self::of), plus the project whose UNIT DIRECTORIES and remote
    /// branches may be read.
    ///
    /// Every consumer that resolves a REAL branch of a REAL repository builds
    /// the model this way, because the name is not always enough: a unit in the
    /// old `{base}_{slug}` shape is told from an integration line by the
    /// branches the repository really has and by the unit's own directory. A
    /// rootless [`of`](Self::of) stays for the pure question — "what does this
    /// flow imply" — which is what the chooser at cut time asks.
    pub(crate) fn of_at(git: &GitConfig, project: &Path) -> Self {
        Self::build(git, Some(project.to_path_buf()))
    }

    /// The one derivation, with or without a project to consult.
    fn build(git: &GitConfig, project: Option<PathBuf>) -> Self {
        let bases: Vec<String> = git.declared_bases().into_iter().collect();
        let work = git.primary_base();
        Self { bases, work, project }
    }

    /// Every declared integration base — for the consumers that iterate them
    /// (ancestry reads, base refreshes) rather than ask about one branch.
    pub(crate) fn bases(&self) -> &[String] {
        &self.bases
    }

    /// The base ordinary work is cut from, `None` when the project declares
    /// no flow.
    pub(crate) fn work_base(&self) -> Option<&str> {
        self.work.as_deref()
    }

    /// The integration base a work branch belongs to.
    ///
    /// Two sources, asked in this order, and the ORDER is the whole point:
    ///
    /// 1. `{base}_{slug}` — a unit still in the pre-kind shape carries its base
    ///    in the name: the LONGEST branch `B` with the name starting `"{B}_"`,
    ///    asked of the declared bases and then of the branches the repository
    ///    really has ([`legacy_base_of`](Self::legacy_base_of)), so a project
    ///    carrying both `dev` and `dev_release` reads `dev_release_x` as the
    ///    latter's — declared or not.
    /// 2. the flow's SINGLE declared base, when it declares exactly one — the
    ///    only case where nothing is being guessed, because there was never a
    ///    choice to make.
    ///
    /// There is no third source. The kind used to imply a base, and that
    /// inference is gone with the coupling that produced it: the base is the
    /// operator's answer to a question they were asked against a real list. That
    /// answer lives in the unit's spec record, written by the open and read by
    /// the pull-request step, never re-derived here.
    ///
    /// And when neither source answers, it says so — [`UnitBase::Ambiguous`].
    /// It used to answer the outermost candidate, which silently replaced the
    /// operator's pick on every read: the pull-request target and the
    /// merged-ancestry check included.
    pub(crate) fn base_of(&self, branch: &str) -> UnitBase {
        let name = branch_of_name(branch);
        if self.is_declared_base(name) {
            return UnitBase::NotAUnit;
        }
        if WorkKind::of_branch(name).is_none() {
            return match self.legacy_base_of(name) {
                Some(base) => UnitBase::Known(base),
                None => UnitBase::NotAUnit,
            };
        }
        match self.bases() {
            [only] => UnitBase::Known(only.clone()),
            candidates => UnitBase::Ambiguous(candidates.to_vec()),
        }
    }

    /// `true` when `name` is one of the branches this project's `git.flow`
    /// names — so it is an integration BASE, and nobody's work unit.
    ///
    /// The kind vocabulary is open, so `{kind}/{slug}` is a shape and not a
    /// list: `release/2026-Q3` splits into a first segment that parses as a kind
    /// and a second that parses as a slug, exactly like `feature/aba` does.
    /// Reading names alone therefore makes a project's own release line answer
    /// "somebody's unit" — and the two doors that ask this question act on it:
    /// the discard of an abandoned unit would offer to REMOVE the release line,
    /// and the pull-request list would refuse to run from it.
    ///
    /// This is the one reading of the declared set that is not a permission.
    /// It refuses the operator nothing — a base is still cut from freely, and a
    /// base nobody declared is still a perfectly good base. All it does is stop
    /// the harness from mistaking a branch the project ITSELF called a base for
    /// a disposable unit, which is the only direction of this question where
    /// being wrong destroys something.
    pub(crate) fn is_declared_base(&self, name: &str) -> bool {
        self.bases.iter().any(|b| b == name)
    }

    /// The `{base}_` half of a name still in the pre-kind shape. Separate from
    /// [`base_of`](Self::base_of) because the slug reader needs the base it
    /// matched, not the base the unit integrates into.
    ///
    /// TWO sources, and the second is why a name is no longer refused for the
    /// company it keeps: the DECLARED bases first (unchanged — nothing that
    /// resolved before stops resolving), then, for a rooted model, the branches
    /// the repository REALLY has. A unit cut as `hml_x` back when `hml` was the
    /// project's base is still that unit after `mustard.json` stopped naming it,
    /// and a project whose install wrote no flow at all — which is every project
    /// the current installer touches — resolves the shape at all.
    ///
    /// Longest match on both sides, so a repository carrying `dev` and
    /// `dev_release` reads `dev_release_x` as the latter's.
    ///
    /// An unmeasured probe answers `None`, exactly as an unmatched one does, and
    /// that sameness is deliberate here: the caller reads `None` as "nobody's
    /// unit", which costs a non-unit cut, while a positive answer nobody
    /// measured would cut a UNIT from a base that may not exist.
    ///
    /// **One name the catalogue leg refuses**, and refusing it is what keeps
    /// this leg from doing the damage [`is_declared_base`](Self::is_declared_base)
    /// exists to prevent: a branch the REMOTE ITSELF CARRIES that this project
    /// holds no unit for. Branch names carry no mark separating a base from a
    /// unit, so a project whose integration line is spelled `hml_prod` —
    /// undeclared, like every branch of a project the current installer touched
    /// — matches `hml` on the catalogue and reads as "the unit `prod`": the
    /// discard would offer to remove the integration line, and the pull-request
    /// list would refuse to run from it. The two facts together are what tell
    /// them apart.
    /// A branch that is already ON the remote is one of the project's own; a
    /// unit of THIS harness has a directory under `.claude/spec/` naming it, and
    /// a name the remote has never seen cannot be a branch of the project at all
    /// — it is the unit about to be cut, which is how the worktree door reaches
    /// here. Being wrong in the "somebody's unit" direction destroys a branch
    /// and in the other direction costs a refusal, so where nothing distinguishes
    /// them the refusal wins.
    fn legacy_base_of(&self, name: &str) -> Option<String> {
        let declared = self
            .bases
            .iter()
            .filter(|b| name.starts_with(&format!("{b}_")))
            .max_by_key(|b| b.len())
            .cloned();
        if declared.is_some() {
            return declared;
        }
        if !name.contains('_') {
            return None; // no probe for a name that cannot carry the shape
        }
        let project = self.project.as_deref()?;
        with_remote_names(project, |names| {
            let names = names?;
            let base = names
                .iter()
                .filter(|b| name.starts_with(&format!("{b}_")))
                .max_by_key(|b| b.len())?
                .clone();
            let slug = name.strip_prefix(&format!("{base}_"))?.trim();
            let holds_a_unit =
                !slug.is_empty() && unit_dir(project, slug).is_some_and(|dir| dir.is_dir());
            (holds_a_unit || !names.contains(name)).then_some(base)
        })
    }

    /// The unit a work branch names — the slug half, whichever shape carries it.
    ///
    /// `None` for anything that is not a work unit of THIS project. That
    /// refusal is load-bearing: inventing a slug out of an unrecognised name
    /// would mint a second name for a unit that already has one, which is the
    /// exact drift this module exists to prevent.
    pub(crate) fn slug_of(&self, branch: &str) -> Option<String> {
        let name = branch_of_name(branch);
        if self.is_declared_base(name) {
            return None;
        }
        if let Some(kind) = WorkKind::of_branch(name) {
            let slug = name.strip_prefix(&format!("{}/", kind.token()))?.trim();
            return (!slug.is_empty()).then(|| slug.to_string());
        }
        let base = self.legacy_base_of(name)?;
        let slug = name.strip_prefix(&format!("{base}_"))?.trim();
        (!slug.is_empty()).then(|| slug.to_string())
    }

    /// `true` when this project holds a RECORD proving `branch` is one of ITS
    /// work units — the unit's own directory under `.claude/spec/`.
    ///
    /// **Why a record and not the name.** Two answers were tried here and both
    /// destroyed something. The name's SHAPE cannot answer it: the kind
    /// vocabulary is open by design, so `release/2026-Q3` splits into a kind and
    /// a slug exactly like `fix/aba` does, and a project's own release line
    /// therefore read as somebody's disposable unit. The DECLARED set cannot
    /// answer it either: `mustard init` no longer writes `git.flow`, so the
    /// declared set is EMPTY for the projects the installer produces — a guard
    /// built on it guards nothing, and the discard of an abandoned unit would
    /// remove a real release line from the remote in a project shaped exactly
    /// that way.
    ///
    /// A branch this harness CUT has a directory; a branch the project has
    /// always had does not. That is evidence the project itself recorded, and it
    /// is what the two doors that may destroy or refuse must read.
    ///
    /// **`false` is the safe answer, and it is deliberate.** No project to
    /// consult, an unreadable path, a name that parses to no slug, or a slug
    /// with no directory all answer `false` — for an irreversible action,
    /// absence of evidence must REFUSE rather than permit. A caller that only
    /// wants to know what a name looks like should keep asking
    /// [`base_of`](Self::base_of); this question is for the callers where being
    /// wrong costs a branch.
    pub(crate) fn has_unit_record(&self, branch: &str) -> bool {
        let Some(project) = self.project.as_deref() else {
            return false;
        };
        let Some(slug) = self.slug_of(branch) else {
            return false;
        };
        if slug.is_empty() {
            return false;
        }
        if unit_dir(project, &slug).is_some_and(|dir| dir.is_dir()) {
            return true;
        }
        // **The working tree is not the only place the record lives, and reading
        // only it inverted two doors.** The unit's directory is authored ON the
        // unit's branch, while every door that asks this question runs from
        // somewhere else: from the base, where the branch's files are not
        // checked out, or from a linked worktree, whose checkout IS the unit
        // while `project` points at the main one. Both answered `false` for a
        // real unit, and both then did the opposite of what they promise —
        // the cancel path of an abandoned unit (`crate::commands::git_delete`)
        // destroyed the worktree the caller was standing in instead of
        // refusing, and the exit ritual (`crate::commands::git_settle`) refused
        // the very position its own hint prescribes.
        //
        // So the question is asked of git too, in the two refs that can carry
        // it. This is ONE reading shared by all three doors on purpose: a weak
        // reading guarding while a strong one permits is how a destructive
        // command ends up more permissive than the check in front of it.
        let path = format!(".claude/spec/{slug}");
        ["", "origin/"].iter().any(|prefix| ref_carries(project, &format!("{prefix}{branch}"), &path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two-tier flow this project declares: `dev` for ordinary work, `main`
    /// as its outermost base.
    fn two_tier() -> GitConfig {
        let mut git = GitConfig::default();
        git.flow.insert("*".to_string(), "dev".to_string());
        git.flow.insert("dev".to_string(), "main".to_string());
        git
    }

    /// A three-tier flow — `dev` → `qas` → `main` — where an emergency has more
    /// than one candidate base and the operator has a real choice to make.
    fn three_tier() -> GitConfig {
        let mut git = GitConfig::default();
        git.flow.insert("*".to_string(), "dev".to_string());
        git.flow.insert("dev".to_string(), "qas".to_string());
        git.flow.insert("qas".to_string(), "main".to_string());
        git
    }

    /// The vocabulary is open. A token the project never suggested makes
    /// a branch name exactly like a suggested one; what is still refused is a
    /// token that could not be a git ref segment at all.
    #[test]
    fn accepts_a_type_outside_the_suggested_list() {
        let chore = WorkKind::parse("chore").expect("an ordinary conventional-commit type");
        assert_eq!(chore.branch_name("limpa-lockfile"), "chore/limpa-lockfile");

        let invented = WorkKind::parse("spike").expect("a token no list mentions");
        assert_eq!(invented.branch_name("prova-de-conceito"), "spike/prova-de-conceito");
        assert_eq!(
            WorkKind::of_branch("spike/prova-de-conceito").as_ref().map(WorkKind::token),
            Some("spike"),
            "and it reads back as its own kind",
        );

        assert_eq!(
            WorkKind::parse("  FEATURE  ").as_ref().map(WorkKind::token),
            Some("feature"),
            "the answer comes from a person: trimmed and lower-cased",
        );

        for refused in ["", "   ", "feat/ure", "com espaco", "acentuação"] {
            assert!(
                WorkKind::parse(refused).is_none(),
                "not a possible ref segment: {refused:?}",
            );
        }
    }

    #[test]
    fn a_kind_round_trips_through_its_token_and_its_branch_prefix() {
        for token in WorkKind::SUGGESTED {
            let kind = WorkKind::parse(token).expect("suggested token parses");
            assert_eq!(WorkKind::parse(kind.token()).as_ref(), Some(&kind));
            assert_eq!(WorkKind::of_branch(&kind.branch_name("my-unit")).as_ref(), Some(&kind));
        }
        // A person's answer, not a machine's: spacing and case are tolerated.
        assert_eq!(
            WorkKind::parse("  HotFix ").as_ref().map(WorkKind::token),
            Some("hotfix"),
        );
        // `chore` used to be rejected for not being one of three. The list is a
        // suggestion now, so it parses like any other possible ref segment.
        assert_eq!(WorkKind::parse("chore").as_ref().map(WorkKind::token), Some("chore"));

        // Names that are NOT of this shape carry no kind — including the one
        // that merely starts with the same letters.
        // Names that carry no kind: no slash at all, or a slash with nothing
        // after it. `features/x` and `fixup/x` DO carry one now — `features`
        // and `fixup` are possible ref segments, and refusing them was the
        // closed vocabulary talking.
        for other in ["dev", "dev_my-unit", "feature", "feature/"] {
            assert_eq!(WorkKind::of_branch(other), None, "not a kind branch: {other}");
        }
        // …and the harness's own worktree prefix is tolerated.
        assert_eq!(
            WorkKind::of_branch("worktree-fix/my-unit").as_ref().map(WorkKind::token),
            Some("fix"),
        );
    }

    /// What the flow derives now that the separately ordered emergency list is
    /// gone: the DECLARED set, and the one base ordinary work is cut from.
    ///
    /// `bases()` comes out of a `BTreeSet`, so it is sorted and not
    /// promotion-ordered. That is the point — ordering candidates only mattered
    /// while something picked one off the list unasked, and `base_of` stopped
    /// doing that: several candidates answer `Ambiguous` and the operator picks.
    #[test]
    fn the_flow_derives_its_declared_bases_and_the_work_base() {
        let two = BaseFlow::of(&two_tier());
        assert_eq!(two.work_base(), Some("dev"));
        assert_eq!(two.bases(), ["dev", "main"]);

        let three = BaseFlow::of(&three_tier());
        assert_eq!(three.work_base(), Some("dev"));
        assert_eq!(three.bases(), ["dev", "main", "qas"], "several candidates to choose from");

        // A single-base project has one answer and no choice.
        let mut single = GitConfig::default();
        single.flow.insert("*".to_string(), "main".to_string());
        assert_eq!(BaseFlow::of(&single).bases(), ["main"]);

        // A flow that loops back is still a finite set of names.
        let mut cyclic = GitConfig::default();
        cyclic.flow.insert("*".to_string(), "dev".to_string());
        cyclic.flow.insert("dev".to_string(), "main".to_string());
        cyclic.flow.insert("main".to_string(), "dev".to_string());
        assert_eq!(BaseFlow::of(&cyclic).bases(), ["dev", "main"]);

        // `spike` is declared (it is a flow VALUE) even though the chain from
        // `dev` never reaches it — being off the promotion path never took a
        // base off the list, and now nothing walks the path at all.
        let mut spike = three_tier();
        spike.flow.insert("spike".to_string(), "spike".to_string());
        assert!(BaseFlow::of(&spike).bases().contains(&"spike".to_string()));
    }

    #[test]
    fn both_branch_shapes_resolve_to_one_base_and_one_slug() {
        let flow = BaseFlow::of(&two_tier());

        // The base no longer comes from the KIND — it is the operator's answer,
        // kept in the spec's record. With two declared bases the name alone
        // answers nothing, every kind reads the same way, and that sameness IS
        // the change: the prefix stopped carrying a base.
        for name in ["feature/my-unit", "fix/my-unit", "hotfix/my-unit"] {
            assert!(
                flow.base_of(name).known().is_none(),
                "the prefix no longer answers where {name} came from",
            );
            assert!(flow.base_of(name).is_unit(), "{name} is still a unit of this project");
        }
        assert_eq!(flow.slug_of("feature/my-unit").as_deref(), Some("my-unit"));
        assert_eq!(flow.slug_of("hotfix/my-unit").as_deref(), Some("my-unit"));

        // The shape units in flight carry: the base comes from the prefix.
        assert_eq!(flow.base_of("dev_my-unit").known(), Some("dev"));
        assert_eq!(flow.base_of("main_my-unit").known(), Some("main"));
        assert_eq!(flow.slug_of("dev_my-unit").as_deref(), Some("my-unit"));
        assert_eq!(flow.slug_of("worktree-dev_my-unit").as_deref(), Some("my-unit"));

        // Neither shape: not a work unit, and no slug is invented out of it.
        for other in ["dev", "main", "nounderscore", "feature_x", "HEAD"] {
            assert_eq!(flow.base_of(other), UnitBase::NotAUnit, "not a unit: {other}");
            assert!(!flow.base_of(other).is_unit(), "not a unit: {other}");
            assert_eq!(flow.slug_of(other), None, "no slug invented: {other}");
        }
        // An empty slug is not a unit either.
        assert_eq!(flow.slug_of("feature/"), None);
        assert_eq!(flow.slug_of("dev_"), None);
    }

    #[test]
    fn a_nested_base_wins_the_longest_match_in_the_old_shape() {
        let mut git = GitConfig::default();
        git.flow.insert("*".to_string(), "dev".to_string());
        git.flow.insert("dev".to_string(), "dev_release".to_string());
        git.flow.insert("dev_release".to_string(), "main".to_string());
        let flow = BaseFlow::of(&git);
        assert_eq!(flow.base_of("dev_release_thing").known(), Some("dev_release"));
        assert_eq!(flow.slug_of("dev_release_thing").as_deref(), Some("thing"));
    }


    /// A repository whose REMOTE really has `branches` — the refs the existence
    /// probe reads. `false` when git is unusable here, so a caller can skip
    /// instead of asserting against a probe that measured nothing.
    fn seed_remote_refs(root: &Path, branches: &[&str]) -> bool {
        let git = |args: &[&str]| git::run(root, args).ok;
        if !git(&["init", "-q", "-b", "dev", "."]) {
            return false;
        }
        let _ = git(&["config", "user.email", "t@t.t"]);
        let _ = git(&["config", "user.name", "t"]);
        if !git(&["commit", "-q", "--allow-empty", "-m", "seed"]) {
            return false;
        }
        branches.iter().all(|b| git(&["update-ref", &format!("refs/remotes/origin/{b}"), "HEAD"]))
    }

    /// A base file an earlier version left in the unit's directory answers
    /// nothing: with several declared bases the flow alone speaks, so the unit is
    /// `Ambiguous` and names every candidate, whether or not the remote still has
    /// the branch the file names.
    #[test]
    fn a_base_file_left_in_the_unit_directory_does_not_answer_the_base() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path();
        if !seed_remote_refs(project, &["dev", "qas", "main", "release/2026-Q3"]) {
            return; // no usable git here — nothing here is measurable at all
        }
        let unit = project.join(".claude").join("spec").join("na-linha-de-release");
        std::fs::create_dir_all(&unit).expect("unit dir");
        std::fs::write(unit.join(".cut-base"), "release/2026-Q3\n").expect("leftover file");

        let answer = BaseFlow::of_at(&three_tier(), project).base_of("hotfix/na-linha-de-release");
        assert!(answer.is_unit(), "it is still this project's unit");
        assert_eq!(answer.known(), None, "the file is not read, even for a branch the remote has");
        assert_eq!(answer.candidates(), ["dev", "main", "qas"], "the flow names what it could not choose");
    }

    /// An integration line whose NAME carries an underscore is not somebody's
    /// unit, and the catalogue leg of the legacy reader must not turn it into
    /// one.
    ///
    /// `hml_prod` in a project that declares no flow — every project the current
    /// installer touches — matches `hml` on the catalogue, so reading the name
    /// alone answers "the unit `prod`": the discard would offer to remove the
    /// integration line and the pull-request list would refuse to run from it,
    /// which is exactly the damage `is_declared_base` prevents, arriving
    /// through the other door. Both kinds of real unit still resolve —
    /// the one this harness already cut, and the one that does not
    /// exist on the remote yet because it is about to be cut.
    #[test]
    fn an_underscored_base_is_not_mistaken_for_a_legacy_unit() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path();
        if !seed_remote_refs(project, &["hml", "hml_prod", "hml_minha-unidade"]) {
            return; // no usable git here — nothing here is measurable at all
        }
        // The project declares nothing, so `hml_prod` is undeclared exactly as
        // the release line of any freshly installed project is.
        let flow = BaseFlow::of_at(&GitConfig::default(), project);

        assert_eq!(
            flow.base_of("hml_prod"),
            UnitBase::NotAUnit,
            "a branch the remote carries, that no unit of this project names, is a BASE",
        );
        assert_eq!(flow.slug_of("hml_prod"), None, "…so it has no slug to be retired under");

        // A name the remote has never seen is the unit about to be cut — the
        // shape the worktree door hands over.
        assert_eq!(
            flow.base_of("hml_ainda-nao-empurrada").known(),
            Some("hml"),
            "a name no branch carries is nobody's base — it is the unit being opened",
        );

        // …and the unit this harness really cut in that shape still reads,
        // pushed or not, because its directory names it.
        std::fs::create_dir_all(project.join(".claude").join("spec").join("minha-unidade"))
            .expect("unit dir");
        assert_eq!(
            flow.base_of("hml_minha-unidade").known(),
            Some("hml"),
            "a unit whose directory this project holds resolves by its name, declared or not",
        );
        assert_eq!(flow.slug_of("hml_minha-unidade").as_deref(), Some("minha-unidade"));
    }
}

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
//! project declaring several candidate bases is a CHOICE, and the name no longer
//! carries it. The cut writes it into the unit's own directory as harness state
//! ([`CUT_BASE_FILE`]) and the draft folds it into `meta.json#base`; both are
//! read back here, in that order. It is deliberately NOT written into
//! `meta.json` by the cut: the cut runs first, and a sidecar in that directory
//! is exactly what makes the draft refuse it as already drafted.
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
    let root = project.to_string_lossy().to_string();
    std::process::Command::new("git")
        .args(["-C", &root, "cat-file", "-e", &format!("{rev}:{path}")])
        .output()
        .is_ok_and(|out| out.status.success())
}

/// The cut's OWN record of the base, inside the unit's directory.
///
/// **Why a file of its own, and why this name.** The answer's durable home is
/// `meta.json#base` — the sidecar that already holds every machine-parseable
/// fact about a unit — but the CUT cannot write it there: the cut runs BEFORE
/// the draft, and `spec-draft` refuses to draft into a directory that already
/// holds anything but harness state ([`crate::commands::spec::spec_draft`]'s
/// `holds_only_harness_state`, whose allowlist is dot-prefixed harness state and
/// whose whole reason for existing is that a `meta.json` there IS a drafted
/// spec). Writing the base into `meta.json` at cut time therefore made step one
/// block step two: the unit was cut and got no spec at all.
///
/// So the cut writes HERE, and the draft folds it into `meta.json#base` when it
/// writes the sidecar ([`crate::commands::spec::spec_scaffold::write_meta_json`])
/// and retires the file. This name is harness state, not authored work, on every
/// term the rest of the per-spec spill is (`.events`, `.dispatch`, `.blobs`,
/// `.memory-approved`): nobody authors it, it holds one machine token the
/// harness wrote to itself, it is derivable again for every unit whose flow can
/// answer, and it never reaches the merge — the unit's authored work is
/// `spec.md`, the waves, the proof, the change log and the review verdicts.
pub(crate) const CUT_BASE_FILE: &str = ".cut-base";

/// The base recorded in `dir`'s cut record, `None` when there is none (or it is
/// empty/unreadable). Trimmed — the file carries one line and a newline.
pub(crate) fn cut_base_in(dir: &Path) -> Option<String> {
    let body = std::fs::read_to_string(dir.join(CUT_BASE_FILE)).ok()?;
    let base = body.trim();
    (!base.is_empty()).then(|| base.to_string())
}

/// Retire `dir`'s cut record — called once its content has been folded into the
/// sidecar, so the answer has exactly one home again. Best-effort: a file that
/// could not be removed is still redundant, never wrong.
pub(crate) fn clear_cut_base_in(dir: &Path) {
    let _ = std::fs::remove_file(dir.join(CUT_BASE_FILE));
}

/// Process-wide memo of [`mustard_core::remote_branch_names`], keyed by the
/// root it was measured in. `None` inside the entry is the probe's own "could
/// not measure", memoised like any other answer.
///
/// One `git for-each-ref` is cheap; asking it once per BRANCH is not, and that
/// is what the repository-wide sweeps do — [`crate::shared::branch_state`]
/// resolves EVERY ref through [`BaseFlow::base_of`], so an unmemoised probe
/// turns one spawn into one per unit branch that has a record.
///
/// Same shape and same lifetime as the config memo in
/// [`crate::shared::context`]: `mustard-rt` is a one-shot process, so
/// "process-wide" is "for this dispatch", and a repository does not gain or
/// lose branches inside one.
fn remote_names_memo() -> &'static Mutex<HashMap<PathBuf, Option<BTreeSet<String>>>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Option<BTreeSet<String>>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// `true` when `base` is a branch the remote STILL has — and `true` as well
/// when its existence could NOT be measured.
///
/// The one spelling of the question both readers of a recorded base ask
/// ([`BaseFlow::recorded_base_of`] for the unit's durable record,
/// [`crate::commands::event::work_branch::recorded_or_derived_base`] for the
/// pending marker), so the cut and every later read agree about which recorded
/// bases still count.
///
/// **What it measures, and what it used to.** The test used to be membership in
/// `git.flow`'s declared set, which refuses a base the operator really picked
/// out of the real catalogue for the sole reason that a file written at install
/// time does not list it. Existence is the fact the protection was always
/// after: a base that no longer exists cannot be cut from, and one that exists
/// can — whoever declared it.
///
/// **Why unmeasured obeys.** A recorded base is a MEASUREMENT of a person's
/// answer, taken against the real catalogue at cut time. Dropping it because
/// the probe stayed silent — no git, no remote, a clone whose refs were never
/// fetched — refuses a real choice on the strength of a source that said
/// nothing, which is the very defect the membership test was. An empty answer
/// counts as silence too: a repository with no remote-tracking refs cannot
/// testify about the remote, the same reading
/// [`crate::commands::event::work_branch::resolve_kind_base`] takes of an empty
/// catalogue and `base-candidates` reports as `measured: false`.
pub(crate) fn base_still_on_remote(root: &Path, base: &str) -> bool {
    with_remote_names(root, |names| names_obey(names, base))
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
fn names_obey(names: Option<&BTreeSet<String>>, base: &str) -> bool {
    match names {
        Some(names) if !names.is_empty() => names.contains(base),
        _ => true,
    }
}

/// `Some(true)` when the repository at `root` really offers MORE THAN ONE
/// branch to cut from, `Some(false)` when it offers exactly one, and `None`
/// when nothing could be measured.
///
/// This is "was there a choice to make?", asked of the catalogue the operator
/// was actually shown ([`mustard_core::branch_catalog`] reads the same refs) —
/// not of the declared flow. A project declaring a single base still carries
/// every branch `origin` has, and the picker offers all of them, so counting
/// the DECLARATION answered a question nobody asked: it reported "nothing was
/// chosen" about a repository where the operator had just chosen.
///
/// Same probe and same memo as [`base_still_on_remote`], so the two halves of
/// one pick — whether it is written down, and whether it is still obeyed — read
/// the identical set of refs.
fn catalogue_offers_a_choice(root: &Path) -> Option<bool> {
    with_remote_names(root, |names| match names {
        Some(names) if !names.is_empty() => Some(names.len() > 1),
        _ => None,
    })
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

    /// The kind a chooser pre-marks when the caller named none — the first
    /// SUGGESTED token. A constructor rather than a `const` because the token
    /// is owned now; a default that cannot be spelled is worse than a function
    /// call.
    pub(crate) fn suggested_default() -> Self {
        Self(Self::SUGGESTED[0].to_string())
    }

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

    /// `true` when `segment` is a kind's own path segment — the directory a
    /// unit's `{kind}/{slug}` worktree sits INSIDE, rather than a worktree of
    /// anybody's. A collector walking `.claude/worktrees/` meets these before it
    /// meets any unit, and deleting one would take every unit under it.
    ///
    /// Opening the vocabulary makes this answer `true` more often, and that is
    /// the SAFE direction for both callers: here a false `true` leaves a stale
    /// directory standing, while a false `false` deletes every unit inside it.
    pub(crate) fn is_container_segment(segment: &str) -> bool {
        Self::parse(segment).is_some()
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
    /// The base, KNOWN: recorded by the cut, carried by an old-shape prefix, or
    /// derived where the flow leaves no choice.
    Known(String),
    /// A work unit whose base nothing established: a `hotfix/…` in a project
    /// declaring SEVERAL emergency bases, with no record from its cut. Carries
    /// the candidates, so a caller that must refuse can name what it could not
    /// choose between.
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
    /// Every base `git.flow` PRE-SELECTS, in
    /// [`GitConfig::preselected_bases`] order. A hint about where a picker
    /// opens — it refuses nothing.
    bases: Vec<String>,
    /// The base ordinary work is cut from — `flow["*"]`.
    work: String,
    /// The bases that are NOT the work base, ordered so the LAST is the
    /// project's outermost one. See [`BaseFlow::of`].
    emergency: Vec<String>,
    /// The project root whose UNIT RECORDS this model may consult
    /// ([`BaseFlow::of_at`]), `None` for the pure derivation ([`BaseFlow::of`]).
    ///
    /// It is the only reason this type touches the filesystem, and it touches it
    /// for exactly one question: which base a unit was ACTUALLY cut from, when
    /// the flow alone cannot answer. A rootless model still answers everything
    /// it can derive — it simply reports [`UnitBase::Ambiguous`] where a rooted
    /// one would have read the operator's own answer.
    project: Option<PathBuf>,
}

impl BaseFlow {
    /// Derive the model from one project's declared flow.
    ///
    /// The emergency ORDER is the only non-obvious part, and it exists so
    /// [`base_of_kind`](Self::base_of_kind) can name a default without guessing:
    /// the promotion chain is walked outward from the work base
    /// (`flow[work]`, then `flow[that]`, …), each step being one closer to
    /// production, so the chain's end is the project's outermost base. Bases the
    /// chain never reaches are off the promotion path altogether, so they are
    /// listed BEFORE it — leaving the outermost base last whatever else the
    /// project declares. A cyclic flow terminates on the first repeat.
    pub(crate) fn of(git: &GitConfig) -> Self {
        Self::build(git, None)
    }

    /// [`of`](Self::of), plus the project whose UNIT RECORDS may be read.
    ///
    /// Every consumer that resolves a REAL branch of a REAL repository builds
    /// the model this way, because the derivation is not always enough: the base
    /// a hotfix was cut from is the operator's choice whenever the project
    /// declares more than one candidate, and the unit's own directory
    /// ([`recorded_base_of`](Self::recorded_base_of)) is where that choice was
    /// written down. A rootless [`of`](Self::of) stays
    /// for the pure question — "what does this flow imply" — which is what the
    /// chooser at cut time asks.
    pub(crate) fn of_at(git: &GitConfig, project: &Path) -> Self {
        Self::build(git, Some(project.to_path_buf()))
    }

    /// The one derivation, with or without a project to consult.
    fn build(git: &GitConfig, project: Option<PathBuf>) -> Self {
        let bases: Vec<String> = git.preselected_bases().into_iter().collect();
        let work = git.primary_base();

        let mut chain: Vec<String> = Vec::new();
        let mut seen: Vec<String> = vec![work.clone()];
        let mut cursor = work.clone();
        while let Some(next) =
            git.flow.get(&cursor).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
        {
            if seen.contains(&next) {
                break; // a flow that loops back is still a finite walk
            }
            seen.push(next.clone());
            chain.push(next.clone());
            cursor = next;
        }
        let mut emergency: Vec<String> =
            bases.iter().filter(|b| !seen.contains(b)).cloned().collect();
        emergency.extend(chain);

        Self { bases, work, emergency, project }
    }

    /// Every declared integration base — for the consumers that iterate them
    /// (ancestry reads, base refreshes) rather than ask about one branch.
    pub(crate) fn bases(&self) -> &[String] {
        &self.bases
    }

    /// The base ordinary work is cut from.
    pub(crate) fn work_base(&self) -> &str {
        &self.work
    }

    /// The bases an emergency may be cut from — every integration base that is
    /// not the work base, outermost LAST.
    ///
    /// The COUNT is what decides whether the operator is asked anything: with
    /// exactly one candidate there is nothing to choose and a question would be
    /// pure ceremony; with several, the choice is theirs and this list is what
    /// they choose from.
    pub(crate) fn emergency_bases(&self) -> &[String] {
        &self.emergency
    }

    /// `true` when a hotfix leaves the operator a real choice of base.
    pub(crate) fn emergency_is_ambiguous(&self) -> bool {
        self.emergency.len() > 1
    }


    /// The integration base a work branch belongs to.
    ///
    /// Three sources, asked in this order, and the ORDER is the whole point:
    ///
    /// 1. `{base}_{slug}` — a unit still in the pre-kind shape carries its base
    ///    in the name: the LONGEST branch `B` with the name starting `"{B}_"`,
    ///    asked of the declared bases and then of the branches the repository
    ///    really has ([`legacy_base_of`](Self::legacy_base_of)), so a project
    ///    carrying both `dev` and `dev_release` reads `dev_release_x` as the
    ///    latter's — declared or not.
    /// 2. the unit's OWN RECORD ([`recorded_base_of`](Self::recorded_base_of)) —
    ///    what the cut wrote down. It wins over the derivation because it is a
    ///    MEASUREMENT of where the branch really came from, while the derivation
    ///    is an inference from the kind; where the two can differ, only the
    ///    operator ever knew the answer.
    /// 3. the flow's SINGLE declared base, when it declares exactly one — the
    ///    only case where nothing is being guessed, because there was never a
    ///    choice to make.
    ///
    /// There is no fourth source any more. The kind used to imply a base
    /// (`base_of_kind`), and that inference is gone with the coupling that
    /// produced it: the base is now the operator's answer to a question they
    /// were asked against a real list, so it is a MEASUREMENT to be read, never
    /// a value to be re-derived.
    ///
    /// And when none of them answers, it says so — [`UnitBase::Ambiguous`].
    /// It used to answer the outermost candidate, which silently replaced the
    /// operator's pick on every read after the cut: the pull-request target and
    /// the merged-ancestry check included.
    pub(crate) fn base_of(&self, branch: &str) -> UnitBase {
        let name = branch_of_name(branch);
        if self.is_declared_base(name) {
            return UnitBase::NotAUnit;
        }
        let Some(kind) = WorkKind::of_branch(name) else {
            return match self.legacy_base_of(name) {
                Some(base) => UnitBase::Known(base),
                None => UnitBase::NotAUnit,
            };
        };
        if let Some(recorded) = self.recorded_base_of(name) {
            return UnitBase::Known(recorded);
        }
        let _ = kind; // the kind no longer says anything about the base
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
    /// Reading names alone therefore made a project's own release line answer
    /// "somebody's unit" — and the two doors that ask this question acted on it:
    /// `git delete` offered to REMOVE the release line, and `pr list` refused to
    /// run from it.
    ///
    /// This is the one reading of the declared set that is not a permission.
    /// It refuses the operator nothing — a base is still cut from freely, and a
    /// base nobody declared is still a perfectly good base. All it does is stop
    /// the harness from mistaking a branch the project ITSELF called a base for
    /// a disposable unit, which is the only direction of this question where
    /// being wrong destroys something.
    fn is_declared_base(&self, name: &str) -> bool {
        self.bases.iter().any(|b| b == name)
    }

    /// `true` when the operator had a real choice of base, so the cut must write
    /// their answer down or it is lost.
    ///
    /// **TWO sources of a choice, and either one is enough.**
    ///
    /// 1. The DECLARED set cannot land on one answer (`bases().len() != 1`) —
    ///    so [`base_of`](Self::base_of)'s derivation would come back
    ///    [`Ambiguous`](UnitBase::Ambiguous) and the answer has nowhere else to
    ///    live. Unchanged, and it must stay: this is the leg `settle`'s
    ///    `ambiguous-base` hint depends on when it sends the operator to
    ///    `work-unit-open --base …` to write exactly this down.
    /// 2. The CATALOGUE really offered more than one branch
    ///    ([`catalogue_offers_a_choice`]) — which the first leg cannot see. The
    ///    picker offers every branch `origin` has, declared or not, so in a
    ///    project declaring ONE base and carrying five branches the operator
    ///    chose from five while the count reported "nothing to choose": the
    ///    answer was dropped before it was ever written, and every later read
    ///    re-derived the single declared base instead. That is this spec's
    ///    defect one step earlier than the filter it removed — same closed list,
    ///    same discarded pick.
    ///
    /// So this only ever records MORE than the count alone did. A rootless model
    /// has no catalogue to ask and keeps leg 1 by itself; an unmeasurable one
    /// (no git, no remote, an unfetched clone) reads the same way, because a
    /// probe that said nothing must not be the thing that decides an answer was
    /// worth keeping.
    ///
    /// The one spelling of that condition, shared by the emitter that records
    /// the pick in the pending marker and by the record the cut leaves in the
    /// unit's own directory ([`record_cut_base`](Self::record_cut_base)), which
    /// is what [`base_of`](Self::base_of) reads back — consumers that must agree
    /// about when an answer exists to be remembered.
    pub(crate) fn base_must_be_recorded(&self, branch: &str) -> bool {
        if WorkKind::of_branch(branch).is_none() {
            return false;
        }
        if self.bases().len() != 1 {
            return true;
        }
        self.project.as_deref().and_then(catalogue_offers_a_choice).unwrap_or(false)
    }

    /// The base recorded FOR THIS UNIT, `None` when nothing was recorded, when
    /// this model has no project to consult, or when what was recorded is a
    /// branch the remote no longer has.
    ///
    /// TWO places, one answer, in the order the answer travels: the sidecar
    /// (`meta.json#base`, its durable home once the draft has folded it) and
    /// then the cut's own record ([`CUT_BASE_FILE`], where the cut writes it
    /// because at cut time the draft does not exist yet). A unit that was cut
    /// and never drafted still answers, and one that was drafted answers from
    /// the single file every other machine-parseable fact about it lives in.
    ///
    /// The check on the way out matters, and WHAT it checks matters more: the
    /// repository may have moved on since the cut, and answering with a branch
    /// that is gone is worse than falling back to the derivation. So the record
    /// is measured against the branches the remote really has
    /// ([`base_still_on_remote`]) — never against `git.flow`, which would drop
    /// the answer of every operator who picked a base the install never
    /// declared. The same posture, through the same helper,
    /// [`crate::commands::event::work_branch::recorded_or_derived_base`] takes
    /// with the marker.
    fn recorded_base_of(&self, name: &str) -> Option<String> {
        let project = self.project.as_deref()?;
        let slug = self.slug_of(name)?;
        let dir = unit_dir(project, &slug)?;
        let recorded = mustard_core::read_meta(&dir.join("meta.json"))
            .and_then(|meta| meta.base)
            .or_else(|| cut_base_in(&dir))?;
        let recorded = recorded.trim().to_string();
        base_still_on_remote(project, &recorded).then_some(recorded)
    }

    /// Write down the base a unit was ACTUALLY cut from, in the cut's own record
    /// ([`CUT_BASE_FILE`]) inside the unit's directory.
    ///
    /// NOT `meta.json`, and that is the whole point: the cut runs before the
    /// draft, and a `meta.json` sitting in the directory is precisely what makes
    /// `spec-draft` refuse the directory as already drafted — so recording the
    /// base there cut the unit and then denied it a spec. The file this writes is
    /// harness state the draft's guard tolerates by category, and the draft folds
    /// it into `meta.json#base` on its way past (see [`CUT_BASE_FILE`]).
    ///
    /// A no-op unless [`base_must_be_recorded`](Self::base_must_be_recorded):
    /// freezing a derivable answer would make the record the thing that goes
    /// stale when `git.flow` changes, and it would leave a file in the directory
    /// of every unit for a question the flow already answers. A no-op too once
    /// the answer is already on disk — the folded sidecar is not resurrected
    /// into a second copy by a later checkout of the same branch.
    ///
    /// Fail-open at every step (no project, no slug, an unwritable directory):
    /// this runs inside a HOOK that has already cut the branch, and a record
    /// that could not be written must never turn a successful cut into a blocked
    /// session.
    pub(crate) fn record_cut_base(&self, branch: &str, base: &str) {
        if !self.base_must_be_recorded(branch) {
            return;
        }
        let Some(project) = self.project.as_deref() else { return };
        let Some(slug) = self.slug_of(branch) else { return };
        let Some(dir) = unit_dir(project, &slug) else { return };
        let already = mustard_core::read_meta(&dir.join("meta.json"))
            .and_then(|meta| meta.base)
            .or_else(|| cut_base_in(&dir));
        if already.as_deref() == Some(base) {
            return; // already says exactly this — write nothing
        }
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let _ = mustard_core::io::fs::write_atomic(
            dir.join(CUT_BASE_FILE),
            format!("{base}\n").as_bytes(),
        );
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
    /// — matches `hml` on the catalogue and read as "the unit `prod`":
    /// `git delete` offered to remove the integration line, and `pr list`
    /// refused to run from it. The two facts together are what tell them apart.
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
    /// answer it either: `mustard init` no longer writes `git.flow`, so
    /// [`GitConfig::preselected_bases`] degrades to the hardcoded
    /// `{main, master}` — that guard protected two literals and nothing else,
    /// and `git delete` was measured removing a real release line from the
    /// remote in a project shaped exactly the way the installer writes them.
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
        // `git delete` destroyed the worktree the caller was standing in
        // instead of refusing, and `git settle` refused the very position its
        // own hint prescribes.
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

    /// AC-3 — the vocabulary is open. A token the project never suggested makes
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

    #[test]
    fn the_emergency_candidates_end_at_the_outermost_base() {
        let two = BaseFlow::of(&two_tier());
        assert_eq!(two.work_base(), "dev");
        assert_eq!(two.emergency_bases(), ["main"]);
        assert!(!two.emergency_is_ambiguous(), "one candidate is nothing to ask about");

        let three = BaseFlow::of(&three_tier());
        assert_eq!(three.emergency_bases(), ["qas", "main"], "outermost last");
        assert!(three.emergency_is_ambiguous(), "several candidates — the operator picks");

        // A single-base project declares no emergency route at all.
        let mut single = GitConfig::default();
        single.flow.insert("*".to_string(), "main".to_string());
        let one = BaseFlow::of(&single);
        assert!(one.emergency_bases().is_empty());
        assert_eq!(one.bases(), ["main"], "a single-base project has one answer and no choice");
    }

    #[test]
    fn a_cyclic_flow_still_terminates() {
        let mut git = GitConfig::default();
        git.flow.insert("*".to_string(), "dev".to_string());
        git.flow.insert("dev".to_string(), "main".to_string());
        git.flow.insert("main".to_string(), "dev".to_string());
        let flow = BaseFlow::of(&git);
        assert_eq!(flow.emergency_bases(), ["main"]);
    }

    #[test]
    fn a_base_off_the_promotion_path_never_displaces_the_outermost_one() {
        // `spike` is declared (it is a flow VALUE) but the chain from `dev`
        // never reaches it, so it is a candidate that is not the outermost base.
        let mut git = three_tier();
        git.flow.insert("spike".to_string(), "spike".to_string());
        let flow = BaseFlow::of(&git);
        assert_eq!(flow.emergency_bases().last().map(String::as_str), Some("main"));
        assert!(flow.emergency_bases().contains(&"spike".to_string()));
    }

    #[test]
    fn both_branch_shapes_resolve_to_one_base_and_one_slug() {
        let flow = BaseFlow::of(&two_tier());

        // The base no longer comes from the KIND — it is the operator's answer,
        // recorded at the cut. With two declared bases and nothing recorded,
        // every kind reads the same way, and that sameness IS the change: the
        // prefix stopped carrying a base.
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

    /// AC-5, second half — the operator's chosen base SURVIVES the cut.
    ///
    /// With three bases a hotfix has two candidates and the pick is the
    /// operator's. The branch name records what the unit IS, so it cannot carry
    /// that pick, and the pending marker that did carry it is consumed the
    /// moment the branch is cut. Without a durable record every later read
    /// answered the OUTERMOST candidate — which is not the base the unit came
    /// from, and not the base its pull request should target.
    #[test]
    fn a_recorded_base_outlives_the_cut_and_an_unrecorded_one_is_never_guessed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path();
        let git = three_tier();

        // Nothing recorded yet: the flow ALONE cannot choose, and naming one
        // here is exactly the silent replacement of the operator's answer. It
        // must say it does not know.
        //
        // The candidate list widened from the emergency bases to ALL of them,
        // and that follows from the same change: while the kind implied a base,
        // a `hotfix/` could only have come from an emergency one, so only those
        // were candidates. With the prefix carrying nothing, every declared base
        // is equally possible.
        let flow = BaseFlow::of_at(&git, project);
        assert_eq!(
            flow.base_of("hotfix/my-unit"),
            UnitBase::Ambiguous(vec![
                "dev".to_string(),
                "main".to_string(),
                "qas".to_string(),
            ]),
            "several candidates and no record — the answer was never established",
        );
        assert!(flow.base_of("hotfix/my-unit").is_unit(), "it is still a unit of this project");
        assert_eq!(flow.base_of("hotfix/my-unit").known(), None, "and it is not answered");
        assert_eq!(flow.base_of("hotfix/my-unit").candidates(), ["dev", "main", "qas"]);

        // The cut records the MIDDLE base — the operator's pick.
        flow.record_cut_base("hotfix/my-unit", "qas");
        let after = BaseFlow::of_at(&git, project);
        assert_eq!(
            after.base_of("hotfix/my-unit").known(),
            Some("qas"),
            "every later read answers the base the unit was really cut from",
        );

        // The record is HARNESS STATE inside the unit's directory — never the
        // sidecar, which at this moment does not exist and whose presence is
        // exactly what makes `spec-draft` refuse the directory as already
        // drafted. The cut must leave the draft a directory it can still write.
        let unit = project.join(".claude").join("spec").join("my-unit");
        assert_eq!(
            std::fs::read_to_string(unit.join(CUT_BASE_FILE)).expect("the cut's record").trim(),
            "qas",
        );
        assert!(
            !unit.join("meta.json").exists(),
            "the cut drafts nothing — a meta.json here is a DRAFT, and writing one \
             would leave the unit cut and spec-less",
        );

        // The draft folds it into the sidecar; from then on the sidecar answers
        // and the cut's record is spent.
        let sidecar = unit.join("meta.json");
        let folded = mustard_core::domain::meta::Meta {
            base: cut_base_in(&unit),
            ..Default::default()
        };
        mustard_core::write_meta(&sidecar, &folded).expect("fold");
        clear_cut_base_in(&unit);
        assert_eq!(
            BaseFlow::of_at(&git, project).base_of("hotfix/my-unit").known(),
            Some("qas"),
            "the answer survives the fold — one home at a time, never none",
        );

        // A flow that no longer declares the recorded base OBEYS it anyway.
        // The configuration is not the test: `qas` is where this unit really
        // came from, and a `mustard.json` edited afterwards cannot move a cut
        // that already happened. What DOES retire a record is the branch itself
        // disappearing — see `a_vanished_recorded_base_is_ignored`.
        let moved_on = BaseFlow::of_at(&two_tier(), project);
        assert_eq!(
            moved_on.base_of("hotfix/my-unit").known(),
            Some("qas"),
            "the operator's answer outlives a flow that never mentioned it",
        );

        // The condition GENERALISED with the change. It used to mean "a hotfix
        // where the emergency bases are several", because that was the one case
        // the kind could not resolve. Now the kind resolves NOTHING, so every
        // unit of a multi-base project records its base — the feature included.
        let two = BaseFlow::of_at(&two_tier(), project);
        assert!(two.base_must_be_recorded("hotfix/other"), "two bases — the pick must survive");
        assert!(two.base_must_be_recorded("feature/other"), "and a feature makes the same pick");

        // …and where NOTHING can be measured — this `project` is a bare
        // directory, not a repository — the declared count is the last resort,
        // so a single declared base still reads as "nothing was chosen". That
        // is the FALLBACK and not the rule: given a real catalogue the question
        // is asked of the branches that exist, which is what lets a
        // single-base project keep the operator's pick
        // (`the_recorded_base_survives_to_the_cut_in_any_project`).
        let mut single = GitConfig::default();
        single.flow.insert("*".to_string(), "main".to_string());
        let one = BaseFlow::of_at(&single, project);
        assert!(
            !one.base_must_be_recorded("feature/other"),
            "one declared base and no catalogue to ask — nothing to remember",
        );
        one.record_cut_base("feature/other", "main");
        assert!(
            !project.join(".claude").join("spec").join("other").exists(),
            "a derivable base is never frozen into a record",
        );

        // A model with no project consults no record, so it can only say what
        // the flow leaves undisputed. With three bases declared that is nothing
        // — for EVERY kind, which is the same generalisation as above.
        let rootless = BaseFlow::of(&git);
        for name in ["hotfix/my-unit", "feature/my-unit"] {
            assert!(
                matches!(rootless.base_of(name), UnitBase::Ambiguous(_)),
                "no record and several bases: {name} has no established base",
            );
        }

        // …and the single-base project is where it CAN answer without a record,
        // because there is only one thing the answer could ever have been.
        let mut single = GitConfig::default();
        single.flow.insert("*".to_string(), "main".to_string());
        assert_eq!(BaseFlow::of(&single).base_of("feature/my-unit").known(), Some("main"));
    }

    /// A repository whose REMOTE really has `branches` — the refs the existence
    /// probe reads. `false` when git is unusable here, so a caller can skip
    /// instead of asserting against a probe that measured nothing.
    fn seed_remote_refs(root: &Path, branches: &[&str]) -> bool {
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(root)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        };
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

    /// A recorded base that VANISHED from the remote is ignored, and the
    /// derivation takes over — while one the CONFIGURATION never declared is
    /// obeyed.
    ///
    /// The two halves are one subject: this is the protection the old
    /// declared-list filter was reaching for, pointed at something the question
    /// can actually be asked of. `git.flow` cannot say whether a branch still
    /// exists; the refs can. So the record is dropped in the one case that
    /// justifies dropping it — the repository answered, and did not name it.
    #[test]
    fn a_vanished_recorded_base_is_ignored() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = dir.path();
        if !seed_remote_refs(project, &["dev", "qas", "main", "release/2026-Q3"]) {
            return; // no usable git here — nothing here is measurable at all
        }
        let git = three_tier();
        let flow = BaseFlow::of_at(&git, project);

        // A pick no `git.flow` ever declared, which the remote really has.
        flow.record_cut_base("hotfix/na-linha-de-release", "release/2026-Q3");
        assert_eq!(
            BaseFlow::of_at(&git, project).base_of("hotfix/na-linha-de-release").known(),
            Some("release/2026-Q3"),
            "an existing branch is the operator's answer, declared or not",
        );

        // A pick that is GONE — the release line was merged and deleted.
        flow.record_cut_base("hotfix/na-linha-extinta", "release/2025-Q1");
        let vanished = BaseFlow::of_at(&git, project).base_of("hotfix/na-linha-extinta");
        assert!(vanished.is_unit(), "it is still this project's unit");
        assert_eq!(vanished.known(), None, "a base that no longer exists cannot be cut from");
        assert_eq!(
            vanished.candidates(),
            ["dev", "main", "qas"],
            "…and the derivation takes over, naming what it could not choose between",
        );
    }

    /// An integration line whose NAME carries an underscore is not somebody's
    /// unit, and the catalogue leg of the legacy reader must not turn it into
    /// one.
    ///
    /// `hml_prod` in a project that declares no flow — every project the current
    /// installer touches — matches `hml` on the catalogue, so reading the name
    /// alone answered "the unit `prod`": `git delete` would have offered to
    /// remove the integration line and `pr list` would have refused to run from
    /// it, which is exactly the damage `is_declared_base` was added to prevent,
    /// arriving through the other door. Both kinds of real unit still resolve —
    /// the one this harness already cut and drafted, and the one that does not
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

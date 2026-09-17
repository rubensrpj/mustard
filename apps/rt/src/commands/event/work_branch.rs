//! The work unit's branch: its NAME, and the CUT that creates it.
//!
//! Two halves, one subject. The naming half is pure: given a spec or intent
//! plus what the unit IS, compute the `{kind}/{slug}` work-branch name the unit
//! lives on (the only I/O is reading `mustard.json` for the slug locale). The
//! cutting half runs git: refresh the integration bases from `origin`, then
//! check the branch out, creating it off its base.
//!
//! The base is NOT in the name. It used to be — a unit was `dev_my-thing` and
//! every consumer recovered the base by parsing the prefix back — and the
//! prefix now records what the unit IS instead, because that is what an
//! operator reading a branch list needs. The base follows from the kind through
//! the declared flow ([`crate::shared::work_kind::BaseFlow`]), which is also
//! the ONE reader of the old shape, so units in flight are never orphaned.
//!
//! Both halves live here because two callers must agree about them and a
//! second spelling is how they stop agreeing:
//! [`crate::commands::spec::spec_draft`] (the draft, which cuts the branch so
//! the spec is written INSIDE the unit rather than on the base) and
//! [`super::emit_pipeline`] (which pre-computes the name into the pending
//! marker). The base set itself is never re-derived here — it comes from
//! [`mustard_core::domain::config::GitConfig`], the single owner.

use std::path::Path;

use mustard_core::platform::git;
use mustard_core::platform::i18n::translate;

use crate::shared::work_kind::{BaseFlow, UnitBase, WorkKind, CUT_BASE_FILE};

/// Resolve the base a unit is cut from.
///
/// The base is no longer a CONSEQUENCE of the kind. It used to be: an ordinary
/// unit came from the work base, an emergency from one that was not, and a
/// `hotfix` cut from the work base was refused as a contradiction. That whole
/// mechanism existed because the base could not be asked for — the candidate
/// set was a two-entry list in `mustard.json` and the kind was the only signal
/// available to choose between its members.
///
/// The base is now ASKED, against the branches git really has, so:
///
/// - an explicit base is the operator's answer and is taken;
/// - it is validated against the real catalogue, not against a declaration, so
///   a typo is still caught while `release/2026-Q3` is not;
/// - an empty catalogue is UNMEASURED (no git, offline, no remote) and accepts
///   whatever was asked for — refusing on an unmeasured fact is how a gate
///   grounds a session it cannot reason about;
/// - no base at all falls back to the project's primary, which is a default
///   for the cursor and never a correction of an answer.
///
/// The hotfix-versus-work-base refusal is gone with the inference that made it
/// meaningful. `hotfix/` is a prefix on a name now; where the unit lands is the
/// base the operator picked, and picking is the whole feature.
pub(crate) fn resolve_kind_base(
    root: &Path,
    requested: Option<&str>,
    config: &mustard_core::ProjectConfig,
) -> Result<String, String> {
    let Some(requested) = requested.map(str::trim).filter(|b| !b.is_empty()) else {
        // **The default must be a branch that EXISTS, not a literal.**
        // The declared primary used to floor to a hardcoded name when no
        // `git.flow` was written — the shape the installer produces today — so
        // with no `--base` this recorded a branch the repository does not have.
        // It passed through unchallenged; once the reader started checking
        // existence, the invented name was correctly dropped and the write gate
        // DENIED the first edit of every such project.
        //
        // Order: what the project STATES, then what the remote states, then the
        // branch the operator is standing on — each a name that really exists.
        // There is no fourth rung: when none of the three can answer, the cut
        // says so instead of naming a branch nobody measured.
        return config
            .git
            .primary_base()
            .or_else(|| mustard_core::default_branch(root))
            .or_else(|| mustard_core::current_branch(root))
            .ok_or_else(|| {
                translate("base.unmeasured", config.language().text_or_default()).to_string()
            });
    };
    let catalog = mustard_core::branch_catalog(root, &config.git, false);
    if catalog.is_empty() || catalog.iter().any(|b| b.name == requested) {
        return Ok(requested.to_string());
    }
    let mut known: Vec<&str> = catalog.iter().map(|b| b.name.as_str()).take(12).collect();
    known.sort_unstable();
    Err(format!(
        "a branch '{requested}' não existe no remoto deste repositório. \
         Branches disponíveis (mais recentes primeiro): {}.",
        known.join(", ")
    ))
}

/// A short, ref-safe fallback token from the session id. `unknown`/empty →
/// `work` so the branch always has a non-empty tail.
fn short_sid(sid: &str) -> String {
    let s = sid.trim();
    if s.is_empty() || s == "unknown" {
        return "work".to_string();
    }
    s.chars().take(8).collect()
}

/// Sanitise `{kind}/{slug}` into a valid git ref: keep `[A-Za-z0-9-_./]`,
/// map everything else to `-`, collapse `..` runs (git forbids them), and trim
/// leading `-`/`.`/`/` and trailing `/`/`.`. Never empty — floors to `work`.
///
/// Idempotent, and that is what makes it usable as a COMPARISON normaliser and
/// not only as a builder: a name a branch already carries is a fixed point, so
/// putting both sides of a slug equality through it lets a raw slug meet the ref
/// it was spelled as ([`crate::commands::pipeline::resume_bootstrap`]'s
/// `inside_own_work_branch`, which asks whether the checkout IS a spec's own
/// branch). Comparing a ref's slug against an unsanitised one answers `false`
/// for every slug that needed sanitising at all.
pub(crate) fn sanitize_git_ref(raw: &str) -> String {
    let mut out: String = raw
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '/' => ch,
            _ => '-',
        })
        .collect();
    while out.contains("..") {
        out = out.replace("..", "-");
    }
    let trimmed = out
        .trim_start_matches(['-', '.', '/'])
        .trim_end_matches(['/', '.']);
    if trimmed.is_empty() {
        "work".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Compute the auto-branch name for a `pipeline.kind` work-type signal:
/// `{kind}/{slug}`, sanitised to a valid git ref. The prefix records WHAT the
/// unit is — a feature, a fix, an emergency — which is what an operator reading
/// a branch list needs; the base is no longer parsed back out of the name, and
/// it is not derived from the kind either — it is the operator's own answer,
/// taken against the real catalogue and recorded with the unit
/// ([`BaseFlow::base_of`]). Slug precedence:
/// 1. `--spec` when present (already a slug — at the base gate that is the
///    name `emit_pipeline` just MINTED, so this leg carries the canonical one);
/// 2. else `--intent` through the canonical derivation
///    ([`crate::commands::spec::spec_slug::canonical_for_project`]) — the SAME
///    one `spec-draft` names the spec directory with, so the branch half and
///    the directory cannot be two different spellings of one unit;
/// 3. else a date-based fallback (`YYYY-MM-DD` from the event `ts`) suffixed
///    with a short session id for uniqueness.
///    Never fails — every branch degrades to a valid ref.
pub(crate) fn compute_work_branch(
    kind: WorkKind,
    spec: &str,
    intent: Option<&str>,
    sid: &str,
    ts: &str,
    project: &str,
) -> String {
    let slug = if !spec.trim().is_empty() {
        spec.trim().to_string()
    } else if let Some(intent) = intent.map(str::trim).filter(|s| !s.is_empty()) {
        crate::commands::spec::spec_slug::canonical_for_project(intent, Path::new(project))
    } else {
        // Date-based fallback from the shared event timestamp, plus a short
        // session id so two spec-less/intent-less runs on the same day differ.
        let date = ts.split('T').next().unwrap_or("").trim();
        if date.is_empty() {
            short_sid(sid)
        } else {
            format!("{date}-{}", short_sid(sid))
        }
    };
    sanitize_git_ref(&kind.branch_name(&slug))
}

// ---------------------------------------------------------------------------
// The cut — git primitives shared by the hook gate and the draft
// ---------------------------------------------------------------------------

/// `true` when a local branch `refs/heads/<branch>` exists.
pub(crate) fn local_branch_exists(root: &Path, branch: &str) -> bool {
    git::run(root, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")]).ok
}

/// `true` when the remote-tracking ref `refs/remotes/origin/<branch>` exists.
///
/// The clone's ONLY record of a branch nobody checked out locally — which is
/// every branch of a fresh clone but the default one, and therefore the shape
/// the pick this module carries lands in most often.
pub(crate) fn remote_branch_exists(root: &Path, branch: &str) -> bool {
    git::run(root, &["rev-parse", "--verify", "--quiet", &format!("refs/remotes/origin/{branch}")])
        .ok
}

/// Run one git subcommand in `root`, mapping a non-zero exit (or a program that
/// could not be spawned) to `Err(<stderr|io error>)`. Never panics.
///
/// The one reading this file adds on top of the executor: a step that only has
/// to have HAPPENED, and whose refusal text the operator must see when it did
/// not. A refusal git wrote with nothing on stderr still has to say something,
/// or the failure reaches a report as an empty line.
fn run_git(root: &Path, args: &[&str]) -> Result<(), String> {
    let run = git::run(root, args);
    if run.ok {
        return Ok(());
    }
    let msg = run.stderr.trim();
    Err(if msg.is_empty() { "git refused without saying why".to_string() } else { msg.to_string() })
}

/// Check out `target`, creating it off `base` when it does not yet exist.
/// Carries the working-tree changes along (a plain `checkout`, no stash).
/// Returns the git error string on failure.
///
/// **Where the cut STARTS is a cascade, and each step exists for a shape the
/// one before it cannot serve:**
///
/// 1. the LOCAL head `refs/heads/{base}`, when the clone has one. It is first
///    because it is the base the operator is actually standing in: a base
///    carrying commits that were never pushed is still that operator's base,
///    and starting from the remote instead would silently drop them out of the
///    unit's history. Staleness is not the reason to skip it — the settlement
///    the caller just obeyed handed this very base to [`fast_forward_base`],
///    which fast-forwards it toward `origin` where git would allow it, keeps it
///    exactly where those unpushed commits are, and REFUSES the cut where it
///    still trails the remote.
/// 2. the REMOTE-TRACKING ref `refs/remotes/origin/{base}` — the clone shape,
///    and the reason this step had to exist. The base is now the operator's
///    pick out of the REAL catalogue ([`resolve_kind_base`]), which offers
///    every branch `origin` has; a fresh clone materialises a local head for
///    exactly ONE of them, so any other pick has no `refs/heads/` entry until
///    the refresh above creates one — and the refresh is skipped whole whenever
///    the machine is offline or the repository has no remote to answer.
///    Cutting from HEAD there would have recorded the operator's answer
///    ([`crate::shared::work_kind::BaseFlow::record_cut_base`]) over a branch
///    the unit never came from — every later read, the pull-request target and
///    `git settle`'s containment check included, asserting a base that was
///    never the cut point. The sibling door reads the same ref to answer the
///    same question ([`crate::commands::work_unit_open`]); it reaches for it
///    FIRST because a worktree is cut fresh with nothing local to preserve,
///    while this door cuts in place and carries the operator's tree along.
/// 3. the current HEAD, when NEITHER ref carries the base — an unmeasurable
///    repository, not a choice. A cut has to come from somewhere.
pub(crate) fn checkout_work_branch(
    root: &Path,
    target: &str,
    base: &str,
) -> Result<(), String> {
    if local_branch_exists(root, target) {
        return run_git(root, &["checkout", target]);
    }
    if local_branch_exists(root, base) {
        return run_git(root, &["checkout", "-b", target, base]);
    }
    if remote_branch_exists(root, base) {
        return run_git(root, &["checkout", "-b", target, &format!("origin/{base}")]);
    }
    // Neither ref carries the base — branch off the current HEAD.
    run_git(root, &["checkout", "-b", target])
}

/// `git fetch origin`, so the remote-tracking refs describe what `origin` has
/// NOW. `false` when the fetch failed — offline, no remote — and nothing about
/// the remote could be measured; the caller then cuts from the local base as it
/// always did, since refusing there would ground every offline session on a
/// fact nobody measured.
///
/// The fetch changes which branches `origin` is known to have, and that answer
/// is memoised per process. Leaving the stale picture in place means a branch
/// that only MATERIALISED here reads as absent for the rest of this dispatch —
/// and the reader that consults it then drops the operator's recorded base for
/// a branch that does exist.
///
/// Called from ONE place — [`crate::commands::event::census_settlement::settle`]
/// — never from a door: the refresh is a step of the settlement, in the order
/// that function's body states.
pub(crate) fn fetch_origin(root: &Path) -> bool {
    if run_git(root, &["fetch", "origin"]).is_err() {
        return false;
    }
    crate::shared::work_kind::forget_remote_names(Path::new(root));
    true
}

/// `true` when `ancestor` is reachable from `descendant` — `git merge-base
/// --is-ancestor`. `false` on any failure, including a ref that does not
/// exist: a relation git could not establish is not a fact.
fn is_ancestor(root: &Path, ancestor: &str, descendant: &str) -> bool {
    run_git(root, &["merge-base", "--is-ancestor", ancestor, descendant]).is_ok()
}

/// Which of `candidates` the advance of the checked-out `base` to
/// `origin/{base}` would OVERWRITE — the paths a `merge --ff-only` refuses on
/// when they are dirty. Measured with `git diff --name-only HEAD origin/{base}`
/// and intersected with what the caller hands in; never a whole-tree probe.
///
/// Empty when the advance is not a fast-forward at all (HEAD is not an
/// ancestor of `origin/{base}`): there the merge refuses on its own for a
/// reason no set-aside could fix, so nothing must be set aside ahead of a
/// refusal — a refused move leaves the tree as it found it.
///
/// Read only after [`fetch_origin`] answered `true`: the remote-tracking ref it
/// compares against is the one that fetch just wrote.
pub(crate) fn paths_the_advance_overwrites(
    root: &Path,
    base: &str,
    candidates: &[String],
) -> Vec<String> {
    let remote = format!("origin/{base}");
    if candidates.is_empty() || !is_ancestor(root, "HEAD", &remote) {
        return Vec::new();
    }
    let Some(changed) = crate::commands::git_settle::git_out(
        Path::new(root),
        &["diff", "--name-only", "HEAD", &remote],
    ) else {
        return Vec::new();
    };
    let changed: std::collections::BTreeSet<&str> = changed.lines().map(str::trim).collect();
    candidates.iter().filter(|p| changed.contains(p.as_str())).cloned().collect()
}

/// The TOPLEVEL of the repository that contains `root` — `git rev-parse
/// --show-toplevel` — or `None` when git could not answer (not a repository,
/// no binary).
///
/// Every path a settlement handles is read from `git status --porcelain` and
/// `git diff --name-only`, which answer RELATIVE TO THE TOPLEVEL — while every
/// pathspec it hands back (`stash push -- p`, `status -- p`) is resolved
/// against the process CWD. Run from anywhere but the toplevel the two
/// disagree in silence: the pathspec matches nothing, the set-aside is a no-op,
/// and the advance refuses on the very paths it was meant to clear. So the
/// settlement resolves its root HERE, once, whatever a door
/// passed it — a door cannot get this wrong because a door no longer chooses.
pub(crate) fn toplevel_of(root: &Path) -> Option<std::path::PathBuf> {
    let top = git::run(root, &["rev-parse", "--show-toplevel"]).out()?;
    (!top.is_empty()).then(|| std::path::PathBuf::from(top))
}

/// The census paths a settlement put out of the advance's way — and how to
/// put them back, or account for every one of them once the base advanced.
///
/// SET ASIDE, never discarded. The previous shape of this step restored each
/// path to the index and removed the untracked ones, which had three costs at
/// once: a STAGED census change was invisible to it (the fast-forward stayed
/// blocked, and the remedy the refusal prescribed failed identically); a base
/// that then failed to advance had already lost its untracked census files
/// before refusing; and a `source: scan` mold — LLM-authored by the
/// enrichment pass, NOT regenerated by the deterministic mine — was deleted
/// outright whenever `origin`'s advance touched it too.
///
/// A `git stash push --include-untracked -- <paths>` holds all three states
/// (staged, unstaged, untracked) in one entry that git keeps until this value
/// says what becomes of it: [`Self::put_back`] on a refused advance restores
/// the tree exactly as it was found; [`Self::settle_after_advance`] on a
/// successful one accounts for each path by what it IS — miner output yields
/// to `origin`'s (the re-mine wins), an authored mold is kept BESIDE `origin`'s
/// — and only then drops the entry.
#[derive(Debug)]
pub(crate) struct CensusSetAside {
    /// The stash entry's commit, `None` when nothing was stashed (no paths, or
    /// git declined — the fast-forward then refuses on its own and there is
    /// nothing to put back).
    stash: Option<String>,
    /// Every path that was asked to be set aside, toplevel-relative.
    paths: Vec<String>,
    /// The authored molds among `paths`, with their bytes as found — read
    /// BEFORE the stash so a mold `origin` also rewrote can be kept beside
    /// `origin`'s without a second visit to the stash's trees.
    authored: Vec<(String, Vec<u8>)>,
}

/// What [`CensusSetAside::settle_after_advance`] did with each path.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct SetAsideReport {
    /// Miner output `origin`'s advance rewrote: `origin`'s version stands, the
    /// local one was regenerable and is gone with the stash entry.
    pub(crate) origin_kept: Vec<String>,
    /// Authored molds `origin`'s advance rewrote: `(path, sibling)` — `origin`'s
    /// text is at `path`, the text found here is at `sibling`, and nobody's
    /// text was deleted.
    pub(crate) kept_both: Vec<(String, String)>,
    /// Paths still held in the stash because the sibling could not be written.
    pub(crate) left_in_stash: Vec<String>,
}

/// `true` for a census path whose content is AUTHORED — a `.claude/skills/
/// <mold>/SKILL.md` — as opposed to miner output. The caller already
/// classified it [`DirtyPathKind::Census`] (which for a mold means `source:
/// scan`, read by [`census_skill_kind`]), so the shape of the path is the whole
/// remaining question: the miner writes only [`CENSUS_FILES`].
pub(crate) fn census_path_is_authored(path: &str) -> bool {
    let normalised = path.replace('\\', "/");
    let mut segments = normalised.split('/').filter(|s| !s.is_empty());
    if !segments.any(|s| s == ".claude") {
        return false;
    }
    let rest: Vec<&str> = segments.collect();
    rest.first() == Some(&CENSUS_SKILLS_DIR) && rest.last() == Some(&CENSUS_SKILL_FILE)
}

/// Where an authored mold found here is kept when `origin` rewrote it too:
/// `SKILL.md` → `SKILL.set-aside.md`, beside the original. A path that is not
/// `.md` gets `.set-aside` appended. The sibling is never a `SKILL.md`, so it
/// reads as the operator's work at the next measurement — which is what it is:
/// two authored texts, theirs to reconcile.
pub(crate) fn set_aside_sibling(path: &str) -> String {
    match path.strip_suffix(".md") {
        Some(stem) => format!("{stem}.set-aside.md"),
        None => format!("{path}.set-aside"),
    }
}

/// The stash entry currently at `refs/stash`, `None` when there is none.
fn stash_head(root: &Path) -> Option<String> {
    let sha = git::run(root, &["rev-parse", "--verify", "--quiet", "refs/stash"]).out()?;
    (!sha.is_empty()).then_some(sha)
}

impl CensusSetAside {
    /// Nothing set aside — the value every settlement starts from.
    pub(crate) fn none() -> Self {
        Self {
            stash: None,
            paths: Vec::new(),
            authored: Vec::new(),
        }
    }

    /// Put `paths` — census paths only, toplevel-relative, the ones
    /// [`paths_the_advance_overwrites`] named — out of the advance's way.
    ///
    /// Best-effort: a stash git declined leaves every path where it was, the
    /// fast-forward then refuses on it, and the refusal says so in git's words.
    /// The entry is identified by its commit, never by position, so a stash the
    /// operator already had is never mistaken for this one.
    pub(crate) fn push(root: &Path, base: &str, paths: &[String]) -> Self {
        if paths.is_empty() {
            return Self::none();
        }
        let authored: Vec<(String, Vec<u8>)> = paths
            .iter()
            .filter(|p| census_path_is_authored(p))
            .filter_map(|p| std::fs::read(root.join(p)).ok().map(|bytes| (p.clone(), bytes)))
            .collect();
        let before = stash_head(root);
        let message = format!("mustard: census set aside so '{base}' can advance");
        let mut args: Vec<&str> = vec![
            "stash",
            "push",
            "--include-untracked",
            "--quiet",
            "-m",
            &message,
            "--",
        ];
        args.extend(paths.iter().map(String::as_str));
        let pushed = run_git(root, &args).is_ok();
        let after = stash_head(root);
        // A push that saved nothing ("No local changes to save") exits 0 and
        // writes no entry: only a NEW head is ours.
        let stash = (pushed && after != before).then_some(after).flatten();
        Self {
            stash,
            paths: paths.to_vec(),
            authored,
        }
    }

    /// The paths this value set aside (empty when nothing was).
    pub(crate) fn paths(&self) -> &[String] {
        if self.stash.is_some() {
            &self.paths
        } else {
            &[]
        }
    }

    /// `true` when the stash entry is still the one at `refs/stash`.
    fn is_at_head(&self, root: &Path) -> bool {
        self.stash.is_some() && stash_head(root) == self.stash
    }

    /// The advance was REFUSED: restore every path — index state included —
    /// so the tree is exactly as the settlement found it. HEAD did not move,
    /// so the restore is a clean reverse of the push. `true` when the stash
    /// entry was consumed; `false` (and a line on stderr naming the entry)
    /// when git could not apply it, in which case nothing was lost: the entry
    /// stays in `git stash list`.
    pub(crate) fn put_back(&self, root: &Path) -> bool {
        let Some(sha) = &self.stash else {
            return true;
        };
        if !self.is_at_head(root) {
            eprintln!(
                "base-gate: census set-aside {sha} is no longer the newest stash entry; left \
                 in `git stash list` for you to apply — {}",
                self.paths.join(", ")
            );
            return false;
        }
        let restored = run_git(root, &["stash", "pop", "--index", "--quiet"]).is_ok()
            || run_git(root, &["stash", "pop", "--quiet"]).is_ok();
        if !restored {
            eprintln!(
                "base-gate: census set-aside could not be put back and stays in `git stash \
                 list` ({sha}) — {}",
                self.paths.join(", ")
            );
        }
        restored
    }

    /// The advance SUCCEEDED and `origin`'s version of every path set aside is
    /// now in the tree. Account for each one:
    ///
    /// - miner output (the model, the dictionary, a subproject map, the
    ///   declined list): `origin`'s stands — the deterministic mine regenerates
    ///   it on the next scan;
    /// - an authored mold: `origin`'s stays at its path and the text found here
    ///   is written to [`set_aside_sibling`] beside it, so BOTH survive.
    ///
    /// Only when every authored text has a home is the stash entry dropped; a
    /// sibling that could not be written keeps the entry, and the report names
    /// it. The report is returned, not printed: the settlement says it in its
    /// own words.
    pub(crate) fn settle_after_advance(self, root: &Path) -> SetAsideReport {
        let mut report = SetAsideReport::default();
        let Some(sha) = &self.stash else {
            return report;
        };
        let mut all_homed = true;
        for (path, bytes) in &self.authored {
            let sibling = set_aside_sibling(path);
            if std::fs::write(root.join(&sibling), bytes).is_ok() {
                report.kept_both.push((path.clone(), sibling));
            } else {
                all_homed = false;
                report.left_in_stash.push(path.clone());
            }
        }
        for path in &self.paths {
            if !census_path_is_authored(path) {
                report.origin_kept.push(path.clone());
            }
        }
        if all_homed && self.is_at_head(root) {
            let _ = run_git(root, &["stash", "drop", "--quiet"]);
        } else if !all_homed {
            eprintln!(
                "base-gate: census set-aside kept in `git stash list` ({sha}): an authored mold \
                 could not be written beside origin's — {}",
                report.left_in_stash.join(", ")
            );
        }
        report
    }
}

/// What [`fast_forward_base`] established about ONE base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BaseRefresh {
    /// The base is at — or ahead of — `origin/{base}`, or there was nothing to
    /// measure it against: no `origin/{base}`, or no local head at all (the cut
    /// then starts from the remote-tracking ref, which is current by
    /// definition). The cut may start from it.
    Current,
    /// `origin/{base}` carries commits the local base does not, and the
    /// fast-forward could not be made. Carries git's own words, so the operator
    /// reads the reason git gave and not a paraphrase of it.
    Stale { base: String, error: String },
}

/// Fast-forward ONE base — the one a settlement is about — to `origin/{base}`,
/// and say whether it is current afterwards.
///
/// - when `base` is the checked-out branch (`current == Some(base)`) →
///   `git merge --ff-only origin/{base}` advances it in place;
/// - otherwise → `git fetch origin {base}:{base}`, a refspec fetch git refuses
///   to make non-ff, so it safely advances the local ref without a checkout.
///
/// **Scope is the point.** This used to walk every preselected base of the
/// declared flow and advance each one, and it ran from the explicit open too —
/// so `emit-pipeline` moved local `main` and `release/*` refs the operator never
/// asked about. Moving other refs was never this decision's job: a settlement
/// is about the base the unit is cut from or opened on, and it refreshes that
/// one.
///
/// **The result is read, never dropped.** A base carrying an unpushed commit of
/// its own is kept, never rewritten, and is `Current` — the operator's base is
/// the one they are standing in. A base that still TRAILS its remote after the
/// attempt is `Stale`, with git's words: a unit cut from it re-does merged work,
/// and the `git pull --ff-only origin {base}` the gate prescribes afterwards
/// cannot succeed on a branch that has meanwhile diverged. Dropping this result
/// is how a unit came to be cut from a stale base in silence.
///
/// Read only after [`fetch_origin`] answered `true`: offline there is no
/// evidence the base is behind, and the cut takes the local base as before.
pub(crate) fn fast_forward_base(
    root: &Path,
    current: Option<&str>,
    base: &str,
) -> BaseRefresh {
    if !remote_branch_exists(root, base) {
        return BaseRefresh::Current;
    }
    let remote = format!("origin/{base}");
    let attempt = if current == Some(base) {
        run_git(root, &["merge", "--ff-only", &remote])
    } else if local_branch_exists(root, base) {
        run_git(root, &["fetch", "origin", &format!("{base}:{base}")])
    } else {
        // No local head to advance: the cut starts from `origin/{base}` itself
        // ([`checkout_work_branch`], step 2), and that ref was just fetched.
        return BaseRefresh::Current;
    };
    match attempt {
        Ok(()) => BaseRefresh::Current,
        // Ahead of (or level with) the remote: nothing to advance, and an
        // unpushed commit of the operator's is kept, not a defect.
        Err(_) if is_ancestor(root, &remote, base) => BaseRefresh::Current,
        Err(error) => BaseRefresh::Stale {
            base: base.to_string(),
            error,
        },
    }
}

/// The integration base a work branch belongs to — `Err` when the answer was
/// never established, carrying the bases it could not be chosen between.
///
/// The reading is [`BaseFlow::base_of`] — the crate's one parser — asked of a
/// model rooted at `root`, so the answer the CUT recorded for this unit wins
/// over any derivation. A `feature/…` or `fix/…` unit integrates into the base
/// ordinary work is cut from, a `hotfix/…` into one that is not, and a unit
/// still in the `{base}_{slug}` shape keeps being resolved by its prefix, so
/// nothing in flight is orphaned.
///
/// One degradation, and one refusal:
///
/// - a name that is nobody's unit answers the base ordinary work is cut from — a
///   cut has to come from somewhere, and that is the only defensible answer for
///   a name nothing recognises;
/// - an emergency whose base nothing established ([`UnitBase::Ambiguous`]) is
///   `Err`. It used to answer the outermost candidate with a `WARN` on stderr,
///   which is not a warning at all for the caller that matters: both cut doors
///   reach this through [`recorded_or_derived_base`], and one of them is a
///   PreToolUse hook that exits 0 and whose stderr no operator ever sees. A
///   guess nobody can see is a fact, and this one aimed the unit — its pull
///   request target and its merged-ancestry check included — at a base the
///   operator never chose. Callers that must produce a name now say so
///   themselves, where they can be heard.
pub(crate) fn base_for(
    root: &Path,
    target: &str,
    config: &mustard_core::ProjectConfig,
) -> Result<String, Vec<String>> {
    let flow = BaseFlow::of_at(&config.git, root);
    match flow.base_of(target) {
        UnitBase::Known(base) => Ok(base),
        UnitBase::NotAUnit => flow
            .work_base()
            .map(str::to_string)
            .ok_or_else(Vec::new),
        UnitBase::Ambiguous(candidates) => Err(candidates),
    }
}

/// The SLUG half of a work branch — `feature/my-unit` → `my-unit`, and
/// `dev_my-unit` → `my-unit` for a unit still in flight. The inverse of
/// [`compute_work_branch`]'s `{kind}/{slug}` join.
///
/// This is the DURABLE record of a unit's name. The `pending-work-branch`
/// marker that carried the name from the gate is consumed and deleted by the
/// first checkout ([`cut_pending_work_branch`]), so after that moment the
/// branch itself is the only thing that still remembers what the unit is
/// called — which is what lets `spec-draft` consume the gate's name instead of
/// deriving a second one.
///
/// `None` when the name carries neither a kind prefix nor a declared `{base}_`
/// one: it is then not a work unit's branch at all, and inventing a slug out of
/// it would mint the very third name this module exists to prevent. The reading
/// is [`BaseFlow::slug_of`] — the same question the worktree engine and the
/// `/pr` door ask, never a second parser.
pub(crate) fn slug_of_work_branch(
    branch: &str,
    config: &mustard_core::ProjectConfig,
) -> Option<String> {
    BaseFlow::of(&config.git).slug_of(branch)
}

/// The base to cut `target` from: the one RECORDED with the pending marker when
/// the operator's answer could not be re-derived, else the one the kind implies
/// ([`base_for`]) — and `Err(candidates)` when NEITHER can answer.
///
/// The recorded leg exists for exactly one situation, and only that one: a
/// project declaring several emergency bases leaves a hotfix a real choice, and
/// the branch name — which now says what the unit IS, not where it came from —
/// cannot carry which one was picked. Deriving anyway would cut the emergency
/// from a base the operator did not choose, silently.
///
/// **The recorded base is checked, and the check asks whether it EXISTS.** The
/// protection is real and stays: the repository may have moved on since the
/// marker was written, and cutting from a branch that is gone is worse than
/// falling back to the derivation. What it must never go back to asking is
/// whether the recorded name appears in `git.flow`'s declared set — that test
/// refuses a base the operator picked out of the REAL catalogue
/// ([`resolve_kind_base`], which validates against git and not against a
/// declaration) for the sole reason that a file written at `mustard init` does
/// not list it, and `git.flow` refuses nothing any more
/// ([`mustard_core::domain::config::GitConfig::preselected_bases`]). Existence
/// is measurable; membership in a list nobody maintains measures nothing. The
/// reading is [`crate::shared::work_kind::base_still_on_remote`], shared with
/// the unit's durable record so both readings of a pick agree — and an
/// existence nobody could measure OBEYS the pick, because discarding a real
/// answer over a silent probe is the same mistake pointed at a different
/// source.
///
/// The `Err` is the third state, handed to the caller instead of resolved
/// behind its back: an emergency whose pick nothing carries has no base a
/// derivation can honestly supply, and the cut that consumes it
/// can refuse or warn in its own shape, where the operator reads it.
pub(crate) fn recorded_or_derived_base(
    root: &str,
    session: &str,
    target: &str,
    config: &mustard_core::ProjectConfig,
) -> Result<String, Vec<String>> {
    let recorded = crate::shared::context::pending_base_for(root, session)
        .filter(|b| crate::shared::work_kind::base_still_on_remote(Path::new(root), b));
    match recorded {
        Some(recorded) => Ok(recorded),
        None => base_for(Path::new(root), target, config),
    }
}

/// `true` when `branch` is one of this repository's INTEGRATION BASES.
///
/// Two sources, and neither of them is a name written here: what the project
/// declared (`git.flow`, plus the `git.protected` list), and what the remote
/// itself calls its default branch. The second matters because the installer
/// writes no flow — for such a project the declaration is empty, and without
/// the remote's own answer every branch, `origin/HEAD` included, would read as
/// somebody's work unit.
///
/// This used to be spelled as "is it protected?", and the two questions came
/// apart the moment protection began reading the declaration alone: a project
/// that declares nothing protects nothing, which is the honest answer to THAT
/// question and the wrong answer to this one. Standing on a base and standing
/// on a branch nobody may commit to are different facts.
pub(crate) fn on_integration_base(
    root: &Path,
    branch: &str,
    config: &mustard_core::ProjectConfig,
) -> bool {
    mustard_core::protected_branches(&config.git).contains(branch)
        || mustard_core::default_branch(root).as_deref() == Some(branch)
}

/// `true` when the tree HOLDS work that is not this unit's — its HEAD names a
/// branch that is neither `target` nor a bare integration base, i.e. ANOTHER
/// unit's branch.
///
/// The partition is the one both doors already use: an integration base is
/// nobody's work (the ordinary first unit cuts off it in place), and everything
/// else is somebody's. It is deliberately not narrowed to a `{base}_` shape — a
/// hand-made `feature/x` carries edits exactly the same way, and taking its
/// checkout costs exactly the same.
///
/// `None` (an unreadable HEAD, or a detached checkout, which
/// [`current_branch`] already answers as no branch) is NOT a measurement of a
/// position, so it never counts: an unmeasured HEAD keeps today's cut rather
/// than triggering a refusal the operator did not ask for.
pub(crate) fn holds_other_work(
    root: &Path,
    current: Option<&str>,
    target: &str,
    config: &mustard_core::ProjectConfig,
) -> bool {
    let Some(branch) = current else {
        return false;
    };
    branch != target && !on_integration_base(root, branch, config)
}

// ---------------------------------------------------------------------------
// The cut's OWN work probe
// ---------------------------------------------------------------------------

/// What the cut could ESTABLISH about the checkout it is about to take over.
///
/// Deliberately not a `Vec<String>`: a caller that CHECKS OUT OVER a tree has to
/// tell "I measured nothing here" from "I could not measure", and only the first
/// authorises the checkout. A caller that DELETES reads an unproven candidate as
/// KEEP; this one carries another unit's
/// work off, so an unproven checkout is REFUSED. Refusing costs the operator one
/// commit; being wrong in the other direction costs them their work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CheckoutWork {
    /// git answered for this tree and reported nothing: a plain checkout carries
    /// nothing off, so the cut is safe to make.
    ProvenClean,
    /// A única coisa suja na árvore são artefatos do censo
    /// ([`DirtyPathKind::Census`]), nomeados aqui. Não é trabalho de ninguém: a
    /// ferramenta os escreveu e a ferramenta os grava.
    ///
    /// Isso NÃO quer dizer "o corte nunca é recusado por causa deles": a
    /// gravação só existe na BASE, e fora dela esses caminhos viajariam para
    /// dentro da branch nova exatamente como o trabalho de alguém viajaria. Quem
    /// pesa as duas coisas — o que está sujo E onde o checkout está — é
    /// [`crate::commands::event::census_settlement::settle`], numa resposta só.
    ///
    /// Mantido APARTE de [`Self::ProvenClean`] porque quem grava precisa saber
    /// QUAIS caminhos são, e uma árvore limpa não tem nenhum.
    CensusOnly(Vec<String>),
    /// Paths positively observed as uncommitted or untracked — `.claude/`
    /// included. `theirs` é só o que é do OPERADOR, para que a recusa nomeie
    /// apenas o que é dele; `census` é o que a ferramenta escreveu e veio JUNTO
    /// na mesma árvore. O segundo campo existe porque a base avança por cima
    /// do censo tanto quanto numa árvore [`Self::CensusOnly`], e um censo que
    /// sumisse da leitura por haver trabalho do operador ao lado prendia a base
    /// atrás do `origin` com uma recusa cujo remédio falhava do mesmo jeito.
    Holds {
        theirs: Vec<String>,
        census: Vec<String>,
    },
    /// Nothing could be established: the probe failed, or git answered in a
    /// shape this parser does not understand. Never an authorisation.
    Unproven,
}

/// What `root`'s working tree holds, measured the way the CUT decision needs it.
///
/// NOT [`crate::commands::work_unit_open::dirty_paths`], and the two differences
/// between them are the whole reason this probe exists:
///
/// 1. **`.claude/` counts.** Everything the harness generates for a unit —
///    `spec.md`, the waves, `ac-proof.json`, the change log, the review verdicts
///    — lives IN the work branch and is integrated into the base at merge time:
///    `spec-draft` cuts the branch FIRST and writes the spec afterwards, and a
///    spec write on a bare integration base is denied
///    (the write gate). So between approval and the
///    merge, a unit's uncommitted work IS its `.claude/spec/…`, and a probe that
///    drops those paths reads the NORMAL state of an in-flight unit as an empty
///    tree. `dirty_paths`' carve-out was written when `.claude/` was treated as
///    redirected shared state; that reasoning does not hold for this consumer,
///    where `.claude/spec/…` is branch content that rides a checkout exactly
///    like source code does. The VOLATILE harness state is separated from it by
///    [`classify_dirty_path`], a list this probe OWNS — not by the project's
///    `.gitignore`, which cannot be relied on to say anything (see there). O
///    censo sai por essa mesma porta, para uma categoria só dele.
/// 2. **A failed measurement is not "clean".** `dirty_paths` reads an
///    unanswerable probe as an empty list, which is right for ITS callers: they
///    REFUSE a cut, so an unmeasured probe merely lets the ordinary path
///    through. Here the failure mode runs the other way — an unmeasured probe
///    would carry another unit's uncommitted work onto a second branch,
///    silently. So an unanswerable probe is [`CheckoutWork::Unproven`], and the
///    caller refuses on it.
pub(crate) fn checkout_work(root: &Path) -> CheckoutWork {
    // ONE settlement, ONE measurement. The counter is the regression test for
    // that: it is per-THREAD, and cargo's harness gives each test its own
    // thread, so a test can assert how many times a door walked the tree
    // without the count of a neighbour running in parallel leaking into it.
    #[cfg(test)]
    TREE_PROBES.with(|n| n.set(n.get() + 1));
    // `--untracked-files=all` is load-bearing, not tidiness. By default git
    // COLLAPSES an entirely-untracked directory into one entry: a fresh
    // `.claude/` arrives as a single `?? .claude/` line that stands for the
    // harness's own scratch and the unit's `spec.md` alike, and no reading of
    // that line can be right for both — call it scratch and a unit's work
    // vanishes, call it work and a project whose `.claude/` was never committed
    // refuses every cut forever. Asking git to ENUMERATE dissolves the choice
    // instead of guessing at it: each file is then classified on its own name.
    let Some(out) = crate::commands::git_settle::git_out(
        root,
        &["status", "--porcelain", "--untracked-files=all"],
    ) else {
        return CheckoutWork::Unproven;
    };
    let mut paths = Vec::new();
    let mut census = Vec::new();
    let mut unparsed = 0usize;
    for line in out.lines() {
        let line = line.trim_start();
        if line.is_empty() {
            continue;
        }
        // `XY <path>`, but NEVER sliced at a fixed column: `git_out` trims the
        // whole output, so the FIRST entry loses its leading status space
        // (`" M a.txt"` arrives as `"M a.txt"`). Split on the first space
        // instead — the status codes never contain one, the path may.
        let Some((code, rest)) = line.split_once(' ') else {
            unparsed += 1;
            continue;
        };
        if code.len() > 2 || !code.chars().all(|c| "MADRCU?!".contains(c)) {
            unparsed += 1;
            continue; // not a status entry we understand — skip, never guess
        }
        // A rename reports `old -> new`; the destination is the live path.
        let rest = rest.trim_start();
        let path = rest.rsplit(" -> ").next().unwrap_or(rest).trim().trim_matches('"');
        if path.is_empty() {
            unparsed += 1;
            continue;
        }
        match classify_dirty_path(root, path) {
            // the harness's own droppings are nobody's work
            DirtyPathKind::Scratch => {}
            // …e o censo também não é de ninguém, mas precisa ser GRAVADO, não
            // descartado: fica de lado, com nome e tudo.
            DirtyPathKind::Census => census.push(path.to_string()),
            DirtyPathKind::Work => paths.push(path.to_string()),
        }
    }
    // O trabalho do operador decide primeiro e sozinho: com uma linha dele na
    // árvore o corte é recusado, e a recusa nomeia só o que é dele — o censo
    // que porventura veio junto não entra na frase, mas continua VISÍVEL na
    // leitura: é ele que a base precisa pôr de lado para avançar.
    if !paths.is_empty() {
        return CheckoutWork::Holds {
            theirs: paths,
            census,
        };
    }
    // git SPOKE and this parser did not understand part of it. That is a failed
    // measurement, not an empty tree — and reading it as empty is exactly how
    // another unit's work rides off on a plain checkout. Note the asymmetry
    // with the scratch skip above: a line we UNDERSTOOD and recognised as the
    // harness's own is measured and dismissed; a line we could not parse is not
    // measured at all.
    if unparsed > 0 {
        return CheckoutWork::Unproven;
    }
    // Nada do operador e nada por medir: sobrou o censo, e ele tem dono.
    if !census.is_empty() {
        return CheckoutWork::CensusOnly(census);
    }
    CheckoutWork::ProvenClean
}

#[cfg(test)]
thread_local! {
    /// How many whole-tree probes [`checkout_work`] has run on THIS thread.
    ///
    /// Test scaffolding for one specific claim: the settlement measures the
    /// tree exactly once per door, and nothing after it measures again.
    pub(crate) static TREE_PROBES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Directory names the harness writes DIRECTLY under a `.claude/` for itself.
///
/// Owned here, deliberately. The seeded `.claude/.gitignore` covers the same
/// ground, and this probe used to justify its safety by that coverage — but
/// `seed_gitignore` is `overwrite: false` and `mustard init` offers a "Merge
/// (keep my files)" choice, so widening the template reaches NEW installs only.
/// Every project already in the field keeps the file it was installed with;
/// this very repository carries the eight-rule version, with no `.session/`
/// entry, and the only reason its cuts work is an unrelated line in its ROOT
/// `.gitignore`. A correctness-critical decision cannot rest on configuration
/// that is stale in most projects, hand-editable in all of them, and absent in
/// some. The harness knows what the harness writes; it asks itself.
///
/// The first thing the delegation misread was `.claude/.session/`, where
/// [`crate::shared::context::set_pending_branch`] writes the very marker this
/// decision consumes: the cut refused over the gate's own droppings, and told
/// the operator to commit or stash them.
///
/// Derived from what the harness writes, cross-read against
/// `packages/core/templates/.gitignore` and this repository's root `.gitignore`
/// and checked against the code that creates each one. Deliberately NOT
/// everything those files ignore: anything not named here COUNTS, which is the
/// safe direction for this caller. `spec/<unit>/spec.md`, `wave-plan.md`,
/// `ac-proof.json`, `change-log.md`, the wave directories and `review/` are the
/// WORK — they live in the branch and are integrated at merge time.
const HARNESS_SCRATCH_DIRS: &[&str] = &[
    ".cache",
    ".harness",
    ".metrics",
    ".agent-state",
    ".pipeline-states",
    ".compact-state",
    ".session",
    ".dispatch",
    ".agent-memory",
    ".obsidian",
    "agent-memory",
    "knowledge",
    "memory",
    // Cut by `work-unit-open`, pruned by `git-settle` — never branch content.
    "worktrees",
];

/// File names the harness writes directly under a `.claude/`, each regenerable
/// or per-machine. Matched only at that exact depth. See [`HARNESS_SCRATCH_DIRS`].
const HARNESS_SCRATCH_FILES: &[&str] = &[
    "feature-digest.json",
    "settings.local.json",
    ".dashboard.pid",
    ".dashboard.port",
];

/// Os arquivos que o próprio Mustard escreve ao MAPEAR o projeto — o censo.
/// Lidos diretamente sob um `.claude/`, em qualquer profundidade da árvore: o
/// `scan-map.md` de cada subprojeto mora no `.claude/` dele, e o modelo, o
/// dicionário e a lista de recusas moram no do raiz.
///
/// Categoria PRÓPRIA, nem rascunho nem trabalho, e as duas leituras erradas
/// custam coisas diferentes. Rascunho não serve: nenhuma regra de ignore os
/// cobre num install compartilhado — eles são versionados de propósito, e
/// descartá-los aqui os deixaria de fora de todo commit. Trabalho também não:
/// a ferramenta os escreve e a ferramenta os grava, então cobrá-los do operador
/// NA BASE é recusar o corte por causa da saída da própria ferramenta — o atrito
/// que [`crate::commands::event::census_settlement::settle`] fecha.
///
/// FORA da base não há gravação nenhuma para fechar esse atrito, e aí a
/// categoria continua nomeando os caminhos mas não libera nada: ver
/// [`crate::commands::event::census_settlement`].
const CENSUS_FILES: &[&str] = &["grain.model.json", "scan-declined.json", "scan-map.md"];

/// A subárvore sob um `.claude/` onde os moldes `{papel}-pattern` do censo
/// vivem, e o nome do arquivo que fecha cada um. A passagem de enriquecimento
/// reescreve `.claude/skills/<molde>/SKILL.md`, então o par identifica a outra
/// metade do censo sem precisar conhecer o nome de nenhum molde.
const CENSUS_SKILLS_DIR: &str = "skills";
/// Ver [`CENSUS_SKILLS_DIR`].
const CENSUS_SKILL_FILE: &str = "SKILL.md";

/// Who owns a `.claude/skills/<mold>/SKILL.md`, read from the `source:` of its
/// frontmatter.
///
/// The file name does NOT answer the question. The two curated molds of this
/// repository (`rt-verdict-pattern`, `core-function-pattern`) have exactly the
/// same path as a generated mold and carry `source: manual` — the marker whose
/// documented meaning is "hand edits are not overwritten". Treating every
/// `SKILL.md` as census makes the operator's hand edit stop being their work:
/// the cut stops refusing because of it and it travels into the new unit — the
/// swap the category exists to prevent.
///
/// The ABSENT key is the same case as `manual`, and it is the common one: a
/// hand-written `.claude/skills/<something>/SKILL.md` — the standard place of a
/// project skill in Claude Code — almost never declares `source:`. Reading the
/// absence as census would reopen the same swap by OMITTING the key instead of
/// by writing it.
///
/// That is why the read is [`DirtyPathKind::Census`] ONLY when `source:`
/// resolves to `scan`. Everything else — `manual`, an absent key, any other
/// value, an unreadable file, a frontmatter that does not close — is
/// [`DirtyPathKind::Work`], the same safe direction the truncation rule above
/// takes: erring towards "there is work" costs a commit, erring the other way
/// costs someone's work.
///
/// The `source:` field is read through the core's frontmatter parser (BOM and
/// CRLF tolerant, and aware of the `metadata:` block, so an indented `source:`
/// in there is never mistaken for the top key), which is the one reader of that
/// key in the crate.
fn census_skill_kind(root: &Path, path: &str) -> DirtyPathKind {
    let Ok(text) = std::fs::read_to_string(root.join(path)) else {
        return DirtyPathKind::Work;
    };
    let machine_written = mustard_core::domain::skill::frontmatter::parse(&text)
        .ok()
        .and_then(|fm| fm.source())
        .is_some_and(|s| s.eq_ignore_ascii_case("scan"));
    if machine_written {
        DirtyPathKind::Census
    } else {
        DirtyPathKind::Work
    }
}

/// O que uma linha do `git status` É, para quem está prestes a levar o checkout
/// embora — as três categorias que [`checkout_work`] separa.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DirtyPathKind {
    /// Rascunho do harness: regenerável ou por-máquina. Ninguém precisa dele, e
    /// levá-lo junto não custa nada — some da medição.
    Scratch,
    /// Artefato do censo: escrito pela ferramenta, versionado de propósito.
    Census,
    /// O trabalho de alguém — ou um caminho que não se provou ser nenhuma das
    /// duas coisas acima, que é a direção segura para este chamador.
    Work,
}

/// The per-spec spill: written INSIDE a unit's own directory, but by the
/// harness for itself, not by the unit. Everything else under
/// `.claude/spec/<unit>/` is the unit's work.
const SPEC_SCRATCH_DIRS: &[&str] = &[".events", ".blobs", ".dispatch"];

/// Per-spec marker files, same reasoning as [`SPEC_SCRATCH_DIRS`].
///
/// [`CUT_BASE_FILE`] is here because THIS decision is what would otherwise trip
/// over it: the cut writes that record and the very next cut probes the tree, so
/// a refusal over it would be the gate refusing over its own droppings — the
/// same defect the `.claude/.session/` entry above exists to prevent. It is
/// named, never a wildcard: everything else the harness drops in a unit's
/// directory is that unit's work.
const SPEC_SCRATCH_FILES: &[&str] = &[".memory-approved", CUT_BASE_FILE];

/// Classifica um caminho do `git status` nas três categorias de
/// [`DirtyPathKind`], lendo-o sob um `.claude/` a qualquer profundidade da
/// árvore — o `.claude/` aninhado de um subprojeto é o mesmo harness escrevendo
/// o mesmo estado, que é por isso que o `.gitignore` do raiz soletra aquelas
/// regras como `**/.claude/…`.
///
/// TRUNCATION. git can report a DIRECTORY where a file was expected — one entry
/// standing for everything below it. [`checkout_work`] asks git to enumerate,
/// which removes the collapse for every directory git can descend; what still
/// arrives truncated is a directory git would NOT descend (a nested repository,
/// say), i.e. contents this probe did not measure. So the rule is: a truncated
/// directory is scratch only when the WHOLE of it is scratch by name
/// (`.claude/.session/`), and counts as work whenever it could hold anything
/// else. That is why a bare `.claude/` counts — it holds the scratch AND the
/// specs — and why `.claude/spec/` and `.claude/spec/<unit>/` count too. Erring
/// toward "there is work" is the safe direction here (it costs one commit;
/// being wrong the other way costs somebody their work), and with the
/// enumeration in place it no longer strands the project whose `.claude/` was
/// never committed: that tree now arrives as its individual files.
///
/// A mesma regra vale para o censo: só um caminho INTEIRO se classifica como
/// [`DirtyPathKind::Census`] — `.claude/skills/` truncado continua sendo
/// trabalho, porque este teste não mediu o que há embaixo.
///
/// `root` existe por causa de UMA pergunta que o caminho não responde: um
/// `SKILL.md` é do censo ou do operador conforme o `source:` do frontmatter
/// dele, então esse caso — e só ele — abre o arquivo ([`census_skill_kind`]).
fn classify_dirty_path(root: &Path, path: &str) -> DirtyPathKind {
    let normalised = path.replace('\\', "/");
    let mut segments = normalised.split('/').filter(|s| !s.is_empty());
    // Everything up to the first `.claude` segment is somebody else's tree.
    // `.claude/.claude/` cannot occur (guarded in `ClaudePaths`), so the first
    // occurrence is the only one worth reading.
    if !segments.any(|s| s == ".claude") {
        return DirtyPathKind::Work;
    }
    let rest: Vec<&str> = segments.collect();
    // A bare `.claude` / `.claude/`: measured nothing, so it counts.
    let Some(first) = rest.first() else {
        return DirtyPathKind::Work;
    };
    if HARNESS_SCRATCH_DIRS.contains(first) {
        return DirtyPathKind::Scratch;
    }
    if rest.len() == 1 {
        if HARNESS_SCRATCH_FILES.contains(first) {
            return DirtyPathKind::Scratch;
        }
        if CENSUS_FILES.contains(first) {
            return DirtyPathKind::Census;
        }
        return DirtyPathKind::Work;
    }
    // `.claude/skills/<molde>/…/SKILL.md` — o molde inteiro, nunca um diretório
    // truncado que só COMEÇA em `skills/`. Quem escreve o molde está no
    // frontmatter dele, não no caminho.
    if *first == CENSUS_SKILLS_DIR && rest.last() == Some(&CENSUS_SKILL_FILE) {
        return census_skill_kind(root, &normalised);
    }
    if *first != "spec" {
        return DirtyPathKind::Work;
    }
    // `.claude/spec/<unit>/<child>/…` — only the spill is scratch. A shorter
    // path is a truncated directory that may hold the unit's own spec.
    match rest.get(2) {
        Some(child) if SPEC_SCRATCH_DIRS.contains(child) => DirtyPathKind::Scratch,
        Some(child) if rest.len() == 3 && SPEC_SCRATCH_FILES.contains(child) => {
            DirtyPathKind::Scratch
        }
        _ => DirtyPathKind::Work,
    }
}

/// How many dirty paths a verdict spells out before summarising — a hook
/// message is one line in the transcript.
const MAX_DIRTY_NAMED: usize = 5;

/// `{paths}` and `{more}` for a dirty-path list: the first
/// [`MAX_DIRTY_NAMED`] names, then ` (+N)` for whatever is left (empty when
/// nothing is). ONE truncation, rendered into two different sentences — the
/// refusal below and the gate's checkout-failure note.
pub(crate) fn name_dirty_paths(dirty: &[String]) -> (String, String) {
    let shown: Vec<&str> = dirty.iter().take(MAX_DIRTY_NAMED).map(String::as_str).collect();
    let more = dirty.len().saturating_sub(shown.len());
    let tail = if more == 0 { String::new() } else { format!(" (+{more})") };
    (shown.join(", "), tail)
}

/// The checkout is BUSY: it holds ANOTHER unit's branch AND that unit's work is
/// still uncommitted, so [`checkout_work_branch`] — a plain checkout, no stash —
/// would carry those edits onto this unit's branch and leave the session already
/// working here on a branch it never asked for.
///
/// The measured facts, kept apart from the sentence built out of them, so the
/// gate and the draft REPORT the same refusal in their own shapes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BusyCheckout {
    /// The branch the checkout is on — another unit's, a protected base, or
    /// the stale base itself, depending on [`Self::cause`].
    pub(crate) current: String,
    /// The branch that was going to be cut here. Empty at the explicit open,
    /// which cuts nothing and can only be refused for a stale base.
    pub(crate) target: String,
    /// WHAT was established about the tree: the paths positively observed
    /// ([`CheckoutWork::Holds`], [`CheckoutWork::CensusOnly`]), or the fact
    /// that the probe could not answer ([`CheckoutWork::Unproven`]).
    ///
    /// [`CheckoutWork::ProvenClean`] appears only under
    /// [`RefusalCause::BaseStale`] — a clean tree on a base that trails its
    /// remote is refused for the base, not for the tree. Under
    /// [`RefusalCause::BaseBlockedByWork`] it is the [`CheckoutWork::Holds`]
    /// reading whose `theirs` the advance would overwrite. Quem constrói isto é
    /// [`crate::commands::event::census_settlement::settle`], o único construtor
    /// desta struct.
    pub(crate) work: CheckoutWork,
    /// WHY the move was refused — the row of the settlement's table that
    /// answered, so the sentence can say what unblocks it. See
    /// [`RefusalCause`].
    pub(crate) cause: RefusalCause,
}

/// The reasons a settlement refuses — one per ROW of the decision table in
/// [`crate::commands::event::census_settlement::settle`], because each row
/// needs a different sentence: what is in the way differs, and so does what
/// the operator has to do about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RefusalCause {
    /// The checkout is another unit's branch and holds work that is not this
    /// unit's (or a tree that could not be measured); a plain checkout would
    /// carry it onto the new branch.
    WorkWouldTravel,
    /// The base this move is about trails `origin/{base}` and could not be
    /// fast-forwarded ([`BaseRefresh::Stale`]). Carries git's own words.
    BaseStale { base: String, error: String },
    /// The base trails `origin/{base}`, the advance IS a fast-forward, and it
    /// would overwrite uncommitted work of the OPERATOR's — `paths`, measured
    /// with [`paths_the_advance_overwrites`] before anything was touched. The
    /// census beside it is the tool's and would have been set aside; their
    /// files are theirs, so the sentence names them and prescribes the stash
    /// that unblocks the advance — not a `git pull` that fails the same way.
    BaseBlockedByWork { base: String, paths: Vec<String> },
}

impl BusyCheckout {
    /// The one sentence every door says: WHERE the checkout is, WHAT is in the
    /// way, and WHAT to do about it. Catalogue-rendered in the project's
    /// configured language, one sentence per [`RefusalCause`].
    ///
    /// Under [`RefusalCause::WorkWouldTravel`] the sentence follows what was
    /// MEASURED: an unmeasured checkout says exactly that, because rendering
    /// the named-paths sentence with an empty list would print "uncommitted
    /// work in: ." and teach the operator that the refusal is noise.
    pub(crate) fn reason(&self, lang: mustard_core::platform::i18n::Locale) -> String {
        use mustard_core::platform::i18n::translate;
        let named = match (&self.cause, &self.work) {
            // The advance was blocked by SPECIFIC files of theirs: those are
            // the ones to name, not everything dirty in the tree.
            (RefusalCause::BaseBlockedByWork { paths, .. }, _) => Some(name_dirty_paths(paths)),
            (_, CheckoutWork::Holds { theirs, .. }) => Some(name_dirty_paths(theirs)),
            (_, CheckoutWork::CensusOnly(dirty)) => Some(name_dirty_paths(dirty)),
            (_, CheckoutWork::ProvenClean | CheckoutWork::Unproven) => None,
        };
        let (paths, more) = named.clone().unwrap_or_default();
        let key = match &self.cause {
            RefusalCause::WorkWouldTravel if named.is_none() => "workbranch.busy.unmeasured",
            RefusalCause::WorkWouldTravel => "workbranch.busy.refusal",
            RefusalCause::BaseStale { .. } => "workbranch.busy.base_stale",
            RefusalCause::BaseBlockedByWork { .. } => "workbranch.busy.base_blocked",
        };
        let (base, error) = match &self.cause {
            RefusalCause::BaseBlockedByWork { base, .. } => (base.as_str(), ""),
            RefusalCause::BaseStale { base, error } => (base.as_str(), error.as_str()),
            RefusalCause::WorkWouldTravel => ("", ""),
        };
        translate(key, lang)
            .replace("{current}", &self.current)
            .replace("{target}", &self.target)
            .replace("{paths}", &paths)
            .replace("{more}", &more)
            .replace("{base}", base)
            .replace("{error}", error)
    }
}

/// What [`cut_pending_work_branch`] did — the closed set, so a caller that must
/// decide (refuse? warn? say nothing?) reads a state instead of guessing from a
/// bool. `NoPending` and `AlreadyThere` are deliberately apart: "no work unit
/// was signalled" and "the unit's branch is already the checkout" both leave
/// git untouched and mean opposite things to the caller.
///
/// No serde derive — the JSON shape belongs to whichever command reports it
/// (`spec-draft` folds it into its own document).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CutOutcome {
    /// No `pending-work-branch` marker for this session (or no VCS at all):
    /// nothing was ever promised, so nothing was cut.
    NoPending,
    /// The checkout was ALREADY the pending branch; the marker was consumed and
    /// git was not touched.
    AlreadyThere(String),
    /// The branch was created (or checked out) by this call. Carries its name.
    Cut(String),
    /// REFUSED: the checkout holds another unit's branch with uncommitted work,
    /// so cutting here would carry that work off
    /// ([`crate::commands::event::census_settlement::settle`]). Nothing
    /// was touched and the marker is KEPT — the unit was never started, so
    /// there is nothing to consume.
    Refused(BusyCheckout),
    /// The base could not be established, so nothing was cut: the unit is an
    /// emergency, the project declares several bases it could have come from,
    /// and NOTHING — neither the pending marker nor the unit's own record —
    /// says which one the operator chose ([`recorded_or_derived_base`]).
    ///
    /// Deliberately not folded into [`Failed`](Self::Failed): git was never
    /// asked, so "resolve the git state and try again" is the wrong sentence.
    /// What unblocks this is re-opening the unit with an explicit base. Carries
    /// the branch that was wanted, where the tree sits (so the caller can tell a
    /// protected base from a work branch, as it does for a failed checkout) and
    /// the candidates nothing chose between. The marker is KEPT.
    BaseUnknown {
        target: String,
        current: Option<String>,
        candidates: Vec<String>,
    },
    /// Git refused the checkout. Carries the branch that was wanted, the branch
    /// the tree actually sits on (`None` on a detached HEAD / probe failure),
    /// and git's own message. The JUDGEMENT of how bad that is lives in the
    /// caller: staying on an integration base is a refusal for one caller and
    /// merely a warning for another.
    Failed {
        target: String,
        current: Option<String>,
        error: String,
    },
}

/// Consume this session's `pending-work-branch` marker and check that branch
/// out in `project`, creating it off its base.
///
/// The only cut: no hook cuts a branch on a file mutation. `spec-draft` calls
/// it because the spec
/// must be written INSIDE the unit: the draft is the first thing the work
/// produces, and it used to land on the integration base (a `.claude/spec/`
/// carve-out existed precisely to let it). Cutting here moves the draft, the
/// wave layout and the negative proof onto the branch, in that one call.
///
/// Idempotent by construction: the marker is cleared on every outcome that
/// leaves the tree on the target branch, so a second call answers `NoPending`.
/// The marker is KEPT on a failure and on a refusal — the intent survives for a
/// retry, exactly as the hook gate keeps it.
///
/// The refusal is the point the review found missing: this door opens FIRST
/// (`spec-draft` calls it at approval, before any `Write` reaches the hook
/// gate), so a guard living only in the gate never ran. The decision is
/// [`crate::commands::event::census_settlement::settle`], the same one the gate
/// takes — one question, one answer, and the base refresh happens inside it
/// rather than in either door.
pub(crate) fn cut_pending_work_branch(project: &Path, session: &str) -> CutOutcome {
    let config = mustard_core::ProjectConfig::load(project);
    // An explicit `vcs: ""` opt-out (or a non-git tree) means there is no
    // branch to cut and nothing to guard. The executor would refuse every step
    // below anyway; answering here keeps the door from reading markers and
    // reporting a failure for a project that simply does not cut branches.
    if config.vcs().is_none() {
        return CutOutcome::NoPending;
    }
    let root = project.to_string_lossy().into_owned();
    let Some(target) = crate::shared::context::pending_branch_for(&root, session) else {
        return CutOutcome::NoPending;
    };

    let current = mustard_core::current_branch(project);
    if current.as_deref() == Some(target.as_str()) {
        crate::shared::context::clear_pending_branch(&root, session);
        return CutOutcome::AlreadyThere(target);
    }

    // WHERE from, RESOLVED FIRST and only once: it is one of the inputs of the
    // question below, which needs to know which base to update. Resolving it
    // here does not change the ORDER of the refusals — `Refused` still comes
    // before `BaseUnknown`, because this step returns nothing by itself.
    let resolved_base = recorded_or_derived_base(&root, session, &target, &config);
    let base_hint = resolved_base.as_deref().ok();

    // THE WHOLE QUESTION, asked once: what is dirty and where the checkout
    // stands. The answer already comes with the base updated from `origin`,
    // and this door executes no step.
    match crate::commands::event::census_settlement::settle(
        project,
        crate::commands::event::census_settlement::CheckoutPosition::at(
            current.as_deref(),
            Some(&target),
            base_hint,
        ),
        &config,
    ) {
        // Work of ANOTHER unit would travel, or the base could not advance:
        // refusing before cutting leaves everything where the author left it.
        crate::commands::event::census_settlement::CensusSettlement::Refuse(busy) => {
            return CutOutcome::Refused(busy)
        }
        crate::commands::event::census_settlement::CensusSettlement::Proceed => {}
    }

    // An emergency whose pick nothing carries has no honest base, and cutting it
    // from the outermost candidate is the silent replacement of the operator's
    // answer this refuses to make.
    let base = match resolved_base {
        Ok(base) => base,
        Err(candidates) => {
            return CutOutcome::BaseUnknown {
                target,
                current,
                candidates,
            }
        }
    };

    match checkout_work_branch(project, &target, &base) {
        Ok(()) => {
            // The marker that carried the operator's answer is about to be
            // consumed, so this is the LAST moment the answer exists. Write it
            // into the unit's own directory first, as the HARNESS STATE the
            // draft folds into `meta.json#base` — a no-op wherever the flow can
            // still re-derive it (see `BaseFlow::record_cut_base`).
            BaseFlow::of_at(&config.git, project).record_cut_base(&target, &base);
            crate::shared::context::clear_pending_branch(&root, session);
            CutOutcome::Cut(target)
        }
        Err(error) => CutOutcome::Failed {
            target,
            current,
            error,
        },
    }
}

#[cfg(test)]
mod tests {
    // -----------------------------------------------------------------------
    // Auto-branch name computation
    // -----------------------------------------------------------------------

    use crate::shared::work_kind::{BaseFlow, WorkKind, CUT_BASE_FILE};

    /// Uma base é o que o projeto declarou, ou o que o próprio remoto chama de
    /// padrão — e nada mais.
    ///
    /// A distinção que este teste prende: `dev` e `main` estão no `git.flow`
    /// deste projeto e por isso são base; `producao` é o que este repositório
    /// chama de padrão e por isso também é; `feature/x` não é nem uma coisa
    /// nem outra. Nenhum dos três nomes está escrito no código.
    #[test]
    fn a_base_sai_da_declaracao_ou_do_proprio_remoto() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let git = |args: &[&str]| mustard_core::platform::git::run(root, args).ok;
        if !git(&["init", "-q", "-b", "producao", "."]) {
            return; // no usable git here
        }
        let _ = git(&["config", "user.email", "t@t.t"]);
        let _ = git(&["config", "user.name", "t"]);
        if !git(&["commit", "-q", "--allow-empty", "-m", "seed"]) {
            return;
        }
        // `producao` is what this repository calls its default branch — a name
        // no fallback list and no flow map contains.
        if !git(&["update-ref", "refs/remotes/origin/producao", "HEAD"])
            || !git(&["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/producao"])
        {
            return;
        }

        let config = two_tier(); // declara dev e main
        assert!(
            super::on_integration_base(root, "producao", &config),
            "a branch que o próprio remoto chama de padrão é base, sem configuração nenhuma",
        );
        assert!(
            super::on_integration_base(root, "dev", &config),
            "e a que o `git.flow` declara também é",
        );
        assert!(
            super::on_integration_base(root, "main", &config),
            "inclusive a de cima do fluxo",
        );
        assert!(
            !super::on_integration_base(root, "feature/x", &config),
            "uma branch de trabalho nunca é",
        );
        assert!(
            mustard_core::protected_branches(&config.git).contains("dev"),
            "e a proteção sai da declaração: sem `git.flow`, nada fica protegido",
        );
    }

    /// A project declaring the ordinary two-tier flow: `dev` for common work,
    /// `main` as its outermost base.
    fn two_tier() -> mustard_core::ProjectConfig {
        let mut config = mustard_core::ProjectConfig::default();
        config.git.flow.insert("*".to_string(), "dev".to_string());
        config.git.flow.insert("dev".to_string(), "main".to_string());
        config
    }

    /// A project root that holds no unit records — so the assertion below is
    /// about the DERIVATION alone, with nothing recorded to prefer over it.
    fn nowhere() -> &'static std::path::Path {
        std::path::Path::new("/no/project")
    }

    /// The branch is named by what the unit IS, never by the base it was
    /// cut from.
    ///
    /// The name an operator reads in `git branch` is the assertion: a feature
    /// says `feature/`, a fix says `fix/`, an emergency says `hotfix/`, and no
    /// integration base appears in any of them. The old shape put the base
    /// there, which is the information the operator did NOT need and the only
    /// one the program did — that split is what this unit undoes.
    #[test]
    fn a_unit_branch_is_named_by_its_kind() {
        let named = |kind| {
            super::compute_work_branch(
                kind,
                "parcelas-virtuais",
                None,
                "sess-abcdef12",
                "2026-07-02T10:00:00.000Z",
                "/no/project",
            )
        };
        assert_eq!(named(WorkKind::parse("feature").expect("suggested token parses")), "feature/parcelas-virtuais");
        assert_eq!(named(WorkKind::parse("fix").expect("suggested token parses")), "fix/parcelas-virtuais");
        assert_eq!(named(WorkKind::parse("hotfix").expect("suggested token parses")), "hotfix/parcelas-virtuais");

        // No integration base of this project appears in any of the names —
        // stated against the DECLARED bases, not against the literals, so the
        // assertion means the same thing in a develop/master project.
        let config = two_tier();
        for token in WorkKind::SUGGESTED {
            let kind = WorkKind::parse(token).expect("suggested token parses");
            let branch = named(kind);
            for base in config.git.declared_bases() {
                assert!(
                    !branch.starts_with(&format!("{base}_")),
                    "the name records the kind, not the cut: {branch}",
                );
            }
        }
    }

    #[test]
    fn compute_work_branch_prefers_spec_slug() {
        let b = super::compute_work_branch(WorkKind::parse("feature").expect("suggested token parses"), "2026-07-02-my-spec", None, "sess-abcdef12", "2026-07-02T10:00:00.000Z", "/no/project");
        assert_eq!(b, "feature/2026-07-02-my-spec");
    }

    #[test]
    fn compute_work_branch_falls_back_to_intent_slug() {
        // No spec → the intent is slugified (pt-BR strips accents by default).
        let b = super::compute_work_branch(WorkKind::parse("fix").expect("suggested token parses"), "", Some("Corrigir botão de login"), "sess-abcdef12", "2026-07-02T10:00:00.000Z", "/no/project");
        assert_eq!(b, "fix/corrigir-botao-login");
    }

    #[test]
    fn compute_work_branch_date_fallback_when_no_spec_or_intent() {
        // No spec, no intent → date-from-ts + short session id.
        let b = super::compute_work_branch(WorkKind::parse("feature").expect("suggested token parses"), "", None, "sess-abcdef1234", "2026-07-02T10:00:00.000Z", "/no/project");
        assert_eq!(b, "feature/2026-07-02-sess-abc");
    }

    #[test]
    fn compute_work_branch_sanitizes_unsafe_slug() {
        // A spec with unsafe chars is sanitised into a valid ref.
        let b = super::compute_work_branch(WorkKind::parse("feature").expect("suggested token parses"), "weird ..slug/", None, "unknown", "2026-07-02T10:00:00.000Z", "/no/project");
        // ".." collapsed, spaces mapped to '-', trailing '/' trimmed.
        assert_eq!(b, "feature/weird--slug");
        assert!(!b.contains(".."), "no `..` runs in a git ref");
        assert!(!b.starts_with('-'), "no leading dash");
    }

    /// With the base gone from the name, it is recovered from the
    /// DECLARED FLOW and from nothing else.
    ///
    /// The proof that the branch string is not consulted is the second project:
    /// the same branch names, a different `git.flow`, and every answer moves
    /// with the configuration. A reader still parsing the name would answer the
    /// same thing twice.
    #[test]
    fn the_base_comes_from_the_declared_flow_not_from_the_branch_name() {
        // With two bases declared and nothing recorded, the NAME answers
        // nothing for any kind — the prefix stopped carrying a base at all.
        // `base_for` reports the candidates instead of picking one.
        let dev_main = two_tier();
        for name in ["feature/my-unit", "fix/my-unit", "hotfix/my-unit"] {
            assert!(
                super::base_for(nowhere(), name, &dev_main).is_err(),
                "the name alone cannot say where {name} came from",
            );
        }

        // The SAME names, a different declared flow — the CANDIDATES follow the
        // configuration, so nothing is read out of the string. That was always
        // this test's real subject; what changed is that the answer is now a
        // list to choose from instead of one value derived from the prefix.
        let mut develop_master = mustard_core::ProjectConfig::default();
        develop_master.git.flow.insert("*".to_string(), "develop".to_string());
        develop_master.git.flow.insert("develop".to_string(), "master".to_string());
        for name in ["feature/my-unit", "hotfix/my-unit"] {
            assert_eq!(
                super::base_for(nowhere(), name, &develop_master),
                Err(vec!["develop".to_string(), "master".to_string()]),
                "the candidates are THIS project's, with no dev/main literal anywhere",
            );
        }

        // And where the project leaves no choice, there is nothing to ask: a
        // single declared base answers without a record.
        let mut single = mustard_core::ProjectConfig::default();
        single.git.flow.insert("*".to_string(), "trunk".to_string());
        assert_eq!(
            super::base_for(nowhere(), "feature/my-unit", &single).as_deref(),
            Ok("trunk"),
        );

        // With TWO bases every answer above is derivable, which is why they are
        // answers. Where the flow leaves a real choice and nothing recorded one,
        // this refuses instead of naming the outermost candidate — the guess
        // that used to travel as a fact through both cut doors.
        let mut three = mustard_core::ProjectConfig::default();
        three.git.flow.insert("*".to_string(), "dev".to_string());
        three.git.flow.insert("dev".to_string(), "qas".to_string());
        three.git.flow.insert("qas".to_string(), "main".to_string());
        assert_eq!(
            super::base_for(nowhere(), "hotfix/my-unit", &three),
            Err(vec!["dev".to_string(), "main".to_string(), "qas".to_string()]),
            "several candidates and nothing recorded — it says so, it does not pick",
        );
        // …and the ordinary kinds no longer derive there either. The refusal
        // used to be scoped to the emergency case, because that was the only
        // question the kind could not answer; with the kind answering nothing,
        // the scope is every unit of a multi-base project — which is what makes
        // the operator's pick worth recording in the first place.
        assert_eq!(
            super::base_for(nowhere(), "feature/my-unit", &three),
            Err(vec!["dev".to_string(), "main".to_string(), "qas".to_string()]),
            "an ordinary unit is asked the same question, and answered the same way",
        );
    }

    /// A name that is nobody's unit still has to be cut from somewhere: the base
    /// ordinary work is cut from, read from the flow and never a hardcoded
    /// branch. This is the only leg of `base_for` that answers without
    /// recognising the name, so it is stated on its own.
    #[test]
    fn base_for_falls_back_to_the_work_base_when_nothing_owns_the_name() {
        assert_eq!(super::base_for(nowhere(), "whatever", &two_tier()).as_deref(), Ok("dev"));

        let mut develop_master = mustard_core::ProjectConfig::default();
        develop_master.git.flow.insert("*".to_string(), "develop".to_string());
        develop_master.git.flow.insert("develop".to_string(), "master".to_string());
        assert_eq!(
            super::base_for(nowhere(), "whatever", &develop_master).as_deref(),
            Ok("develop"),
        );
    }

    /// A branch in the `{base}_{slug}` shape is still this unit's branch,
    /// and still resolves to its base.
    ///
    /// Units in flight would be orphaned otherwise: the pull-request target, the
    /// merged-ancestry check and the second-unit refusal all resolve a unit
    /// through its branch name, so the reading of the old shape has to survive
    /// the change of the new one.
    #[test]
    fn an_old_shape_branch_is_still_understood() {
        let config = two_tier();
        assert_eq!(super::base_for(nowhere(), "dev_my-unit", &config).as_deref(), Ok("dev"));
        assert_eq!(super::base_for(nowhere(), "main_my-unit", &config).as_deref(), Ok("main"));
        assert_eq!(super::slug_of_work_branch("dev_my-unit", &config).as_deref(), Some("my-unit"));
        assert_eq!(super::slug_of_work_branch("main_my-unit", &config).as_deref(), Some("my-unit"));

        // …alongside the new shape, through the same reader.
        assert_eq!(
            super::slug_of_work_branch("feature/my-unit", &config).as_deref(),
            Some("my-unit"),
        );
        assert_eq!(
            super::slug_of_work_branch("hotfix/my-unit", &config).as_deref(),
            Some("my-unit"),
        );

        // A bare base is nobody's unit in either shape, and neither is a name
        // whose prefix names no base and no kind.
        for other in ["dev", "main", "feature_x", "nounderscore"] {
            assert_eq!(super::slug_of_work_branch(other, &config), None, "not a unit: {other}");
        }
    }

    /// The rule that replaced "the kind decides the base": nothing decides it
    /// but the operator, and the only thing validated is that the branch EXISTS.
    ///
    /// The test this replaced asserted the opposite — that a `hotfix` could not
    /// be cut from the work base — and that refusal was only meaningful while
    /// the base was inferred. `hotfix/` is a prefix on a name now.
    #[test]
    fn the_base_is_the_operators_answer_not_a_consequence_of_the_kind() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path(); // not a repository: the catalogue is UNMEASURED
        let config = two_tier();

        assert_eq!(
            super::resolve_kind_base(root, None, &config).as_deref(),
            Ok(config.git.primary_base().expect("o fluxo declara uma base").as_str()),
            "no answer falls back to the primary base, which is a default",
        );
        assert_eq!(
            super::resolve_kind_base(root, Some("release/2026-Q3"), &config).as_deref(),
            Ok("release/2026-Q3"),
            "an unmeasured catalogue accepts the answer rather than refusing on a fact nobody measured",
        );
        assert_eq!(
            super::resolve_kind_base(root, Some("dev"), &config).as_deref(),
            Ok("dev"),
            "and the work base is an ordinary answer for ANY kind — the old contradiction is gone",
        );
    }

    /// With NO flow declared and NO `--base`, the default is the remote's own
    /// answer — never the literal `main`.
    ///
    /// This is the shape `mustard init` produces today, and it had no test at
    /// all. `primary_base()` floors to the hardcoded `main` there, so the gate
    /// recorded a branch the repository does not have; once the reader began
    /// checking existence, that invented name was correctly dropped and the
    /// write gate DENIED the first edit of every such project. Measured A/B on
    /// one fixture: the baseline cut the branch, the wave denied it. A default
    /// nobody can check out is not a default.
    #[test]
    fn with_no_flow_and_no_answer_the_default_is_the_remote_own_head() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        seed_repo_declaring(root, None);

        let config = mustard_core::ProjectConfig::load(root);
        assert!(
            config.git.declared_bases().is_empty(),
            "fixture must be the installer's shape: no flow declared",
        );
        let answer = super::resolve_kind_base(root, None, &config);
        assert_ne!(
            answer.as_deref(),
            Ok("main"),
            "the default is the hardcoded literal again — in a repository that has \
             no `main`, that name is recorded and then dropped, and the first edit \
             is denied",
        );
        assert_eq!(
            answer.as_deref(),
            Ok("dev"),
            "the default must be the branch `origin/HEAD` really names",
        );
    }


    /// The half the sibling above could not reach — *"and the operator
    /// chooses when more than one candidate exists"*.
    ///
    /// Choosing is only half of it: the choice has to SURVIVE. With three bases
    /// the pick rides to the cut in the pending marker, so the branch really is
    /// cut from `qas` — and then the marker is CONSUMED, and every later reader
    /// re-derived the base from the kind and answered the outermost candidate
    /// (`main`). The pull-request target and the merged-ancestry check both read
    /// through that derivation, so the unit's work was aimed at a base the
    /// operator never chose. Two bases never diverge, which is why this project's
    /// own configuration could not expose it.
    ///
    /// Driven through the REAL cut in a REAL repository, deliberately: the
    /// defect lives in the seam between the cut consuming the marker and the
    /// next read asking the flow, and no test of either half alone meets it.
    #[test]
    fn a_hotfix_is_cut_from_a_base_that_is_not_the_work_base_and_the_pick_survives_the_cut() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let root_s = root.to_string_lossy().to_string();
        seed_three_tier_repo(root);
        let config = mustard_core::ProjectConfig::load(root);
        let flow = BaseFlow::of(&config.git);
        assert_eq!(flow.bases(), ["dev", "main", "qas"], "the fixture really does leave a choice");

        // The operator picks the MIDDLE base. That answer reaches the cut the
        // one way it can — the pending marker `emit-pipeline` writes.
        let sid = "sess-hotfix-pick";
        crate::shared::context::set_pending_branch(&root_s, sid, "hotfix/my-unit", Some("qas"));

        let outcome = super::cut_pending_work_branch(root, sid);
        assert_eq!(outcome, super::CutOutcome::Cut("hotfix/my-unit".to_string()), "{outcome:?}");
        // The cut itself honoured the pick: the branch sits on `qas`, not `main`.
        assert_eq!(
            mustard_core::current_branch(root).as_deref(),
            Some("hotfix/my-unit"),
        );
        let head = git_rev(root, "HEAD");
        assert_eq!(head, git_rev(root, "qas"), "cut from the base the operator chose");
        assert_ne!(head, git_rev(root, "main"), "…and not from the pre-marked one");

        // The marker is GONE — which is why it cannot be the durable answer.
        assert!(crate::shared::context::pending_base_for(&root_s, sid).is_none());

        // Every LATER read answers the middle base. This is the assertion the
        // unit shipped without: each of these resolved `hotfix/…` through the
        // kind and answered `main`.
        assert_eq!(
            super::base_for(root, "hotfix/my-unit", &config).as_deref(),
            Ok("qas"),
            "the pull-request target follows the operator, not the derivation",
        );
        assert_eq!(
            BaseFlow::of_at(&config.git, root).base_of("hotfix/my-unit").known(),
            Some("qas"),
            "…and so does the crate's one parser, which every consumer folds through",
        );

        // The honest case, kept honest: a hotfix nobody cut has no recorded base
        // and several candidates, so the answer is that there ISN'T one — never
        // the outermost dressed up as a fact.
        let never_cut = BaseFlow::of_at(&config.git, root).base_of("hotfix/never-cut");
        assert!(never_cut.is_unit(), "it is still this project's unit");
        assert_eq!(never_cut.known(), None, "and its base was never established");
        assert_eq!(
            never_cut.candidates(),
            ["dev", "main", "qas"],
            "naming what it could not choose — every declared base, now that the \
             prefix narrows nothing",
        );
    }

    /// The pick travels from the REAL catalogue to the branch actually cut —
    /// even when no configuration ever declared it.
    ///
    /// The picker offers every branch `origin` has, so `release/2026-Q3` is a
    /// legitimate answer in a project whose `git.flow` names only `dev`, `qas`
    /// and `main`. That answer used to be thrown away one step later:
    /// [`super::recorded_or_derived_base`] read it back out of the marker,
    /// tested it for membership in the declared set, and let the derivation
    /// replace it — so the unit was cut from `dev`, and every later read agreed
    /// with the wrong base. The gate ACCEPTED the pick and the cut ignored it,
    /// which is why a test that stops at the door certifies nothing.
    ///
    /// **In ANY project**, which is the half a three-base fixture cannot show.
    /// Whether the answer is written down at all was decided by COUNTING the
    /// declared bases, so a project declaring exactly one was ruled to have
    /// offered no choice — and the pick was dropped before any of the checks
    /// above ever saw it. The picker offers the branches the repository REALLY
    /// has, so that count answered a question nobody asked. The last section
    /// drives the same end-to-end path through a one-base project and a project
    /// declaring no flow at all.
    ///
    /// Driven end to end in a real repository, deliberately: the defect lives
    /// between the marker and the checkout, and neither half alone meets it.
    #[test]
    fn the_recorded_base_survives_to_the_cut_in_any_project() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let root_s = root.to_string_lossy().to_string();
        seed_three_tier_repo(root);

        // A release line no `git.flow` mentions, at a commit of its own — and
        // the remote-tracking refs that make its existence MEASURABLE, which is
        // what the check now asks about.
        git(root, &["checkout", "-b", "release/2026-Q3", "main"]);
        std::fs::write(root.join("f.txt"), "on the release line").expect("seed");
        git(root, &["add", "-A"]);
        git(root, &["commit", "-m", "release"]);
        git(root, &["checkout", "dev"]);
        for branch in ["dev", "qas", "main", "release/2026-Q3"] {
            git(root, &["update-ref", &format!("refs/remotes/origin/{branch}"), branch]);
        }

        // The operator picks the release line out of the catalogue.
        let sid = "sess-release-pick";
        crate::shared::context::set_pending_branch(
            &root_s,
            sid,
            "fix/erro-no-boleto",
            Some("release/2026-Q3"),
        );

        let outcome = super::cut_pending_work_branch(root, sid);
        assert_eq!(
            outcome,
            super::CutOutcome::Cut("fix/erro-no-boleto".to_string()),
            "{outcome:?}",
        );
        let head = git_rev(root, "HEAD");
        assert_eq!(
            head,
            git_rev(root, "release/2026-Q3"),
            "the branch is cut from the base the operator chose, undeclared or not",
        );
        assert_ne!(head, git_rev(root, "dev"), "…and not from the derivation that replaced it");

        // The marker is spent, and every later read still answers the pick —
        // the unit's own record carries it from here.
        assert!(crate::shared::context::pending_base_for(&root_s, sid).is_none());
        let config = mustard_core::ProjectConfig::load(root);
        assert_eq!(
            super::base_for(root, "fix/erro-no-boleto", &config).as_deref(),
            Ok("release/2026-Q3"),
            "the pull-request target follows the operator, not the declaration",
        );

        // ── and now the projects a three-base fixture never reaches ──────────
        //
        // One declaring exactly ONE base, and one declaring NO flow at all —
        // which is every project the current installer touches. Both carry the
        // same two real branches, so in both the operator had the same real
        // choice, and in both the whole path must carry it.
        //
        // And both in the CLONE shape: the picked base exists only as
        // `refs/remotes/origin/release/2026-Q3`, with no local head, which is
        // what a `git clone` leaves for every branch but the default one. The
        // section above cannot show this — it checked the release line out to
        // create it, so a local head was sitting there and the cut could reach
        // the pick without ever consulting the remote-tracking ref.
        for flow in [Some(r#"{"*":"dev"}"#), None] {
            let dir = tempfile::tempdir().expect("tempdir");
            let root = dir.path();
            let root_s = root.to_string_lossy().to_string();
            seed_repo_declaring(root, flow);
            let label = flow.unwrap_or("no flow at all");
            assert!(
                !super::local_branch_exists(root, "release/2026-Q3"),
                "{label}: the fixture IS the clone shape — no local head carries the pick",
            );

            // The gate that decides whether the answer is WRITTEN DOWN asks the
            // catalogue: two branches here, so there was something to choose —
            // whatever the declaration counts.
            let config = mustard_core::ProjectConfig::load(root);
            assert!(
                BaseFlow::of_at(&config.git, root).base_must_be_recorded("fix/erro-no-boleto"),
                "{label}: the repository offered two branches — the pick must be remembered",
            );

            let sid = "sess-any-project";
            crate::shared::context::set_pending_branch(
                &root_s,
                sid,
                "fix/erro-no-boleto",
                Some("release/2026-Q3"),
            );
            let outcome = super::cut_pending_work_branch(root, sid);
            assert_eq!(
                outcome,
                super::CutOutcome::Cut("fix/erro-no-boleto".to_string()),
                "{label}: {outcome:?}",
            );
            let head = git_rev(root, "HEAD");
            assert_eq!(
                head,
                git_rev(root, "origin/release/2026-Q3"),
                "{label}: the branch is cut from the base the operator chose",
            );
            assert_ne!(head, git_rev(root, "dev"), "{label}: and not from the derivation");
            assert_eq!(
                super::base_for(root, "fix/erro-no-boleto", &config).as_deref(),
                Ok("release/2026-Q3"),
                "{label}: every later read still answers the operator's pick",
            );
        }

        // ── and now a REAL clone, with a remote that ANSWERS ─────────────────
        //
        // Everything above runs against a repository with no remote at all, so
        // `fetch_origin` fails and the settlement refreshes nothing — which is
        // one road through the cut, not the ordinary one. A machine that is
        // online takes the other: the fetch succeeds, and `fast_forward_base`
        // is what has to reach the picked base, because that step used to
        // iterate the DECLARED flow alone. Both roads have to land on the
        // commit the operator chose, and only a real remote drives this one.
        let dir = tempfile::tempdir().expect("tempdir");
        let bare = seed_real_origin(dir.path());

        // (a) the clone as `git clone` leaves it: one local head, `dev`.
        let fresh = dir.path().join("fresh");
        clone_project(&bare, &fresh);
        let fresh_s = fresh.to_string_lossy().to_string();
        assert!(
            !super::local_branch_exists(&fresh, "release/2026-Q3"),
            "a clone carries a local head for the default branch alone",
        );
        let sid = "sess-real-clone";
        crate::shared::context::set_pending_branch(
            &fresh_s,
            sid,
            "fix/erro-no-boleto",
            Some("release/2026-Q3"),
        );
        let outcome = super::cut_pending_work_branch(&fresh, sid);
        assert_eq!(
            outcome,
            super::CutOutcome::Cut("fix/erro-no-boleto".to_string()),
            "{outcome:?}",
        );
        assert_eq!(
            git_rev(&fresh, "HEAD"),
            git_rev(&fresh, "origin/release/2026-Q3"),
            "the clone cuts from the tip the remote really carries for the pick",
        );
        assert_ne!(git_rev(&fresh, "HEAD"), git_rev(&fresh, "dev"), "not the derivation");

        // (b) the same clone, except the operator already has a local
        //     `release/2026-Q3` sitting one commit BEHIND. The pick is honoured
        //     either way — the question this half settles is WHICH commit, and a
        //     cut that stops at the stale local head lands on the wrong one.
        let stale = dir.path().join("stale");
        clone_project(&bare, &stale);
        let stale_s = stale.to_string_lossy().to_string();
        git(&stale, &["branch", "release/2026-Q3", "origin/release/2026-Q3~1"]);
        let behind = git_rev(&stale, "release/2026-Q3");
        assert_ne!(
            behind,
            git_rev(&stale, "origin/release/2026-Q3"),
            "the fixture really does park the local head behind the remote",
        );
        crate::shared::context::set_pending_branch(
            &stale_s,
            sid,
            "fix/erro-no-boleto",
            Some("release/2026-Q3"),
        );
        let outcome = super::cut_pending_work_branch(&stale, sid);
        assert_eq!(
            outcome,
            super::CutOutcome::Cut("fix/erro-no-boleto".to_string()),
            "{outcome:?}",
        );
        assert_eq!(
            git_rev(&stale, "HEAD"),
            git_rev(&stale, "origin/release/2026-Q3"),
            "the pick is cut from the LATEST of it, exactly as a declared base is",
        );
        assert_ne!(git_rev(&stale, "HEAD"), behind, "…and not from the stale local head");
    }

    /// A bare `origin` carrying `dev` and a TWO-commit `release/2026-Q3`, built
    /// through a throwaway seed checkout and returned by path.
    ///
    /// Two commits on the release line so `origin/release/2026-Q3~1` names a
    /// real earlier point — the tip a clone that never refreshed would be
    /// sitting on. Returns the bare repository, which is what a clone needs.
    fn seed_real_origin(root: &std::path::Path) -> std::path::PathBuf {
        let bare = root.join("origin.git");
        let seed = root.join("seed");
        std::fs::create_dir_all(&bare).expect("bare dir");
        std::fs::create_dir_all(&seed).expect("seed dir");
        git(&bare, &["init", "--bare", "-b", "dev", "."]);
        git(&seed, &["init", "-b", "dev", "."]);
        git(&seed, &["config", "user.email", "t@example.com"]);
        git(&seed, &["config", "user.name", "t"]);
        std::fs::write(seed.join("f.txt"), "on dev").expect("seed");
        git(&seed, &["add", "-A"]);
        git(&seed, &["commit", "-m", "dev"]);
        git(&seed, &["remote", "add", "origin", bare.to_string_lossy().as_ref()]);
        git(&seed, &["push", "-u", "origin", "dev"]);
        git(&seed, &["checkout", "-b", "release/2026-Q3"]);
        for body in ["release one", "release two"] {
            std::fs::write(seed.join("f.txt"), body).expect("seed");
            git(&seed, &["add", "-A"]);
            git(&seed, &["commit", "-m", body]);
        }
        git(&seed, &["push", "-u", "origin", "release/2026-Q3"]);
        bare
    }

    /// `git clone` of `bare` into `dest`, installed as a project that declares
    /// NO flow — which is what the current installer leaves behind.
    fn clone_project(bare: &std::path::Path, dest: &std::path::Path) {
        let parent = dest.parent().expect("dest parent");
        git(
            parent,
            &["clone", bare.to_string_lossy().as_ref(), dest.to_string_lossy().as_ref()],
        );
        git(dest, &["config", "user.email", "t@example.com"]);
        git(dest, &["config", "user.name", "t"]);
        std::fs::write(dest.join("mustard.json"), "{}").expect("cfg");
        std::fs::create_dir_all(dest.join(".claude")).expect("claude dir");
        std::fs::write(dest.join(".claude").join(".gitignore"), SHIPPED_SEED_GITIGNORE)
            .expect("ignore");
    }

    /// A repository carrying `dev` and a `release/2026-Q3` line, with the
    /// remote-tracking refs that make BOTH measurable — the catalogue an
    /// operator would be offered.
    ///
    /// **In the CLONE shape**, deliberately: the release line is created,
    /// published into `refs/remotes/origin/`, and then its LOCAL head is
    /// deleted. That is exactly what an operator's clone looks like — `git
    /// clone` materialises `refs/heads/` for the default branch alone, and
    /// every other branch of the catalogue exists there as a remote-tracking
    /// ref only. A fixture that leaves the local head behind cannot tell a cut
    /// that honours the pick from one that merely found a branch of that name
    /// lying around locally.
    ///
    /// `flow` is written verbatim as `git.flow`; `None` writes a `mustard.json`
    /// with no `git` key at all, which is what the current installer leaves.
    fn seed_repo_declaring(root: &std::path::Path, flow: Option<&str>) {
        let cfg = match flow {
            Some(flow) => format!(r#"{{"git":{{"flow":{flow}}}}}"#),
            None => "{}".to_string(),
        };
        std::fs::write(root.join("mustard.json"), cfg).expect("cfg");
        std::fs::create_dir_all(root.join(".claude")).expect("claude dir");
        std::fs::write(root.join(".claude").join(".gitignore"), SHIPPED_SEED_GITIGNORE)
            .expect("ignore");
        git(root, &["init"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        git(root, &["checkout", "-b", "dev"]);
        std::fs::write(root.join("f.txt"), "on dev").expect("seed");
        git(root, &["add", "-A"]);
        git(root, &["commit", "-m", "dev"]);
        git(root, &["checkout", "-b", "release/2026-Q3"]);
        std::fs::write(root.join("f.txt"), "on the release line").expect("seed");
        git(root, &["add", "-A"]);
        git(root, &["commit", "-m", "release"]);
        git(root, &["checkout", "dev"]);
        for branch in ["dev", "release/2026-Q3"] {
            git(root, &["update-ref", &format!("refs/remotes/origin/{branch}"), branch]);
        }
        // …and now it is a CLONE: the picked base lives on the remote only.
        git(root, &["branch", "-D", "release/2026-Q3"]);
    }


    /// `git rev-parse <rev>` in `root` — test scaffolding only.
    fn git_rev(root: &std::path::Path, rev: &str) -> String {
        let out = mustard_core::platform::git::run(root, &["rev-parse", rev]);
        assert!(out.ok, "git rev-parse {rev} failed");
        out.stdout.trim().to_string()
    }

    /// A repo declaring `dev → qas → main`, each base a real local branch at a
    /// DISTINCT commit, checked out on `dev`. The distinct tips are what make
    /// "cut from `qas`" a provable claim rather than a coincidence.
    fn seed_three_tier_repo(root: &std::path::Path) {
        std::fs::write(
            root.join("mustard.json"),
            r#"{"git":{"flow":{"*":"dev","dev":"qas","qas":"main"}}}"#,
        )
        .expect("cfg");
        std::fs::create_dir_all(root.join(".claude")).expect("claude dir");
        std::fs::write(root.join(".claude").join(".gitignore"), SHIPPED_SEED_GITIGNORE)
            .expect("ignore");
        git(root, &["init"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        git(root, &["checkout", "-b", "main"]);
        std::fs::write(root.join("f.txt"), "on main").expect("seed");
        git(root, &["add", "-A"]);
        git(root, &["commit", "-m", "main"]);
        for (branch, body) in [("qas", "on qas"), ("dev", "on dev")] {
            git(root, &["checkout", "-b", branch]);
            std::fs::write(root.join("f.txt"), body).expect("seed");
            git(root, &["add", "-A"]);
            git(root, &["commit", "-m", branch]);
        }
    }

    /// The validation that replaced "is it in `git.flow`?": is it a branch the
    /// REMOTE really has. Same loudness, opposite source — a typo is still
    /// caught, and a branch cut this morning is not.
    #[test]
    fn resolve_kind_base_validates_against_the_real_catalogue() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let run = |args: &[&str]| mustard_core::platform::git::run(root, args).ok;
        if !run(&["init", "-q", "-b", "main", "."]) {
            return;
        }
        let _ = run(&["config", "user.email", "t@t.t"]);
        let _ = run(&["config", "user.name", "t"]);
        if !run(&["commit", "-q", "--allow-empty", "-m", "seed"]) {
            return;
        }
        // The catalogue reads remote-tracking refs, so the fixture states them.
        for branch in ["main", "dev", "release/2026-Q3"] {
            let _ = run(&["update-ref", &format!("refs/remotes/origin/{branch}"), "HEAD"]);
        }

        let config = two_tier(); // declares dev and main, and NOT the release line
        assert_eq!(
            super::resolve_kind_base(root, Some("dev"), &config).as_deref(),
            Ok("dev"),
            "a declared branch resolves, as it always did",
        );
        assert_eq!(
            super::resolve_kind_base(root, Some("release/2026-Q3"), &config).as_deref(),
            Ok("release/2026-Q3"),
            "and so does one no configuration ever mentioned — the whole point",
        );
        assert_eq!(
            super::resolve_kind_base(root, Some("   "), &config).as_deref(),
            Ok(config.git.primary_base().expect("o fluxo declara uma base").as_str()),
            "blank counts as omitted",
        );

        let err = super::resolve_kind_base(root, Some("dve"), &config)
            .expect_err("a branch the remote does not have is refused");
        assert!(err.contains("dve"), "names the rejected base: {err}");
        assert!(err.contains("dev"), "and lists what is really there: {err}");
        assert!(
            !err.contains("git.flow"),
            "and no longer points at a configuration file as the fix: {err}",
        );
    }

    // -----------------------------------------------------------------------
    // The cut itself — the door that opens FIRST
    // -----------------------------------------------------------------------

    /// Run git in `root`, asserting success — test scaffolding only.
    fn git(root: &std::path::Path, args: &[&str]) {
        assert!(mustard_core::platform::git::run(root, args).ok, "git {args:?} failed");
    }

    /// `.claude/.gitignore` EXACTLY as it shipped in
    /// `packages/core/templates/.gitignore` BEFORE this unit widened it — the
    /// eight rules, with no `.session/` entry.
    ///
    /// This is the file every ALREADY-INSTALLED project still has, and it is
    /// the whole point of the fixture. `seed_gitignore` is `overwrite: false`
    /// and `mustard init` offers a "Merge (keep my files)" choice, so widening
    /// the template reaches new installs only; a probe whose correctness came
    /// from the widened rules would be right in this repository and wrong in
    /// every project already in the field. The fixtures used to hand-write a
    /// ROOT `.gitignore` covering `.claude/.session/` — a rule no shipped seed
    /// ever wrote — which is how they passed while the field refused.
    const SHIPPED_SEED_GITIGNORE: &str = "# Mustard harness scratch — runtime state, not versioned.\n\
         .cache/\n.harness/\n.metrics/\n.agent-state/\n.pipeline-states/\n\n\
         # Work-unit worktrees (created by `work-unit-open`, pruned by git-settle).\n\
         worktrees/\n\n\
         # Per-spec event log + blob spill.\n\
         spec/*/.events/\nspec/*/.blobs/\n";

    /// A repo on `dev` (flow `{*: dev, dev: main}`) with one commit carrying
    /// `mustard.json`, the SHIPPED `.claude/.gitignore` and a seed source file
    /// — the shape an already-installed project really has.
    fn seed_repo(root: &std::path::Path) {
        std::fs::write(
            root.join("mustard.json"),
            r#"{"git":{"flow":{"*":"dev","dev":"main"}}}"#,
        )
        .expect("cfg");
        let claude = root.join(".claude");
        std::fs::create_dir_all(&claude).expect("claude dir");
        std::fs::write(claude.join(".gitignore"), SHIPPED_SEED_GITIGNORE).expect("ignore");
        git(root, &["init"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        git(root, &["checkout", "-b", "dev"]);
        std::fs::write(root.join("f.txt"), "seed").expect("seed");
        git(root, &["add", "-A"]);
        git(root, &["commit", "-m", "init"]);
    }

    /// The path a unit's own spec lives at — the shape the field really has.
    const FIRST_UNIT_SPEC: &str = ".claude/spec/first-unit/spec.md";

    /// Put a FIRST unit on the checkout with the uncommitted work the field
    /// actually carries: its own `.claude/spec/…`, tracked and modified. Between
    /// approval and the merge that IS the unit's work — the spec, the waves, the
    /// proof, the change log and the review verdicts all live in the branch and
    /// are integrated at merge time — and a probe that drops `.claude/` sees an
    /// empty tree here.
    fn a_first_unit_holds_the_checkout(root: &std::path::Path) {
        git(root, &["checkout", "-b", "dev_first"]);
        let spec = root.join(".claude").join("spec").join("first-unit");
        std::fs::create_dir_all(&spec).expect("spec dir");
        std::fs::write(spec.join("spec.md"), "# first unit\n").expect("spec");
        git(root, &["add", "-A"]);
        git(root, &["commit", "-m", "first unit: draft"]);
        // …and then the unit keeps working, uncommitted, exactly as it does
        // between one commit and the next.
        std::fs::write(spec.join("spec.md"), "# first unit\n\nuncommitted\n").expect("dirty");
    }

    /// The CUT itself refuses a busy checkout.
    ///
    /// This test deliberately drives [`super::cut_pending_work_branch`] and NOT
    /// the old write-hook gate: the previous round's tests all went through
    /// that gate and passed while the real defect sat here. `spec-draft` calls
    /// this function at APPROVAL — before any `Write` exists for a PreToolUse
    /// hook to see — so a guard living only in the gate was a guard on the door
    /// that opens second.
    ///
    /// The work at risk is the shape the FIELD has: the first unit's own
    /// `.claude/spec/…`, tracked and modified. A source file made this pass
    /// while the live checkout — three modified spec files and nothing else —
    /// was read as clean, because the probe dropped every `.claude/` path.
    #[test]
    fn the_branch_cut_itself_refuses_a_busy_checkout() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let root_s = root.to_string_lossy().to_string();
        seed_repo(root);
        a_first_unit_holds_the_checkout(root);

        // A SECOND unit is signalled — this is what `spec-draft` consumes.
        let sid = "sess-cut-refuses";
        crate::shared::context::set_pending_branch(&root_s, sid, "dev_second", None);

        let outcome = super::cut_pending_work_branch(root, sid);
        let super::CutOutcome::Refused(busy) = outcome else {
            panic!("the cut must refuse a busy checkout, got {outcome:?}");
        };
        assert_eq!(busy.current, "dev_first");
        assert_eq!(busy.target, "dev_second");
        let super::CheckoutWork::Holds { theirs: dirty, .. } = &busy.work else {
            panic!("the paths were positively observed, got {:?}", busy.work);
        };
        assert!(
            dirty.iter().any(|p| p == FIRST_UNIT_SPEC),
            "the unit's own spec is its uncommitted work: {dirty:?}",
        );
        let reason = busy.reason(mustard_core::platform::i18n::Locale::EnUs);
        assert!(
            reason.contains("dev_first")
                && reason.contains("dev_second")
                && reason.contains(FIRST_UNIT_SPEC),
            "the refusal names both branches and the work at risk: {reason}",
        );

        // Nothing was touched: the checkout still holds the first unit, its
        // uncommitted work is intact, and the second branch does not exist.
        assert_eq!(mustard_core::current_branch(root).as_deref(), Some("dev_first"));
        assert_eq!(
            std::fs::read_to_string(root.join(FIRST_UNIT_SPEC)).expect("read"),
            "# first unit\n\nuncommitted\n",
        );
        assert!(
            !super::local_branch_exists(root, "dev_second"),
            "a refused cut creates no branch",
        );
        // The marker SURVIVES — the unit was never started, so nothing was
        // consumed and the next attempt retries after the commit or stash.
        assert_eq!(
            crate::shared::context::pending_branch_for(&root_s, sid).as_deref(),
            Some("dev_second"),
            "a refusal consumes no intent",
        );
    }

    /// The other half of the same decision: a checkout the probe could
    /// NOT measure is refused too.
    ///
    /// "I could not measure" is not "there is nothing here". This caller's
    /// failure mode is that another unit's work rides a plain checkout onto a
    /// second branch, so an unanswerable probe has to refuse: that costs the
    /// operator one commit, while the opposite costs them their work. A corrupt
    /// index is the fixture because it is the real thing — `git status` exits
    /// 128 on it while `rev-parse HEAD` still names the branch, which is exactly
    /// the state where the old probe answered "clean".
    #[test]
    fn an_unmeasurable_checkout_is_refused_by_the_cut_too() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let root_s = root.to_string_lossy().to_string();
        seed_repo(root);
        git(root, &["checkout", "-b", "dev_first"]);

        // The index is unreadable: `git status` cannot answer for this tree.
        std::fs::write(root.join(".git").join("index"), "not-an-index").expect("corrupt");
        assert_eq!(
            super::checkout_work(root),
            super::CheckoutWork::Unproven,
            "precondition: the probe really cannot answer here",
        );
        assert_eq!(
            mustard_core::current_branch(root).as_deref(),
            Some("dev_first"),
            "precondition: the POSITION is still readable — only the WORK is not",
        );

        let sid = "sess-cut-unmeasured";
        crate::shared::context::set_pending_branch(&root_s, sid, "dev_second", None);
        let outcome = super::cut_pending_work_branch(root, sid);
        let super::CutOutcome::Refused(busy) = outcome else {
            panic!("an unmeasurable checkout must be refused, got {outcome:?}");
        };
        assert_eq!(busy.work, super::CheckoutWork::Unproven);

        // The refusal SAYS it could not measure rather than naming an empty
        // list of paths, and still tells the operator what unblocks it.
        let reason = busy.reason(mustard_core::platform::i18n::Locale::EnUs);
        assert!(
            reason.contains("dev_first") && reason.contains("dev_second"),
            "both branches are named: {reason}",
        );
        let lower = reason.to_lowercase();
        assert!(lower.contains("not be measured"), "it says what it could not do: {reason}");
        assert!(lower.contains("stash"), "and what unblocks it: {reason}");

        // Nothing was touched.
        assert_eq!(mustard_core::current_branch(root).as_deref(), Some("dev_first"));
        assert!(
            !super::local_branch_exists(root, "dev_second"),
            "a refused cut creates no branch",
        );
    }

    /// The counterweight: with the SAME arrangement minus the uncommitted work,
    /// the cut proceeds. Nothing rides along from a clean tree, so refusing
    /// there would be friction with no defect behind it.
    ///
    /// The fixture seeds the SHIPPED pre-change `.claude/.gitignore`
    /// ([`SHIPPED_SEED_GITIGNORE`]) — an already-installed project, which is
    /// every project in the field — so `.claude/.session/` is NOT ignored by
    /// anything. The pending marker this very call consumes therefore reaches
    /// `git status`, and only the probe's OWN scratch list keeps it from
    /// reading as somebody's uncommitted work. Against the old probe, which
    /// delegated that judgement to the project's `.gitignore`, this refuses the
    /// cut over the marker the gate itself just wrote.
    #[test]
    fn a_clean_checkout_lets_the_cut_through() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let root_s = root.to_string_lossy().to_string();
        seed_repo(root);
        git(root, &["checkout", "-b", "dev_first"]);
        std::fs::write(root.join("f.txt"), "first unit, committed").expect("work");
        git(root, &["add", "-A"]);
        git(root, &["commit", "-m", "first unit work"]);

        let sid = "sess-cut-clean";
        crate::shared::context::set_pending_branch(&root_s, sid, "dev_second", None);
        // Precondition: the marker really is on disk and really is invisible to
        // this project's ignore rules — otherwise the assertion below would
        // pass for the reason the field does not have.
        assert!(
            root.join(".claude").join(".session").join(sid).join("pending-work-branch").is_file(),
            "precondition: the gate's own marker is written",
        );
        assert_eq!(
            super::checkout_work(root),
            super::CheckoutWork::ProvenClean,
            "the marker under `.claude/.session/` is the harness's own scratch — \
             the probe knows that itself, without asking the project's .gitignore",
        );
        let outcome = super::cut_pending_work_branch(root, sid);
        assert_eq!(outcome, super::CutOutcome::Cut("dev_second".to_string()), "{outcome:?}");
        assert_eq!(mustard_core::current_branch(root).as_deref(), Some("dev_second"));
    }

    /// The classifier's TRUNCATION rule, stated once where it can be read.
    ///
    /// git reports a directory whenever it will not enumerate what is inside
    /// it, and such an entry stands for everything below. The rule: scratch
    /// only when the WHOLE directory is scratch by name; work whenever it could
    /// hold anything else. A bare `.claude/` is the case that decides the
    /// posture — it holds the harness's droppings AND the unit's spec, and this
    /// probe measured neither, so it counts.
    #[test]
    fn a_truncated_directory_only_passes_when_all_of_it_is_scratch() {
        // A árvore só importa para os moldes: todo o resto se decide no nome.
        // UM molde gerado (`source: scan`, o único valor que declara censo), um
        // ADOTADO (`source: manual`) e uma skill de projeto escrita à mão SEM
        // chave `source:` — os dois últimos no bloco de trabalho.
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let mold = |path: &str, body: &str| {
            let full = root.join(path);
            std::fs::create_dir_all(full.parent().expect("parent")).expect("mold dir");
            std::fs::write(&full, body).expect("mold file");
        };
        mold(
            "apps/rt/.claude/skills/rt-gate-pattern/SKILL.md",
            "---\nname: rt-gate-pattern\nsource: scan\n---\n\n## Purpose\n",
        );
        mold(
            ".claude/skills/my-helper/SKILL.md",
            "---\nname: my-helper\ndescription: Use when X.\n---\n\n## Purpose\n",
        );
        mold(
            "apps/rt/.claude/skills/rt-verdict-pattern/SKILL.md",
            "---\nname: rt-verdict-pattern\nsource: manual\n---\n\n## Purpose\n",
        );

        // Scratch, whole — including the collapsed-directory spelling git uses.
        for scratch in [
            ".claude/.session/sess-x/pending-work-branch",
            ".claude/.session/",
            ".claude/.cache/detect.json",
            ".claude/.harness/bus.json",
            ".claude/worktrees/",
            ".claude/knowledge/note.md",
            ".claude/feature-digest.json",
            ".claude/spec/my-unit/.events/log.ndjson",
            ".claude/spec/my-unit/.blobs/",
            ".claude/spec/my-unit/.dispatch/prompt.md",
            ".claude/spec/my-unit/.memory-approved",
            // The cut's own record of the base — written by this very module,
            // moments before the next cut probes the tree. Read as work, a
            // refusal here would be the harness refusing over its own droppings.
            ".claude/spec/my-unit/.cut-base",
            // A subproject's nested `.claude/` is the same harness.
            "apps/rt/.claude/.session/sess-y/pending-work-branch",
            // Windows separators, should git or a caller ever hand them over.
            ".claude\\.session\\sess-x\\pending-work-branch",
        ] {
            assert_eq!(
                super::classify_dirty_path(root, scratch),
                super::DirtyPathKind::Scratch,
                "scratch: {scratch}",
            );
        }

        // Censo — escrito pela ferramenta, versionado de propósito. Nem
        // rascunho (seria descartado de todo commit) nem trabalho (recusaria o
        // corte pela saída da própria ferramenta).
        for census in [
            ".claude/grain.model.json",
            ".claude/scan-declined.json",
            ".claude/scan-map.md",
            "apps/rt/.claude/scan-map.md",
            "apps/rt/.claude/skills/rt-gate-pattern/SKILL.md",
            ".claude\\scan-map.md",
        ] {
            assert_eq!(
                super::classify_dirty_path(root, census),
                super::DirtyPathKind::Census,
                "census: {census}",
            );
        }

        // Work — the unit's own artefacts, and every truncated directory that
        // could still be hiding one.
        for work in [
            ".claude/spec/my-unit/spec.md",
            ".claude/spec/my-unit/wave-plan.md",
            ".claude/spec/my-unit/ac-proof.json",
            ".claude/spec/my-unit/change-log.md",
            ".claude/spec/my-unit/review/findings-apps-rt.md",
            ".claude/spec/my-unit/wave-1-rt/spec.md",
            // THE case: `.claude/` entirely untracked, collapsed to one entry.
            // It stands for the scratch and the spec alike and this probe read
            // neither, so it counts — refusing costs a commit, the other
            // direction costs somebody their work.
            ".claude/",
            ".claude",
            ".claude/spec/",
            ".claude/spec/my-unit/",
            // Not under a `.claude/` segment at all.
            "src/.session/x",
            ".claudeignore",
            // O censo tem nome inteiro ou não é censo: um diretório truncado
            // que só COMEÇA em `skills/` não foi medido, e um `scan-map.md`
            // fora de um `.claude/` é de quem o escreveu.
            ".claude/skills/",
            ".claude/skills/rt-gate-pattern/",
            "docs/scan-map.md",
            ".claude/grain.model.json.bak",
            // O molde ADOTADO (`source: manual`) é escrita do OPERADOR: o nome
            // do arquivo é o mesmo de um gerado e o frontmatter é o que separa.
            "apps/rt/.claude/skills/rt-verdict-pattern/SKILL.md",
            // A skill de projeto ORDINÁRIA: escrita à mão em
            // `.claude/skills/<nome>/SKILL.md` — o lugar padrão do Claude Code —
            // e SEM chave `source:`, que é como quase toda skill à mão nasce. A
            // regra canônica diz humano; lê-la como censo faria o portão varrer
            // a escrita do operador para dentro de um commit da ferramenta.
            ".claude/skills/my-helper/SKILL.md",
            // Um molde que não existe no disco não pôde ser lido, e um que
            // abre frontmatter sem fechar não pôde ser interpretado — as duas
            // formas caem para o lado seguro.
            "apps/rt/.claude/skills/ausente-pattern/SKILL.md",
        ] {
            assert_eq!(
                super::classify_dirty_path(root, work),
                super::DirtyPathKind::Work,
                "work: {work}",
            );
        }

        // …e o frontmatter que abre, não declara `source:` e nunca fecha: não há
        // bloco a ler, e o lado seguro é dizer que ali tem trabalho.
        mold(".claude/skills/truncado-pattern/SKILL.md", "---\nname: truncado-pattern\n");
        assert_eq!(
            super::classify_dirty_path(root, ".claude/skills/truncado-pattern/SKILL.md"),
            super::DirtyPathKind::Work,
            "frontmatter sem fim não é bloco que se possa ler",
        );

        // O leitor é o CANÔNICO do core, então o BOM antes da cerca não muda
        // nada — a mesma tolerância que `is_mustard_generated` documenta.
        mold(
            ".claude/skills/bom-pattern/SKILL.md",
            "\u{feff}---\nname: bom-pattern\nsource: scan\n---\n\n## Purpose\n",
        );
        assert_eq!(
            super::classify_dirty_path(root, ".claude/skills/bom-pattern/SKILL.md"),
            super::DirtyPathKind::Census,
            "um molde gerado com BOM não deixa de ser gerado",
        );
    }

    /// The other side of the same list: in that SAME seeded shape, a real
    /// `.claude/spec/<unit>/spec.md` still counts.
    ///
    /// The fix names the harness's scratch and nothing else, so it cannot
    /// degrade into "ignore all of `.claude/` again" — the carve-out the
    /// previous round removed, which read the normal state of an in-flight unit
    /// (its spec, its waves, its proof) as an empty tree. Both files sit under
    /// `.claude/` in one untracked tree here, which is exactly the shape git
    /// collapses into a single `?? .claude/` entry: the verdict has to separate
    /// them anyway.
    #[test]
    fn a_seeded_project_still_counts_the_units_own_spec_as_work() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let root_s = root.to_string_lossy().to_string();
        seed_repo(root);
        git(root, &["checkout", "-b", "dev_first"]);

        // The first unit's own spec, never committed — its work.
        let spec = root.join(".claude").join("spec").join("first-unit");
        std::fs::create_dir_all(&spec).expect("spec dir");
        std::fs::write(spec.join("spec.md"), "# first unit\n").expect("spec");

        let sid = "sess-cut-spec-counts";
        crate::shared::context::set_pending_branch(&root_s, sid, "dev_second", None);

        let super::CheckoutWork::Holds { theirs: dirty, .. } = super::checkout_work(root) else {
            panic!("the unit's own spec is uncommitted work: {:?}", super::checkout_work(root));
        };
        assert!(
            dirty.iter().any(|p| p == FIRST_UNIT_SPEC),
            "the spec is named: {dirty:?}",
        );
        assert!(
            !dirty.iter().any(|p| p.contains(".session")),
            "the harness's own marker is not somebody's work: {dirty:?}",
        );

        let outcome = super::cut_pending_work_branch(root, sid);
        let super::CutOutcome::Refused(busy) = outcome else {
            panic!("a checkout holding another unit's spec must be refused, got {outcome:?}");
        };
        assert!(
            busy.reason(mustard_core::platform::i18n::Locale::EnUs).contains(FIRST_UNIT_SPEC),
            "the refusal names the work at risk",
        );
        assert_eq!(mustard_core::current_branch(root).as_deref(), Some("dev_first"));
    }

    /// Com o checkout solto, a branch atual responde "nenhuma", e não o texto
    /// que o git dá no lugar do nome; numa branch sem nenhum commit, também.
    /// Sem branch, o checkout não conta como trabalho de outra unidade.
    #[test]
    fn the_current_branch_is_none_on_a_detached_checkout() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        seed_repo(root);
        git(root, &["checkout", "-q", "-b", "dev_first"]);
        assert_eq!(mustard_core::current_branch(root).as_deref(), Some("dev_first"));

        git(root, &["checkout", "-q", "--detach"]);
        let detached = mustard_core::current_branch(root);
        assert_eq!(detached, None, "a detached checkout stands on no branch");
        let config = mustard_core::ProjectConfig::load(root);
        assert!(!super::holds_other_work(root, detached.as_deref(), "dev_second", &config));

        git(root, &["checkout", "-q", "--orphan", "vazia"]);
        assert_eq!(mustard_core::current_branch(root), None, "a branch without any commit");
    }
}

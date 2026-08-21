//! `base_gate` — the check that runs BEFORE ANALYZE, at the single
//! pipeline-opening door (`emit-pipeline --kind pipeline.kind`).
//!
//! ## What it guards
//!
//! A work unit is the branch plus everything the work produces, so the unit is
//! only coherent if the branch is cut from a base the project actually promotes
//! through, at that base's LATEST commit. Both facts are cheap to establish
//! exactly once — before ANALYZE reads a single file — and expensive to
//! discover later: a unit cut off another unit cannot be reviewed apart, and a
//! unit cut off a stale base re-does work that is already merged.
//!
//! Two answers, never more ([`BaseVerdict`]):
//!
//! 1. **Behind its remote** → `Refuse`, naming the exact pull to run.
//! 2. Otherwise → `Open`, and the census refresh fires when it is due.
//!
//! There is no membership answer any more. "Not an integration base" used to be
//! the first of three, tested against `git.flow`'s declared set — which refused
//! a branch cut last Tuesday with a sentence about a configuration file, in a
//! repository whose branch convention the operator does not own. What a base IS
//! is now measured where it can be: the cut point is every branch git has
//! ([`mustard_core::branch_catalog`]) and the branches that refuse a direct
//! commit are [`mustard_core::protected_branches`]. See the `evaluate` body for
//! what that deliberately gives up.
//!
//! ## Abstention is not a pass
//!
//! `Abstain` is a fourth state kept deliberately apart from `Open`: an explicit
//! `vcs: ""` opt-out, a directory that is not a repository, a git that would
//! not answer. The gate did not run — it did not approve, and the caller must
//! not read it as one. Only a POSITIVE observation ever refuses, so the gate
//! can never wedge a project it cannot reason about (the same invariant
//! [`crate::hooks::write::scan_clean_gate`] states for itself).
//!
//! **Offline is not a verdict either.** Freshness needs the network; when the
//! fetch fails there is no evidence the base is behind, so the gate opens.
//! Refusing there would ground every offline session on a fact nobody measured.
//!
//! ## Why the census refresh lives here
//!
//! In a SHARED install `/scan` rewrites VERSIONED artifacts — the grain model,
//! its dictionary — so it needs a clean tree to stay its own reviewable commit;
//! that is precisely what `scan_clean_gate` refuses to let happen on a dirty
//! one. A freshly updated base, before the first edit, is the one moment in the
//! flow where a clean tree holds by construction, which is why the refresh is
//! triggered from this gate instead of from a door the user has to remember. It
//! is best-effort throughout: a stale census is a worse map, never a blocker.
//!
//! And it FINISHES that commit rather than announcing it: on a tree the refresh
//! itself found clean, the re-mined census is recorded on the spot
//! ([`record_census`]). Leaving it dirty made the next unit's branch cut refuse,
//! attributing the write to another unit of the operator's work — one manual
//! commit per pipeline opened.
//!
//! In a PRIVATE install the census is invisible to the host repository's git,
//! so there is no commit to keep apart and the tree's state decides nothing —
//! staleness alone is the whole question. Both readings come from the ONE
//! predicate [`crate::hooks::write::scan_clean_gate::scan_output_is_versioned`],
//! shared with the door that refuses, so the automatic path can never start
//! mining exactly where the user-invoked one is turned away.

use std::path::Path;

use mustard_core::{
    record_written_path, worktree_is_clean, ProjectConfig, RecordOutcome, Scan,
};

use crate::commands::git_settle::git_out;
use crate::commands::scan::{default_model_path, hollow_submodules};
use crate::hooks::write::scan_clean_gate::{scan_output_is_versioned, tree_is_dirty};
use crate::util::format_gate_message;

/// The closed set of answers the base gate can return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BaseVerdict {
    /// The checkout IS an integration base and carries no commit its remote
    /// has already published. The pipeline may open; the named base is the one
    /// the unit will be cut from.
    Open(String),
    /// Nothing to judge — VCS opt-out, not a repository, or a branch probe that
    /// did not answer. Never an approval: the gate simply did not run.
    Abstain,
    /// The pipeline must NOT open here. Carries the didactic refusal, which
    /// always names the command that resolves it.
    Refuse(String),
}

/// Judge the current checkout as a base to cut a unit from.
///
/// One question survives: is it up to date with its remote? A unit cut from a
/// stale base re-does merged work and conflicts on the way back, and NO branch
/// convention protects against that — which is why this is the check that
/// stayed when the membership test went.
///
/// `project` is the state root (where `mustard.json` and `.claude/` live) and
/// also the tree the branch is read from — opening a NEW pipeline from inside
/// a work unit's own worktree is exactly the case this gate exists to refuse,
/// so there is no local-tree redirect here.
pub(crate) fn evaluate(project: &Path, config: &ProjectConfig) -> BaseVerdict {
    // An explicit `vcs: ""` opt-out means the project declined branch
    // management altogether; there is no base to be on.
    if config.vcs().is_none() {
        return BaseVerdict::Abstain;
    }
    let Some(current) = git_out(project, &["rev-parse", "--abbrev-ref", "HEAD"])
        .map(|b| b.trim().to_string())
        .filter(|b| !b.is_empty())
    else {
        // Not a repository, no git on PATH, an unborn HEAD — unmeasured.
        return BaseVerdict::Abstain;
    };

    // NO membership test any more. It used to read the declared base set and
    // refuse everything outside it, which meant a branch cut last Tuesday was
    // told it "is not an integration base of this project" — a sentence about a
    // configuration file, delivered as if it were a sentence about the
    // repository. In a client repository, where
    // the operator does not own the branch convention, the only offered way out
    // was to edit that file per project.
    //
    // What is DELIBERATELY given up: the gate no longer distinguishes a base
    // from another unit's work branch, so it can no longer refuse a unit cut
    // off another unit. That refusal was only ever possible because the base
    // set was closed, and a closed set is exactly what made the common case
    // wrong. Stacking a unit on another branch is legitimate in the flows this
    // opens up for; the picker shows what each candidate IS, and the choice is
    // the operator's. The safety that survives is the one no convention can
    // supply for itself — see below.
    match commits_behind_remote(project, &current) {
        Some(behind) if behind > 0 => BaseVerdict::Refuse(behind_reason(&current, behind)),
        // `None` = unmeasured (offline, no remote-tracking ref): open.
        _ => BaseVerdict::Open(current),
    }
}

/// Gate title every refusal carries — the `[Base Gate]` prefix
/// [`format_gate_message`] renders.
const GATE: &str = "Base Gate";

/// The refusal for a base that trails its remote, naming the exact pull.
fn behind_reason(base: &str, behind: u64) -> String {
    let plural = if behind == 1 { "commit" } else { "commits" };
    format_gate_message(
        GATE,
        &format!("the integration base '{base}' is {behind} {plural} behind origin/{base}"),
        "a unit cut from a stale base re-does work that is already merged and conflicts \
         on the way back",
        &format!("git pull --ff-only origin {base}"),
    )
}

/// How many commits `origin/<base>` carries that the checkout does not.
///
/// `None` whenever the question could not be answered — the fetch failed
/// (offline, no remote), or there is no `origin/<base>` ref to compare with.
/// The caller reads that as "unmeasured" and opens; see the module doc.
fn commits_behind_remote(project: &Path, base: &str) -> Option<u64> {
    // Refresh the remote-tracking refs first: without it the count is measured
    // against whatever the last fetch left behind, which is exactly the stale
    // reading this check exists to catch.
    git_out(project, &["fetch", "origin"])?;
    let range = format!("HEAD..origin/{base}");
    git_out(project, &["rev-list", "--count", &range])?.trim().parse::<u64>().ok()
}

/// `true` when the deterministic census is worth re-mining AND re-mining it
/// can still be a commit of its own — the conjunction the gate acts on.
///
/// Split out of [`refresh_census_if_stale`] so the DECISION is testable without
/// the grain sidecar binary: the effect needs it, the judgement does not.
pub(crate) fn census_refresh_due(project: &Path, model: &Path) -> bool {
    if !census_is_stale(project, model) {
        return false;
    }
    // A census git cannot see never fuses with the user's work, so staleness is
    // the whole question there. Without this, a client repository — dirty
    // nearly always — would carry a census that silently never refreshed.
    //
    // The question is asked of the FILES, not of the install mode. The mode
    // predicate reads "private install ⇒ invisible", and this very repository
    // falsifies it: it carries both private marks in `info/exclude` AND a
    // tracked census. Under the coarse answer the gate re-mined on a dirty tree
    // and then had to leave the versioned result uncommitted — the debt-
    // admission this whole unit exists to delete. `record_written_path` already
    // judges per path; this now asks the same fact of the same paths, so the
    // two halves of one decision can no longer disagree.
    if !census_is_visible_to_git(project, model) {
        return true;
    }
    // Shared install: only a POSITIVE clean tree qualifies. `None` (no git,
    // unreadable status) is unmeasured, and a refresh mined over unknown dirt
    // is exactly what `scan_clean_gate` refuses for the user-invoked door.
    tree_is_dirty(project) == Some(false)
}

/// `true` when the census on disk describes an older tree than the one checked
/// out: it is absent, or HEAD's commit is newer than the model file.
///
/// The commit date is the honest clock here. A working-tree mtime sweep would
/// re-mine after every checkout touch, and a content hash costs a full walk —
/// the thing the refresh itself is trying to earn. Unreadable either side ⇒
/// `false`: with no evidence the tree moved, a full workspace walk is not
/// something to spend on a guess.
fn census_is_stale(project: &Path, model: &Path) -> bool {
    if !model.is_file() {
        return true;
    }
    let Some(head_committed_at) = git_out(project, &["log", "-1", "--format=%ct", "HEAD"])
        .and_then(|s| s.trim().parse::<u64>().ok())
    else {
        return false;
    };
    let Some(model_written_at) = model
        .metadata()
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
    else {
        return false;
    };
    head_committed_at > model_written_at
}

/// Re-mine `<project>/.claude/grain.model.json` when [`census_refresh_due`]
/// says so. Deterministic census only — the `--full` pass that rewrites every
/// `scan-map.md` and each subproject's `## Guards` stays with the FLOW, which
/// dispatches it as a work unit of its own once this gate reports the gap
/// ([`super::enrichment_gap`]). It is not an explicit door the user types: that
/// one was sealed, and rewriting versioned files needs a clean tree and a commit
/// apart, which is a unit, not a side effect of opening another one.
///
/// Fail-open at every step, and loud on stderr rather than on stdout: this runs
/// inside `emit-pipeline`, whose one JSON line is byte-compared by gates.
///
/// The refresh RECORDS itself. Where the census is versioned, re-mining it used
/// to leave the file dirty and say so — a task handed to the operator that the
/// product never finished, and that the next unit's branch cut then refused,
/// blaming the write on another unit of their work. The tree is sampled before
/// the miner runs, so the refresh can tell its own change apart from theirs.
pub(crate) fn refresh_census_if_stale(project: &Path) {
    let model = default_model_path(project);
    if !census_refresh_due(project, &model) {
        return;
    }
    // The same preflight `scan` runs: an unpopulated submodule is
    // indistinguishable from an absent subtree once the walk starts, and the
    // previous complete model is strictly better than a hollow replacement.
    let hollow = hollow_submodules(project);
    if !hollow.is_empty() {
        eprintln!(
            "base-gate: census refresh skipped — empty submodule(s) {}; the model would \
             silently omit them. Run: git submodule update --init --recursive",
            hollow.join(", ")
        );
        return;
    }
    // Sampled BEFORE the miner writes a byte: the only moment at which the
    // operator's work and what this refresh is about to write can still be told
    // apart. Read by `record_census` below.
    let found_clean = worktree_is_clean(project);
    match Scan::locate().scan(project, &model) {
        Ok(()) => {
            let path = model.display();
            match record_census(project, &model, found_clean) {
                RecordOutcome::Recorded => eprintln!(
                    "base-gate: census refreshed ({path}) and recorded on this base — the tree \
                     is clean again"
                ),
                RecordOutcome::Nothing => eprintln!(
                    "base-gate: census refreshed ({path}) — this repository's git has nothing \
                     to see"
                ),
                RecordOutcome::TreeNotClean => eprintln!(
                    "base-gate: census refreshed ({path}) — the tree already carried your work, \
                     so it was left for you to commit alongside it"
                ),
                RecordOutcome::Unavailable => eprintln!(
                    "base-gate: census refreshed ({path}) — git would not record it, so it \
                     stays uncommitted"
                ),
            }
        }
        Err(e) => eprintln!("base-gate: census refresh failed ({e}); the previous model stands"),
    }
}

/// The commit subject the gate writes when it records a census it re-mined.
///
/// Deliberately plain: it describes the file that changed and names no tool.
/// The commit lands in the OPERATOR's history, next to their own work.
const CENSUS_COMMIT_SUBJECT: &str = "chore: refresh the deterministic project census";

/// The scan's second versioned artifact, written beside the model on every run.
/// Named here because the recording has to cover everything the miner wrote:
/// leaving it out left the tree dirty under a message claiming it was clean.
const GRAIN_DICTIONARY: &str = "grain.dictionary.json";

/// Record the census this gate just re-mined, through the ONE mechanism the
/// product uses for every artifact it writes into a tree the host repository
/// versions ([`record_written_path`]).
///
/// Split out of [`refresh_census_if_stale`] for the same reason
/// [`census_refresh_due`] is: the RECORDING is testable without the grain
/// sidecar binary — the effect needs it, the bookkeeping does not.
/// Whether git would SEE the census — asked of the files, not of the install
/// mode.
///
/// Visible means "no ignore rule hides it", which is the same fact
/// [`record_written_path`] judges when it decides whether a write is worth
/// recording. Tracked would be the wrong question: a first mine is untracked by
/// definition and still shows up as `??`, so answering "invisible" there would
/// re-mine onto a dirty tree and leave exactly the dirt this unit removes.
///
/// Not the install mode either. That predicate reads "private install ⇒
/// invisible", and this very repository falsifies it: it carries private marks
/// in `info/exclude` AND a tracked census. Under the coarse answer the gate
/// re-mined on a dirty tree and then had to leave the versioned result
/// uncommitted — the debt-admission this unit exists to delete.
///
/// Unmeasured (no git, no repository) reads as INVISIBLE, the direction the
/// mode predicate also took: a census nobody can see is one staleness alone
/// should decide.
fn census_is_visible_to_git(project: &Path, model: &Path) -> bool {
    [model.to_path_buf(), model.with_file_name(GRAIN_DICTIONARY)]
        .iter()
        .filter_map(|p| p.strip_prefix(project).ok())
        .map(|rel| rel.to_string_lossy().replace('\\', "/"))
        .any(|rel| !path_is_ignored(project, &rel))
}

/// `true` when git would ignore `rel` — an ignore rule or the clone-local
/// exclude file a private install writes into; `check-ignore` reads both.
/// `false` when git could not answer, so an unmeasured path counts as visible
/// and the stricter clean-tree requirement applies.
pub(crate) fn path_is_ignored(project: &Path, rel: &str) -> bool {
    std::process::Command::new("git")
        .args(["check-ignore", "-q", "--", rel])
        .current_dir(project)
        .output()
        .is_ok_and(|out| out.status.success())
}

/// A scan writes TWO artifacts, not one: the model and its byte-stable
/// `grain.dictionary.json` sidecar beside it. Recording only the model left the
/// sidecar modified, so the tree stayed dirty and the next branch cut still
/// refused — while this gate printed that the tree was clean again. Both go in,
/// derived from the model path the same way the rest of the runtime derives the
/// sidecar (`scan.rs` uses `with_file_name` for exactly this).
fn record_census(project: &Path, model: &Path, found_clean: Option<bool>) -> RecordOutcome {
    // Each pathspec is DERIVED from a path that was really written, so no
    // pathspec can name a file the scan did not touch. Forward-slashed: a git
    // pathspec is not a Windows path.
    // Only paths that actually EXIST. `git add -- <a> <missing>` aborts
    // wholesale (`fatal: pathspec did not match any files`), so naming an
    // absent sidecar would take the model down with it and leave the tree dirty
    // — the outcome this function exists to prevent, reached by being thorough.
    let mut paths: Vec<String> = Vec::with_capacity(2);
    for written in [model.to_path_buf(), model.with_file_name(GRAIN_DICTIONARY)] {
        if !written.is_file() {
            continue;
        }
        let Ok(rel) = written.strip_prefix(project) else {
            return RecordOutcome::Unavailable;
        };
        paths.push(rel.to_string_lossy().replace('\\', "/"));
    }
    if paths.is_empty() {
        return RecordOutcome::Nothing; // the miner wrote nothing to record.
    }
    let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
    record_written_path(project, &refs, CENSUS_COMMIT_SUBJECT, found_clean)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// Run a git command in `root`, asserting success — test scaffolding only.
    fn git(root: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        assert!(ok, "git {args:?} failed");
    }

    /// A `dev`/`main` project config — the base set is derived, never hardcoded.
    fn flow_config() -> ProjectConfig {
        let mut config = ProjectConfig::default();
        config.git.flow.insert("*".to_string(), "dev".to_string());
        config.git.flow.insert("dev".to_string(), "main".to_string());
        config
    }

    /// Init a repo whose single commit lives on `base`.
    fn init_repo_on(root: &Path, base: &str) {
        git(root, &["init"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        git(root, &["checkout", "-b", base]);
        std::fs::write(root.join("f.txt"), "hi").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-m", "init"]);
    }

    /// AC-1 — the refusal this test used to assert is GONE, and its absence is
    /// the feature. A branch the project never declared is an ordinary base:
    /// `release/2026-Q3` is cut on a Tuesday and works the same afternoon,
    /// where before it was told it "is not an integration base of this
    /// project" — a sentence about a configuration file dressed up as a
    /// sentence about the repository.
    #[test]
    fn accepts_any_real_branch_as_base() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_repo_on(root, "release/2026-Q3");

        assert_eq!(
            evaluate(root, &flow_config()),
            BaseVerdict::Open("release/2026-Q3".to_string()),
            "a branch git really has is a base, declared or not",
        );
    }

    /// AC-6 — the compatibility half, and the reason `git.flow` was kept rather
    /// than deleted: a project that still declares one is not restricted BY it.
    /// The declaration survives as a hint for where a picker opens; it decides
    /// nothing here.
    #[test]
    fn a_declared_flow_preselects_without_refusing_others() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_repo_on(root, "squad-b/integration");

        let config = flow_config(); // declares dev and main, and neither is this
        assert_eq!(
            evaluate(root, &config),
            BaseVerdict::Open("squad-b/integration".to_string()),
            "an undeclared branch opens exactly like a declared one",
        );

        let declared = config.git.preselected_bases();
        assert!(
            declared.contains("dev") && !declared.contains("squad-b/integration"),
            "the flow still says what it always said — it just no longer refuses: {declared:?}",
        );
        assert_eq!(config.git.primary_base(), "dev", "and it still seeds the cursor");
    }

    /// Agnostic: a `develop`/`master` project judges against ITS bases — being
    /// on `develop` opens, and no `dev`/`main` literal is involved.
    #[test]
    fn opens_on_an_integration_base_of_any_flow() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_repo_on(root, "develop");

        let mut config = ProjectConfig::default();
        config.git.flow.insert("*".to_string(), "develop".to_string());
        config.git.flow.insert("develop".to_string(), "master".to_string());

        // No `origin` remote ⇒ freshness is unmeasured, which opens (offline is
        // not a verdict).
        assert_eq!(
            evaluate(root, &config),
            BaseVerdict::Open("develop".to_string()),
            "a bare integration base with no measurable remote opens",
        );
    }

    /// A base whose remote has moved ahead refuses, and the refusal spells the
    /// pull out — the whole point of measuring instead of warning.
    #[test]
    fn refuses_when_the_base_is_behind_origin_and_names_the_pull() {
        let tmp = tempfile::tempdir().unwrap();

        // A bare "remote" whose HEAD is `dev` (set explicitly — do not depend
        // on the git version's default-branch flag).
        let remote = tmp.path().join("remote.git");
        std::fs::create_dir_all(&remote).unwrap();
        let remote_s = remote.to_str().unwrap();
        git(&remote, &["init", "--bare"]);
        git(&remote, &["symbolic-ref", "HEAD", "refs/heads/dev"]);

        // A seed clone publishes the first `dev` commit.
        let seed = tmp.path().join("seed");
        std::fs::create_dir_all(&seed).unwrap();
        init_repo_on(&seed, "dev");
        git(&seed, &["remote", "add", "origin", remote_s]);
        git(&seed, &["push", "origin", "dev"]);

        // The project clone starts level with origin/dev...
        let proj = tmp.path().join("proj");
        std::fs::create_dir_all(&proj).unwrap();
        git(&proj, &["clone", remote_s, "."]);
        assert_eq!(
            evaluate(&proj, &flow_config()),
            BaseVerdict::Open("dev".to_string()),
            "level with its remote, the base opens",
        );

        // ...then origin/dev gains a commit this clone has never seen.
        std::fs::write(seed.join("f.txt"), "two").unwrap();
        git(&seed, &["add", "."]);
        git(&seed, &["commit", "-m", "two"]);
        git(&seed, &["push", "origin", "dev"]);

        let BaseVerdict::Refuse(reason) = evaluate(&proj, &flow_config()) else {
            panic!("a base behind its remote must refuse before ANALYZE");
        };
        assert!(reason.contains("behind origin/dev"), "says what it measured: {reason}");
        assert!(
            reason.contains("git pull --ff-only origin dev"),
            "names the pull command: {reason}",
        );
    }

    /// A directory that is not a repository, and an explicit `vcs: ""` opt-out,
    /// both ABSTAIN — the gate never blocks what it could not measure.
    #[test]
    fn abstains_without_a_repository_or_with_vcs_opted_out() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            evaluate(dir.path(), &flow_config()),
            BaseVerdict::Abstain,
            "not a repository — unmeasured, never refused",
        );

        let repo = tempfile::tempdir().unwrap();
        init_repo_on(repo.path(), "dev_unit");
        let mut opted_out = flow_config();
        opted_out.vcs = Some(String::new());
        assert_eq!(
            evaluate(repo.path(), &opted_out),
            BaseVerdict::Abstain,
            "an explicit vcs opt-out has no base to be on",
        );
    }

    /// The hidden-census reading of the same decision: a census no git can see
    /// has no commit of its own to keep apart from the dirt, so a dirty tree
    /// disqualifies nothing and staleness alone decides. Without this the
    /// census on a client repository silently never refreshed — the tree there
    /// is dirty nearly always.
    ///
    /// The fixture excludes the CENSUS, not merely the two marks that DETECT a
    /// private install (`settings.local.json`, `CLAUDE.local.md`). A real
    /// private install excludes both census artifacts, and writing only the
    /// marks modelled an install that does not exist: the census stayed plainly
    /// visible while the test asserted it was hidden. That gap is why the
    /// decision now asks whether git can SEE these files instead of which mode
    /// the install is in — this repository carries the marks AND a tracked
    /// census, and the coarse answer sent it down the wrong branch.
    #[test]
    fn a_hidden_census_refreshes_on_a_dirty_tree() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_repo_on(root, "dev");
        let model = default_model_path(root);

        let info = root.join(".git").join("info");
        std::fs::create_dir_all(&info).unwrap();
        let mut rules: Vec<String> =
            mustard_core::PRIVATE_MARKS.iter().map(|m| (*m).to_string()).collect();
        rules.push(".claude/grain.model.json".to_string());
        rules.push(".claude/grain.dictionary.json".to_string());
        std::fs::write(info.join("exclude"), rules.join("\n") + "\n").unwrap();

        std::fs::write(root.join("stray.txt"), "x").unwrap();
        assert!(
            census_refresh_due(root, &model),
            "a census git cannot see has no commit of its own to keep apart from the dirt",
        );
    }

    /// …and the counter-case the old mode predicate got wrong. Private marks
    /// present, census NOT excluded — this repository's own shape. The census
    /// is visible, so a dirty tree must postpone the re-mine rather than mine
    /// into it and leave a versioned file uncommitted.
    #[test]
    fn a_visible_census_postpones_the_refresh_on_a_dirty_tree() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_repo_on(root, "dev");
        let model = default_model_path(root);

        let info = root.join(".git").join("info");
        std::fs::create_dir_all(&info).unwrap();
        std::fs::write(info.join("exclude"), mustard_core::PRIVATE_MARKS.join("\n") + "\n")
            .unwrap();

        std::fs::write(root.join("stray.txt"), "x").unwrap();
        assert!(
            !census_refresh_due(root, &model),
            "the marks say `private` but nothing hides the census: mining here would leave \
             a versioned file dirty, which is the debt this unit removes",
        );
    }

    /// The refresh decision is the CONJUNCTION: an absent model on a clean tree
    /// is due; the same absent model on a dirty tree is not, because the refresh
    /// could no longer be committed apart from the user's work.
    #[test]
    fn census_refresh_needs_both_staleness_and_a_clean_tree() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_repo_on(root, "dev");
        let model = default_model_path(root);

        assert!(
            census_refresh_due(root, &model),
            "no model at all on a clean tree is the clearest possible staleness",
        );

        // Dirty the tree with a file `git add -A` would stage.
        std::fs::write(root.join("stray.txt"), "x").unwrap();
        assert!(
            !census_refresh_due(root, &model),
            "a dirty tree fuses the refresh with the user's work — never mine there",
        );

        // Clean again, with the model written AFTER the last commit: the census
        // already describes this tree, so there is nothing to re-mine. The
        // commit lands FIRST on purpose — `%ct` has one-second resolution, so
        // writing the model afterwards is what makes the comparison decidable
        // instead of a race with the clock.
        std::fs::remove_file(root.join("stray.txt")).unwrap();
        std::fs::write(root.join(".gitignore"), ".claude/\n").unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-m", "ignore claude"]);
        std::fs::create_dir_all(model.parent().unwrap()).unwrap();
        std::fs::write(&model, "{}").unwrap();
        assert!(
            !census_refresh_due(root, &model),
            "a model newer than HEAD is not stale: {}",
            model.display(),
        );
    }

    /// `git status --porcelain` for `root` — the tree as the NEXT command's
    /// clean-tree guard will read it.
    fn porcelain(root: &Path) -> String {
        let out = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(root)
            .output()
            .expect("git status");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A repo on `dev` whose `.claude/grain.model.json` is TRACKED and
    /// committed — the shape this repository has, and the only one where a
    /// census refresh can dirty anything at all. Returns the model path.
    /// The fixture tracks BOTH artifacts a scan writes, because the real miner
    /// writes both. Tracking only the model made the AC-2 test a false
    /// positive: it passed while the field run left the dictionary sidecar
    /// modified and the tree dirty.
    fn repo_tracking_the_census(root: &Path) -> std::path::PathBuf {
        init_repo_on(root, "dev");
        let model = default_model_path(root);
        std::fs::create_dir_all(model.parent().expect("model parent")).unwrap();
        std::fs::write(&model, "{\"projects\":[]}\n").unwrap();
        std::fs::write(model.with_file_name(GRAIN_DICTIONARY), "{\"terms\":[]}\n").unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-m", "track the census"]);
        assert_eq!(porcelain(root), "", "the fixture must start clean");
        model
    }

    /// Everything a scan writes, as the miner would — model AND sidecar.
    fn remine(model: &Path) {
        std::fs::write(model, "{\"projects\":[{\"dir\":\"apps/rt\"}]}\n").unwrap();
        std::fs::write(model.with_file_name(GRAIN_DICTIONARY), "{\"terms\":[\"wave\"]}\n").unwrap();
    }

    /// AC-2 — the refresh finishes its own job. Re-mining a VERSIONED census on
    /// a tree the gate found clean leaves the tree clean again, with no manual
    /// commit in between.
    ///
    /// This is the defect the installer's version stamp had, with another file:
    /// the write landed, the gate announced it as work the operator could
    /// "commit apart", and the next unit's branch cut refused — five times in
    /// one session.
    #[test]
    fn census_refresh_leaves_the_tree_clean() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let model = repo_tracking_the_census(root);

        // Sampled BEFORE the write, exactly as `refresh_census_if_stale` does.
        let found_clean = worktree_is_clean(root);
        assert_eq!(found_clean, Some(true), "the fixture tree is clean");

        // The miner's effect, without the grain binary — BOTH artifacts move.
        remine(&model);
        assert_ne!(porcelain(root), "", "the re-mined census really did dirty the tree");

        assert_eq!(
            record_census(root, &model, found_clean),
            RecordOutcome::Recorded,
            "a census the gate itself wrote on a clean base is the gate's to record",
        );
        assert_eq!(
            porcelain(root),
            "",
            "the next unit's branch cut must find nothing to blame on the operator",
        );
    }

    /// AC-3 — and it never finishes SOMEONE ELSE's. A tree that already carried
    /// the operator's work is left entirely alone: nothing is committed, and
    /// their change is neither swept into a commit of ours nor staged.
    #[test]
    fn census_refresh_never_commits_over_the_operators_work() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let model = repo_tracking_the_census(root);

        let head_before = git_out(root, &["rev-parse", "HEAD"]).expect("HEAD");

        // The operator's work, present BEFORE the gate looks.
        std::fs::write(root.join("theirs.txt"), "mine, not yours\n").unwrap();
        let found_clean = worktree_is_clean(root);
        assert_eq!(found_clean, Some(false), "the tree already carried their work");

        std::fs::write(&model, "{\"projects\":[{\"dir\":\"apps/rt\"}]}\n").unwrap();

        assert_eq!(
            record_census(root, &model, found_clean),
            RecordOutcome::TreeNotClean,
            "with the operator's work in the tree the gate records nothing",
        );
        assert_eq!(
            git_out(root, &["rev-parse", "HEAD"]).expect("HEAD"),
            head_before,
            "no commit was written at all",
        );
        let status = porcelain(root);
        assert!(
            status.contains("theirs.txt"),
            "their file is untouched and still theirs to commit: {status}",
        );
        assert_eq!(
            std::fs::read_to_string(root.join("theirs.txt")).unwrap(),
            "mine, not yours\n",
            "and its bytes were never rewritten",
        );
    }
}

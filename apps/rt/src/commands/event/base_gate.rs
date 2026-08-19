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
//! Three answers, never more ([`BaseVerdict`]):
//!
//! 1. **Not an integration base** → `Refuse`. The base set is `git.flow`'s
//!    non-`*` keys ∪ values
//!    ([`mustard_core::domain::config::GitConfig::integration_bases`]) — the
//!    same derivation `work_branch_gate` protects and
//!    [`super::work_branch::resolve_base`] validates `--base` against, so the
//!    three can never disagree about what a base IS. Nothing here names a
//!    branch.
//! 2. **Behind its remote** → `Refuse`, naming the exact pull to run.
//! 3. Otherwise → `Open`, and the census refresh fires when it is due.
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
//! In a PRIVATE install the census is invisible to the host repository's git,
//! so there is no commit to keep apart and the tree's state decides nothing —
//! staleness alone is the whole question. Both readings come from the ONE
//! predicate [`crate::hooks::write::scan_clean_gate::scan_output_is_versioned`],
//! shared with the door that refuses, so the automatic path can never start
//! mining exactly where the user-invoked one is turned away.

use std::path::Path;

use mustard_core::{ProjectConfig, Scan};

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

    // NO membership test any more. It used to read
    // `config.git.integration_bases()` and refuse everything outside it, which
    // meant a branch cut last Tuesday was told it "is not an integration base
    // of this project" — a sentence about a configuration file, delivered as if
    // it were a sentence about the repository. In a client repository, where
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
    // A private install's census never reaches the host's git, so no state of
    // the tree can fuse it with the user's work: staleness is the whole
    // question. Without this, a client repository — dirty nearly always —
    // would carry a census that silently never refreshed.
    if !scan_output_is_versioned(project) {
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
/// `scan-map.md` and each subproject's `## Guards` stays with the explicit
/// `/scan`, which is where a human reviews that much rewriting.
///
/// Fail-open at every step, and loud on stderr rather than on stdout: this runs
/// inside `emit-pipeline`, whose one JSON line is byte-compared by gates.
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
    match Scan::locate().scan(project, &model) {
        Ok(()) => eprintln!(
            "base-gate: census refreshed ({}) — it is uncommitted work on a clean base, so \
             it can still be committed apart from this unit",
            model.display()
        ),
        Err(e) => eprintln!("base-gate: census refresh failed ({e}); the previous model stands"),
    }
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

        #[allow(deprecated)]
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

    /// The private-install reading of the same decision: the census never
    /// reaches the host's git, so a dirty tree disqualifies nothing and
    /// staleness alone decides. Without this the census on a client repository
    /// silently never refreshed — the tree there is dirty nearly always.
    #[test]
    fn a_private_install_refreshes_the_census_on_a_dirty_tree() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_repo_on(root, "dev");
        let model = default_model_path(root);

        let info = root.join(".git").join("info");
        std::fs::create_dir_all(&info).unwrap();
        std::fs::write(
            info.join("exclude"),
            mustard_core::PRIVATE_MARKS.join("\n") + "\n",
        )
        .unwrap();

        std::fs::write(root.join("stray.txt"), "x").unwrap();
        assert!(
            census_refresh_due(root, &model),
            "a private census has no commit of its own to keep apart from the dirt",
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
}

//! `work_branch_gate` — auto-branch per work unit on the first file mutation.
//!
//! ## Contract (slim)
//!
//! A `PreToolUse(Write|Edit|MultiEdit)` [`Check`] with exactly two behaviors:
//!
//! 1. **Pending work-unit marker** → cut `{base}_{slug}` off a freshly
//!    fetched base and check it out (fail-open: any git failure warns, never
//!    blocks — unless staying would leave the edit on a protected branch).
//!    The router pre-computed the branch name and stored it as the session's
//!    `pending-work-branch` marker via `emit-pipeline --kind pipeline.kind`
//!    (see [`crate::commands::event::emit_pipeline`]); this hook is the
//!    consumer. Read-only requests never Write/Edit, so the marker is never
//!    consumed and no branch is ever created.
//! 2. **No marker** → deny only a direct edit on a BARE integration branch
//!    (an exact member of `git.flow`'s bases); any work branch edits freely.
//!
//! ## Two roots: state vs. the local tree (the nested-worktree fix)
//!
//! Inside a linked worktree (`.claude/worktrees/{slug}`) the workspace
//! resolver redirects every `.claude/` path to the MAIN checkout — the
//! worktree carries code only (see `tests/worktree_redirect.rs`). That is
//! correct for STATE (the pending-branch marker, `mustard.json` git flow),
//! but this gate used to read the git BRANCH from that redirected root too,
//! so inside a linked worktree it judged the MAIN checkout's branch (`dev`)
//! and false-blocked every edit. Branch detection and the branch cut now run
//! against the LOCAL tree of the edit ([`local_tree_of`]: the edited file's
//! own directory, else the process cwd), while state keeps the main-checkout
//! resolution. A per-session worktree is its own tree, so cutting the work
//! branch there collides with nobody.
//!
//! ## Agnostic base model
//!
//! Everything derives from `mustard.json#git.flow` (via
//! [`mustard_core::domain::config::GitConfig`]). The project's **integration
//! bases** are `git.flow`'s non-`*` keys ∪ values (`{"*":"dev","dev":"main"}` →
//! `{dev, main}`; `{"*":"develop","develop":"master"}` → `{develop, master}`);
//! nothing hardcodes `dev`/`main`. A work branch's base is recovered from its
//! NAME: the LONGEST integration base `B` with `target == "{B}_…"`
//! ([`base_for`], falling back to `config.git.primary_base()`).
//!
//! ## Contract (apps/rt/CLAUDE.md)
//!
//! Never panics — no `unwrap`/`expect` outside tests; every git call is
//! `Command::…output()` matched into a `Result`, and an `Err` from the `Check`
//! itself folds to `Allow` in the dispatcher. It raises `Deny` ONLY to keep an
//! edit off a bare integration branch; otherwise it `Allow`s or `Warn`s.

use mustard_core::platform::error::Error;
use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::ProjectConfig;
use std::path::Path;
use std::process::Command;

use crate::shared::context;

/// The auto-branch gate. Stateless — every invocation rebuilds from the hook
/// input and the on-disk marker.
pub struct WorkBranchGate;

/// Resolve the session id for this invocation: the harness-provided
/// [`HookInput::session_id`] when present, else the env/filesystem fallback in
/// [`context::session_id`].
fn session_id_from(input: &HookInput) -> String {
    if let Some(s) = input.session_id.as_deref().filter(|s| !s.is_empty()) {
        return s.to_string();
    }
    context::session_id()
}

/// The working tree that HOSTS the edit — branch detection runs here.
///
/// Priority: the edited file's own directory (nearest EXISTING ancestor, so a
/// Write that creates parent directories still resolves) → the process cwd →
/// the state root. An absolute edit into another tree (session cwd in a
/// worktree, target in the main checkout) is judged by the TARGET's tree.
fn local_tree_of(input: &HookInput, state_root: &str) -> String {
    if let Some(fp) = super::boundary_gate::file_path_of(input) {
        let p = Path::new(&fp);
        if p.is_absolute() {
            let mut dir = p.parent();
            while let Some(d) = dir {
                if d.is_dir() {
                    return d.to_string_lossy().into_owned();
                }
                dir = d.parent();
            }
        }
        // Relative path → anchored at the process cwd; fall through.
    }
    if let Some(cwd) = input.cwd.as_deref().filter(|c| !c.is_empty()) {
        return cwd.to_string();
    }
    state_root.to_string()
}

/// The current branch name (`git rev-parse --abbrev-ref HEAD`), or `None` on
/// any failure (not a repo, detached HEAD reported as `"HEAD"`, git absent).
fn current_branch(vcs: &str, root: &str) -> Option<String> {
    let out = Command::new(vcs)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(root)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let name = String::from_utf8(out.stdout).ok()?.trim().to_string();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// `true` when a local branch `refs/heads/<branch>` exists.
fn local_branch_exists(vcs: &str, root: &str, branch: &str) -> bool {
    Command::new(vcs)
        .args([
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ])
        .current_dir(root)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Run one git subcommand in `root`, mapping a non-zero exit (or spawn error)
/// to `Err(<stderr|io error>)`. Never panics.
fn run_git(vcs: &str, root: &str, args: &[&str]) -> Result<(), String> {
    let out = Command::new(vcs)
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let msg = stderr.trim();
        Err(if msg.is_empty() {
            format!("git exited with status {}", out.status)
        } else {
            msg.to_string()
        })
    }
}

/// Check out `target`, creating it off `base` when it does not yet exist.
/// Carries the working-tree changes along (a plain `checkout`, no stash). If
/// `base` is absent locally, branch off the current HEAD instead. Returns the
/// git error string on failure.
fn checkout_work_branch(vcs: &str, root: &str, target: &str, base: &str) -> Result<(), String> {
    if local_branch_exists(vcs, root, target) {
        return run_git(vcs, root, &["checkout", target]);
    }
    if local_branch_exists(vcs, root, base) {
        return run_git(vcs, root, &["checkout", "-b", target, base]);
    }
    // Base branch not present locally — branch off the current HEAD.
    run_git(vcs, root, &["checkout", "-b", target])
}

/// Refresh the project's integration bases (`git.flow`) to their `origin`
/// remotes BEFORE a work branch is cut, so the branch is always based on the
/// latest `dev`/`main`. Fire-and-forget: it returns nothing the caller must
/// act on, and every git failure is swallowed. Fully FAIL-OPEN — offline, no
/// remote, or a diverged base never blocks the edit and never panics.
///
/// 1. `git fetch origin` — on failure (offline / no remote) RETURN early and
///    do nothing else; the branch is still cut from the local base.
/// 2. For each integration base `B`:
///    - when `B` is the checked-out branch (`Some(B) == current`) →
///      `git merge --ff-only origin/B` fast-forwards it in place;
///    - otherwise → `git fetch origin B:B`, a refspec fetch git refuses to
///      make non-ff, so it safely fast-forwards the local ref without a
///      checkout.
///    Every per-base error (no matching origin ref, a diverged base, a base
///    checked out in another worktree, …) is ignored — best-effort, keep going.
fn refresh_integration_bases(vcs: &str, root: &str, config: &ProjectConfig, current: Option<&str>) {
    // Offline / no remote → nothing to refresh; the branch is cut from the
    // local base as before. Do NOT propagate the error.
    if run_git(vcs, root, &["fetch", "origin"]).is_err() {
        return;
    }
    for base in config.git.integration_bases() {
        // Best-effort per base — drop the result either way.
        let _ = if current == Some(base.as_str()) {
            run_git(vcs, root, &["merge", "--ff-only", &format!("origin/{base}")])
        } else {
            run_git(vcs, root, &["fetch", "origin", &format!("{base}:{base}")])
        };
    }
}

/// Recover the integration base a work branch was cut from, from its NAME:
/// among the project's integration bases (`git.flow`), the LONGEST base `B`
/// such that `target` starts with `"{B}_"`. When none match, the project's
/// primary base (`config.git.primary_base()`).
///
/// Longest-match disambiguates nested bases (a `dev_release` base wins over
/// `dev` for `dev_release_x`). Agnostic — the base set and the primary both
/// come from `git.flow`; no branch name is hardcoded.
fn base_for(target: &str, config: &ProjectConfig) -> String {
    let bases = config.git.integration_bases();
    let mut best: Option<&str> = None;
    for b in &bases {
        if target.starts_with(&format!("{b}_")) && best.is_none_or(|cur| b.len() > cur.len()) {
            best = Some(b.as_str());
        }
    }
    best.map_or_else(|| config.git.primary_base(), str::to_string)
}

/// `true` when `branch` is a bare integration branch that must never be edited
/// directly — an exact member of `config.git.integration_bases()` (`dev`,
/// `main`/`master`, `develop`, … whatever `git.flow` declares). The `{base}_*`
/// work branches (`dev_rubens`, `main_close-gate`, …) are NOT protected.
fn is_protected(branch: &str, config: &ProjectConfig) -> bool {
    config.git.integration_bases().contains(branch)
}

impl Check for WorkBranchGate {
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        // Defensive: only PreToolUse(Write|Edit|MultiEdit) should reach us.
        if ctx.trigger != Some(Trigger::PreToolUse) {
            return Ok(Verdict::Allow);
        }
        // STATE root: the main checkout (worktree redirect) — markers, config.
        let project = ctx.project_dir_or_cwd(input);
        let sid = session_id_from(input);

        // Branch protection guards the REPO's tree only. A mutation whose
        // target lives outside the project root (`~/.claude` memory files,
        // temp dirs, …) is not repo work: never block it, never cut a branch
        // for it, and keep the pending marker for the first IN-repo edit.
        if let Some(fp) = super::boundary_gate::file_path_of(input) {
            match super::boundary_gate::relative_to_cwd(&project, &fp) {
                None => return Ok(Verdict::Allow),
                // Plan-mode artifacts (`.claude/plans/…`) are harness state,
                // not repo work — planning happens BEFORE the work unit, so
                // blocking them deadlocks native plan mode on a protected
                // branch (the session cannot even write the plan it needs
                // approved). Same contract as out-of-repo: allow, cut
                // nothing, keep the marker.
                Some(rel) if rel.starts_with(".claude/plans/") => return Ok(Verdict::Allow),
                Some(_) => {}
            }
        }

        // VCS binary policy: default `git`; an explicit `""` opt-out (or a
        // non-git tree) means we cannot branch and there is nothing to guard.
        let config = crate::shared::context::project_config_cached(Path::new(&project));
        let Some(vcs) = config.vcs() else {
            context::clear_pending_branch(&project, &sid);
            return Ok(Verdict::Allow);
        };

        // LOCAL tree of the edit — branch detection and the cut run HERE, not
        // on the (possibly redirected) state root. See module doc.
        let local = local_tree_of(input, &project);

        let current = current_branch(&vcs, &local);
        let on_protected = current
            .as_deref()
            .map(|b| is_protected(b, &config))
            .unwrap_or(false);

        // 1. No pending work unit signalled for this session.
        let Some(target) = context::pending_branch_for(&project, &sid) else {
            // Hard block: never develop directly on a bare integration
            // branch. On a work branch (or non-git tree) there is nothing to do.
            if on_protected {
                return Ok(Verdict::Deny {
                    reason: format!(
                        "Você está na branch de integração protegida '{}'. O Mustard não \
                         desenvolve direto aqui — descreva o trabalho para eu criar a branch \
                         {{base}}_{{slug}}, ou crie uma branch manualmente antes de editar.",
                        current.as_deref().unwrap_or("?")
                    ),
                });
            }
            return Ok(Verdict::Allow);
        };

        // 2. Already on the target branch → clear and allow.
        if current.as_deref() == Some(target.as_str()) {
            context::clear_pending_branch(&project, &sid);
            return Ok(Verdict::Allow);
        }

        // 3. Refresh the integration bases from origin FIRST so the branch is
        //    cut from the latest dev/main. Fail-open: offline / no remote /
        //    non-ff never blocks the edit (see refresh_integration_bases).
        refresh_integration_bases(&vcs, &local, &config, current.as_deref());

        // 4. Recover the base from the target's `{base}_` prefix and check out
        //    IN THE LOCAL TREE (a per-session worktree is isolated; the shared
        //    main checkout keeps the historical single-session behavior).
        let base = base_for(&target, &config);
        match checkout_work_branch(&vcs, &local, &target, &base) {
            Ok(()) => {
                context::clear_pending_branch(&project, &sid);
                Ok(Verdict::Allow)
            }
            Err(e) => {
                // Clear anyway so we do not re-fire on every edit.
                context::clear_pending_branch(&project, &sid);
                if on_protected {
                    // We could not leave the protected branch — refuse rather
                    // than let the edit land directly on it.
                    Ok(Verdict::Deny {
                        reason: format!(
                            "Não consegui sair da branch protegida '{}' para '{target}': {e}. \
                             O Mustard não desenvolve direto na branch protegida — resolva o \
                             git e tente de novo.",
                            current.as_deref().unwrap_or("?")
                        ),
                    })
                } else {
                    // On a work branch already: warn and let the edit proceed.
                    Ok(Verdict::Warn {
                        message: format!(
                            "não consegui criar a branch {target}: {e} — seguindo na branch atual"
                        ),
                    })
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::model::contract::{Ctx, HookInput, Trigger};
    use serde_json::json;

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

    fn pre_edit_input(root: &str, sid: &str) -> (HookInput, Ctx) {
        pre_edit_input_for(root, sid, "f.txt")
    }

    /// Like [`pre_edit_input`] but with an explicit mutation target path.
    fn pre_edit_input_for(root: &str, sid: &str, file_path: &str) -> (HookInput, Ctx) {
        let input = HookInput {
            tool_name: Some("Write".to_string()),
            tool_input: json!({ "file_path": file_path, "content": "x" }),
            hook_event_name: Some("PreToolUse".to_string()),
            cwd: Some(root.to_string()),
            session_id: Some(sid.to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx {
            project_dir: root.to_string(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        };
        (input, ctx)
    }

    /// Write a `mustard.json` declaring the given `git.flow` so the gate derives
    /// this project's integration bases from it (agnostic — no hardcoded flow).
    fn seed_flow(root: &Path, flow_json: &str) {
        std::fs::write(
            root.join("mustard.json"),
            format!(r#"{{"git":{{"flow":{flow_json}}}}}"#),
        )
        .unwrap();
    }

    /// Init a git repo whose sole commit lives on `base` (created before the
    /// first commit so it exists regardless of the platform's default branch).
    fn init_repo_on(root: &Path, base: &str) {
        git(root, &["init"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        git(root, &["checkout", "-b", base]);
        std::fs::write(root.join("f.txt"), "hi").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-m", "init"]);
    }

    /// Marker `dev_my-thing` on a repo whose base is `dev` → the gate recovers
    /// `dev` from the `dev_` prefix and checks the work branch out off `dev`.
    #[test]
    fn creates_work_branch_off_prefix_base_dev() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_str().unwrap();

        seed_flow(root, r#"{"*":"dev","dev":"main"}"#);
        init_repo_on(root, "dev");

        // The router pre-computed this branch for the work unit.
        let sid = "sess-branch-test";
        context::set_pending_branch(root_s, sid, "dev_my-thing");

        // First Write of the work unit fires the gate.
        let (input, ctx) = pre_edit_input(root_s, sid);
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        assert!(matches!(verdict, Verdict::Allow), "the edit proceeds: {verdict:?}");

        // HEAD is now on the target branch, created off `dev` (from the prefix)...
        assert_eq!(
            current_branch("git", root_s).as_deref(),
            Some("dev_my-thing"),
            "checked out the pre-computed branch off dev",
        );
        // ...and the marker was cleared so subsequent edits do not re-fire.
        assert!(
            context::pending_branch_for(root_s, sid).is_none(),
            "marker cleared after checkout",
        );
    }

    /// Marker whose target is ALREADY checked out (by hand) → clear and allow,
    /// no git mutation.
    #[test]
    fn already_on_target_clears_marker_and_allows() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_str().unwrap();
        seed_flow(root, r#"{"*":"dev","dev":"main"}"#);
        init_repo_on(root, "dev");
        git(root, &["checkout", "-b", "dev_manual"]);

        let sid = "sess-already";
        context::set_pending_branch(root_s, sid, "dev_manual");
        let (input, ctx) = pre_edit_input(root_s, sid);
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        assert!(matches!(verdict, Verdict::Allow), "got {verdict:?}");
        assert_eq!(current_branch("git", root_s).as_deref(), Some("dev_manual"));
        assert!(context::pending_branch_for(root_s, sid).is_none());
    }

    /// REGRESSION (nested worktree — the bug that false-blocked every edit):
    /// the harness state-root redirect hands the gate the MAIN checkout as
    /// `project_dir` even though the session edits inside a linked worktree
    /// (modelled on `tests/worktree_redirect.rs`). Branch detection must run
    /// against the WORKTREE (a work branch → free to edit), never against the
    /// main checkout's integration branch — while the same state root still
    /// hard-blocks a direct edit in the main checkout itself.
    #[test]
    fn nested_worktree_edit_judged_by_local_tree_not_state_root() {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("repo");
        std::fs::create_dir_all(&main).unwrap();
        seed_flow(&main, r#"{"*":"dev","dev":"main"}"#);
        init_repo_on(&main, "dev");
        // The harness cuts worktrees at <main>/.claude/worktrees/<slug>, on a
        // WORK branch.
        git(&main, &["worktree", "add", ".claude/worktrees/dev_x", "-b", "dev_x"]);
        let wt = main.join(".claude").join("worktrees").join("dev_x");
        let main_s = main.to_str().unwrap();

        // Edit a file INSIDE the worktree; ctx.project_dir is the MAIN
        // checkout (what the workspace redirect resolves for state).
        let file_in_wt = wt.join("f.txt");
        let input = HookInput {
            tool_name: Some("Write".to_string()),
            tool_input: json!({ "file_path": file_in_wt.to_str().unwrap(), "content": "x" }),
            hook_event_name: Some("PreToolUse".to_string()),
            cwd: Some(wt.to_str().unwrap().to_string()),
            session_id: Some("sess-nested".to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx {
            project_dir: main_s.to_string(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        };
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        assert!(
            matches!(verdict, Verdict::Allow),
            "a worktree on a work branch edits freely even when state resolves \
             to a main checkout sitting on an integration branch: {verdict:?}",
        );

        // The SAME state root judged at the main checkout itself still blocks.
        let (input2, ctx2) = pre_edit_input(main_s, "sess-nested-main");
        let verdict2 = WorkBranchGate.evaluate(&input2, &ctx2).expect("no error");
        assert!(
            matches!(verdict2, Verdict::Deny { .. }),
            "a bare-integration-branch edit in the main checkout stays blocked: {verdict2:?}",
        );
    }

    /// A pending marker consumed INSIDE a linked worktree cuts the target
    /// branch in the WORKTREE — the main checkout's HEAD is untouched.
    #[test]
    fn nested_worktree_marker_cuts_branch_locally() {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("repo");
        std::fs::create_dir_all(&main).unwrap();
        seed_flow(&main, r#"{"*":"dev","dev":"main"}"#);
        init_repo_on(&main, "dev");
        git(&main, &["worktree", "add", ".claude/worktrees/dev_x", "-b", "dev_x"]);
        let wt = main.join(".claude").join("worktrees").join("dev_x");
        let wt_s = wt.to_str().unwrap();
        let main_s = main.to_str().unwrap();

        // Marker lives in the STATE root (the main checkout).
        let sid = "sess-nested-marker";
        context::set_pending_branch(main_s, sid, "dev_other");

        let file_in_wt = wt.join("f.txt");
        let input = HookInput {
            tool_name: Some("Write".to_string()),
            tool_input: json!({ "file_path": file_in_wt.to_str().unwrap(), "content": "x" }),
            hook_event_name: Some("PreToolUse".to_string()),
            cwd: Some(wt_s.to_string()),
            session_id: Some(sid.to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx {
            project_dir: main_s.to_string(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        };
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        assert!(matches!(verdict, Verdict::Allow), "got {verdict:?}");
        assert_eq!(
            current_branch("git", wt_s).as_deref(),
            Some("dev_other"),
            "the work branch is cut in the LOCAL worktree",
        );
        assert_eq!(
            current_branch("git", main_s).as_deref(),
            Some("dev"),
            "the main checkout's HEAD is untouched",
        );
        assert!(
            context::pending_branch_for(main_s, sid).is_none(),
            "marker consumed from the state root",
        );
    }

    /// FAIL-OPEN: with NO `origin` remote, the base refresh's `git fetch origin`
    /// fails — that must NOT break branch creation. The gate still checks the
    /// work branch out off the local base and Allows the edit.
    #[test]
    fn base_refresh_is_fail_open_without_origin_remote() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_str().unwrap();

        seed_flow(root, r#"{"*":"dev","dev":"main"}"#);
        init_repo_on(root, "dev");

        // No `origin` remote is configured — `git fetch origin` will fail.
        let has_origin = Command::new("git")
            .args(["remote", "get-url", "origin"])
            .current_dir(root)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        assert!(!has_origin, "precondition: repo has no origin remote");

        let sid = "sess-offline";
        context::set_pending_branch(root_s, sid, "dev_offline-thing");

        let (input, ctx) = pre_edit_input(root_s, sid);
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        assert!(
            matches!(verdict, Verdict::Allow),
            "a failing `git fetch origin` must not block the edit: {verdict:?}",
        );
        assert_eq!(
            current_branch("git", root_s).as_deref(),
            Some("dev_offline-thing"),
            "the work branch is still cut from the local base when offline",
        );
        assert!(
            context::pending_branch_for(root_s, sid).is_none(),
            "marker cleared after checkout",
        );
    }

    /// A local bare-remote fixture whose `dev` is BEHIND origin: the base refresh
    /// fast-forwards the local `dev` before the work branch is cut, so the branch
    /// carries the newest commit. Proves the refresh actually advances the base.
    #[test]
    fn base_refresh_fast_forwards_base_behind_origin() {
        let tmp = tempfile::tempdir().unwrap();

        // A bare "remote" repo whose HEAD points at `dev` (set explicitly so we
        // do not depend on the git version's default-branch flag).
        let remote = tmp.path().join("remote.git");
        std::fs::create_dir_all(&remote).unwrap();
        let remote_s = remote.to_str().unwrap();
        git(&remote, &["init", "--bare"]);
        git(&remote, &["symbolic-ref", "HEAD", "refs/heads/dev"]);

        // A working clone that publishes the first `dev` commit.
        let seed = tmp.path().join("seed");
        std::fs::create_dir_all(&seed).unwrap();
        git(&seed, &["init"]);
        git(&seed, &["config", "user.email", "t@example.com"]);
        git(&seed, &["config", "user.name", "t"]);
        git(&seed, &["checkout", "-b", "dev"]);
        std::fs::write(seed.join("f.txt"), "one").unwrap();
        git(&seed, &["add", "."]);
        git(&seed, &["commit", "-m", "one"]);
        git(&seed, &["remote", "add", "origin", remote_s]);
        git(&seed, &["push", "origin", "dev"]);

        // The project clone — its local `dev` starts at the first commit.
        let proj = tmp.path().join("proj");
        std::fs::create_dir_all(&proj).unwrap();
        let proj_s = proj.to_str().unwrap();
        git(&proj, &["clone", remote_s, "."]);
        git(&proj, &["config", "user.email", "t@example.com"]);
        git(&proj, &["config", "user.name", "t"]);
        seed_flow(&proj, r#"{"*":"dev","dev":"main"}"#);

        // Now advance origin/dev with a SECOND commit the project has not seen.
        std::fs::write(seed.join("f.txt"), "two").unwrap();
        git(&seed, &["add", "."]);
        git(&seed, &["commit", "-m", "two"]);
        git(&seed, &["push", "origin", "dev"]);
        let ahead = Command::new("git")
            .args(["rev-parse", "dev"])
            .current_dir(&seed)
            .output()
            .unwrap();
        let ahead_sha = String::from_utf8(ahead.stdout).unwrap().trim().to_string();

        // First edit of the work unit fires the gate; base refresh must ff `dev`.
        let sid = "sess-behind";
        context::set_pending_branch(proj_s, sid, "dev_new-thing");
        let (input, ctx) = pre_edit_input(proj_s, sid);
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        assert!(matches!(verdict, Verdict::Allow), "edit proceeds: {verdict:?}");
        assert_eq!(
            current_branch("git", proj_s).as_deref(),
            Some("dev_new-thing"),
            "the work branch is checked out",
        );

        // The work branch was cut from the fast-forwarded `dev` (second commit).
        let head = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&proj)
            .output()
            .unwrap();
        let head_sha = String::from_utf8(head.stdout).unwrap().trim().to_string();
        assert_eq!(
            head_sha, ahead_sha,
            "the work branch is based on the fast-forwarded dev (latest origin commit)",
        );
    }

    /// Agnostic: a `develop`/`master` project cuts `develop_feature` off
    /// `develop` — nothing in the gate assumes `dev`/`main`.
    #[test]
    fn creates_work_branch_off_prefix_base_develop() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_str().unwrap();

        seed_flow(root, r#"{"*":"develop","develop":"master"}"#);
        init_repo_on(root, "develop");

        let sid = "sess-develop";
        context::set_pending_branch(root_s, sid, "develop_feature");

        let (input, ctx) = pre_edit_input(root_s, sid);
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        assert!(matches!(verdict, Verdict::Allow), "the edit proceeds: {verdict:?}");
        assert_eq!(
            current_branch("git", root_s).as_deref(),
            Some("develop_feature"),
            "checked out off develop (its prefix base)",
        );
    }

    #[test]
    fn no_marker_is_a_silent_noop() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_str().unwrap();
        // No git repo, no marker: the gate must simply Allow (never panic).
        let (input, ctx) = pre_edit_input(root_s, "sess-none");
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        assert!(matches!(verdict, Verdict::Allow));
    }

    /// A repo on a protected branch (`main`) with NO pending-branch marker and
    /// NO `mustard.json`: a direct edit must be BLOCKED — the harness never
    /// develops on an integration branch. With no `git.flow`, the base set
    /// falls back to `{main, master}`, so `main` is protected.
    #[test]
    fn blocks_direct_edit_on_protected_branch_without_marker() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_str().unwrap();
        git(root, &["init"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        git(root, &["checkout", "-b", "main"]);
        std::fs::write(root.join("f.txt"), "hi").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-m", "init"]);

        let (input, ctx) = pre_edit_input(root_s, "sess-protected");
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        assert!(
            matches!(verdict, Verdict::Deny { .. }),
            "direct edit on main is blocked: {verdict:?}",
        );
    }

    /// A repo already on a work branch (`feature/x`) with NO marker: editing is
    /// fine — the block only guards protected integration branches.
    #[test]
    fn allows_direct_edit_on_work_branch_without_marker() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_str().unwrap();
        git(root, &["init"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        git(root, &["checkout", "-b", "feature/x"]);
        std::fs::write(root.join("f.txt"), "hi").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-m", "init"]);

        let (input, ctx) = pre_edit_input(root_s, "sess-work");
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        assert!(
            matches!(verdict, Verdict::Allow),
            "editing on a work branch proceeds: {verdict:?}",
        );
    }

    /// A `dev`/`main` flow makes the BARE `dev` branch protected (derived from
    /// `git.flow`, not hardcoded): a direct edit on `dev` with no marker → Deny.
    #[test]
    fn blocks_direct_edit_on_bare_dev_from_flow() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_str().unwrap();
        seed_flow(root, r#"{"*":"dev","dev":"main"}"#);
        init_repo_on(root, "dev");

        let (input, ctx) = pre_edit_input(root_s, "sess-bare-dev");
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        assert!(
            matches!(verdict, Verdict::Deny { .. }),
            "direct edit on the bare integration branch `dev` is blocked: {verdict:?}",
        );
    }

    /// A `dev_*` WORK branch is NOT protected: editing on `dev_thing` with no
    /// marker proceeds (the block only guards the bare integration bases).
    #[test]
    fn allows_direct_edit_on_dev_work_branch_without_marker() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_str().unwrap();
        seed_flow(root, r#"{"*":"dev","dev":"main"}"#);
        init_repo_on(root, "dev_thing");

        let (input, ctx) = pre_edit_input(root_s, "sess-dev-work");
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        assert!(
            matches!(verdict, Verdict::Allow),
            "editing on a `dev_*` work branch proceeds: {verdict:?}",
        );
    }

    /// A Write targeting a path OUTSIDE the repo (e.g. `~/.claude` memory
    /// files) on a bare protected branch with NO marker: Allow — branch
    /// protection guards the repo's tree only.
    #[test]
    fn allows_out_of_repo_write_on_protected_branch() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_str().unwrap();
        seed_flow(root, r#"{"*":"dev","dev":"main"}"#);
        init_repo_on(root, "dev");

        let outside = tempfile::tempdir().unwrap();
        let outside_file = outside.path().join("memo.md");
        let (input, ctx) =
            pre_edit_input_for(root_s, "sess-outside", outside_file.to_str().unwrap());
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        assert!(
            matches!(verdict, Verdict::Allow),
            "an out-of-repo write is not repo work — never blocked: {verdict:?}",
        );
    }

    /// A Write targeting the native plan-mode artifact (`.claude/plans/…`) on a
    /// bare protected branch: Allow, no branch cut, marker kept — planning
    /// precedes the work unit, and blocking it deadlocks plan mode (the session
    /// cannot even write the plan it needs approved).
    #[test]
    fn allows_plan_file_write_on_protected_branch() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_str().unwrap();
        seed_flow(root, r#"{"*":"dev","dev":"main"}"#);
        init_repo_on(root, "dev");

        // Even with a pending work unit, a plan write must not consume it.
        let sid = "sess-plan-file";
        context::set_pending_branch(root_s, sid, "dev_planned-thing");

        let (input, ctx) = pre_edit_input_for(root_s, sid, ".claude/plans/my-plan.md");
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        assert!(matches!(verdict, Verdict::Allow), "plan file writes freely: {verdict:?}");
        assert_eq!(
            current_branch("git", root_s).as_deref(),
            Some("dev"),
            "no branch is cut for a plan-file mutation",
        );
        assert_eq!(
            context::pending_branch_for(root_s, sid).as_deref(),
            Some("dev_planned-thing"),
            "the marker survives for the first code edit",
        );
    }

    /// A Write targeting a path OUTSIDE the repo with a PENDING marker: Allow,
    /// no branch is cut, and the marker survives for the first in-repo edit.
    #[test]
    fn out_of_repo_write_keeps_marker_and_branch() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_str().unwrap();
        seed_flow(root, r#"{"*":"dev","dev":"main"}"#);
        init_repo_on(root, "dev");

        let sid = "sess-outside-marker";
        context::set_pending_branch(root_s, sid, "dev_pending-thing");

        let outside = tempfile::tempdir().unwrap();
        let outside_file = outside.path().join("memo.md");
        let (input, ctx) = pre_edit_input_for(root_s, sid, outside_file.to_str().unwrap());
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        assert!(matches!(verdict, Verdict::Allow), "out-of-repo write proceeds: {verdict:?}");
        assert_eq!(
            current_branch("git", root_s).as_deref(),
            Some("dev"),
            "no branch is cut for an out-of-repo mutation",
        );
        assert_eq!(
            context::pending_branch_for(root_s, sid).as_deref(),
            Some("dev_pending-thing"),
            "the marker survives for the first in-repo edit",
        );
    }

    #[test]
    fn is_protected_matches_integration_bases_only() {
        let mut config = ProjectConfig::default();
        config.git.flow.insert("*".into(), "develop".into());
        config.git.flow.insert("develop".into(), "master".into());
        assert!(is_protected("develop", &config), "bare integration base protected");
        assert!(is_protected("master", &config), "bare integration base protected");
        assert!(!is_protected("develop_x", &config), "work branch not protected");
        assert!(!is_protected("main", &config), "not a base of THIS project");
    }
}

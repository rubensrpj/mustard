//! `work_branch_gate` — enforce per-work-unit ISOLATION on the first file
//! mutation.
//!
//! ## What it does
//!
//! A `PreToolUse(Write|Edit|MultiEdit)` [`Check`] that fires on the FIRST file
//! mutation of a work request. Its job is to stop concurrent sessions from
//! colliding on ONE shared working tree.
//!
//! The router pre-computes a branch name (`{base}_{slug}`) for the work unit and
//! stores it as the session's `pending-work-branch` marker via
//! `emit-pipeline --kind pipeline.kind` (see
//! [`crate::commands::event::emit_pipeline`]); this hook is the consumer.
//!
//! Read-only requests never Write/Edit, so the marker is never consumed and the
//! gate never fires — isolation is bound to an actual mutation, not to opening a
//! pipeline. A mutation whose target lives OUTSIDE the project root (`~/.claude`
//! memory files, temp dirs, …) is not repo work: the gate self-allows without
//! consuming the marker.
//!
//! ## Worktree isolation (the collision fix)
//!
//! The gate used to `git checkout -b {base}_{slug}` on the first edit. On a
//! SHARED working tree that mutates state two sessions — or the desktop app's
//! own per-session worktree — contend over. It no longer checks out anything.
//!
//! - When the session already runs in a LINKED git worktree (the desktop app
//!   creates one per session, or the user ran `EnterWorktree`), the tree is
//!   already isolated with its own branch: the gate refreshes the integration
//!   bases (fail-open), consumes the marker, and `Allow`s. See
//!   [`is_isolated_worktree`].
//! - When the session runs on the SHARED main tree, the gate no longer switches
//!   branches for it. A direct edit of a bare integration branch is still hard-
//!   blocked (never develop on `dev`/`main`). Otherwise it steers the session to
//!   an isolated worktree per `MUSTARD_WORKTREE_ISOLATION_MODE`
//!   (`off` | `warn` | `strict`, default `warn`; see [`isolation_mode`] and
//!   [`shared_tree_verdict`]): `strict` `Deny`s (run `EnterWorktree` first),
//!   `warn` `Allow`s with an advisory, `off` is silent. The base refresh runs
//!   regardless (fail-open).
//!
//! ## Agnostic base model
//!
//! Everything derives from `mustard.json#git.flow` (via
//! [`mustard_core::domain::config::GitConfig`]). The project's **integration
//! bases** are `git.flow`'s non-`*` keys ∪ values (`{"*":"dev","dev":"main"}` →
//! `{dev, main}`; `{"*":"develop","develop":"master"}` → `{develop, master}`);
//! nothing hardcodes `dev`/`main`. A work branch's base is recovered from its
//! NAME: the LONGEST integration base `B` with `target == "{B}_…"`.
//!
//! ## Marker lifecycle
//!
//! The marker is single-use: it is cleared once the first in-repo mutation is
//! handled (isolated `Allow`, shared `warn`/`off`, or the already-on-target
//! shortcut). The ONE exception is shared-tree `strict`: the marker is kept so
//! the block persists on every retry until the session actually isolates —
//! consuming it would let a plain retry slip past a hard gate.
//!
//! ## Contract (apps/rt/CLAUDE.md)
//!
//! Never panics — no `unwrap`/`expect` outside tests; every git call is
//! `Command::…output()` matched into a `Result`, and an `Err` from the `Check`
//! itself folds to `Allow` in the dispatcher. It raises `Deny` ONLY to keep an
//! edit off a bare integration branch or (in `strict`) off the shared tree;
//! otherwise it `Allow`s or `Warn`s.

use mustard_core::platform::config::Mode;
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

/// `true` when a local branch `refs/heads/<branch>` exists. Retained as part of
/// the gate's git-plumbing surface — no live caller since the shared-tree
/// checkout was retired in favour of worktree isolation.
#[allow(dead_code)]
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

/// Run `git rev-parse <flag>` in `root`, returning trimmed stdout on success.
/// Modelled on [`current_branch`]; `None` on any failure (not a repo, git
/// absent, empty output).
fn capture_git(vcs: &str, root: &str, flag: &str) -> Option<String> {
    let out = Command::new(vcs)
        .args(["rev-parse", flag])
        .current_dir(root)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?.trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Absolutise a `git rev-parse --git-*dir` reading against `root` and
/// canonicalise it when possible, so a relative `.git` (what the main tree
/// reports) and an absolute path to the same directory compare equal. On a
/// canonicalisation error the absolutised (uncanonicalised) path is used as-is.
fn resolve_git_path(root: &str, reading: &str) -> std::path::PathBuf {
    let p = Path::new(reading.trim());
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        Path::new(root).join(p)
    };
    std::fs::canonicalize(&abs).unwrap_or(abs)
}

/// Decide worktree isolation from the two git-dir readings. The main working
/// tree reports the SAME location for `--git-dir` and `--git-common-dir`; a
/// LINKED worktree (what `EnterWorktree` and the desktop app create, one per
/// session) reports a per-worktree git dir (`<common>/worktrees/<name>`)
/// distinct from the shared common dir. So the two readings DIFFER ⇔ isolated.
///
/// Pure and directly testable — the caller supplies the two raw readings and
/// the `root` they resolve against (see [`resolve_git_path`]).
fn readings_indicate_linked(root: &str, git_dir: &str, common_dir: &str) -> bool {
    resolve_git_path(root, git_dir) != resolve_git_path(root, common_dir)
}

/// `true` when the working tree at `root` is a LINKED git worktree — its own
/// isolated tree with its own branch, as created by `EnterWorktree` or the
/// desktop app per session. Compares `git rev-parse --git-dir` against
/// `--git-common-dir` (see [`readings_indicate_linked`]).
///
/// Any failure to read either value (not a repo, git absent, a git too old for
/// `--git-common-dir`) yields `false`: we only skip the shared-tree guidance
/// when we can PROVE the session already owns a worktree; when we cannot tell,
/// we treat the tree as shared and keep the protections.
fn is_isolated_worktree(vcs: &str, root: &str) -> bool {
    match (
        capture_git(vcs, root, "--git-dir"),
        capture_git(vcs, root, "--git-common-dir"),
    ) {
        (Some(git_dir), Some(common)) => readings_indicate_linked(root, &git_dir, &common),
        _ => false,
    }
}

/// Resolve the worktree-isolation mode from `MUSTARD_WORKTREE_ISOLATION_MODE`
/// (`off` | `warn` | `strict`). Default [`Mode::Warn`] — nudge toward
/// `EnterWorktree` without blocking. Mirrors the sibling gates' env-mode shape
/// (`rtk_gate_mode`, `boundary_mode`); parse goes through [`Mode::parse`], any
/// unrecognised value collapses to `warn`.
fn isolation_mode() -> Mode {
    std::env::var("MUSTARD_WORKTREE_ISOLATION_MODE")
        .ok()
        .and_then(|raw| Mode::parse(&raw))
        .unwrap_or(Mode::Warn)
}

/// Verdict for a marker that asked to branch, evaluated on the SHARED main tree
/// under the resolved isolation `mode`. Pure — the env read ([`isolation_mode`])
/// and the base refresh happen in `evaluate`; keeping the decision here lets the
/// three modes be exercised without mutating process-global env. `target` is the
/// pre-computed `{base}_{slug}` branch name the session should isolate under.
fn shared_tree_verdict(mode: Mode, target: &str) -> Verdict {
    match mode {
        Mode::Off => Verdict::Allow,
        Mode::Warn => Verdict::Warn {
            message: format!(
                "Isolamento por unidade de trabalho: rode EnterWorktree (nome `{target}`) \
                 antes de editar — a árvore principal é compartilhada entre sessões; \
                 seguindo na árvore atual."
            ),
        },
        Mode::Strict => Verdict::Deny {
            reason: format!(
                "Isolamento por unidade de trabalho: rode EnterWorktree (nome `{target}`) \
                 antes de editar; a árvore principal é compartilhada entre sessões. Para \
                 editar mesmo assim na árvore principal, defina \
                 MUSTARD_WORKTREE_ISOLATION_MODE=warn."
            ),
        },
    }
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
///    Every per-base error (no matching origin ref, a diverged base, …) is
///    ignored — best-effort, keep going.
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
///
/// Retained (with unit coverage) as the canonical base-recovery helper even
/// though the shared-tree checkout that consumed it was retired; exercised only
/// under `#[cfg(test)]`, hence the allow in a non-test build.
#[allow(dead_code)]
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
        let project = ctx.project_dir_or_cwd(input);
        let sid = session_id_from(input);

        // Branch protection guards the REPO's tree only. A mutation whose
        // target lives outside the project root (`~/.claude` memory files,
        // temp dirs, …) is not repo work: never block it, never cut a branch
        // for it, and keep the pending marker for the first IN-repo edit.
        if let Some(fp) = super::path_gate::file_path_of(input) {
            if super::path_gate::relative_to_cwd(&project, &fp).is_none() {
                return Ok(Verdict::Allow);
            }
        }

        // VCS binary policy: default `git`; an explicit `""` opt-out (or a
        // non-git tree) means we cannot branch and there is nothing to guard.
        let config = ProjectConfig::load(Path::new(&project));
        let Some(vcs) = config.vcs() else {
            context::clear_pending_branch(&project, &sid);
            return Ok(Verdict::Allow);
        };

        let current = current_branch(&vcs, &project);
        let pending = context::pending_branch_for(&project, &sid);

        // ISOLATED tree: a linked worktree (the desktop app's per-session tree,
        // or a prior `EnterWorktree`) already owns its branch, so nothing another
        // session touches collides here. Never cut a branch; just refresh the
        // bases (fail-open), consume any marker, and let the edit proceed.
        if is_isolated_worktree(&vcs, &project) {
            refresh_integration_bases(&vcs, &project, &config, current.as_deref());
            if pending.is_some() {
                context::clear_pending_branch(&project, &sid);
            }
            return Ok(Verdict::Allow);
        }

        // SHARED main tree from here on — the tree concurrent sessions contend
        // over.
        let on_protected = current
            .as_deref()
            .map(|b| is_protected(b, &config))
            .unwrap_or(false);

        // No marker: keep the hard block on editing a bare integration branch
        // directly; a work branch (or non-git tree) is free to edit.
        let Some(target) = pending else {
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

        // Already on the target branch (checked out by hand) → clear and allow.
        if current.as_deref() == Some(target.as_str()) {
            context::clear_pending_branch(&project, &sid);
            return Ok(Verdict::Allow);
        }

        // A marker asked for a NEW branch on the SHARED tree. We no longer
        // `git checkout -b` here — that mutates a tree other sessions share.
        // Refresh the bases (fail-open, unchanged) and steer the session to an
        // isolated worktree per the configured mode (see `shared_tree_verdict`).
        refresh_integration_bases(&vcs, &project, &config, current.as_deref());
        let mode = isolation_mode();
        let verdict = shared_tree_verdict(mode, &target);
        // Single-use marker: consume it in `warn`/`off` (one-shot nudge). In
        // `strict` keep it so the block re-fires on every retry until the
        // session isolates — a consumed marker would let a plain retry slip past.
        if mode != Mode::Strict {
            context::clear_pending_branch(&project, &sid);
        }
        Ok(verdict)
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

    /// Marker `dev_my-thing` on the SHARED main tree (`dev`), default mode
    /// (`warn`): the gate no longer checks a branch out on the shared tree — it
    /// `Warn`s (advisory, non-blocking), leaves HEAD on `dev`, and consumes the
    /// one-shot marker. Steering to `EnterWorktree`, not switching branches.
    #[test]
    fn shared_tree_marker_warns_without_checkout() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_str().unwrap();

        seed_flow(root, r#"{"*":"dev","dev":"main"}"#);
        init_repo_on(root, "dev");

        // The router pre-computed this branch for the work unit.
        let sid = "sess-branch-test";
        context::set_pending_branch(root_s, sid, "dev_my-thing");

        // First Write of the work unit fires the gate. No env override → warn.
        let (input, ctx) = pre_edit_input(root_s, sid);
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        assert!(
            matches!(verdict, Verdict::Warn { .. }) && !verdict.is_blocking(),
            "shared-tree marker warns, never blocks in the default mode: {verdict:?}",
        );

        // HEAD stayed on `dev` — the gate did NOT cut/switch a branch...
        assert_eq!(
            current_branch("git", root_s).as_deref(),
            Some("dev"),
            "no branch switch on the shared tree",
        );
        // ...and the one-shot marker was consumed so `warn` does not re-fire.
        assert!(
            context::pending_branch_for(root_s, sid).is_none(),
            "marker cleared after the warn nudge",
        );
    }

    /// Detection seam (pure): the main working tree reports the SAME path for
    /// `--git-dir` and `--git-common-dir` (not a linked worktree); a linked
    /// worktree reports a distinct per-worktree git dir. Exercised directly so
    /// the git-dir comparison is covered without spinning up a worktree.
    #[test]
    fn readings_indicate_linked_only_when_dirs_differ() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_str().unwrap();
        assert!(
            !readings_indicate_linked(root, ".git", ".git"),
            "identical git-dir/common-dir → shared main tree",
        );
        assert!(
            readings_indicate_linked(root, ".git/worktrees/wt", ".git"),
            "distinct per-worktree git dir → isolated linked worktree",
        );
    }

    /// The shared-tree verdict (pure) for each isolation mode: `off` allows
    /// silently, `warn` allows with an `EnterWorktree` advisory, `strict` denies
    /// with the same instruction. Exercised without mutating process env.
    #[test]
    fn shared_tree_verdict_per_mode() {
        assert!(matches!(shared_tree_verdict(Mode::Off, "dev_x"), Verdict::Allow));

        match shared_tree_verdict(Mode::Warn, "dev_x") {
            Verdict::Warn { message } => {
                assert!(message.contains("EnterWorktree"), "warn steers to EnterWorktree: {message}");
                assert!(message.contains("dev_x"), "warn names the target branch: {message}");
            }
            other => panic!("warn mode must Warn, got {other:?}"),
        }

        match shared_tree_verdict(Mode::Strict, "dev_x") {
            Verdict::Deny { reason } => {
                assert!(reason.contains("EnterWorktree"), "strict instructs EnterWorktree: {reason}");
                assert!(reason.contains("dev_x"), "strict names the target branch: {reason}");
            }
            other => panic!("strict mode must Deny, got {other:?}"),
        }
    }

    /// An ISOLATED linked worktree (the desktop-app / `EnterWorktree` shape) is
    /// already its own tree with its own branch: the gate never checks a branch
    /// out — it consumes the marker and Allows, leaving HEAD untouched.
    #[test]
    fn isolated_worktree_allows_without_checkout() {
        let main = tempfile::tempdir().unwrap();
        let main_root = main.path();
        seed_flow(main_root, r#"{"*":"dev","dev":"main"}"#);
        init_repo_on(main_root, "dev");

        // A linked worktree on its OWN branch (`git worktree add` requires the
        // path to not pre-exist, so hang it under a second tempdir).
        let parent = tempfile::tempdir().unwrap();
        let wt_path = parent.path().join("linked");
        let wt_s = wt_path.to_str().unwrap();
        git(main_root, &["worktree", "add", wt_s, "-b", "dev_iso"]);

        // A marker whose target DIFFERS from the worktree branch: proves the
        // isolated path does not switch to it.
        let sid = "sess-iso";
        context::set_pending_branch(wt_s, sid, "dev_other");

        let (input, ctx) = pre_edit_input(wt_s, sid);
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        assert!(
            matches!(verdict, Verdict::Allow),
            "an isolated worktree edits freely: {verdict:?}",
        );
        assert_eq!(
            current_branch("git", wt_s).as_deref(),
            Some("dev_iso"),
            "no branch switch — HEAD stays on the worktree's own branch",
        );
        assert!(
            context::pending_branch_for(wt_s, sid).is_none(),
            "the marker is consumed in the isolated path",
        );
    }

    /// FAIL-OPEN: with NO `origin` remote, the base refresh's `git fetch origin`
    /// fails — that must NOT turn the shared-tree nudge into a block or a panic.
    /// The gate still `Warn`s (default mode) and lets the edit proceed on the
    /// current branch.
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
            !verdict.is_blocking(),
            "a failing `git fetch origin` must not block the edit: {verdict:?}",
        );
        assert!(
            matches!(verdict, Verdict::Warn { .. }),
            "shared-tree default mode warns even when offline: {verdict:?}",
        );
        assert_eq!(
            current_branch("git", root_s).as_deref(),
            Some("dev"),
            "no branch switch on the shared tree",
        );
        assert!(
            context::pending_branch_for(root_s, sid).is_none(),
            "marker cleared after the warn nudge",
        );
    }

    /// A local bare-remote fixture whose `dev` is BEHIND origin: even without a
    /// checkout, the base refresh fast-forwards the shared `dev` IN PLACE before
    /// the gate nudges. Proves the refresh still advances the checked-out base.
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
        assert!(!verdict.is_blocking(), "edit proceeds (shared-tree warn): {verdict:?}");
        assert_eq!(
            current_branch("git", proj_s).as_deref(),
            Some("dev"),
            "no checkout — HEAD stays on the shared base",
        );

        // Even without a checkout, the shared `dev` was fast-forwarded in place
        // to the second commit by the base refresh.
        let head = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&proj)
            .output()
            .unwrap();
        let head_sha = String::from_utf8(head.stdout).unwrap().trim().to_string();
        assert_eq!(
            head_sha, ahead_sha,
            "the shared dev is fast-forwarded to the latest origin commit in place",
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
    fn base_for_picks_longest_matching_prefix_else_primary() {
        let mut config = ProjectConfig::default();
        config.git.flow.insert("*".into(), "dev".into());
        config.git.flow.insert("dev".into(), "main".into());
        assert_eq!(base_for("dev_thing", &config), "dev");
        assert_eq!(base_for("main_close-gate", &config), "main");
        // No `{base}_` prefix match → the primary base (flow["*"]).
        assert_eq!(base_for("random-branch", &config), "dev");

        // Agnostic: a develop/master project resolves against ITS bases.
        let mut dm = ProjectConfig::default();
        dm.git.flow.insert("*".into(), "develop".into());
        dm.git.flow.insert("develop".into(), "master".into());
        assert_eq!(base_for("master_hotfix", &dm), "master");
        assert_eq!(base_for("develop_x", &dm), "develop");
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

//! `work_branch_gate` — auto-branch per work unit on the first file mutation.
//!
//! ## What it does
//!
//! A `PreToolUse(Write|Edit|MultiEdit)` [`Check`] that, on the FIRST file
//! mutation of a work request, creates and checks out the branch the router
//! pre-computed for this work unit. The branch name (`{work_kind}/{slug}`) was
//! stored as the session's `pending-work-branch` marker by
//! `emit-pipeline --kind pipeline.kind` (see
//! [`crate::commands::event::emit_pipeline`]); this hook is the consumer.
//!
//! Read-only requests never Write/Edit, so the marker is never consumed and no
//! branch is ever created — branching is bound to an actual mutation, not to
//! opening a pipeline.
//!
//! ## Flow
//!
//! 1. No pending-branch marker:
//!    - on a PROTECTED branch (`main`/`master`, or a `git.flow` parent such as
//!      `dev`) → `Deny`. Work is never developed directly on an integration
//!      branch; describe the work (so the router seeds a branch) or branch by
//!      hand first.
//!    - otherwise (already on a work branch, or non-git tree) → `Allow`.
//! 2. Current branch already IS the target → clear the marker, `Allow`.
//! 3. Otherwise resolve the base branch (`mustard.json#git.flow["*"]`, default
//!    `dev`) and the VCS binary (`mustard.json#vcs`, default `git`), then:
//!    - target branch exists → `git checkout <target>`;
//!    - else → `git checkout -b <target> <base>` (carries the working tree
//!      along); if `<base>` is absent locally, `git checkout -b <target>` off
//!      the current HEAD.
//! 4. On success → clear the marker, `Allow`.
//! 5. On git failure → clear the marker (so it does not re-fire on every edit);
//!    if we are still on a PROTECTED branch → `Deny` (never edit it directly),
//!    otherwise `Warn` and let the edit proceed on the current work branch.
//!
//! ## Contract (apps/rt/CLAUDE.md)
//!
//! Never panics — no `unwrap`/`expect` outside tests; every git call is
//! `Command::…output()` matched into a `Result`, and an `Err` from the `Check`
//! itself folds to `Allow` in the dispatcher. It raises `Deny` ONLY to keep an
//! edit off a protected integration branch; otherwise it `Allow`s or `Warn`s.

use mustard_core::platform::error::Error;
use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::ProjectConfig;
use std::path::Path;
use std::process::Command;

use crate::shared::context;

/// The auto-branch gate. Stateless — every invocation rebuilds from the hook
/// input and the on-disk marker.
pub struct WorkBranchGate;

/// The default base branch when `mustard.json#git.flow["*"]` is unset.
const DEFAULT_BASE: &str = "dev";

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

/// `true` when `branch` is a protected integration branch that must never be
/// edited directly: `main`/`master`, or any parent target in `git.flow`
/// (e.g. `dev`). Ordinary work branches (`feature/…`, `dev_rubens`, …) are not
/// protected.
fn is_protected(branch: &str, config: &ProjectConfig) -> bool {
    if branch == "main" || branch == "master" {
        return true;
    }
    config.git.flow.values().any(|parent| parent.as_str() == branch)
}

impl Check for WorkBranchGate {
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        // Defensive: only PreToolUse(Write|Edit|MultiEdit) should reach us.
        if ctx.trigger != Some(Trigger::PreToolUse) {
            return Ok(Verdict::Allow);
        }
        let project = ctx.project_dir_or_cwd(input);
        let sid = session_id_from(input);

        // VCS binary policy: default `git`; an explicit `""` opt-out (or a
        // non-git tree) means we cannot branch and there is nothing to guard.
        let config = ProjectConfig::load(Path::new(&project));
        let Some(vcs) = config.vcs() else {
            context::clear_pending_branch(&project, &sid);
            return Ok(Verdict::Allow);
        };

        let current = current_branch(&vcs, &project);
        let on_protected = current
            .as_deref()
            .map(|b| is_protected(b, &config))
            .unwrap_or(false);

        // 1. No pending branch signalled for this session.
        let Some(target) = context::pending_branch_for(&project, &sid) else {
            // Hard block: never develop directly on a protected integration
            // branch. On a work branch (or non-git tree) there is nothing to do.
            if on_protected {
                return Ok(Verdict::Deny {
                    reason: format!(
                        "Você está na branch protegida '{}'. O Mustard não desenvolve \
                         direto aqui — descreva o trabalho para eu criar a branch \
                         {{kind}}/{{slug}}, ou crie uma branch manualmente antes de editar.",
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

        // 3-4. Resolve base and check out.
        let base = config
            .git
            .flow
            .get("*")
            .cloned()
            .unwrap_or_else(|| DEFAULT_BASE.to_string());
        match checkout_work_branch(&vcs, &project, &target, &base) {
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
        let input = HookInput {
            tool_name: Some("Write".to_string()),
            tool_input: json!({ "file_path": "f.txt", "content": "x" }),
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

    #[test]
    fn creates_and_checks_out_pending_branch_on_first_edit() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let root_s = root.to_str().unwrap();

        // A git repo whose sole commit lives on `dev` (the base branch).
        git(root, &["init"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        // Move onto an (unborn) `dev` branch before the first commit so the base
        // exists regardless of the platform's default branch name.
        git(root, &["checkout", "-b", "dev"]);
        std::fs::write(root.join("f.txt"), "hi").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-m", "init"]);

        // The router pre-computed this branch for the work unit.
        let sid = "sess-branch-test";
        context::set_pending_branch(root_s, sid, "feature/my-thing");

        // First Write of the work unit fires the gate.
        let (input, ctx) = pre_edit_input(root_s, sid);
        let verdict = WorkBranchGate.evaluate(&input, &ctx).expect("no error");
        assert!(matches!(verdict, Verdict::Allow), "the edit proceeds: {verdict:?}");

        // HEAD is now on the target branch...
        assert_eq!(
            current_branch("git", root_s).as_deref(),
            Some("feature/my-thing"),
            "checked out the pre-computed branch off dev",
        );
        // ...and the marker was cleared so subsequent edits do not re-fire.
        assert!(
            context::pending_branch_for(root_s, sid).is_none(),
            "marker cleared after checkout",
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

    /// A repo on a protected branch (`main`) with NO pending-branch marker: a
    /// direct edit must be BLOCKED — the harness never develops on an
    /// integration branch. (`main` is protected regardless of `git.flow`.)
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
}

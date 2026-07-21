//! `scan_clean_gate` — the scan's clean-tree precondition.
//!
//! ## Scope (ONE behavior)
//!
//! A `PreToolUse(Skill)` gate that blocks `/mustard:scan` while the working
//! tree carries uncommitted work.
//!
//! `/scan` rewrites **versioned** artifacts across the whole repo — the grain
//! model (`.claude/grain.model.json`, `grain.dictionary.json`), every
//! `.claude/scan-map.md`, the `## Guards` block of every subproject
//! `CLAUDE.md`, and the `{role}-pattern` skill molds. None of that is
//! gitignored (`.claude/.gitignore` only covers runtime scratch), so a scan
//! run on a dirty tree fuses the regenerated model with whatever the user was
//! doing. Under the `/git` iron law (`add -A`, never a partial scope) the two
//! can no longer be committed apart — the model refresh stops being its own
//! reviewable unit.
//!
//! Requiring a clean tree is what makes the refresh atomic: scan on clean →
//! commit + push the refresh alone → keep working.
//!
//! ## Mode
//!
//! The gate has **no `MUSTARD_*_MODE`** — it is always strict, mirroring its
//! sibling [`super::scan_gate`] (and `bash-safety`). A knob here would let the
//! very mixing this gate exists to prevent happen silently.
//!
//! ## Fail-open invariant
//!
//! Every unknown answers `Allow`: no `git` on PATH, not a repository, a failed
//! invocation, or unreadable output. The gate only ever blocks on a *positive*
//! observation of dirt, so it can never wedge a session it cannot reason about.

use std::path::Path;
use std::process::Command;

use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::platform::error::Error;

use crate::util::format_gate_message;

/// The scan skill names this gate applies to (prefixed + bare, mirroring
/// [`super::scan_gate`]).
const SCAN_SKILLS: &[&str] = &["mustard:scan", "scan"];

/// The scan clean-tree gate module.
pub struct ScanCleanGate;

/// `Some(true)` when the working tree has changes `git add -A` would stage —
/// tracked modifications, staged entries, or untracked non-ignored files.
/// `Some(false)` when clean. `None` for every unknown (no git, not a repo,
/// failed invocation) so the caller can fail open.
fn tree_is_dirty(cwd: &Path) -> Option<bool> {
    let out = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(cwd)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(!String::from_utf8_lossy(&out.stdout).trim().is_empty())
}

/// Compute the gate verdict for a `PreToolUse(Skill)` invocation rooted at
/// `cwd`. `Allow` for any non-scan skill, any non-`Skill` tool, a clean tree,
/// and every unknown; `Deny` only on observed dirt.
fn clean_verdict(input: &HookInput, cwd: &str) -> Verdict {
    if input.tool_name.as_deref() != Some("Skill") {
        return Verdict::Allow;
    }
    let skill = input
        .tool_input
        .get("skill")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if !SCAN_SKILLS.contains(&skill) {
        return Verdict::Allow;
    }
    if tree_is_dirty(Path::new(cwd)) != Some(true) {
        return Verdict::Allow;
    }
    Verdict::Deny {
        reason: format_gate_message(
            "Scan Clean-Tree Gate",
            "the working tree has uncommitted changes",
            "/scan rewrites versioned artifacts (grain model, scan-map.md, CLAUDE.md Guards, \
             {role}-pattern skills); with `git add -A` the refresh could not be committed apart \
             from your work",
            "commit or stash what you have, re-run /scan, then commit + push the refresh as its \
             own unit",
        ),
    }
}

impl Check for ScanCleanGate {
    /// Gate a `PreToolUse(Skill)` invocation of the scan skill on a clean
    /// working tree. Always strict — no mode.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::PreToolUse) {
            return Ok(Verdict::Allow);
        }
        let cwd = if ctx.project_dir.is_empty() {
            input.cwd.as_deref().unwrap_or(".")
        } else {
            ctx.project_dir.as_str()
        };
        Ok(clean_verdict(input, cwd))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn skill_input(skill: &str, cwd: &str) -> (HookInput, Ctx) {
        let input = HookInput {
            tool_name: Some("Skill".to_string()),
            tool_input: json!({ "skill": skill }),
            hook_event_name: Some("PreToolUse".to_string()),
            cwd: Some(cwd.to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx {
            project_dir: cwd.to_string(),
            trigger: Some(Trigger::PreToolUse),
            workspace_root: None,
        };
        (input, ctx)
    }

    /// Initialise a git repo with one commit so the tree starts clean.
    fn init_repo(dir: &Path) -> bool {
        let run = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(dir)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        };
        if !run(&["init"]) {
            return false;
        }
        let _ = run(&["config", "user.email", "t@t.t"]);
        let _ = run(&["config", "user.name", "t"]);
        std::fs::write(dir.join("seed.txt"), "seed\n").unwrap();
        run(&["add", "-A"]) && run(&["commit", "-m", "seed"])
    }

    #[test]
    fn clean_tree_allows_the_scan() {
        let dir = tempdir().unwrap();
        if !init_repo(dir.path()) {
            return; // no usable git here — the gate fails open anyway.
        }
        let (input, ctx) = skill_input("mustard:scan", dir.path().to_str().unwrap());
        assert_eq!(ScanCleanGate.evaluate(&input, &ctx).unwrap(), Verdict::Allow);
    }

    #[test]
    fn dirty_tree_denies_the_scan() {
        let dir = tempdir().unwrap();
        if !init_repo(dir.path()) {
            return;
        }
        std::fs::write(dir.path().join("seed.txt"), "edited\n").unwrap();
        let (input, ctx) = skill_input("mustard:scan", dir.path().to_str().unwrap());
        match ScanCleanGate.evaluate(&input, &ctx).unwrap() {
            Verdict::Deny { reason } => {
                assert!(reason.contains("uncommitted"), "names the dirt: {reason}");
            }
            other => panic!("expected Deny on a dirty tree, got {other:?}"),
        }
    }

    #[test]
    fn untracked_file_also_denies() {
        // `git add -A` would stage it, so it is pending work like any other.
        let dir = tempdir().unwrap();
        if !init_repo(dir.path()) {
            return;
        }
        std::fs::write(dir.path().join("stray.txt"), "x\n").unwrap();
        let (input, ctx) = skill_input("mustard:scan", dir.path().to_str().unwrap());
        assert!(matches!(
            ScanCleanGate.evaluate(&input, &ctx).unwrap(),
            Verdict::Deny { .. }
        ));
    }

    #[test]
    fn other_skills_pass_through_even_when_dirty() {
        let dir = tempdir().unwrap();
        if !init_repo(dir.path()) {
            return;
        }
        std::fs::write(dir.path().join("seed.txt"), "edited\n").unwrap();
        let (input, ctx) = skill_input("mustard:feature", dir.path().to_str().unwrap());
        assert_eq!(ScanCleanGate.evaluate(&input, &ctx).unwrap(), Verdict::Allow);
    }

    #[test]
    fn non_git_directory_fails_open() {
        let dir = tempdir().unwrap(); // never `git init`ed
        let (input, ctx) = skill_input("mustard:scan", dir.path().to_str().unwrap());
        assert_eq!(ScanCleanGate.evaluate(&input, &ctx).unwrap(), Verdict::Allow);
    }

    #[test]
    fn non_pretooluse_trigger_allows() {
        let dir = tempdir().unwrap();
        let (input, mut ctx) = skill_input("mustard:scan", dir.path().to_str().unwrap());
        ctx.trigger = Some(Trigger::PostToolUse);
        assert_eq!(ScanCleanGate.evaluate(&input, &ctx).unwrap(), Verdict::Allow);
    }
}

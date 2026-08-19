//! `scan_clean_gate` — the scan's clean-tree precondition.
//!
//! ## Scope (ONE behavior)
//!
//! A `PreToolUse(Skill)` gate that blocks the scan flow (`Skill(mustard:scan)`)
//! while the working tree carries uncommitted work. The flow stopped being a
//! door in the four-door prune, but the router still dispatches it as a skill,
//! so this is still the moment to judge the tree.
//!
//! In a SHARED install the scan rewrites **versioned** artifacts across the
//! whole repo — the grain model (`.claude/grain.model.json`,
//! `grain.dictionary.json`), every `.claude/scan-map.md`, the `## Guards` block
//! of every subproject `CLAUDE.md`, and the `{role}-pattern` skill molds. None
//! of that is gitignored (`.claude/.gitignore` only covers runtime scratch), so
//! a scan run on a dirty tree fuses the regenerated model with whatever the
//! user was doing. Under the `/git` iron law (`add -A`, never a partial scope)
//! the two can no longer be committed apart — the model refresh stops being its
//! own reviewable unit.
//!
//! Requiring a clean tree is what makes the refresh atomic: scan on clean →
//! commit + push the refresh alone → keep working.
//!
//! ## The premise is MIXING, not dirt — so a private install is exempt
//!
//! Every sentence above depends on the scan's output reaching the host
//! repository's git. A PRIVATE install is the case where it does not: every
//! path the scan writes is covered by this clone's exclude file
//! (`mustard_core::footprint_rules`), and the host's own `CLAUDE.md` is never
//! opened at all — a full scan writes `CLAUDE.local.md` beside it instead. So
//! there is nothing to stage, nothing to diff, and no second commit to keep
//! apart: no state of the working tree can fuse anything with anything.
//!
//! Refusing there blocked a scan to prevent a mixing that cannot occur — and
//! since a client's working tree is dirty nearly always, that made the flow
//! effectively unreachable in the mode Mustard now installs by default.
//! [`scan_output_is_versioned`] is the single place the question is asked.
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
///
/// `pub(crate)` so the base gate — which TRIGGERS a census refresh — consults
/// the same predicate as this gate, which REFUSES one. Two spellings of
/// "clean" is how the automatic path would start mining exactly where the
/// user-invoked door is refused. Note it counts `.claude/` as dirt (unlike
/// [`crate::commands::work_unit_open::dirty_paths`]), which is the whole point:
/// the model the scan rewrites lives there.
pub(crate) fn tree_is_dirty(cwd: &Path) -> Option<bool> {
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

/// Whether this install's scan output is VISIBLE to the host repository's git —
/// the precondition that makes a dirty tree mean anything here at all.
///
/// `true` for a shared install (the historical behaviour, unchanged) and for
/// every unknown: [`crate::shared::context::install_mode`] answers
/// `InstallMode::Shared` when there is no git, no repository, or an unreadable
/// exclude file, so an unmeasured project keeps the stricter reading.
///
/// `pub(crate)` for the same reason [`tree_is_dirty`] is: the door that REFUSES
/// a scan and the base gate that TRIGGERS one must consult one predicate. Two
/// spellings of "the output is versioned" is how a private install would get
/// fixed in the gate and stay broken in the automatic refresh.
pub(crate) fn scan_output_is_versioned(root: &Path) -> bool {
    !crate::shared::context::install_mode(root).is_private()
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
    // Asked BEFORE the tree is measured: in a private install the answer is the
    // same whatever `git status` says, and the status call is a process spawn.
    if !scan_output_is_versioned(Path::new(cwd)) {
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

    /// Mark this clone as a PRIVATE install exactly the way `run upsert
    /// --private` does — the two `PRIVATE_MARKS` rules in the clone-local
    /// exclude file, which is the only place the mode is ever recorded.
    fn make_private(dir: &Path) {
        let info = dir.join(".git").join("info");
        std::fs::create_dir_all(&info).unwrap();
        let body = mustard_core::PRIVATE_MARKS.join("\n") + "\n";
        std::fs::write(info.join("exclude"), body).unwrap();
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

    /// The regression this gate shipped with: a PRIVATE install writes nothing
    /// the host's git can see, so a dirty tree is not evidence of a mixing that
    /// could happen. Refusing there made the flow unreachable on a client
    /// repository, where the tree is dirty nearly always.
    #[test]
    fn private_install_allows_the_scan_on_a_dirty_tree() {
        let dir = tempdir().unwrap();
        if !init_repo(dir.path()) {
            return;
        }
        make_private(dir.path());
        std::fs::write(dir.path().join("seed.txt"), "edited\n").unwrap();
        let (input, ctx) = skill_input("mustard:scan", dir.path().to_str().unwrap());
        assert_eq!(
            ScanCleanGate.evaluate(&input, &ctx).unwrap(),
            Verdict::Allow,
            "a private install has no second commit to keep apart",
        );
    }

    /// The exemption is keyed on the MARKS, not on the exclude file existing:
    /// a clone whose exclude file carries unrelated rules is still shared, and
    /// still refuses.
    #[test]
    fn an_unrelated_exclude_file_is_not_a_private_install() {
        let dir = tempdir().unwrap();
        if !init_repo(dir.path()) {
            return;
        }
        let info = dir.path().join(".git").join("info");
        std::fs::create_dir_all(&info).unwrap();
        std::fs::write(info.join("exclude"), "build/\n*.log\n").unwrap();
        std::fs::write(dir.path().join("seed.txt"), "edited\n").unwrap();
        let (input, ctx) = skill_input("mustard:scan", dir.path().to_str().unwrap());
        assert!(matches!(
            ScanCleanGate.evaluate(&input, &ctx).unwrap(),
            Verdict::Deny { .. }
        ));
    }

    #[test]
    fn non_pretooluse_trigger_allows() {
        let dir = tempdir().unwrap();
        let (input, mut ctx) = skill_input("mustard:scan", dir.path().to_str().unwrap());
        ctx.trigger = Some(Trigger::PostToolUse);
        assert_eq!(ScanCleanGate.evaluate(&input, &ctx).unwrap(), Verdict::Allow);
    }
}

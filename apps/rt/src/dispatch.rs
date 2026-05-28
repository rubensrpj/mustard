//! The dispatcher — turns one harness invocation into one [`Outcome`].
//!
//! This is the single place the b3 fail-open contract lives (spec §
//! Arquitetura): a module never has to defend against bad input or its own
//! errors — the dispatcher absorbs them. The flow per invocation:
//!
//! 1. Resolve the applicable modules from the [`Registry`].
//! 2. Run each `Observer` fire-and-forget (telemetry never blocks, never
//!    fails the run).
//! 3. Run each `Check`; fold its `Verdict` into one [`Outcome`]. Per-concern
//!    off/warn/strict lives inside the individual checks (each reads its own
//!    `MUSTARD_*_MODE` env), not in the dispatcher.
//!
//! A `Check` that returns `Err` is treated as `Allow` — the dispatcher
//! degrades, it never panics.

use crate::registry::{Module, Registry};
use mustard_core::domain::model::contract::{Ctx, HookInput, Outcome, Trigger, Verdict};
use mustard_core::io::workspace::workspace_root;
use std::path::PathBuf;

/// Run every module applicable to a whole harness event (`mustard-rt on`).
///
/// `trigger` is `None` when the harness event name was unrecognised — the
/// fail-open path: no module matches, the outcome is a bare `Allow`.
#[must_use]
pub fn run_event(trigger: Option<Trigger>, input: &HookInput) -> Outcome {
    let Some(trigger) = trigger else {
        return Outcome::allow();
    };
    let registry = Registry::new();
    let tool = input.tool_name.as_deref();
    let ctx = build_ctx(trigger, input);

    let mut outcome = Outcome::allow();
    for module in registry.applicable(trigger, tool) {
        run_module(module, input, &ctx, &mut outcome);
    }
    outcome
}

/// Run a single named module (`mustard-rt check <id>`).
///
/// An unknown id is fail-open: nothing matches, the outcome is `Allow`.
#[must_use]
pub fn run_check(id: &str, input: &HookInput) -> Outcome {
    let registry = Registry::new();
    let Some(module) = registry.by_id(id) else {
        return Outcome::allow();
    };
    // For a single-module run the trigger comes from the input itself; if it
    // is missing the module's own logic still fails open.
    let trigger = input.trigger().unwrap_or(Trigger::PreToolUse);
    let ctx = build_ctx(trigger, input);

    let mut outcome = Outcome::allow();
    run_module(module, input, &ctx, &mut outcome);
    outcome
}

/// Build the ambient [`Ctx`] for a check from the harness input.
///
/// Resolves the Mustard workspace root once per invocation via
/// [`workspace_root`] and stashes it on [`Ctx`]. On resolver error the
/// dispatcher fails open: the `Ctx` still gets a sane `project_dir`, but
/// `workspace_root` is `None` and a structured warning is logged to stderr.
/// Hooks must NOT block users on a resolution failure.
///
/// AC-G2 guard: when `cwd` is `"."`, empty, or another relative placeholder,
/// the dispatcher resolves it to an absolute path via `std::env::current_dir()`
/// before walking for the workspace root. Without this step `walk_ancestors`
/// only sees `"."` as its own parent (no absolute ancestor walk), so
/// `workspace_root` returns "anchor not found" and the project_dir stays as
/// the raw placeholder — causing downstream writers to materialise
/// `apps/rt/.claude/` during `cargo test` (whose cwd is `apps/rt/`).
fn build_ctx(trigger: Trigger, input: &HookInput) -> Ctx {
    let raw_cwd = input.cwd.clone().unwrap_or_default();
    // Resolve relative / empty cwd to an absolute path so workspace_root's
    // ancestor walk starts from the real filesystem location.
    let resolved_cwd = if raw_cwd.is_empty() || raw_cwd == "." {
        std::env::current_dir()
            .ok()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| raw_cwd.clone())
    } else {
        raw_cwd.clone()
    };
    let workspace_root = resolve_workspace_root_fail_open(&resolved_cwd);
    // Prefer the resolved workspace root over the raw cwd so downstream
    // writers (tracker, amend_capture, …) target the monorepo root, not the
    // crate directory the test binary happens to run in.
    let project_dir = workspace_root
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or(resolved_cwd);
    Ctx {
        project_dir,
        trigger: Some(trigger),
        workspace_root,
    }
}

/// Best-effort wrapper around [`workspace_root`] that logs a single structured
/// warning on failure and returns `None`. Never panics.
fn resolve_workspace_root_fail_open(project_dir: &str) -> Option<PathBuf> {
    let start = PathBuf::from(project_dir);
    match workspace_root(&start) {
        Ok(root) => Some(root),
        Err(err) => {
            // Structured single-line log; non-fatal — the dispatcher carries
            // on with `workspace_root: None` so modules can degrade.
            let _ = serde_json::to_string(&serde_json::json!({
                "level": "warn",
                "module": "dispatch",
                "event": "workspace_root.unresolved",
                "project_dir": project_dir,
                "error": err.to_string(),
            }))
            .map(|s| eprintln!("{s}"));
            None
        }
    }
}

/// Run one module: its observer (fire-and-forget), then its check (folded into
/// the outcome).
fn run_module(module: &Module, input: &HookInput, ctx: &Ctx, outcome: &mut Outcome) {
    // Observers are pure telemetry: they cannot fail (the trait returns `()`)
    // and cannot affect the outcome. Run unconditionally.
    if let Some(observer) = &module.observer {
        observer.observe(input, ctx);
    }

    let Some(check) = &module.check else {
        return;
    };

    // A `Check` that errors is degraded to `Allow` — fail-open lives here, not
    // in the module. Per-concern off/warn/strict is decided inside each check
    // (it reads its own `MUSTARD_*_MODE`); the dispatcher just folds the
    // verdict the check returns.
    let verdict = check.evaluate(input, ctx).unwrap_or(Verdict::Allow);
    outcome.fold(verdict);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn bash_input(command: &str, event: &str) -> HookInput {
        HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: json!({ "command": command }),
            hook_event_name: Some(event.to_string()),
            ..HookInput::default()
        }
    }

    #[test]
    fn unknown_event_fails_open_to_allow() {
        let outcome = run_event(None, &HookInput::default());
        assert_eq!(outcome.verdict, Verdict::Allow);
    }

    #[test]
    fn unknown_check_id_fails_open_to_allow() {
        let outcome = run_check("does-not-exist", &HookInput::default());
        assert_eq!(outcome.verdict, Verdict::Allow);
    }

    #[test]
    fn dispatch_runs_bash_guard_for_bash_pretooluse() {
        let input = bash_input("rm -rf /", "PreToolUse");
        let outcome = run_event(Some(Trigger::PreToolUse), &input);
        assert!(outcome.is_blocking());
    }

    #[test]
    fn dispatch_denies_bare_ls_for_bash_pretooluse() {
        let input = bash_input("ls", "PreToolUse");
        let outcome = run_event(Some(Trigger::PreToolUse), &input);
        assert!(
            outcome.is_blocking(),
            "expected blocking outcome for bare ls; got {:?}, warnings {:?}",
            outcome.verdict,
            outcome.warnings
        );
    }
}

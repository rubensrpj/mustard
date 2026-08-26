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

/// Run every module applicable to a whole harness event (`mustard-rt on`),
/// optionally narrowed to a single declared injectable.
///
/// `trigger` is `None` when the harness event name was unrecognised — the
/// fail-open path: no module matches, the outcome is a bare `Allow`.
///
/// `inject_only` is the `--inject <file>` a sibling hook registration carries.
/// Each injectable is delivered by its own hook invocation, so each is measured
/// alone against the 10,000-character ceiling a hook RESPONSE holds — siblings
/// do not share a budget (measured 2026-08-25; see
/// `plugin/refs/mustard/router-rationale.md`). `None` delivers every entry of
/// the trigger, which is what every non-injectable module wants.
#[must_use]
pub fn run_event(
    trigger: Option<Trigger>,
    input: &HookInput,
    inject_only: Option<&str>,
) -> Outcome {
    let Some(trigger) = trigger else {
        return Outcome::allow();
    };
    let registry = Registry::new();
    let tool = input.tool_name.as_deref();
    let mut ctx = build_ctx(trigger, input);
    ctx.inject_only = inject_only.map(str::to_string);
    let carries_shared_modules = carries_shared_modules(
        &ctx.project_dir,
        &trigger.as_event_name().to_ascii_lowercase(),
        inject_only,
    );

    let mut outcome = Outcome::allow();
    for module in registry.applicable(trigger, tool) {
        // Every sibling hook of one event fires for the SAME prompt, so a
        // module that is not about one injectable must run exactly once across
        // all of them. Letting each sibling run the whole registry executed
        // every observer once per sibling: `user.prompt` was logged twice for
        // one prompt, and `change_request_log` appended twice.
        //
        // Skipping them on every scoped invocation is the opposite mistake, and
        // worse — with only sibling hooks registered, no invocation carries
        // them and the trace records nothing at all. So ONE sibling is elected
        // to carry them: the one claiming the first declared injectable.
        if !carries_shared_modules && !INJECTING_MODULES.contains(&module.id) {
            continue;
        }
        run_module(module, input, &ctx, &mut outcome);
    }
    outcome
}

/// The modules a `--inject` invocation always runs — the ones that ARE about
/// the injectable it claims.
const INJECTING_MODULES: &[&str] = &["prompt_submit_inject", "session_start_inject"];

/// Does this invocation carry the modules that belong to the whole event?
///
/// An unscoped invocation always does. A `--inject` one does only when it
/// claims the FIRST injectable the project declares for that event: declaration
/// order elects exactly one sibling, needs no extra configuration, and every
/// sibling reaches the same answer from the same config.
///
/// Fail-open: a project that declares nothing for the trigger, or whose config
/// cannot be read, answers `true` — running an observer twice is recoverable,
/// losing the event trace entirely is not.
pub(crate) fn carries_shared_modules(project_dir: &str, trigger_on: &str, inject_only: Option<&str>) -> bool {
    let Some(only) = inject_only else {
        return true;
    };
    let declared: Vec<String> = mustard_core::ProjectConfig::load(std::path::Path::new(project_dir))
        .injectables()
        .into_iter()
        .filter(|e| e.on.eq_ignore_ascii_case(trigger_on))
        .map(|e| e.file)
        .collect();
    if declared.is_empty() {
        return true;
    }
    // The elected sibling is the first declared entry that a hook ACTUALLY
    // claims, and the manifest is the only authority on that. Electing on
    // declaration order alone elects nobody when the first entry is a file no
    // hook claims — a hand-edited `mustard.json` naming a custom injectable —
    // and every observer was then skipped, leaving the event trace empty
    // (found in review, measured at 0 records for one prompt).
    let claimed = claimed_injectables(project_dir);
    let position_among_claimed = declared
        .iter()
        .filter(|f| {
            claimed
                .iter()
                .any(|c| crate::shared::paths::same_declared_file(f, c))
        })
        .position(|f| crate::shared::paths::same_declared_file(f, only));
    match position_among_claimed {
        // First among the CLAIMED entries: this invocation carries them.
        Some(0) => true,
        Some(_) => false,
        // Unclaimed, or the manifest could not be read. A sibling that cannot
        // see the list cannot count on another one being there, and running an
        // observer twice is recoverable where an empty trace is not.
        None => true,
    }
}

/// The injectable files the installed hook manifest actually claims, via
/// `--inject`.
///
/// Empty when the manifest cannot be found or read, which makes every caller
/// fall through to the fail-open branch — the safe direction.
fn claimed_injectables(project_dir: &str) -> Vec<String> {
    let Some(manifest) = mustard_core::platform::harness::installed_plugin_hooks_manifest()
        .or_else(|| {
            let p = std::path::Path::new(project_dir)
                .join("plugin")
                .join("hooks")
                .join("hooks.json");
            p.is_file().then_some(p)
        })
    else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(&manifest) else {
        return Vec::new();
    };
    // Parsed as JSON, then the flag is read out of each command STRING. A
    // split over the raw file text picks up the JSON punctuation that follows
    // the path (`… .md",`) and matches nothing afterwards — measured, and it
    // produced the opposite defect: every sibling read itself as unclaimed and
    // carried the shared modules, so the trace was written twice.
    let Ok(doc) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let Some(events) = doc.get("hooks").and_then(serde_json::Value::as_object) else {
        return out;
    };
    for entries in events.values().filter_map(serde_json::Value::as_array) {
        for hook in entries
            .iter()
            .filter_map(|e| e.get("hooks")?.as_array())
            .flatten()
        {
            let Some(cmd) = hook.get("command").and_then(serde_json::Value::as_str) else {
                continue;
            };
            if let Some(rest) = cmd.split("--inject").nth(1) {
                if let Some(path) = rest.split_whitespace().next() {
                    out.push(path.trim_matches('"').to_string());
                }
            }
        }
    }
    out
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
        inject_only: None,
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
        let outcome = run_event(None, &HookInput::default(), None);
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
        let outcome = run_event(Some(Trigger::PreToolUse), &input, None);
        assert!(outcome.is_blocking());
    }

    #[test]
    fn dispatch_denies_bare_ls_for_bash_pretooluse() {
        let input = bash_input("ls", "PreToolUse");
        let outcome = run_event(Some(Trigger::PreToolUse), &input, None);
        assert!(
            outcome.is_blocking(),
            "expected blocking outcome for bare ls; got {:?}, warnings {:?}",
            outcome.verdict,
            outcome.warnings
        );
    }
}

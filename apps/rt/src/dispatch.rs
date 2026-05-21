//! The dispatcher — turns one harness invocation into one [`Outcome`].
//!
//! This is the single place the b3 fail-open contract lives (spec §
//! Arquitetura): a module never has to defend against bad input or its own
//! errors — the dispatcher absorbs them. The flow per invocation:
//!
//! 1. Resolve the applicable modules from the [`Registry`].
//! 2. Run each `Observer` fire-and-forget (telemetry never blocks, never
//!    fails the run).
//! 3. Run each `Check`; map its `Verdict` through the module's enforcement
//!    [`Mode`] (off/warn/strict); fold the result into one [`Outcome`].
//!
//! A `Check` that returns `Err` is treated as `Allow` — the dispatcher
//! degrades, it never panics.

use crate::registry::{self, Module, Registry};
use mustard_core::config::Mode;
use mustard_core::model::contract::{Ctx, HookInput, Outcome, Trigger, Verdict};

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
fn build_ctx(trigger: Trigger, input: &HookInput) -> Ctx {
    Ctx {
        project_dir: input.cwd.clone().unwrap_or_default(),
        trigger: Some(trigger),
    }
}

/// Run one module: its observer (fire-and-forget), then its check (folded
/// through the module's enforcement mode).
fn run_module(module: &Module, input: &HookInput, ctx: &Ctx, outcome: &mut Outcome) {
    // Observers are pure telemetry: they cannot fail (the trait returns `()`)
    // and cannot affect the outcome. Run unconditionally.
    if let Some(observer) = &module.observer {
        observer.observe(input, ctx);
    }

    let Some(check) = &module.check else {
        return;
    };

    let mode = registry::mode_for(module.id);
    if mode == Mode::Off {
        return;
    }

    // A `Check` that errors is degraded to `Allow` — fail-open lives here, not
    // in the module.
    let verdict = check.evaluate(input, ctx).unwrap_or(Verdict::Allow);
    outcome.fold(apply_mode(verdict, mode));
}

/// Map a raw [`Verdict`] through the module's enforcement [`Mode`].
///
/// In `Warn` mode a blocking `Deny` is downgraded to a non-blocking `Warn`
/// carrying the same reason — the JS gates do exactly this (`review-gate.js`
/// emits `permissionDecision: 'allow'` with the reason as advisory text when
/// not in strict mode). `Strict` passes the verdict through unchanged. `Off`
/// is handled by the caller (the module never runs).
fn apply_mode(verdict: Verdict, mode: Mode) -> Verdict {
    match (mode, verdict) {
        (Mode::Warn, Verdict::Deny { reason }) => Verdict::Warn { message: reason },
        (_, verdict) => verdict,
    }
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

    #[test]
    fn warn_mode_downgrades_deny_to_warn() {
        let downgraded = apply_mode(
            Verdict::Deny {
                reason: "blocked".into(),
            },
            Mode::Warn,
        );
        assert_eq!(
            downgraded,
            Verdict::Warn {
                message: "blocked".into()
            }
        );
    }
}

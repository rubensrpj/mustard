//! The harness response shape: turning a consolidated [`Outcome`] into the JSON
//! object Claude Code reads back from a hook.
//!
//! ## Why this is a module and not part of `main.rs`
//!
//! It used to live in `main.rs`, with its tests beside it. `main.rs` declares
//! the same seven modules `lib.rs` does rather than consuming the library, so
//! every `#[cfg(test)]` block under `src/` was compiled AND EXECUTED twice —
//! once for the `lib` target and once for the `bin`. Measured on 2026-08-14:
//! `unittests src/lib.rs` ran 1968 tests in 150,8s and `unittests src/main.rs`
//! ran 1973 of the same tests in 187,6s, out of 397,7s of total execution. Half
//! the local test time, and the largest single piece of the CI's 729s Windows
//! test step, was the same assertions running a second time.
//!
//! The binary now declares `test = false` (`apps/rt/Cargo.toml`), so the `lib`
//! target runs them once. That deletes the five tests below along with the
//! duplicates unless they live somewhere the library reaches — which is what
//! this module is. `emit_outcome` stays in `main.rs`: it calls
//! `process::exit`, so it is the binary's own business and no test exercises it.
//!
//! Full figures: `docs/2026-08-14-build-cycle-measurements.md`.

use mustard_core::domain::model::contract::{Outcome, Verdict};

/// `true` when the harness accepts a `hookSpecificOutput` object for
/// `event_name`. The harness models `hookSpecificOutput` as a discriminated
/// union keyed by `hookEventName`; these seven events carry an
/// `additionalContext` (or `permissionDecision`) slot. `SessionStart` is one of
/// them — it injects persistent memory at the top of a session. The
/// context-free lifecycle events (`PreCompact`, `SessionEnd`, `Notification`,
/// …) have no slot, so the harness rejects the whole response when an object is
/// emitted for them.
pub(crate) fn event_accepts_hook_output(event_name: &str) -> bool {
    matches!(
        event_name,
        "PreToolUse"
            | "UserPromptSubmit"
            | "PostToolUse"
            | "PostToolBatch"
            | "Stop"
            | "SubagentStop"
            | "SessionStart"
    )
}

/// Build the `hookSpecificOutput` JSON for an outcome, or `None` for a bare
/// `Allow` with no warnings (the JS hooks stay silent in that case).
///
/// `event_name` is echoed back as `hookEventName` so the response matches
/// the harness event that was dispatched (e.g. `UserPromptSubmit`,
/// `PostToolUse`). Claude Code rejects the response when this disagrees
/// with the dispatched event.
///
/// Events outside [`event_accepts_hook_output`] get no output at all: the
/// harness has no `hookSpecificOutput` member for them and rejects the whole
/// response when one is present, dropping the hook's `additionalContext`. The
/// hook's side-effects have already run by this point, so the binary writes
/// nothing for those events and exits clean.
pub(crate) fn hook_specific_output(event_name: &str, outcome: &Outcome) -> Option<String> {
    if !event_accepts_hook_output(event_name) {
        return None;
    }
    // `UserPromptSubmit` blocks through a DIFFERENT shape than `PreToolUse`:
    // the harness's discriminated union gives this event no `permissionDecision`
    // member — a denial is the TOP-LEVEL `{"decision": "block", "reason": …}`
    // pair (the prompt is erased and the reason shown to the user). Emitting
    // the PreToolUse shape here would be rejected wholesale and the gate would
    // silently never fire. Non-deny verdicts keep the shared path below
    // (`additionalContext` is a valid member for this event).
    if event_name == "UserPromptSubmit" {
        if let Verdict::Deny { reason } = &outcome.verdict {
            let root = serde_json::json!({
                "decision": "block",
                "reason": reason,
            });
            return Some(root.to_string());
        }
    }

    // `Stop` blocks the same way: a top-level `{"decision":"block", …}` (the
    // `stop_gate` QA-verification gate), NOT the `PreToolUse` permissionDecision
    // member — that member does not exist for Stop, so the harness would reject
    // the whole response and the gate would silently never fire. We carry the
    // reason on BOTH channels: the top-level `reason` and a
    // `hookSpecificOutput.additionalContext` mirror (the feedback member the
    // current Stop contract reads), so the failing-AC guidance reaches Claude
    // regardless of which member the running harness honours. Exit stays 0 —
    // the whole binary expresses blocking through JSON, never a non-zero exit
    // (rt `## Guards`), so the exit-2 blocking path never applies here.
    if event_name == "Stop" {
        if let Verdict::Deny { reason } = &outcome.verdict {
            let root = serde_json::json!({
                "decision": "block",
                "reason": reason,
                "hookSpecificOutput": {
                    "hookEventName": "Stop",
                    "additionalContext": reason,
                },
            });
            return Some(root.to_string());
        }
    }
    let mut hook_output = serde_json::Map::new();
    hook_output.insert(
        "hookEventName".to_string(),
        serde_json::Value::String(event_name.to_string()),
    );

    match &outcome.verdict {
        Verdict::Allow if outcome.warnings.is_empty() => return None,
        Verdict::Deny { reason } => {
            hook_output.insert(
                "permissionDecision".to_string(),
                serde_json::Value::String("deny".to_string()),
            );
            hook_output.insert(
                "permissionDecisionReason".to_string(),
                serde_json::Value::String(reason.clone()),
            );
        }
        Verdict::Rewrite { tool_input } => {
            hook_output.insert(
                "permissionDecision".to_string(),
                serde_json::Value::String("allow".to_string()),
            );
            hook_output.insert("updatedInput".to_string(), tool_input.clone());
        }
        Verdict::Inject { context } => {
            hook_output.insert(
                "permissionDecision".to_string(),
                serde_json::Value::String("allow".to_string()),
            );
            hook_output.insert(
                "additionalContext".to_string(),
                serde_json::Value::String(context.clone()),
            );
        }
        Verdict::Allow | Verdict::Warn { .. } => {
            // `Allow` only reaches here with warnings present; `Warn` verdicts
            // never sit in `outcome.verdict` (the fold routes them to
            // `warnings`). Either way it is an advisory: allow + a message.
            hook_output.insert(
                "permissionDecision".to_string(),
                serde_json::Value::String("allow".to_string()),
            );
        }
        _ => {
            // `Verdict` is `#[non_exhaustive]`; an unknown future variant
            // degrades to a silent allow rather than a panic (fail-open).
            return None;
        }
    }

    if !outcome.warnings.is_empty() {
        hook_output.insert(
            "additionalContext".to_string(),
            serde_json::Value::String(outcome.warnings.join("\n")),
        );
    }

    let mut root = serde_json::Map::new();
    root.insert(
        "hookSpecificOutput".to_string(),
        serde_json::Value::Object(hook_output),
    );
    Some(serde_json::Value::Object(root).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inject_outcome() -> Outcome {
        Outcome {
            verdict: Verdict::Inject {
                context: "remember this".to_string(),
            },
            warnings: Vec::new(),
        }
    }

    #[test]
    fn pre_compact_emits_no_hook_output() {
        // The harness has no `hookSpecificOutput` slot for `PreCompact`, so the
        // builder stays silent even for an injecting outcome.
        assert!(hook_specific_output("PreCompact", &inject_outcome()).is_none());
    }

    #[test]
    fn user_prompt_submit_emits_additional_context() {
        // `UserPromptSubmit` is an accepted event, so an injecting outcome
        // serialises its context into `additionalContext`.
        let json = hook_specific_output("UserPromptSubmit", &inject_outcome())
            .expect("accepted event must emit output");
        assert!(json.contains("additionalContext"));
    }

    #[test]
    fn session_start_emits_additional_context() {
        // `SessionStart` carries an `additionalContext` slot (it injects
        // persistent memory at the top of a session), so an injecting outcome
        // must serialise rather than stay silent — dropping it would lose the
        // session-start memory injection.
        let json = hook_specific_output("SessionStart", &inject_outcome())
            .expect("SessionStart must emit output");
        assert!(json.contains("additionalContext"));
    }

    #[test]
    fn user_prompt_submit_deny_emits_top_level_block_decision() {
        // A Deny on `UserPromptSubmit` (the installation gate) must speak the
        // event's OWN blocking shape — top-level `decision: "block"` + `reason`
        // — never the PreToolUse `permissionDecision` member, which this
        // event's hookSpecificOutput union does not carry (the harness would
        // reject the whole response and the gate would silently not fire).
        let outcome = Outcome {
            verdict: Verdict::Deny {
                reason: "Mustard is not installed".to_string(),
            },
            warnings: Vec::new(),
        };
        let json = hook_specific_output("UserPromptSubmit", &outcome)
            .expect("a denying UserPromptSubmit outcome must emit output");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed.get("decision").and_then(|v| v.as_str()), Some("block"));
        assert_eq!(
            parsed.get("reason").and_then(|v| v.as_str()),
            Some("Mustard is not installed")
        );
        assert!(
            !json.contains("permissionDecision"),
            "PreToolUse shape must not leak into UserPromptSubmit: {json}"
        );

        // A PreToolUse deny keeps the historical permissionDecision shape.
        let json = hook_specific_output("PreToolUse", &outcome)
            .expect("a denying PreToolUse outcome must emit output");
        assert!(json.contains("\"permissionDecision\":\"deny\""), "unexpected: {json}");
    }

    #[test]
    fn stop_deny_emits_top_level_block_with_additional_context() {
        // A Stop deny (the `stop_gate`) speaks the Stop blocking shape: a
        // top-level `decision: "block"` + `reason`, mirrored into
        // `hookSpecificOutput.additionalContext` — never the PreToolUse
        // permissionDecision member (rejected wholesale for Stop). Exit stays 0.
        let outcome = Outcome {
            verdict: Verdict::Deny {
                reason: "QA criterion AC-2 still fails".to_string(),
            },
            warnings: Vec::new(),
        };
        let json = hook_specific_output("Stop", &outcome)
            .expect("a denying Stop outcome must emit output");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed.get("decision").and_then(|v| v.as_str()), Some("block"));
        assert_eq!(
            parsed.get("reason").and_then(|v| v.as_str()),
            Some("QA criterion AC-2 still fails")
        );
        assert_eq!(
            parsed
                .get("hookSpecificOutput")
                .and_then(|h| h.get("additionalContext"))
                .and_then(|v| v.as_str()),
            Some("QA criterion AC-2 still fails")
        );
        assert!(
            !json.contains("permissionDecision"),
            "PreToolUse shape must not leak into Stop: {json}"
        );

        // A Stop ALLOW with no warnings stays silent (no forced continue).
        assert!(hook_specific_output("Stop", &Outcome::allow()).is_none());
    }
}

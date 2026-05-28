//! The harness wire protocol — how a hook reads stdin and writes stdout.
//!
//! A port of the conventions in `_lib/hook-env.js` and the JS hooks: the
//! harness pipes a single JSON object on stdin, and a `PreToolUse` hook
//! answers by printing a `{ "hookSpecificOutput": { … } }` object and exiting
//! `0`. This module owns the *encoding* of that contract; the *decisions* live
//! in [`mustard_core`] (`Verdict` / `Outcome`) and the enforcement modules.

use mustard_core::domain::model::contract::{HookInput, Outcome, Trigger, Verdict};
use serde_json::{Value, json};
use std::io::Read;

/// Read a [`HookInput`] from stdin.
///
/// **Central fail-open.** The spec (`## Arquitetura`) makes any stdin parse
/// failure resolve to "allow" — so the dispatcher, not each module, owns the
/// fallback. An unreadable or non-JSON stdin therefore returns
/// `HookInput::default()`, which carries no `tool_name` and no trigger; every
/// module then sees an empty invocation and produces `Verdict::Allow`.
#[must_use]
pub fn read_hook_input() -> HookInput {
    let mut buf = String::new();
    if std::io::stdin().read_to_string(&mut buf).is_err() {
        return HookInput::default();
    }
    serde_json::from_str(&buf).unwrap_or_default()
}

/// Encode an [`Outcome`] as the stdout JSON the harness expects, plus the
/// process exit code.
///
/// Parity with the JS hooks (`bash-safety.js`, `bash-native-redirect.js`,
/// `rtk-rewrite.js`, `review-gate.js`):
///
/// - `Allow` with no warnings → no stdout, exit `0` (the JS "exit 0 silently").
/// - `Deny` → `permissionDecision: "deny"` + `permissionDecisionReason`.
/// - `Warn` (or accumulated warnings) → `permissionDecision: "allow"` +
///   `additionalContext` carrying the advisory text.
/// - `Rewrite` → `permissionDecision: "allow"` + `updatedInput`.
/// - `Inject` → `permissionDecision: "allow"` + `additionalContext`.
///
/// Every branch exits `0`: a hook never signals failure through its exit code
/// — a `deny` is communicated in the JSON body, matching the JS protocol.
#[must_use]
pub fn encode_outcome(outcome: &Outcome, trigger: Option<Trigger>) -> EncodedResponse {
    let event_name = trigger.unwrap_or(Trigger::PreToolUse).as_event_name();

    match &outcome.verdict {
        Verdict::Deny { reason } => EncodedResponse::deny(event_name, reason),
        Verdict::Rewrite { tool_input } => {
            EncodedResponse::rewrite(event_name, tool_input.clone(), &outcome.warnings)
        }
        Verdict::Inject { context } => {
            // An injection and any accumulated warnings both surface as
            // `additionalContext`; join them so none is dropped.
            let mut parts = vec![context.clone()];
            parts.extend(outcome.warnings.iter().cloned());
            EncodedResponse::allow_with_context(event_name, &parts.join("\n"))
        }
        Verdict::Warn { message } => {
            // A bare `Warn` verdict never reaches a folded `Outcome` (fold
            // routes it into `warnings`), but handle it for completeness.
            let mut parts = vec![message.clone()];
            parts.extend(outcome.warnings.iter().cloned());
            EncodedResponse::allow_with_context(event_name, &parts.join("\n"))
        }
        Verdict::Allow => {
            if outcome.warnings.is_empty() {
                EncodedResponse::silent()
            } else {
                EncodedResponse::allow_with_context(event_name, &outcome.warnings.join("\n"))
            }
        }
        // `Verdict` is `#[non_exhaustive]`: an unknown future variant fails
        // open to a silent allow rather than panicking.
        _ => EncodedResponse::silent(),
    }
}

/// A fully encoded harness response: optional stdout JSON + an exit code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedResponse {
    /// The JSON to print on stdout, or `None` for the silent-allow case.
    pub stdout: Option<String>,
    /// The process exit code — always `0` in the hook protocol.
    pub exit_code: i32,
}

impl EncodedResponse {
    /// A silent allow: no stdout, exit `0`.
    #[must_use]
    fn silent() -> Self {
        Self { stdout: None, exit_code: 0 }
    }

    /// Build a response from a `hookSpecificOutput` payload.
    fn from_payload(payload: Value) -> Self {
        let body = json!({ "hookSpecificOutput": payload });
        Self {
            stdout: Some(body.to_string()),
            exit_code: 0,
        }
    }

    /// A `deny` response — blocks the tool call.
    #[must_use]
    fn deny(event_name: &str, reason: &str) -> Self {
        Self::from_payload(json!({
            "hookEventName": event_name,
            "permissionDecision": "deny",
            "permissionDecisionReason": reason,
        }))
    }

    /// An `allow` response carrying advisory `additionalContext`.
    #[must_use]
    fn allow_with_context(event_name: &str, context: &str) -> Self {
        Self::from_payload(json!({
            "hookEventName": event_name,
            "permissionDecision": "allow",
            "additionalContext": context,
        }))
    }

    /// An `allow` response carrying a rewritten tool input (`updatedInput`),
    /// plus any advisory `additionalContext`.
    #[must_use]
    fn rewrite(event_name: &str, tool_input: Value, warnings: &[String]) -> Self {
        let mut payload = json!({
            "hookEventName": event_name,
            "permissionDecision": "allow",
            "updatedInput": tool_input,
        });
        if !warnings.is_empty() {
            if let Some(obj) = payload.as_object_mut() {
                obj.insert("additionalContext".into(), json!(warnings.join("\n")));
            }
        }
        Self::from_payload(payload)
    }

    /// Print the stdout (if any) and return the exit code. The single I/O
    /// boundary of the binary.
    #[must_use]
    pub fn emit(&self) -> i32 {
        if let Some(ref body) = self.stdout {
            println!("{body}");
        }
        self.exit_code
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allow_with_no_warnings_is_silent() {
        let resp = encode_outcome(&Outcome::allow(), Some(Trigger::PreToolUse));
        assert_eq!(resp.stdout, None);
        assert_eq!(resp.exit_code, 0);
    }

    #[test]
    fn deny_encodes_permission_decision() {
        let mut outcome = Outcome::allow();
        outcome.fold(Verdict::Deny { reason: "nope".into() });
        let resp = encode_outcome(&outcome, Some(Trigger::PreToolUse));
        let json: Value = serde_json::from_str(&resp.stdout.unwrap()).unwrap();
        assert_eq!(json["hookSpecificOutput"]["permissionDecision"], "deny");
        assert_eq!(
            json["hookSpecificOutput"]["permissionDecisionReason"],
            "nope"
        );
    }

    #[test]
    fn warnings_surface_as_additional_context() {
        let mut outcome = Outcome::allow();
        outcome.fold(Verdict::Warn { message: "be careful".into() });
        let resp = encode_outcome(&outcome, Some(Trigger::PreToolUse));
        let json: Value = serde_json::from_str(&resp.stdout.unwrap()).unwrap();
        assert_eq!(json["hookSpecificOutput"]["permissionDecision"], "allow");
        assert_eq!(
            json["hookSpecificOutput"]["additionalContext"],
            "be careful"
        );
    }

    #[test]
    fn rewrite_encodes_updated_input() {
        let mut outcome = Outcome::allow();
        outcome.fold(Verdict::Rewrite {
            tool_input: json!({ "command": "rtk grep x" }),
        });
        let resp = encode_outcome(&outcome, Some(Trigger::PreToolUse));
        let json: Value = serde_json::from_str(&resp.stdout.unwrap()).unwrap();
        assert_eq!(
            json["hookSpecificOutput"]["updatedInput"]["command"],
            "rtk grep x"
        );
    }
}

//! `plan_approval_observer` — PostToolUse(ExitPlanMode) approval recorder.
//!
//! Plan-mode counterpart of [`super::approval_marker_observer`] (the
//! AskUserQuestion recorder). With `plansDirectory` configured, a Full-scope
//! PLAN is presented as a plan-mode plan file and the user approves it by
//! accepting `ExitPlanMode`. The harness reports that acceptance in the
//! PostToolUse `tool_response` — a payload the model does not author — so it
//! can mint the same unforgeable `<spec>/.approved-by-user` marker that
//! `approve-spec` requires in strict mode. Plan mode is the primary approval
//! path; the AskUserQuestion ritual stays registered as the fallback source
//! (both mint the identical marker).
//!
//! ## What counts as approval — conservative by construction
//!
//! The marker is minted only when ALL hold:
//!
//! 1. **State (load-bearing, unforgeable).** The active spec is `scope=full`,
//!    `stage=Plan`, and carries no `pipeline.status{to:approved}` yet — the
//!    exact predicates of the AskUserQuestion recorder, imported from it
//!    rather than duplicated.
//! 2. **A clear approval response.** An APPROVED `ExitPlanMode` returns a
//!    structured `tool_response` object carrying the accepted `plan` text
//!    (the observed wire contract) and no error marker. A REJECTED
//!    `ExitPlanMode` surfaces as a tool error (`is_error: true`, the
//!    "User rejected tool use" string) — PostToolUse either skips it or
//!    delivers a string/error shape, and both fall outside the
//!    object-with-`plan` check, so nothing is minted.
//!
//! Fail-closed on any doubt, fail-open on IO. Pure [`Observer`] — never
//! blocks, never returns a verdict.

use mustard_core::domain::model::contract::{Ctx, HookInput, Observer};
use serde_json::Value;

use super::approval_marker_observer::{active_spec, already_approved, is_full_plan};
use crate::shared::context::approval_marker_path;

/// The PostToolUse(ExitPlanMode) approval recorder.
pub struct PlanApprovalObserver;

/// `true` when `tool_response` is a clear `ExitPlanMode` approval: a
/// structured object carrying a non-empty `plan` and no error marker.
/// Anything else — an absent response, a bare string (the rejection echo),
/// or an error-marked object — is not an approval and records nothing.
fn plan_approved(input: &HookInput) -> bool {
    let Some(resp) = input.raw.get("tool_response") else {
        return false;
    };
    let Some(obj) = resp.as_object() else {
        return false;
    };
    let errored = obj.get("is_error").and_then(Value::as_bool).unwrap_or(false)
        || obj.get("isError").and_then(Value::as_bool).unwrap_or(false)
        || obj.contains_key("error");
    if errored {
        return false;
    }
    obj.get("plan")
        .and_then(Value::as_str)
        .is_some_and(|p| !p.trim().is_empty())
}

impl Observer for PlanApprovalObserver {
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        let cwd = ctx.project_dir_or_cwd(input);

        // Fact 1 — an unapproved Full spec in PLAN, else nothing is pending.
        let Some(spec) = active_spec(&cwd, input) else {
            return;
        };
        if !is_full_plan(&cwd, &spec) || already_approved(&cwd, &spec) {
            return;
        }

        // Fact 2 — a clear plan-mode approval.
        if !plan_approved(input) {
            return;
        }

        // Both facts hold → record the genuine approval, best-effort.
        if let Some(marker) = approval_marker_path(&cwd, &spec) {
            let body = format!(
                "spec={spec}\nvia=ExitPlanMode\nsession={}\n",
                input.session_id.as_deref().unwrap_or("unknown")
            );
            let _ = mustard_core::io::fs::write_atomic(&marker, body.as_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::domain::model::contract::Trigger;
    use serde_json::json;
    use std::path::Path;
    use tempfile::tempdir;

    fn ctx(dir: &str) -> Ctx {
        Ctx {
            project_dir: dir.to_string(),
            trigger: Some(Trigger::PostToolUse),
            workspace_root: None,
        }
    }

    /// A PostToolUse(ExitPlanMode) input carrying `tool_response`.
    fn exit_plan_input(session: &str, tool_response: Value) -> HookInput {
        HookInput {
            hook_event_name: Some("PostToolUse".to_string()),
            tool_name: Some("ExitPlanMode".to_string()),
            session_id: Some(session.to_string()),
            tool_input: json!({ "plan": "# The plan" }),
            raw: json!({ "tool_response": tool_response }),
            ..HookInput::default()
        }
    }

    /// Seed `.claude/spec/<spec>/meta.json` with the given scope + stage.
    fn seed_spec(root: &Path, spec: &str, scope: &str, stage: &str) {
        let dir = root.join(".claude").join("spec").join(spec);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("meta.json"),
            format!(r#"{{"scope":"{scope}","stage":"{stage}","outcome":"Active"}}"#),
        )
        .unwrap();
    }

    /// Bind `session` → `spec` via the `.session/<id>/active-spec` marker.
    fn bind_session(root: &Path, session: &str, spec: &str) {
        let d = root.join(".claude").join(".session").join(session);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("active-spec"), spec).unwrap();
    }

    fn marker_exists(root: &Path, spec: &str) -> bool {
        approval_marker_path(root.to_str().unwrap(), spec)
            .map(|p| p.exists())
            .unwrap_or(false)
    }

    // ── The approval recognizer (unit) ───────────────────────────────────

    #[test]
    fn approved_response_object_with_plan_is_approval() {
        let input = exit_plan_input("s", json!({ "plan": "# The plan body" }));
        assert!(plan_approved(&input));
    }

    #[test]
    fn rejection_string_and_error_shapes_are_not_approval() {
        // The observed rejection echo is a bare string.
        assert!(!plan_approved(&exit_plan_input("s", json!("User rejected tool use"))));
        // Defensive: error-marked objects never approve.
        assert!(!plan_approved(&exit_plan_input(
            "s",
            json!({ "plan": "# p", "is_error": true })
        )));
        assert!(!plan_approved(&exit_plan_input(
            "s",
            json!({ "error": "rejected" })
        )));
        // An object without a plan proves nothing.
        assert!(!plan_approved(&exit_plan_input("s", json!({}))));
        assert!(!plan_approved(&exit_plan_input("s", json!({ "plan": "  " }))));
        // No tool_response at all.
        let mut input = exit_plan_input("s", json!(null));
        input.raw = json!({});
        assert!(!plan_approved(&input));
    }

    // ── The observer (integration over a tempdir) ────────────────────────

    #[test]
    fn approved_exit_plan_mode_in_full_plan_writes_marker() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_spec(root, "epic", "full (wave plan)", "Plan");
        bind_session(root, "s-1", "epic");
        let input = exit_plan_input("s-1", json!({ "plan": "# Approved plan" }));
        PlanApprovalObserver.observe(&input, &ctx(root.to_str().unwrap()));
        assert!(marker_exists(root, "epic"), "a plan-mode approval must mint the marker");
        let body = std::fs::read_to_string(
            approval_marker_path(root.to_str().unwrap(), "epic").unwrap(),
        )
        .unwrap();
        assert!(body.contains("via=ExitPlanMode"), "provenance recorded: {body}");
        assert!(body.contains("spec=epic"));
        assert!(body.contains("session=s-1"));
    }

    #[test]
    fn rejected_exit_plan_mode_writes_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_spec(root, "epic", "full", "Plan");
        bind_session(root, "s-1", "epic");
        let input = exit_plan_input("s-1", json!("User rejected tool use"));
        PlanApprovalObserver.observe(&input, &ctx(root.to_str().unwrap()));
        assert!(!marker_exists(root, "epic"), "a rejection must NOT mint the marker");
    }

    #[test]
    fn spec_past_plan_writes_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_spec(root, "epic", "full", "Execute");
        bind_session(root, "s-1", "epic");
        let input = exit_plan_input("s-1", json!({ "plan": "# p" }));
        PlanApprovalObserver.observe(&input, &ctx(root.to_str().unwrap()));
        assert!(!marker_exists(root, "epic"), "no PLAN approval pending → no marker");
    }

    #[test]
    fn light_spec_writes_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        seed_spec(root, "small", "light", "Plan");
        bind_session(root, "s-1", "small");
        let input = exit_plan_input("s-1", json!({ "plan": "# p" }));
        PlanApprovalObserver.observe(&input, &ctx(root.to_str().unwrap()));
        assert!(!marker_exists(root, "small"));
    }

    #[test]
    fn no_project_is_failopen() {
        let dir = tempdir().unwrap();
        // No `.claude/` at all — observe must not panic / propagate.
        let input = exit_plan_input("s-1", json!({ "plan": "# p" }));
        PlanApprovalObserver.observe(&input, &ctx(dir.path().to_str().unwrap()));
        // Survival is the contract.
    }
}
